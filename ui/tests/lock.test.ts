/**
 * **The lock screen's keyboard, headless.**
 *
 * Same standard as `keyboard.test.ts`: no DOM, no IPC, no React — a reducer, a
 * table of events, and assertions on the state and on what it asked for.
 *
 * The thing being tested is not really "does Enter work". It is that **the
 * screen cannot let somebody in** — every refusal that matters comes from Rust,
 * and this file proves the reducer never decides one for itself.
 */

import { describe, expect, it } from 'vitest';

import {
  MAX_PIN,
  MIN_PIN,
  initial,
  reduce,
  take,
  type Event,
  type State,
} from '../src/auth/keyboard';
import type { PersonView } from '../src/ipc/generated/PersonView';

function person(id: string, name: string, extra: Partial<PersonView> = {}): PersonView {
  return {
    id,
    name,
    code: null,
    role: 'Cashier',
    status: 'active',
    hasPin: true,
    lockedOut: null,
    permissions: ['bill.create'],
    maxDiscountBp: null,
    maxDiscount: null,
    ...extra,
  };
}

const REKHA = person('staff_1', 'Rekha', { code: 'R1' });
const RAVI = person('staff_2', 'Ravi', { code: 'R2' });

function drive(events: readonly Event[], from?: State): State {
  return events.reduce(reduce, from ?? withPeople());
}

function withPeople(people: readonly PersonView[] = [REKHA, RAVI]): State {
  return reduce(initial(), { kind: 'people', people, canRecover: true });
}

function type(digits: string): Event[] {
  return [...digits].map((digit) => ({ kind: 'key', key: digit }) as Event);
}

describe('choosing who you are', () => {
  it('opens the pad for whoever was tapped', () => {
    const state = drive([{ kind: 'choose', person: REKHA }]);
    expect(state.mode.kind).toBe('pin');
    if (state.mode.kind === 'pin') expect(state.mode.person.name).toBe('Rekha');
  });

  it('finds a person by their staff code and Enter', () => {
    // The same trick as the billing screen's table numbers: a keyboard person
    // never has to reach for the list.
    const state = drive([
      { kind: 'typed', text: 'r2' },
      { kind: 'submit' },
    ]);
    expect(state.mode.kind).toBe('pin');
    if (state.mode.kind === 'pin') expect(state.mode.person.id).toBe('staff_2');
  });

  it('says so when the code matches nobody, rather than doing nothing', () => {
    const state = drive([{ kind: 'typed', text: 'zz' }, { kind: 'submit' }]);
    expect(state.mode.kind).toBe('who');
    expect(state.problem).toContain('staff code');
  });

  it('will not open the pad for somebody who is locked out', () => {
    // Typing six digits and THEN being told to wait is the version that makes
    // people press harder.
    const locked = person('staff_3', 'Anil', {
      lockedOut: 'Wrong PIN. Try again in 26 seconds.',
    });
    const state = drive([{ kind: 'choose', person: locked }], withPeople([locked]));
    expect(state.mode.kind).toBe('who');
    expect(state.problem).toBe('Wrong PIN. Try again in 26 seconds.');
  });

  it('sends somebody home if they are suspended while standing at the pad', () => {
    // T3, on this screen: deactivating takes effect on the next action.
    const state = drive([
      { kind: 'choose', person: REKHA },
      { kind: 'people', people: [RAVI], canRecover: true },
    ]);
    expect(state.mode.kind).toBe('who');
  });
});

