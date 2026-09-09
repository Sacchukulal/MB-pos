/** The lock screen's keyboard, as a pure reducer. */

import type { OwnerProof } from '../ipc/generated/OwnerProof';
import type { PersonView } from '../ipc/generated/PersonView';

/** How long a PIN is. */
export const PIN_DIGITS = 4;

/** Which part of the owner's reset is on screen. */
export type ResetStep = 'prove' | 'pin' | 'again';

/** The text boxes the reset asks to be typed into, and the name filter on the sign-in side. */
export type Field = 'name' | 'email' | 'password' | 'key';

export type Mode =
  /** Signing in: the people down one side, the pad for whoever is marked. */
  | { kind: 'pin'; person: PersonView | null; digits: string; typed: string }
  /**
   * The owner's forgotten PIN: prove the account or the licence key, then a new PIN twice.
   * Rust checks the proof and sets the owner's PIN; the screen never decides who the owner is.
   */
  | {
      kind: 'reset';
      step: ResetStep;
      email: string;
      password: string;
      key: string;
      newPin: string;
      again: string;
    };

export interface State {
  mode: Mode;
  /** Who can sign in — everybody active who has a PIN. */
  people: readonly PersonView[];
  /** The owner's name when the shop has one, so "forgotten your PIN?" has somebody to reset. */
  owner: string | null;
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
  | { do: 'reset'; proof: OwnerProof; newPin: string };

export interface Queued {
  seq: number;
  command: Command;
}

export type Event =
  | { kind: 'key'; key: string }
  | {
      kind: 'people';
      people: readonly PersonView[];
      owner: string | null;
      lastSignedIn: string | null;
    }
  | { kind: 'choose'; person: PersonView }
  /** The arrow keys: the mark moves one row. */
  | { kind: 'move'; by: 1 | -1 }
  | { kind: 'digit'; digit: string }
  | { kind: 'typed'; field: Field; text: string }
  /** Backspace. One digit. */
  | { kind: 'back' }
  /** The pad's C key: everything typed on that pad goes, and nothing else moves. */
  | { kind: 'clear' }
  /** Back, or a full clear of the pad. Leaves the step whole — never a digit at a time. */
  | { kind: 'cancel' }
  | { kind: 'submit' }
  | { kind: 'start-reset' }
  | { kind: 'failed'; message: string }
  | { kind: 'done' };

export function initial(): State {
  return {
    mode: signIn(null),
    people: [],
    owner: null,
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

/** A fresh reset, at its first step. */
function reset(): Mode {
  return {
    kind: 'reset',
    step: 'prove',
    email: '',
    password: '',
    key: '',
    newPin: '',
    again: '',
  };
}

/** The proof the boxes hold: the key when one was pasted, else the account. */
export function proofOf(mode: Extract<Mode, { kind: 'reset' }>): OwnerProof | null {
  if (mode.key.trim() !== '') return { by: 'key', key: mode.key.trim() };
  if (mode.email.trim() !== '' && mode.password !== '') {
    return { by: 'password', email: mode.email.trim(), password: mode.password };
  }
  return null;
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
        owner: event.owner,
        lastSignedIn: event.lastSignedIn,
      };
      if (mode.kind === 'pin') {
        // The mark stays on whoever it was on, as long as they are still on the list.
        const fresh = mode.person && event.people.find((p) => p.id === mode.person?.id);
        if (fresh) return { ...carried, mode: { ...mode, person: fresh } };
        return {
          ...carried,
          mode: signIn(startingMark(event.people, event.lastSignedIn), mode.typed),
        };
      }
      return carried;
    }

    case 'choose':
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

      // The same ceiling on both of the reset's pads.
      const field = padField(state.mode);
      if (!field) return state;
      const current = state.mode[field];
      if (current.length >= PIN_DIGITS) return state;
      return {
        ...state,
        problem: null,
        mode: { ...state.mode, [field]: current + event.digit },
      };
    }

