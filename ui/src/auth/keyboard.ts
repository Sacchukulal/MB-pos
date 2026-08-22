/**
 * **The lock screen's keyboard, as a pure reducer.**
 *
 * Same shape as P10's billing keyboard: `(state, event) -> [state, commands]`,
 * no React, no IPC, no DOM. It can be driven exhaustively in a test, and it is.
 *
 * # A reducer must not have side effects
 *
 * P10 learned this expensively: StrictMode double-invokes reducers, every
 * command ran twice, and one beer came out as 440.00. Commands ride **in the
 * state**, keyed on `seq`, and the screen performs the ones it has not seen.
 * Keep it that way.
 *
 * # Why a state machine for four keys
 *
 * Because it is not four keys. Choosing a person by tap or by typing a staff
 * code, four digits with no auto-submit, a countdown that must not be typed
 * through, and a recovery flow that sets a PIN — that is the shape of thing
 * that becomes scattered handlers and then becomes focus bugs.
 *
 * # Two events that both look like "back"
 *
 * The owner found this one on a real install, 2026-08-22: *"When i press
 * somebody else in the login screen, it deletes typed pin one by one and then
 * goes to selecting user."*
 *
 * There was one `back` event doing two unrelated jobs. On the keyboard,
 * Backspace means **rub out the last digit**, and rubbing out the last digit of
 * an empty pad sensibly drops back to the list. A button labelled *Somebody
 * else* means **I am not this person** — one whole decision, with nothing
 * gradual about it. Sharing an event made the button behave like four presses
 * of Backspace, so it took four taps to do the thing written on it.
 *
 * They are `back` (the rubber) and `cancel` (the decision) now, and every
 * button that means "leave" sends `cancel`.
 */

import type { PersonView } from '../ipc/generated/PersonView';

/**
 * **How long a PIN is. Not a minimum, not a maximum — the length.**
 *
 * `mb_auth::pin::PIN_DIGITS` is the authority and holds the same number; this
 * is what lets the pad ignore the fifth keypress instead of sending it to Rust
 * to be refused.
 *
 * It was `MIN_PIN = 4` with `MAX_PIN = 8`, and the gap between them was the bug
 * the owner hit: eight circles drawn on a four-digit pad, and a keypad that
 * kept taking digits after the PIN was finished. See `mb_auth::pin::PIN_DIGITS`
 * for the whole argument.
 */
export const PIN_DIGITS = 4;

/** Which part of the recovery flow is on screen. See `Mode`. */
export type RecoverStep = 'code' | 'who' | 'pin' | 'again';

export type Mode =
  /** Choosing who you are — by tapping a name, or by typing a staff code. */
  | { kind: 'who'; typed: string }
  /** Typing the PIN. */
  | { kind: 'pin'; person: PersonView; digits: string }
  /**
   * **The recovery flow, as four steps rather than one screen.**
   *
   * The owner, 2026-08-22: *"Forgotton pin also not working, i typed recovery
   * code, but cant even type new pin."* That was exact, and the cause was
   * structural: this mode held the code, the person and the new PIN all at
   * once, and the screen drew them all at once. The recovery-code box took
   * focus, so every digit typed went into the box; the new PIN had no field and
   * no keypad, only a row of bullets reporting a number there was no way to
   * enter. The flow could be started and could never be finished.
   *
   * One thing at a time fixes that rather than patching it: while `step` is
   * `'pin'` or `'again'` there is no text box on screen at all, so a digit has
   * nowhere to go except here.
   *
   * `again` is the new PIN typed a second time. A PIN goes in as dots and is
   * stored scrambled, so one slipped finger is otherwise discovered by somebody
   * who then cannot sign in — which is the exact situation this flow exists to
   * get out of. Set-up already asks twice (`FirstRun`); so does this.
   */
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
  /**
   * **Who the recovery code may set a PIN for.** Rust's list, not a filter of
   * `people` — see `LockState::recoverable`. A manager who has no PIN is on
   * this list and not on that one, and they are exactly the person who needs
   * the recovery code most.
   */
  recoverable: readonly PersonView[];
  /** Whether this shop has a recovery code to offer at all. */
  canRecover: boolean;
  /** The last thing Rust said, shown under the pad. */
  problem: string | null;
  /** True while a command is in flight — the pad stops accepting digits. */
  busy: boolean;
  /** Commands the screen has not performed yet. See the note above. */
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
  /** *Somebody else*, *Back*. Leaves the step whole — never a digit at a time. */
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

