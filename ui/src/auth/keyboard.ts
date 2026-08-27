/** The lock screen's keyboard, as a pure reducer. */

import type { PersonView } from '../ipc/generated/PersonView';

/** How long a PIN is. */
export const PIN_DIGITS = 4;

/** Which part of the recovery flow is on screen. */
export type RecoverStep = 'code' | 'who' | 'pin' | 'again';

export type Mode =
  /** Choosing who you are — by tapping a name, or by typing a staff code. */
  | { kind: 'who'; typed: string }
  /** Typing the PIN. */
  | { kind: 'pin'; person: PersonView; digits: string }
  /** The recovery flow, as four steps rather than one screen. */
  | {
      kind: 'recover';
      step: RecoverStep;
      code: string;
      person: PersonView | null;
      newPin: string;
      again: string;
    }
  | { kind: 'recovered'; freshCode: string };

export interface State {
  mode: Mode;
  /** Who can sign in — everybody active who has a PIN. */
  people: readonly PersonView[];
  /** Who the recovery code may set a PIN for. */
  recoverable: readonly PersonView[];
  /** Whether this shop has a recovery code to offer at all. */
  canRecover: boolean;
  /** The last thing Rust said, shown under the pad. */
  problem: string | null;
  /** True while a command is in flight — the pad stops accepting digits. */
  busy: boolean;
  /** Commands the screen has not performed yet. */
  pending: readonly Queued[];
  seq: number;
}

export type Command =
  | { do: 'sign-in'; staffId: string; pin: string }
  | { do: 'recover'; code: string; staffId: string; newPin: string };

export interface Queued {
  seq: number;
  command: Command;
}

export type Event =
  | { kind: 'key'; key: string }
  | {
      kind: 'people';
      people: readonly PersonView[];
      recoverable: readonly PersonView[];
      canRecover: boolean;
    }
  | { kind: 'choose'; person: PersonView }
  | { kind: 'digit'; digit: string }
  | { kind: 'typed'; text: string }
  /** Backspace. One digit, or one character of a staff code. */
  | { kind: 'back' }
  /** Somebody else, Back. Leaves the step whole — never a digit at a time. */
  | { kind: 'cancel' }
  | { kind: 'submit' }
  | { kind: 'start-recovery' }
  | { kind: 'recovered'; freshCode: string }
  | { kind: 'failed'; message: string }
  | { kind: 'done' };

export function initial(): State {
  return {
    mode: { kind: 'who', typed: '' },
    people: [],
    recoverable: [],
    canRecover: false,
    problem: null,
    busy: false,
    pending: [],
    seq: 0,
  };
}

/* Who the recovery code is allowed to set a PIN for is not decided here. */

function queue(state: State, command: Command): State {
  const seq = state.seq + 1;
  return {
    ...state,
    seq,
    busy: true,
    problem: null,
    pending: [...state.pending, { seq, command }],
  };
}

/** A fresh recovery flow, at its first step. */
function recovery(): Mode {
  return {
    kind: 'recover',
    step: 'code',
    code: '',
    person: null,
    newPin: '',
    again: '',
  };
}