describe('typing the PIN', () => {
  it('takes digits and nothing else', () => {
    const state = drive([
      { kind: 'choose', person: REKHA },
      ...type('12a34'),
    ]);
    expect(state.mode.kind === 'pin' && state.mode.digits).toBe('1234');
  });

  it('stops at the longest a PIN can be', () => {
    const state = drive([{ kind: 'choose', person: REKHA }, ...type('1234567890')]);
    expect(state.mode.kind === 'pin' && state.mode.digits.length).toBe(MAX_PIN);
  });

  it('DOES NOT submit itself at six digits', () => {
    // A PIN may be eight long. An auto-submit at six would make those
    // impossible to type, which is the kind of thing found by a shop, not by us.
    const state = drive([{ kind: 'choose', person: REKHA }, ...type('123456')]);
    expect(state.pending).toHaveLength(0);
  });

  it('refuses to send fewer than the minimum, without a round trip', () => {
    const state = drive([
      { kind: 'choose', person: REKHA },
      ...type('123'),
      { kind: 'submit' },
    ]);
    expect(state.pending).toHaveLength(0);
    expect(state.problem).toContain(`${MIN_PIN}`);
  });

  it('asks Rust the moment there are enough digits', () => {
    const state = drive([
      { kind: 'choose', person: REKHA },
      ...type('123456'),
      { kind: 'submit' },
    ]);
    const [, commands] = take(state);
    expect(commands).toEqual([
      { do: 'sign-in', staffId: 'staff_1', pin: '123456' },
    ]);
    expect(state.busy).toBe(true);
  });

  it('never decides for itself that a PIN is wrong', () => {
    // The ONLY refusal this file may produce is about the shape. "Wrong PIN"
    // is Rust's word, always, because only Rust has the hash.
    const state = drive([
      { kind: 'choose', person: REKHA },
      ...type('999999'),
      { kind: 'submit' },
    ]);
    expect(state.problem).toBeNull();
    expect(take(state)[1]).toHaveLength(1);
  });

  it('clears the digits when Rust says no', () => {
    // Otherwise somebody walks up to a pad with five of six digits already in.
    const after = drive([
      { kind: 'choose', person: REKHA },
      ...type('123456'),
      { kind: 'submit' },
      { kind: 'failed', message: 'Wrong PIN. Try again.' },
    ]);
    expect(after.mode.kind === 'pin' && after.mode.digits).toBe('');
    expect(after.problem).toBe('Wrong PIN. Try again.');
    expect(after.busy).toBe(false);
  });

  it('takes digits back one at a time, then goes back to the list', () => {
    let state = drive([{ kind: 'choose', person: REKHA }, ...type('12')]);
    state = reduce(state, { kind: 'back' });
    expect(state.mode.kind === 'pin' && state.mode.digits).toBe('1');
    state = reduce(state, { kind: 'back' });
    state = reduce(state, { kind: 'back' });
    expect(state.mode.kind).toBe('who');
  });

  it('does not take digits while a sign-in is in flight', () => {
    const busy = drive([
      { kind: 'choose', person: REKHA },
      ...type('123456'),
      { kind: 'submit' },
      ...type('7'),
    ]);
    expect(busy.mode.kind === 'pin' && busy.mode.digits).toBe('123456');
  });
});

describe('Escape does not get past a lock', () => {
  it('clears what was typed and stays on the lock screen', () => {
    const state = drive([
      { kind: 'choose', person: REKHA },
      ...type('1234'),
      { kind: 'key', key: 'Escape' },
    ]);
    expect(state.mode.kind === 'pin' && state.mode.digits).toBe('');
  });

  it('has nowhere to go from the list either', () => {
    const state = drive([
      { kind: 'typed', text: 'abc' },
      { kind: 'key', key: 'Escape' },
    ]);
    expect(state.mode.kind).toBe('who');
    expect(state.mode.kind === 'who' && state.mode.typed).toBe('');
  });
});

describe('the way back in', () => {
  it('needs a code, a person and a long enough PIN before it asks Rust', () => {
    let state = drive([{ kind: 'start-recovery' }]);
    state = reduce(state, { kind: 'submit' });
    expect(state.problem).toContain('who this new PIN is for');
    expect(state.pending).toHaveLength(0);

    // **Choosing a person during recovery means "this new PIN is theirs"**,
    // not "open the sign-in pad for them". The first version did the latter,
    // which threw the flow away with the code already typed into it.
    state = reduce(state, { kind: 'choose', person: REKHA });
    expect(state.mode.kind).toBe('recover');
    expect(state.mode.kind === 'recover' && state.mode.person?.id).toBe('staff_1');
  });

  it('sends the code, the person and the new PIN together', () => {
    let state = drive([{ kind: 'start-recovery' }]);
    state = reduce(state, { kind: 'typed', text: 'ABCDE-FGHJK' });
    state = reduce(state, { kind: 'choose', person: REKHA });
    state = [...'246813'].reduce((s, d) => reduce(s, { kind: 'key', key: d }), state);
    state = reduce(state, { kind: 'submit' });
    expect(take(state)[1]).toEqual([
      { do: 'recover', code: 'ABCDE-FGHJK', staffId: 'staff_1', newPin: '246813' },
    ]);
  });

  it('shows the new code once and then goes back to the start', () => {
    let state = drive([{ kind: 'start-recovery' }]);
    state = reduce(state, { kind: 'recovered', freshCode: 'MNPQR-STUVW' });
    expect(state.mode).toEqual({ kind: 'recovered', freshCode: 'MNPQR-STUVW' });
    state = reduce(state, { kind: 'done' });
    expect(state.mode.kind).toBe('who');
    // The people survive `done`; they came from Rust and did not change.
    expect(state.people).toHaveLength(2);
  });
});

describe('the reducer has no side effects', () => {
  it('gives the same answer twice, which is what StrictMode does to it', () => {
    // P10's most expensive bug: commands performed inside the reducer ran
    // twice under StrictMode and one beer came out at 440.00.
    const before = drive([{ kind: 'choose', person: REKHA }, ...type('123456')]);
    const once = reduce(before, { kind: 'submit' });
    const twice = reduce(before, { kind: 'submit' });
    expect(once).toEqual(twice);
    // And the command is IN the state, keyed, rather than fired.
    expect(once.pending.map((p) => p.seq)).toEqual([1]);
  });

  it('never mutates the state it was given', () => {
    const before = withPeople();
    const snapshot = JSON.stringify(before);
    reduce(before, { kind: 'choose', person: REKHA });
    reduce(before, { kind: 'key', key: 'Enter' });
    expect(JSON.stringify(before)).toBe(snapshot);
  });
});
