/** The lock screen's keyboard, headless. */

import { describe, expect, it } from 'vitest';

import {
  PIN_DIGITS,
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
/** Somebody the recovery code is actually allowed to touch. */
const MEENA = person('staff_3', 'Meena', {
  code: 'M1',
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
): State {
  return reduce(initial(), { kind: 'people', people, recoverable, canRecover: true });
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
    // The same trick as the billing screen's table numbers: a keyboard person never has to
    // reach for the list.
    const state = drive([{ kind: 'typed', text: 'r2' }, { kind: 'submit' }]);
    expect(state.mode.kind).toBe('pin');
    if (state.mode.kind === 'pin') expect(state.mode.person.id).toBe('staff_2');
  });

  it('says so when the code matches nobody, rather than doing nothing', () => {
    const state = drive([{ kind: 'typed', text: 'zz' }, { kind: 'submit' }]);
    expect(state.mode.kind).toBe('who');
    expect(state.problem).toContain('staff code');
  });

  it('will not open the pad for somebody who is locked out', () => {
    // Typing a PIN and THEN being told to wait is the version that makes people press harder.
    const locked = person('staff_9', 'Anil', {
      lockedOut: 'Wrong PIN. Try again in 26 seconds.',
    });
    const state = drive([{ kind: 'choose', person: locked }], withPeople([locked]));
    expect(state.mode.kind).toBe('who');
    expect(state.problem).toBe('Wrong PIN. Try again in 26 seconds.');
  });

  it('sends somebody home if they are suspended while standing at the pad', () => {
    const state = drive([
      { kind: 'choose', person: REKHA },
      { kind: 'people', people: [RAVI], recoverable: [], canRecover: true },
    ]);
    expect(state.mode.kind).toBe('who');
  });
});

describe('a PIN is four digits, and the pad is built out of that', () => {
  it('takes digits and nothing else', () => {
    const state = drive([{ kind: 'choose', person: REKHA }, ...type('12a3')]);
    expect(state.mode.kind === 'pin' && state.mode.digits).toBe('123');
  });

  it('ignores the fifth keypress, and the twentieth', () => {
    const state = drive([{ kind: 'choose', person: REKHA }, ...type('12345678901234567890')]);
    expect(state.mode.kind === 'pin' && state.mode.digits).toBe('1234');
    expect(PIN_DIGITS).toBe(4);
  });

  it('DOES NOT submit itself when the fourth digit lands', () => {
    // Nothing asked for auto-submit, and a pad that fires on the fourth digit spends a lockout
    // attempt on a mistyped third one.
    const state = drive([{ kind: 'choose', person: REKHA }, ...type('1234')]);
    expect(state.pending).toHaveLength(0);
  });

  it('refuses to send three digits, without a round trip', () => {
    const state = drive([
      { kind: 'choose', person: REKHA },
      ...type('123'),
      { kind: 'submit' },
    ]);
    expect(state.pending).toHaveLength(0);
    expect(state.problem).toBe('A PIN is 4 digits.');
  });

  it('asks Rust once there are four', () => {
    const state = drive([
      { kind: 'choose', person: REKHA },
      ...type('1234'),
      { kind: 'submit' },
    ]);
    expect(take(state)[1]).toEqual([{ do: 'sign-in', staffId: 'staff_1', pin: '1234' }]);
    expect(state.busy).toBe(true);
  });

  it('never decides for itself that a PIN is wrong', () => {
    // The ONLY refusal this file may produce is about the shape.
    const state = drive([
      { kind: 'choose', person: REKHA },
      ...type('9999'),
      { kind: 'submit' },
    ]);
    expect(state.problem).toBeNull();
    expect(take(state)[1]).toHaveLength(1);
  });

  it('clears the digits when Rust says no', () => {
    // Otherwise somebody walks up to a pad with three of four digits already in.
    const after = drive([
      { kind: 'choose', person: REKHA },
      ...type('1234'),
      { kind: 'submit' },
      { kind: 'failed', message: 'Wrong PIN. Try again.' },
    ]);
    expect(after.mode.kind === 'pin' && after.mode.digits).toBe('');
    expect(after.problem).toBe('Wrong PIN. Try again.');
    expect(after.busy).toBe(false);
  });

  it('does not take digits while a sign-in is in flight', () => {
    const busy = drive([
      { kind: 'choose', person: REKHA },
      ...type('1234'),
      { kind: 'submit' },
      ...type('7'),
    ]);
    expect(busy.mode.kind === 'pin' && busy.mode.digits).toBe('1234');
  });
});