    case 'typed':
      if (state.mode.kind === 'pin') {
        if (event.field !== 'name') return state;
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
      if (state.mode.step === 'prove' && event.field !== 'name') {
        return { ...state, mode: { ...state.mode, [event.field]: event.text } };
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
      const field = padField(state.mode);
      // On the proof boxes the browser is already editing the text itself.
      if (!field) return state;
      const current = state.mode[field];
      if (current === '') return reduce(state, { kind: 'cancel' });
      return { ...state, mode: { ...state.mode, [field]: current.slice(0, -1) } };
    }

    case 'clear': {
      // The pad is emptied where it stands: the same person, the same step.
      if (state.busy) return state;
      if (state.mode.kind === 'pin') {
        return { ...state, problem: null, mode: { ...state.mode, digits: '' } };
      }
      const field = padField(state.mode);
      if (!field) return state;
      return { ...state, problem: null, mode: { ...state.mode, [field]: '' } };
    }

    case 'cancel': {
      // One tap leaves, whatever has been typed.
      if (state.busy) return state;

      if (state.mode.kind === 'reset') {
        const mode = state.mode;
        const previous: Record<ResetStep, ResetStep | null> = {
          prove: null, // out of the flow altogether
          pin: 'prove',
          again: 'pin',
        };
        const step = previous[mode.step];
        if (step === null) return leave(state);
        // Stepping back clears both pads, always.
        return {
          ...state,
          problem: null,
          mode: { ...mode, step, newPin: '', again: '' },
        };
      }

      if (state.mode.digits === '' && state.problem === null) return state;
      return { ...state, mode: { ...state.mode, digits: '' }, problem: null };
    }

    case 'submit': {
      if (state.busy) return state;
      if (state.mode.kind === 'pin') return submitPin(state, state.mode);
      return submitReset(state, state.mode);
    }

    case 'start-reset':
      if (state.owner === null) return state;
      return { ...state, mode: reset(), problem: null };

    case 'failed':
      return {
        ...state,
        busy: false,
        pending: [],
        problem: event.message,
        // The digits are cleared on a failure; a refused proof goes back to its boxes.
        mode:
          state.mode.kind === 'pin'
            ? { ...state.mode, digits: '' }
            : { ...state.mode, step: 'prove', newPin: '', again: '' },
      };

    case 'done':
      return {
        ...initial(),
        people: state.people,
        owner: state.owner,
        lastSignedIn: state.lastSignedIn,
        mode: signIn(startingMark(state.people, state.lastSignedIn)),
      };

    case 'key':
      return key(state, event.key);
  }
}

/** Which of the reset's two pads is on screen, if either. */
function padField(mode: Mode): 'newPin' | 'again' | null {
  if (mode.kind !== 'reset') return null;
  if (mode.step === 'pin') return 'newPin';
  if (mode.step === 'again') return 'again';
  return null;
}

/** Out of the reset and back to the sign-in screen, whatever was typed. */
function leave(state: State): State {
  return {
    ...state,
    mode: signIn(startingMark(state.people, state.lastSignedIn)),
    problem: null,
  };
}

/** The reset's Next button, which asks a different question at every step. */
function submitReset(state: State, mode: Extract<Mode, { kind: 'reset' }>): State {
  switch (mode.step) {
    case 'prove':
      // Whether the proof is RIGHT is Rust's to say; only that something was typed is checked
      // here.
      if (proofOf(mode) === null) {
        return {
          ...state,
          problem:
            'Type the email and password of your Magic Bill account, or paste the licence key.',
        };
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
      const proof = proofOf(mode);
      if (proof === null) {
        return { ...state, problem: 'Prove it is you first.', mode: { ...mode, step: 'prove' } };
      }
      return queue(state, { do: 'reset', proof, newPin: mode.newPin });
    }
  }
}

function key(state: State, pressed: string): State {
  if (/^[0-9]$/.test(pressed)) {
    // Only the pads take loose digits; the proof boxes are the browser's.
    if (state.mode.kind === 'pin' || padField(state.mode)) {
      return reduce(state, { kind: 'digit', digit: pressed });
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
    // Out of the reset, not one step back: a half-finished reset is not a thing to keep.
    return leave(state);
  }
  return state;
}

/** Commands the screen has not performed yet, and the state with them taken. */
export function take(state: State): [State, readonly Command[]] {
  if (state.pending.length === 0) return [state, []];
  return [{ ...state, pending: [] }, state.pending.map((q) => q.command)];
}
