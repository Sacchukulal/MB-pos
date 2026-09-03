/** The lock screen's keyboard, as a pure reducer. */

import type { PersonView } from '../ipc/generated/PersonView';

/** How long a PIN is. */
export const PIN_DIGITS = 4;

/** Which part of the recovery flow is on screen. */
export type RecoverStep = 'code' | 'who' | 'pin' | 'again';

export type Mode =
  /** Signing in: the people down one side, the pad for whoever is marked. */
  | { kind: 'pin'; person: PersonView | null; digits: string; typed: string }
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
  /** Who signed in last, so the mark starts on them. */
  lastSignedIn: string | null;
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
      lastSignedIn: string | null;
    }
  | { kind: 'choose'; person: PersonView }
  /** The arrow keys: the mark moves one row. */
  | { kind: 'move'; by: 1 | -1 }
  | { kind: 'digit'; digit: string }
  | { kind: 'typed'; text: string }
  /** Backspace. One digit. */
  | { kind: 'back' }
  /** Back, or a full clear of the pad. Leaves the step whole — never a digit at a time. */
  | { kind: 'cancel' }
  | { kind: 'submit' }
  | { kind: 'start-recovery' }
  | { kind: 'recovered'; freshCode: string }
  | { kind: 'failed'; message: string }
  | { kind: 'done' };

export function initial(): State {
  return {
    mode: signIn(null),
    people: [],
    recoverable: [],
    canRecover: false,
    lastSignedIn: null,
    problem: null,
    busy: false,
    pending: [],
    seq: 0,
  };
}

/** The sign-in screen, marked on somebody, with an empty pad. */
function signIn(person: PersonView | null, typed = ''): Mode {
  return { kind: 'pin', person, digits: '', typed };
}

/** The people the list shows: everybody, or those whose name has what was typed. */
export function shown(people: readonly PersonView[], typed: string): readonly PersonView[] {
  const wanted = typed.trim().toLowerCase();
  if (wanted === '') return people;
  return people.filter((p) => p.name.toLowerCase().includes(wanted));
}

/** Who the mark starts on: whoever signed in last, else the first name. */
function startingMark(people: readonly PersonView[], lastSignedIn: string | null): PersonView | null {
  return people.find((p) => p.id === lastSignedIn) ?? people[0] ?? null;
}

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