describe('Backspace rubs out; a button leaves', () => {
  it('Backspace takes digits back one at a time, then goes back to the list', () => {
    // This is the KEYBOARD, and one character at a time is what Backspace means everywhere else
    // on the machine.
    let state = drive([{ kind: 'choose', person: REKHA }, ...type('12')]);
    state = reduce(state, { kind: 'back' });
    expect(state.mode.kind === 'pin' && state.mode.digits).toBe('1');
    state = reduce(state, { kind: 'back' });
    state = reduce(state, { kind: 'back' });
    expect(state.mode.kind).toBe('who');
  });

  it('"Somebody else" leaves in ONE press, with a full pad', () => {
    const state = drive([{ kind: 'choose', person: REKHA }, ...type('1234'), { kind: 'cancel' }]);
    expect(state.mode.kind).toBe('who');
    expect(state.mode.kind === 'who' && state.mode.typed).toBe('');
  });

  it('leaves in one press from every part-typed pad, not just a full one', () => {
    for (const typed of ['', '1', '12', '123', '1234']) {
      const state = drive([
        { kind: 'choose', person: REKHA },
        ...type(typed),
        { kind: 'cancel' },
      ]);
      expect(state.mode.kind, `after typing ${typed.length} digits`).toBe('who');
    }
  });

  it('does not leave while Rust is being asked', () => {
    // The answer is already on its way.
    const state = drive([
      { kind: 'choose', person: REKHA },
      ...type('1234'),
      { kind: 'submit' },
      { kind: 'cancel' },
    ]);
    expect(state.mode.kind).toBe('pin');
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
    const state = drive([{ kind: 'typed', text: 'abc' }, { kind: 'key', key: 'Escape' }]);
    expect(state.mode.kind).toBe('who');
    expect(state.mode.kind === 'who' && state.mode.typed).toBe('');
  });

  it('abandons a half-finished reset rather than stepping back through it', () => {
    const state = drive([
      { kind: 'start-recovery' },
      { kind: 'typed', text: 'ABCDE-FGHJK' },
      { kind: 'submit' },
      { kind: 'choose', person: MEENA },
      ...type('12'),
      { kind: 'key', key: 'Escape' },
    ]);
    expect(state.mode.kind).toBe('who');
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
    // `recover_with_code_on` refuses anybody without `staff.manage`, so a list with Rekha on it
    // is a list that invites the refusal after the code has been spent.
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

  /** This is the bug, in one test. */
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
    // Both pads cleared, back to the first — so the next attempt cannot agree with the wrong
    // one of the two.
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
    // And the half-typed PIN did not survive the trip.
    expect(state.mode.kind === 'recover' && state.mode.newPin).toBe('');
    state = reduce(state, { kind: 'cancel' });
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('code');
    // The code is still there — walking back to it is how somebody fixes a mistyped character,
    // so throwing it away would be the dead end again.
    expect(state.mode.kind === 'recover' && state.mode.code).toBe('ABCDE-FGHJK');
    state = reduce(state, { kind: 'cancel' });
    expect(state.mode.kind).toBe('who');
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
        { kind: 'people', people: [REKHA, RAVI], recoverable: [], canRecover: true },
      ],
      startedWithACode(),
    );
    expect(state.mode.kind === 'recover' && state.mode.step).toBe('who');
    expect(state.mode.kind === 'recover' && state.mode.person).toBeNull();
  });

  it('shows the new code once and then goes back to the start', () => {
    let state = drive([{ kind: 'start-recovery' }]);
    state = reduce(state, { kind: 'recovered', freshCode: 'MNPQR-STUVW' });
    expect(state.mode).toEqual({ kind: 'recovered', freshCode: 'MNPQR-STUVW' });
    state = reduce(state, { kind: 'done' });
    expect(state.mode.kind).toBe('who');
    // The people survive `done`; they came from Rust and did not change.
    expect(state.people).toHaveLength(3);
  });
});

describe('the reducer has no side effects', () => {
  it('gives the same answer twice, which is what StrictMode does to it', () => {
    const before = drive([{ kind: 'choose', person: REKHA }, ...type('1234')]);
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
    reduce(before, { kind: 'cancel' });
    expect(JSON.stringify(before)).toBe(snapshot);
  });
});
