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
 * code, six to eight digits with no auto-submit, a countdown that must not be
 * typed through, and a recovery flow that sets a PIN — that is the shape of
 * thing that becomes scattered handlers and then becomes focus bugs.
 */

import type { PersonView } from '../ipc/generated/PersonView';

/**
 * How many digits the pad will hold. `mb_auth::pin` is the authority; this
 * stops somebody typing forever, and the refusal itself comes from Rust.
 *
 * **A PIN is four digits** (owner, 2026-08-17). `MAX_PIN` stays at eight so a
 * shop whose staff already have longer PINs can still sign in — see
 * `mb_auth::pin::MIN_DIGITS` for the whole argument. The pad draws `MIN_PIN`
 * dots and grows only if somebody types past them, so what a person sees is a
 * four-digit pad.
 */
export const MAX_PIN = 8;
export const MIN_PIN = 4;

export type Mode =
  /** Choosing who you are — by tapping a name, or by typing a staff code. */
  | { kind: 'who'; typed: string }
  /** Typing the PIN. */
  | { kind: 'pin'; person: PersonView; digits: string }
  /** The recovery flow: the code, then a new PIN, then the printed slip. */
  | { kind: 'recover'; code: string; person: PersonView | null; newPin: string }
  | { kind: 'recovered'; freshCode: string };

export interface State {
  mode: Mode;
  people: readonly PersonView[];
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
  | { kind: 'people'; people: readonly PersonView[]; canRecover: boolean }
  | { kind: 'choose'; person: PersonView }
  | { kind: 'digit'; digit: string }
  | { kind: 'typed'; text: string }
  | { kind: 'back' }
  | { kind: 'submit' }
  | { kind: 'start-recovery' }
  | { kind: 'recovered'; freshCode: string }
  | { kind: 'failed'; message: string }
  | { kind: 'done' };

export function initial(): State {
  return {
    mode: { kind: 'who', typed: '' },
    people: [],
    canRecover: false,
    problem: null,
    busy: false,
    pending: [],
    seq: 0,
  };
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

export function reduce(state: State, event: Event): State {
  switch (event.kind) {
    case 'people': {
      // **T3, on the lock screen.** Somebody who was chosen and has since been
      // suspended must not be left standing in front of a pad that can never
      // succeed — deactivating takes effect on the next action, not the next
      // shift.
      const mode = state.mode;
      const stranded =
        mode.kind === 'pin' && !event.people.some((p) => p.id === mode.person.id);
      return {
        ...state,
        people: event.people,
        canRecover: event.canRecover,
        mode: stranded ? { kind: 'who', typed: '' } : mode,
      };
    }

    case 'choose':
      // **In the recovery flow, choosing a person means "this new PIN is
      // theirs"** — not "open the pad for them". Getting this wrong threw the
      // recovery flow back to the sign-in pad the moment somebody picked who
      // they were resetting, which is a dead end with a code already typed.
      if (state.mode.kind === 'recover') {
        return { ...state, mode: { ...state.mode, person: event.person }, problem: null };
      }
      // Locked out: the pad does not open at all. Typing six digits and then
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
      if (state.mode.kind !== 'pin') return state;
      if (!/^[0-9]$/.test(event.digit)) return state;
      if (state.mode.digits.length >= MAX_PIN) return state;
      return {
        ...state,
        problem: null,
        mode: { ...state.mode, digits: state.mode.digits + event.digit },
      };
    }

    case 'typed':
      if (state.mode.kind === 'who') {
        return { ...state, mode: { kind: 'who', typed: event.text } };
      }
      if (state.mode.kind === 'recover') {
        return { ...state, mode: { ...state.mode, code: event.text } };
      }
      return state;

    case 'back': {
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
      if (state.mode.kind === 'who' && state.mode.typed !== '') {
        return { ...state, mode: { kind: 'who', typed: state.mode.typed.slice(0, -1) } };
      }
      if (state.mode.kind === 'recover' || state.mode.kind === 'recovered') {
        return { ...state, mode: { kind: 'who', typed: '' }, problem: null };
      }
      return state;
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
        if (state.mode.digits.length < MIN_PIN) {
          // Said here rather than after a round trip, because it is a fact
          // about the shape and not about the PIN. The refusal that matters —
          // "wrong PIN" — is always Rust's.
          return { ...state, problem: `A PIN is ${MIN_PIN} to ${MAX_PIN} digits.` };
        }
        return queue(state, {
          do: 'sign-in',
          staffId: state.mode.person.id,
          pin: state.mode.digits,
        });
      }

      if (state.mode.kind === 'recover') {
        const { code, person, newPin } = state.mode;
        if (!person) return { ...state, problem: 'Choose who this new PIN is for.' };
        if (code.trim() === '') return { ...state, problem: 'Type the recovery code.' };
        if (newPin.length < MIN_PIN) {
          return { ...state, problem: `A PIN is ${MIN_PIN} to ${MAX_PIN} digits.` };
        }
        return queue(state, {
          do: 'recover',
          code,
          staffId: person.id,
          newPin,
        });
      }
      return state;
    }

    case 'start-recovery':
      return {
        ...state,
        mode: { kind: 'recover', code: '', person: null, newPin: '' },
        problem: null,
      };

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
        // walk up to a pad with five of six digits already typed.
        mode:
          state.mode.kind === 'pin'
            ? { ...state.mode, digits: '' }
            : state.mode,
      };

    case 'done':
      return { ...initial(), people: state.people, canRecover: state.canRecover };

    case 'key':
      return key(state, event.key);
  }
}

function key(state: State, pressed: string): State {
  if (/^[0-9]$/.test(pressed)) {
    if (state.mode.kind === 'pin') return reduce(state, { kind: 'digit', digit: pressed });
    if (state.mode.kind === 'recover') {
      return {
        ...state,
        mode: {
          ...state.mode,
          newPin:
            state.mode.newPin.length >= MAX_PIN
              ? state.mode.newPin
              : state.mode.newPin + pressed,
        },
      };
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
    if (state.mode.kind === 'pin') return { ...state, mode: { ...state.mode, digits: '' }, problem: null };
    if (state.mode.kind === 'who') return { ...state, mode: { kind: 'who', typed: '' }, problem: null };
    return { ...state, mode: { kind: 'who', typed: '' }, problem: null };
  }
  return state;
}

/** Commands the screen has not performed yet, and the state with them taken. */
export function take(state: State): [State, readonly Command[]] {
  if (state.pending.length === 0) return [state, []];
  return [
    { ...state, pending: [] },
    state.pending.map((q) => q.command),
  ];
}