export function reduce(state: State, event: Event): State {
  switch (event.kind) {
    case 'people': {
      const mode = state.mode;
      const carried = {
        ...state,
        people: event.people,
        recoverable: event.recoverable,
        canRecover: event.canRecover,
      };
      // Which list somebody has to still be on depends on what they are in the middle of:
      // signing in, or having a PIN set for them.
      const gone = (id: string, from: readonly PersonView[]) =>
        !from.some((p) => p.id === id);

      if (mode.kind === 'pin' && gone(mode.person.id, event.people)) {
        return { ...carried, mode: { kind: 'who', typed: '' } };
      }
      // The same fact inside the recovery flow: somebody suspended halfway through a reset must
      // not still be the person a new PIN is written to.
      if (mode.kind === 'recover' && mode.person && gone(mode.person.id, event.recoverable)) {
        return {
          ...carried,
          problem: 'That person is not on the staff list any more. Choose somebody else.',
          mode: { ...mode, step: 'who', person: null, newPin: '', again: '' },
        };
      }
      return carried;
    }

    case 'choose':
      // In the recovery flow, choosing a person means "this new PIN is theirs" — not "open the
      // pad for them".
      if (state.mode.kind === 'recover') {
        return {
          ...state,
          problem: null,
          mode: {
            ...state.mode,
            person: event.person,
            step: 'pin',
            newPin: '',
            again: '',
          },
        };
      }
      // Locked out: the pad does not open at all.
      if (event.person.lockedOut) {
        return { ...state, problem: event.person.lockedOut };
      }
      return {
        ...state,
        mode: { kind: 'pin', person: event.person, digits: '' },
        problem: null,
      };

    case 'digit': {
      if (state.busy) return state;
      if (!/^[0-9]$/.test(event.digit)) return state;

      if (state.mode.kind === 'pin') {
        // The fifth keypress does nothing.
        if (state.mode.digits.length >= PIN_DIGITS) return state;
        return {
          ...state,
          problem: null,
          mode: { ...state.mode, digits: state.mode.digits + event.digit },
        };
      }

      if (state.mode.kind === 'recover') {
        // The same ceiling on both of the recovery flow's pads.
        if (state.mode.step !== 'pin' && state.mode.step !== 'again') return state;
        const field = state.mode.step === 'pin' ? 'newPin' : 'again';
        const current = state.mode[field];
        if (current.length >= PIN_DIGITS) return state;
        return {
          ...state,
          problem: null,
          mode: { ...state.mode, [field]: current + event.digit },
        };
      }

      return state;
    }

    case 'typed':
      if (state.mode.kind === 'who') {
        return { ...state, mode: { kind: 'who', typed: event.text } };
      }
      if (state.mode.kind === 'recover' && state.mode.step === 'code') {
        return { ...state, mode: { ...state.mode, code: event.text } };
      }
      return state;

    case 'back': {
      // Backspace, and only Backspace.
      if (state.mode.kind === 'pin') {
        if (state.mode.digits === '') {
          // Backspace on an empty pad goes back to the list, which is how somebody who tapped
          // the wrong name gets out without the mouse.
          return { ...state, mode: { kind: 'who', typed: '' }, problem: null };
        }
        return {
          ...state,
          mode: { ...state.mode, digits: state.mode.digits.slice(0, -1) },
        };
      }
      if (state.mode.kind === 'who') {
        if (state.mode.typed === '') return state;
        return {
          ...state,
          mode: { kind: 'who', typed: state.mode.typed.slice(0, -1) },
        };
      }
      if (state.mode.kind === 'recover') {
        const mode = state.mode;
        if (mode.step === 'pin' || mode.step === 'again') {
          const field = mode.step === 'pin' ? 'newPin' : 'again';
          const current = mode[field];
          if (current === '') return reduce(state, { kind: 'cancel' });
          return { ...state, mode: { ...mode, [field]: current.slice(0, -1) } };
        }
        // On the code box the browser is already editing the text itself, and on the list there
        // is nothing to rub out.
        return state;
      }
      return state;
    }

    case 'cancel': {
      // One tap leaves, whatever has been typed.
      if (state.busy) return state;

      if (state.mode.kind === 'recover') {
        const mode = state.mode;
        const previous: Record<RecoverStep, RecoverStep | null> = {
          code: null, // out of the flow altogether
          who: 'code',
          pin: 'who',
          again: 'pin',
        };
        const step = previous[mode.step];
        if (step === null) {
          return { ...state, mode: { kind: 'who', typed: '' }, problem: null };
        }
        // Stepping back clears both pads, always.
        return {
          ...state,
          problem: null,
          mode: { ...mode, step, newPin: '', again: '' },
        };
      }

      if (state.mode.kind === 'who' && state.mode.typed === '') return state;
      return { ...state, mode: { kind: 'who', typed: '' }, problem: null };
    }

    case 'submit': {
      if (state.busy) return state;
      if (state.mode.kind === 'who') {
        // Type a staff code and press Enter — the same trick as the billing screen's table
        // numbers, and for the same reason: a keyboard person should never have to reach for
        // the list.
        const wanted = state.mode.typed.trim().toLowerCase();
        if (wanted === '') return state;
        const person = state.people.find(
          (p) => (p.code ?? '').toLowerCase() === wanted,
        );
        if (!person) {
          return { ...state, problem: 'No staff code like that. Pick a name instead.' };
        }
        return reduce(state, { kind: 'choose', person });
      }

      if (state.mode.kind === 'pin') {
        if (state.mode.digits.length !== PIN_DIGITS) {
          // Said here rather than after a round trip, because it is a fact about the shape and
          // not about the PIN.
          return { ...state, problem: `A PIN is ${PIN_DIGITS} digits.` };
        }
        return queue(state, {
          do: 'sign-in',
          staffId: state.mode.person.id,
          pin: state.mode.digits,
        });
      }

      if (state.mode.kind === 'recover') {
        return submitRecovery(state, state.mode);
      }
      return state;
    }

    case 'start-recovery':
      return { ...state, mode: recovery(), problem: null };

    case 'recovered':
      return {
        ...state,
        busy: false,
        pending: [],
        problem: null,
        mode: { kind: 'recovered', freshCode: event.freshCode },
      };

    case 'failed':
      return {
        ...state,
        busy: false,
        pending: [],
        problem: event.message,
        // The digits are cleared on a failure.
        mode:
          state.mode.kind === 'pin'
            ? { ...state.mode, digits: '' }
            : state.mode.kind === 'recover'
              ? { ...state.mode, step: 'code', newPin: '', again: '' }
              : state.mode,
      };

    case 'done':
      return {
        ...initial(),
        people: state.people,
        recoverable: state.recoverable,
        canRecover: state.canRecover,
      };

    case 'key':
      return key(state, event.key);
  }
}