/** Send the marked person's PIN, once it has the right number of digits. */
function submitPin(state: State, mode: Extract<Mode, { kind: 'pin' }>): State {
  if (!mode.person) {
    return { ...state, problem: 'Choose who you are.' };
  }
  if (mode.person.lockedOut) {
    return { ...state, problem: mode.person.lockedOut };
  }
  if (mode.digits.length !== PIN_DIGITS) {
    // Said here rather than after a round trip, because it is a fact about the shape and not
    // about the PIN.
    return { ...state, problem: `A PIN is ${PIN_DIGITS} digits.` };
  }
  return queue(state, { do: 'sign-in', staffId: mode.person.id, pin: mode.digits });
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
        lastSignedIn: event.lastSignedIn,
      };
      const gone = (id: string, from: readonly PersonView[]) =>
        !from.some((p) => p.id === id);

      if (mode.kind === 'pin') {
        // The mark stays on whoever it was on, as long as they are still on the list.
        if (mode.person && !gone(mode.person.id, event.people)) {
          const fresh = event.people.find((p) => p.id === mode.person?.id) ?? mode.person;
          return { ...carried, mode: { ...mode, person: fresh } };
        }
        return {
          ...carried,
          mode: signIn(startingMark(event.people, event.lastSignedIn), mode.typed),
        };
      }
      // Somebody suspended halfway through a reset must not still be the person a new PIN is
      // written to.
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
      // In the recovery flow, choosing a person means "this new PIN is theirs".
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
      if (state.mode.kind === 'pin') {
        if (state.busy) return state;
        return { ...state, problem: null, mode: signIn(event.person, state.mode.typed) };
      }
      return state;

    case 'move': {
      if (state.mode.kind !== 'pin' || state.busy) return state;
      const marked = state.mode.person;
      const list = shown(state.people, state.mode.typed);
      if (list.length === 0) return state;
      const at = list.findIndex((p) => p.id === marked?.id);
      const next =
        at < 0
          ? event.by > 0
            ? 0
            : list.length - 1
          : Math.min(list.length - 1, Math.max(0, at + event.by));
      const person = list[next];
      if (!person || person.id === marked?.id) return state;
      return reduce(state, { kind: 'choose', person });
    }

    case 'digit': {
      if (state.busy) return state;
      if (!/^[0-9]$/.test(event.digit)) return state;

      if (state.mode.kind === 'pin') {
        const { person, digits } = state.mode;
        if (!person) return state;
        // Locked out: the pad takes nothing, and says why.
        if (person.lockedOut) return { ...state, problem: person.lockedOut };
        if (digits.length >= PIN_DIGITS) return state;
        const mode = { ...state.mode, digits: digits + event.digit };
        const next = { ...state, problem: null, mode };
        // The fourth digit signs in by itself.
        return mode.digits.length === PIN_DIGITS ? submitPin(next, mode) : next;
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
      if (state.mode.kind === 'pin') {
        const list = shown(state.people, event.text);
        const marked = state.mode.person;
        // Narrowing the list moves the mark onto it when the marked name has dropped out.
        const person =
          marked && list.some((p) => p.id === marked.id) ? marked : (list[0] ?? marked);
        return {
          ...state,
          mode:
            person?.id === marked?.id
              ? { ...state.mode, typed: event.text }
              : signIn(person, event.text),
        };
      }
      if (state.mode.kind === 'recover' && state.mode.step === 'code') {
        return { ...state, mode: { ...state.mode, code: event.text } };
      }
      return state;

    case 'back': {
      // Backspace, and only Backspace.
      if (state.mode.kind === 'pin') {
        if (state.mode.digits === '') return state;
        return {
          ...state,
          mode: { ...state.mode, digits: state.mode.digits.slice(0, -1) },
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
          return {
            ...state,
            mode: signIn(startingMark(state.people, state.lastSignedIn)),
            problem: null,
          };
        }
        // Stepping back clears both pads, always.
        return {
          ...state,
          problem: null,
          mode: { ...mode, step, newPin: '', again: '' },
        };
      }

      if (state.mode.kind === 'pin') {
        if (state.mode.digits === '' && state.problem === null) return state;
        return { ...state, mode: { ...state.mode, digits: '' }, problem: null };
      }
      return state;
    }

    case 'submit': {
      if (state.busy) return state;
      if (state.mode.kind === 'pin') return submitPin(state, state.mode);
      if (state.mode.kind === 'recover') return submitRecovery(state, state.mode);
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
        lastSignedIn: state.lastSignedIn,
        mode: signIn(startingMark(state.people, state.lastSignedIn)),
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
    }
    return state;
  }
  if (pressed === 'ArrowDown') return reduce(state, { kind: 'move', by: 1 });
  if (pressed === 'ArrowUp') return reduce(state, { kind: 'move', by: -1 });
  if (pressed === 'Enter') return reduce(state, { kind: 'submit' });
  if (pressed === 'Backspace') return reduce(state, { kind: 'back' });
  if (pressed === 'Escape') {
    // Escape clears what has been typed.
    if (state.mode.kind === 'pin') {
      return { ...state, mode: signIn(state.mode.person), problem: null };
    }
    // Out of the recovery flow, not one step back: a half-finished reset is not a thing to keep.
    return {
      ...state,
      mode: signIn(startingMark(state.people, state.lastSignedIn)),
      problem: null,
    };
  }
  return state;
}

/** Commands the screen has not performed yet, and the state with them taken. */
export function take(state: State): [State, readonly Command[]] {
  if (state.pending.length === 0) return [state, []];
  return [{ ...state, pending: [] }, state.pending.map((q) => q.command)];
}
