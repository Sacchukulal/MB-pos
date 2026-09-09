/** The lock screen's keyboard, headless. */

import { describe, expect, it } from 'vitest';

import {
  PIN_DIGITS,
  initial,
  proofOf,
  reduce,
  shown,
  take,
  type Event,
  type State,
} from '../src/auth/keyboard';
import type { PersonView } from '../src/ipc/generated/PersonView';

function person(id: string, name: string, extra: Partial<PersonView> = {}): PersonView {
  return {
    id,
    name,
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

const REKHA = person('staff_1', 'Rekha');
const RAVI = person('staff_2', 'Ravi');
const MEENA = person('staff_3', 'Meena', {
  role: 'Owner',
  permissions: ['bill.create', 'staff.manage'],
});

function drive(events: readonly Event[], from?: State): State {
  return events.reduce(reduce, from ?? withPeople());
}

/** The shop as Rust hands it over: who can sign in, and the owner's name. */
function withPeople(
  people: readonly PersonView[] = [REKHA, RAVI, MEENA],
  owner: string | null = 'Meena',
  lastSignedIn: string | null = null,
): State {
  return reduce(initial(), {
    kind: 'people',
    people,
    owner,
    lastSignedIn,
  });
}

function type(digits: string): Event[] {
  return [...digits].map((digit) => ({ kind: 'key', key: digit }) as Event);
}

function marked(state: State): string | null {
  return state.mode.kind === 'pin' ? (state.mode.person?.id ?? null) : null;
}

function digits(state: State): string {
  return state.mode.kind === 'pin' ? state.mode.digits : '';
}

function step(state: State): string | null {
  return state.mode.kind === 'reset' ? state.mode.step : null;
}

function newPin(state: State): string | null {
  return state.mode.kind === 'reset' ? state.mode.newPin : null;
}

describe('the mark: who the pad is for', () => {
  it('starts on whoever signed in last', () => {
    expect(marked(withPeople([REKHA, RAVI, MEENA], undefined, 'staff_2'))).toBe('staff_2');
  });

  it('starts on the first name when nobody has signed in here yet', () => {
    expect(marked(withPeople())).toBe('staff_1');
  });

  it('starts on the first name when the last person is gone from the list', () => {
    expect(marked(withPeople([REKHA, RAVI], undefined, 'staff_9'))).toBe('staff_1');
  });

  it('moves down and up with the arrows, and stops at the ends', () => {
    let state = drive([{ kind: 'key', key: 'ArrowDown' }]);
    expect(marked(state)).toBe('staff_2');
    state = drive([{ kind: 'key', key: 'ArrowDown' }, { kind: 'key', key: 'ArrowDown' }], state);
    expect(marked(state)).toBe('staff_3');
    state = drive(
      [{ kind: 'key', key: 'ArrowUp' }, { kind: 'key', key: 'ArrowUp' }, { kind: 'key', key: 'ArrowUp' }],
      state,
    );
    expect(marked(state)).toBe('staff_1');
  });

  it('moves onto whoever was tapped', () => {
    expect(marked(drive([{ kind: 'choose', person: MEENA }]))).toBe('staff_3');
  });

  it('drops the digits typed for the last person when it moves', () => {
    const state = drive([...type('12'), { kind: 'key', key: 'ArrowDown' }]);
    expect(digits(state)).toBe('');
  });

  it('stays where it is when Rust re-sends the list', () => {
    const state = drive([
      { kind: 'choose', person: RAVI },
      { kind: 'people', people: [REKHA, RAVI, MEENA], owner: 'Meena', lastSignedIn: 'staff_1' },
    ]);
    expect(marked(state)).toBe('staff_2');
  });

  it('falls back when the marked person is suspended', () => {
    const state = drive([
      { kind: 'choose', person: RAVI },
      { kind: 'people', people: [REKHA, MEENA], owner: 'Meena', lastSignedIn: null },
    ]);
    expect(marked(state)).toBe('staff_1');
  });

  it('narrows the list by name and moves the mark onto what is left', () => {
    const state = drive([{ kind: 'typed', field: 'name', text: 'ra' }]);
    expect(shown(state.people, 'ra').map((p) => p.id)).toEqual(['staff_2']);
    expect(marked(state)).toBe('staff_2');
  });

  it('marks nobody in an empty shop, and sends nothing', () => {
    const state = drive([...type('1234'), { kind: 'submit' }], withPeople([]));
    expect(marked(state)).toBeNull();
    expect(state.pending).toHaveLength(0);
  });
});

describe('a PIN is four digits, and the pad is built out of that', () => {
  it('takes digits and nothing else', () => {
    expect(digits(drive(type('12a3')))).toBe('123');
  });

  it('signs in by itself on the fourth digit', () => {
    const state = drive(type('1234'));
    expect(take(state)[1]).toEqual([{ do: 'sign-in', staffId: 'staff_1', pin: '1234' }]);
    expect(state.busy).toBe(true);
    expect(PIN_DIGITS).toBe(4);
  });

  it('ignores the fifth keypress, and the twentieth', () => {
    const state = drive(type('12345678901234567890'));
    expect(digits(state)).toBe('1234');
    expect(state.pending).toHaveLength(1);
  });

  it('refuses to send three digits on Enter, without a round trip', () => {
    const state = drive([...type('123'), { kind: 'submit' }]);
    expect(state.pending).toHaveLength(0);
    expect(state.problem).toBe('A PIN is 4 digits.');
  });

  it('never decides for itself that a PIN is wrong', () => {
    // The ONLY refusal this file may produce is about the shape.
    const state = drive(type('9999'));
    expect(state.problem).toBeNull();
    expect(take(state)[1]).toHaveLength(1);
  });

  it('clears the digits when Rust says no', () => {
    // Otherwise somebody walks up to a pad with three of four digits already in.
    const after = drive([...type('1234'), { kind: 'failed', message: 'Wrong PIN. Try again.' }]);
    expect(digits(after)).toBe('');
    expect(after.problem).toBe('Wrong PIN. Try again.');
    expect(after.busy).toBe(false);
  });

  it('does not take digits while a sign-in is in flight', () => {
    const busy = drive([...type('1234'), ...type('7')]);
    expect(digits(busy)).toBe('1234');
  });

  it('will not take a PIN for somebody who is locked out', () => {
    // Typing a PIN and THEN being told to wait is the version that makes people press harder.
    const locked = person('staff_9', 'Anil', {
      lockedOut: 'Wrong PIN. Try again in 26 seconds.',
    });
    const state = drive(type('1'), withPeople([locked]));
    expect(digits(state)).toBe('');
    expect(state.problem).toBe('Wrong PIN. Try again in 26 seconds.');
  });
});

describe('Backspace rubs out; Escape clears', () => {
  it('Backspace takes digits back one at a time, and stops at none', () => {
    let state = drive(type('12'));
    state = reduce(state, { kind: 'back' });
    expect(digits(state)).toBe('1');
    state = reduce(state, { kind: 'back' });
    state = reduce(state, { kind: 'back' });
    expect(digits(state)).toBe('');
    expect(marked(state)).toBe('staff_1');
  });

  it('Escape clears what was typed and stays on the lock screen', () => {
    const state = drive([...type('123'), { kind: 'key', key: 'Escape' }]);
    expect(digits(state)).toBe('');
    expect(state.mode.kind).toBe('pin');
  });

  it('the C key empties the pad and keeps the same person marked', () => {
    let state = drive([{ kind: 'choose', person: RAVI }, ...type('123')]);
    state = reduce(state, { kind: 'clear' });
    expect(digits(state)).toBe('');
    expect(marked(state)).toBe('staff_2');
    expect(state.mode.kind).toBe('pin');
    // Nothing was sent: clearing is not a submit and not a cancel.
    expect(take(state)[1]).toEqual([]);
  });

  it('the C key on a reset pad empties that pad without stepping back', () => {
    const state = drive([...proved(), ...type('12'), { kind: 'clear' }]);
    expect(step(state)).toBe('pin');
    expect(newPin(state)).toBe('');
  });

  it('Escape abandons a half-finished reset rather than stepping back through it', () => {
    const state = drive([...proved(), ...type('12'), { kind: 'key', key: 'Escape' }]);
    expect(state.mode.kind).toBe('pin');
  });
});

/** The proof step filled in with the key, and Next pressed. */
function proved(): Event[] {
  return [
    { kind: 'start-reset' },
    { kind: 'typed', field: 'key', text: 'MB-STUB-0001' },
    { kind: 'submit' },
  ];
}

/** The owner's way back in. */
describe('the way back in', () => {
  it('starts by asking for the proof, and nothing else', () => {
    const state = drive([{ kind: 'start-reset' }]);
    expect(step(state)).toBe('prove');
  });

  it('is not offered in a shop with no owner row', () => {
    const state = drive([{ kind: 'start-reset' }], withPeople([REKHA], null));
    expect(state.mode.kind).toBe('pin');
  });

  it('will not move on with nothing typed', () => {
    const state = drive([{ kind: 'start-reset' }, { kind: 'submit' }]);
    expect(step(state)).toBe('prove');
    expect(state.problem).toContain('licence key');
  });

  it('will not move on with a password and no email', () => {
    const state = drive([
      { kind: 'start-reset' },
      { kind: 'typed', field: 'password', text: 'correct-horse' },
      { kind: 'submit' },
    ]);
    expect(step(state)).toBe('prove');
  });

  it('never decides whether the proof itself is right', () => {
    // Only Rust asks the cloud, and only Rust holds the key.
    const state = drive(proved());
    expect(state.problem).toBeNull();
    expect(step(state)).toBe('pin');
  });

  it('reads the key as the proof when one was pasted, else the account', () => {
    const byKey = drive([
      { kind: 'start-reset' },
      { kind: 'typed', field: 'email', text: 'meena@example.in' },
      { kind: 'typed', field: 'password', text: 'correct-horse' },
      { kind: 'typed', field: 'key', text: ' mb-stub-0001 ' },
    ]);
    expect(byKey.mode.kind === 'reset' && proofOf(byKey.mode)).toEqual({
      by: 'key',
      key: 'mb-stub-0001',
    });
    const byPassword = drive([
      { kind: 'start-reset' },
      { kind: 'typed', field: 'email', text: ' meena@example.in ' },
      { kind: 'typed', field: 'password', text: 'correct-horse' },
    ]);
    expect(byPassword.mode.kind === 'reset' && proofOf(byPassword.mode)).toEqual({
      by: 'password',
      email: 'meena@example.in',
      password: 'correct-horse',
    });
  });

  it('takes a new PIN from the keypad once the proof is in', () => {
    const state = drive([...proved(), ...type('24')]);
    expect(step(state)).toBe('pin');
    expect(newPin(state)).toBe('24');
  });

  it('ignores loose digits on the proof step, where the boxes are the browser’s', () => {
    const state = drive([{ kind: 'start-reset' }, ...type('24')]);
    expect(step(state)).toBe('prove');
    expect(newPin(state)).toBe('');
  });

  it('holds the new PIN to four digits too', () => {
    const state = drive([...proved(), ...type('24681357')]);
    expect(newPin(state)).toBe('2468');
  });

  it('will not move on from a short PIN', () => {
    const state = drive([...proved(), ...type('246'), { kind: 'submit' }]);
    expect(step(state)).toBe('pin');
    expect(state.problem).toBe('A PIN is 4 digits.');
  });

  it('asks for it a second time, and sends nothing until the two agree', () => {
    let state = drive([...proved(), ...type('2468'), { kind: 'submit' }]);
    expect(step(state)).toBe('again');

    state = drive([...type('2469'), { kind: 'submit' }], state);
    expect(state.pending).toHaveLength(0);
    expect(state.problem).toContain('not the same');
    expect(step(state)).toBe('pin');
    expect(newPin(state)).toBe('');
  });

  it('sends the proof and the new PIN together', () => {
    const state = drive([
      ...proved(),
      ...type('2468'),
      { kind: 'submit' },
      ...type('2468'),
      { kind: 'submit' },
    ]);
    expect(take(state)[1]).toEqual([
      { do: 'reset', proof: { by: 'key', key: 'MB-STUB-0001' }, newPin: '2468' },
    ]);
    expect(state.busy).toBe(true);
  });

  it('walks back one step at a time, and out', () => {
    let state = drive([...proved(), ...type('2468'), { kind: 'submit' }, ...type('24')]);
    state = reduce(state, { kind: 'cancel' });
    expect(step(state)).toBe('pin');
    expect(newPin(state)).toBe('');
    state = reduce(state, { kind: 'cancel' });
    expect(step(state)).toBe('prove');
    // The key is still there — walking back to it is how somebody fixes a mistyped character.
    expect(state.mode.kind === 'reset' && state.mode.key).toBe('MB-STUB-0001');
    state = reduce(state, { kind: 'cancel' });
    expect(state.mode.kind).toBe('pin');
    expect(marked(state)).toBe('staff_1');
  });

  it('sends a refusal back to the proof, where the mistake usually is', () => {
    const state = drive([
      ...proved(),
      ...type('2468'),
      { kind: 'submit' },
      ...type('2468'),
      { kind: 'submit' },
      { kind: 'failed', message: 'That is not this shop’s licence key.' },
    ]);
    expect(step(state)).toBe('prove');
    expect(newPin(state)).toBe('');
    expect(state.busy).toBe(false);
    expect(state.problem).toContain('licence key');
  });

  it('goes back to the sign-in screen once Rust has signed the owner in', () => {
    const state = drive([
      ...proved(),
      ...type('2468'),
      { kind: 'submit' },
      ...type('2468'),
      { kind: 'submit' },
      { kind: 'done' },
    ]);
    expect(state.mode.kind).toBe('pin');
    expect(state.busy).toBe(false);
    expect(state.pending).toHaveLength(0);
  });
});