/** The recovery flow's Next button, which asks a different question at every step. */
function submitRecovery(state: State, mode: Extract<Mode, { kind: 'recover' }>): State {
  switch (mode.step) {
    case 'code': {
      if (mode.code.trim() === '') {
        return { ...state, problem: 'Type the recovery code from the printed slip.' };
      }
      // The code itself is NOT checked here.
      if (state.recoverable.length === 0) {
        return {
          ...state,
          problem:
            'Nobody here manages staff, so this code has no PIN to set. Ring support.',
        };
      }
      return { ...state, problem: null, mode: { ...mode, step: 'who' } };
    }

    case 'who':
      if (!mode.person) {
        return { ...state, problem: 'Choose who this new PIN is for.' };
      }
      return { ...state, problem: null, mode: { ...mode, step: 'pin' } };

    case 'pin':
      if (mode.newPin.length !== PIN_DIGITS) {
        return { ...state, problem: `A PIN is ${PIN_DIGITS} digits.` };
      }
      return { ...state, problem: null, mode: { ...mode, step: 'again', again: '' } };

    case 'again': {
      if (mode.again.length !== PIN_DIGITS) {
        return { ...state, problem: `A PIN is ${PIN_DIGITS} digits.` };
      }
      if (mode.again !== mode.newPin) {
        // Back to the first pad with both cleared, rather than leaving one of two disagreeing
        // PINs standing for the next attempt to agree with.
        return {
          ...state,
          problem: 'The two PINs are not the same. Type the new one again.',
          mode: { ...mode, step: 'pin', newPin: '', again: '' },
        };
      }
      if (!mode.person) {
        return { ...state, problem: 'Choose who this new PIN is for.' };
      }
      return queue(state, {
        do: 'recover',
        code: mode.code,
        staffId: mode.person.id,
        newPin: mode.newPin,
      });
    }
  }
}

function key(state: State, pressed: string): State {
  if (/^[0-9]$/.test(pressed)) {
    if (state.mode.kind === 'pin') {
      return reduce(state, { kind: 'digit', digit: pressed });
    }
    if (state.mode.kind === 'recover') {
      // Only the two pads take loose digits.
      if (state.mode.step === 'pin' || state.mode.step === 'again') {
        return reduce(state, { kind: 'digit', digit: pressed });
      }
      return state;
    }
    // On the "who" screen a digit is part of a staff code.
    if (state.mode.kind === 'who') {
      return { ...state, mode: { kind: 'who', typed: state.mode.typed + pressed } };
    }
    return state;
  }
  if (pressed === 'Enter') return reduce(state, { kind: 'submit' });
  if (pressed === 'Backspace') return reduce(state, { kind: 'back' });
  if (pressed === 'Escape') {
    // Escape clears what has been typed.
    if (state.mode.kind === 'pin') {
      return { ...state, mode: { ...state.mode, digits: '' }, problem: null };
    }
    // Out of the recovery flow, not one step back — Escape has always meant "clear what I
    // typed", and a half-finished reset is not a thing to keep.
    return { ...state, mode: { kind: 'who', typed: '' }, problem: null };
  }
  return state;
}

/** Commands the screen has not performed yet, and the state with them taken. */
export function take(state: State): [State, readonly Command[]] {
  if (state.pending.length === 0) return [state, []];
  return [{ ...state, pending: [] }, state.pending.map((q) => q.command)];
}
