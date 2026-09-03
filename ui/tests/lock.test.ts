/** The lock screen's keyboard, headless. */

import { describe, expect, it } from 'vitest';

import {
  PIN_DIGITS,
  initial,
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
/** Somebody the recovery code is actually allowed to touch. */
const MEENA = person('staff_3', 'Meena', {
  role: 'Owner',
  permissions: ['bill.create', 'staff.manage'],
});

function drive(events: readonly Event[], from?: State): State {
  return events.reduce(reduce, from ?? withPeople());
}

/** The shop as Rust hands it over: two lists, not one filtered twice. */
function withPeople(
  people: readonly PersonView[] = [REKHA, RAVI, MEENA],
  recoverable: readonly PersonView[] = people.filter((p) =>
    p.permissions.includes('staff.manage'),
  ),
  lastSignedIn: string | null = null,
): State {
  return reduce(initial(), {
    kind: 'people',
    people,
    recoverable,
    canRecover: true,
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
      { kind: 'people', people: [REKHA, RAVI, MEENA], recoverable: [MEENA], canRecover: true, lastSignedIn: 'staff_1' },
    ]);
    expect(marked(state)).toBe('staff_2');
  });

  it('falls back when the marked person is suspended', () => {
    const state = drive([
      { kind: 'choose', person: RAVI },
      { kind: 'people', people: [REKHA, MEENA], recoverable: [MEENA], canRecover: true, lastSignedIn: null },
    ]);
    expect(marked(state)).toBe('staff_1');
  });

  it('narrows the list by name and moves the mark onto what is left', () => {
    const state = drive([{ kind: 'typed', text: 'ra' }]);
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

  it('Escape abandons a half-finished reset rather than stepping back through it', () => {
    const state = drive([
      { kind: 'start-recovery' },
      { kind: 'typed', text: 'ABCDE-FGHJK' },
      { kind: 'submit' },
      { kind: 'choose', person: MEENA },
      ...type('12'),
      { kind: 'key', key: 'Escape' },
    ]);
    expect(state.mode.kind).toBe('pin');
  });
});

/** The way back in. */
describe('the way back in', () => {
  const startedWithACode = (): State =>
    drive([
      { kind: 'start-recovery' },
      { kind: 'typed', text: 'ABCDE-FGHJK' },
      { kind: 'submit' },
    ]);

  it('starts by asking for the code, and nothing else', () => {
    const state = drive([{ kind: 'start-recovery' }]);
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('code');
  });

  it('will not move on without a code', () => {
    const state = drive([{ kind: 'start-recovery' }, { kind: 'submit' }]);
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('code');
    expect(state.problem).toContain('recovery code');
  });

  it('never decides whether the code itself is right', () => {
    // Only Rust holds the hash.
    const state = startedWithACode();
    expect(state.problem).toBeNull();
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('who');
  });

  it('offers only the people Rust will accept', () => {
    const state = drive([{ kind: 'choose', person: MEENA }], startedWithACode());
    expect(state.recoverable.map((p) => p.id)).toEqual(['staff_3']);
  });

  /** The lockout this list exists to prevent. */
  it('offers a manager who has no PIN — they are who the code is FOR', () => {
    const pinless = person('staff_4', 'Nadia', {
      role: 'Owner',
      hasPin: false,
      permissions: ['bill.create', 'staff.manage'],
    });
    // Rekha can sign in and cannot be recovered; Nadia is the other way round.
    const shop = withPeople([REKHA], [pinless]);
    const state = drive(
      [{ kind: 'start-recovery' }, { kind: 'typed', text: 'ABCDE-FGHJK' }, { kind: 'submit' }],
      shop,
    );
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('who');
    expect(state.recoverable.map((p) => p.name)).toEqual(['Nadia']);
  });

  it('says so when there is nobody this code could help', () => {
    const state = drive(
      [{ kind: 'start-recovery' }, { kind: 'typed', text: 'ABCDE-FGHJK' }, { kind: 'submit' }],
      withPeople([REKHA, RAVI], []),
    );
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('code');
    expect(state.problem).toContain('manages staff');
  });

  it('takes a new PIN from the keypad once a person is chosen', () => {
    const state = drive([{ kind: 'choose', person: MEENA }, ...type('24')], startedWithACode());
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('pin');
    expect(state.mode.kind === 'recover' && state.mode.newPin).toBe('24');
  });

  it('holds the new PIN to four digits too', () => {
    const state = drive([{ kind: 'choose', person: MEENA }, ...type('24681357')], startedWithACode());
    expect(state.mode.kind === 'recover' && state.mode.newPin).toBe('2468');
  });

  it('will not move on from a short PIN', () => {
    const state = drive(
      [{ kind: 'choose', person: MEENA }, ...type('246'), { kind: 'submit' }],
      startedWithACode(),
    );
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('pin');
    expect(state.problem).toBe('A PIN is 4 digits.');
  });

  it('asks for it a second time, and sends nothing until the two agree', () => {
    let state = drive(
      [{ kind: 'choose', person: MEENA }, ...type('2468'), { kind: 'submit' }],
      startedWithACode(),
    );
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('again');

    state = drive([...type('2469'), { kind: 'submit' }], state);
    expect(state.pending).toHaveLength(0);
    expect(state.problem).toContain('not the same');
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('pin');
    expect(state.mode.kind === 'recover' && state.mode.newPin).toBe('');
  });

  it('sends the code, the person and the new PIN together', () => {
    const state = drive(
      [
        { kind: 'choose', person: MEENA },
        ...type('2468'),
        { kind: 'submit' },
        ...type('2468'),
        { kind: 'submit' },
      ],
      startedWithACode(),
    );
    expect(take(state)[1]).toEqual([
      { do: 'recover', code: 'ABCDE-FGHJK', staffId: 'staff_3', newPin: '2468' },
    ]);
    expect(state.busy).toBe(true);
  });

  it('walks back one step at a time, and out', () => {
    let state = drive([{ kind: 'choose', person: MEENA }, ...type('2468')], startedWithACode());
    state = reduce(state, { kind: 'cancel' });
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('who');
    expect(state.mode.kind === 'recover' && state.mode.newPin).toBe('');
    state = reduce(state, { kind: 'cancel' });
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('code');
    // The code is still there — walking back to it is how somebody fixes a mistyped character.
    expect(state.mode.kind === 'recover' && state.mode.code).toBe('ABCDE-FGHJK');
    state = reduce(state, { kind: 'cancel' });
    expect(state.mode.kind).toBe('pin');
    expect(marked(state)).toBe('staff_1');
  });

  it('sends a refusal back to the code box, where the mistake usually is', () => {
    const state = drive(
      [
        { kind: 'choose', person: MEENA },
        ...type('2468'),
        { kind: 'submit' },
        ...type('2468'),
        { kind: 'submit' },
        { kind: 'failed', message: 'That is not this shop’s recovery code.' },
      ],
      startedWithACode(),
    );
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('code');
    expect(state.mode.kind === 'recover' && state.mode.newPin).toBe('');
    expect(state.busy).toBe(false);
    expect(state.problem).toContain('recovery code');
  });

  it('does not write a PIN to somebody suspended halfway through', () => {
    const state = drive(
      [
        { kind: 'choose', person: MEENA },
        ...type('2468'),
        { kind: 'people', people: [REKHA, RAVI], recoverable: [], canRecover: true, lastSignedIn: null },
      ],
      startedWithACode(),
    );
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('who');
    expect(state.mode.kind === 'recover' && state.mode.person).toBeNull();
  });
});