/*
 * **Who the recovery code is allowed to set a PIN for is not decided here.**
 *
 * There was a `canBeRecovered(people)` in this file that filtered on
 * `staff.manage`. It was deleted rather than kept, because a second copy of a
 * permission rule in TypeScript is how the two halves of a product end up
 * disagreeing and nobody notices the looser one — and this one was already
 * wrong in a way that mattered: it filtered `people`, which is the sign-in
 * list, so a manager with no PIN was invisible to the flow that exists to give
 * them one.
 *
 * Rust computes `LockState::recoverable` from the same rule
 * `recover_with_code_on` enforces, and it arrives as `State.recoverable`.
 */

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
      // **T3, on the lock screen.** Somebody who was chosen and has since been
      // suspended must not be left standing in front of a pad that can never
      // succeed — deactivating takes effect on the next action, not the next
      // shift.
      const mode = state.mode;
      const carried = {
        ...state,
        people: event.people,
        recoverable: event.recoverable,
        canRecover: event.canRecover,
      };
      // Which list somebody has to still be on depends on what they are in the
      // middle of: signing in, or having a PIN set for them.
      const gone = (id: string, from: readonly PersonView[]) =>
        !from.some((p) => p.id === id);

      if (mode.kind === 'pin' && gone(mode.person.id, event.people)) {
        return { ...carried, mode: { kind: 'who', typed: '' } };
      }
      // The same fact inside the recovery flow: somebody suspended halfway
      // through a reset must not still be the person a new PIN is written to.
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
      // **In the recovery flow, choosing a person means "this new PIN is
      // theirs"** — not "open the pad for them". Getting this wrong threw the
      // recovery flow back to the sign-in pad the moment somebody picked who
      // they were resetting, which is a dead end with a code already typed.
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
      // Locked out: the pad does not open at all. Typing a PIN and only then
      // being told to wait is the version that makes people press harder.
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
        // **The fifth keypress does nothing.** The owner, 2026-08-22: *"if i
        // keep pressing number it goes beyond 4 numbers and i could type upto
        // 8."* The ceiling here used to be `MAX_PIN`, which was eight. It is
        // the length of a PIN now, so there is no fifth digit to hold.
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
      // Backspace, and only Backspace. One character. See the note at the top
      // of this file for why no button sends this.
      if (state.mode.kind === 'pin') {
        if (state.mode.digits === '') {
          // Backspace on an empty pad goes back to the list, which is how
          // somebody who tapped the wrong name gets out without the mouse.
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
        // On the code box the browser is already editing the text itself, and
        // on the list there is nothing to rub out.
        return state;
      }
      return state;
    }

    case 'cancel': {
      // **One tap leaves, whatever has been typed.** The whole reason this is
      // not `back`.
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
        // **Stepping back clears both pads, always.** A half-typed PIN sitting
        // behind a Back button is the same hazard the sign-in pad clears on a
        // failure — and going back to the list means the PIN may be about to
        // belong to somebody else entirely, so keeping it is worse than idle.
        // The code survives, because walking back to it is how a mistyped
        // character gets fixed and throwing it away would be the dead end this
        // flow was in to begin with.
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
        // **Type a staff code and press Enter** — the same trick as the
        // billing screen's table numbers, and for the same reason: a keyboard
        // person should never have to reach for the list.
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
          // Said here rather than after a round trip, because it is a fact
          // about the shape and not about the PIN. The refusal that matters —
          // "wrong PIN" — is always Rust's.
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
        // The digits are cleared on a failure. Leaving them would let somebody
        // walk up to a pad with three of four digits already typed.
        //
        // A failed recovery goes back to the code box rather than to the pad:
        // what Rust refused is nearly always the code, and the code is the one
        // thing in this flow somebody can get wrong off a printed slip.
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

/**
 * **The recovery flow's Next button**, which asks a different question at every
 * step. Split out because four steps' worth of checks inside the `submit` arm is
 * how the old single-screen version got unreadable enough to hide the fact that
 * it could not be completed at all.
 */
function submitRecovery(state: State, mode: Extract<Mode, { kind: 'recover' }>): State {
  switch (mode.step) {
    case 'code': {
      if (mode.code.trim() === '') {
        return { ...state, problem: 'Type the recovery code from the printed slip.' };
      }
      // The code itself is NOT checked here. Only Rust holds the hash, and a
      // screen that could decide a code was wrong is a screen that could decide
      // one was right.
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
        // Back to the first pad with both cleared, rather than leaving one of
        // two disagreeing PINs standing for the next attempt to agree with.
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
      // Only the two pads take loose digits. On the code step the box has focus
      // and the browser is already typing into it; on the list a digit means
      // nothing.
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
    // Escape clears what has been typed. It does NOT close the lock screen:
    // there is nothing behind it, and a lock somebody can press Escape past is
    // not a lock.
    if (state.mode.kind === 'pin') {
      return { ...state, mode: { ...state.mode, digits: '' }, problem: null };
    }
    // Out of the recovery flow, not one step back — Escape has always meant
    // "clear what I typed", and a half-finished reset is not a thing to keep.
    return { ...state, mode: { kind: 'who', typed: '' }, problem: null };
  }
  return state;
}

/** Commands the screen has not performed yet, and the state with them taken. */
export function take(state: State): [State, readonly Command[]] {
  if (state.pending.length === 0) return [state, []];
  return [{ ...state, pending: [] }, state.pending.map((q) => q.command)];
}
