/** The lock screen. */

import { useCallback, useEffect, useReducer, useRef } from 'react';

import { Button, Input, Keypad, Logo, Scroller, cx } from '../kit';
import { call, isUiError } from '../ipc/call';
import type { PersonView } from '../ipc/generated/PersonView';
import {
  PIN_DIGITS,
  initial,
  reduce,
  shown,
  take,
  type Event,
  type State,
} from './keyboard';

import './auth.css';

export interface LockProps {
  /** Who can sign in: active, and holding a PIN. */
  people: readonly PersonView[];
  /**
   * Who the recovery code may set a PIN for — Rust's `LockState::recoverable`, which is not a
   * subset of `people`.
   */
  recoverable: readonly PersonView[];
  canRecover: boolean;
  /** Who signed in last at this counter, so the mark starts on them. */
  lastSignedIn: string | null;
  /** Called when somebody got in. */
  onSignedIn: () => void;
}

/** Keys the window takes even while a text box has focus. */
const ALWAYS_OURS = new Set(['Enter', 'ArrowUp', 'ArrowDown']);

export function Lock({ people, recoverable, canRecover, lastSignedIn, onSignedIn }: LockProps) {
  const [state, dispatch] = useReducer(reduce, undefined, initial);
  const runningSeq = useRef(0);

  useEffect(() => {
    dispatch({ kind: 'people', people, recoverable, canRecover, lastSignedIn });
  }, [people, recoverable, canRecover, lastSignedIn]);

  // The commands ride in the state.
  useEffect(() => {
    if (state.pending.length === 0) return;
    const [, commands] = take(state);
    const latest = state.pending[state.pending.length - 1];
    if (!latest || latest.seq <= runningSeq.current) return;
    runningSeq.current = latest.seq;

    void (async () => {
      for (const command of commands) {
        try {
          if (command.do === 'sign-in') {
            await call('login', { staffId: command.staffId, pin: command.pin });
            dispatch({ kind: 'done' });
            onSignedIn();
          } else {
            const fresh = await call('recover_with_code', {
              code: command.code,
              staffId: command.staffId,
              newPin: command.newPin,
            });
            dispatch({ kind: 'recovered', freshCode: fresh });
          }
        } catch (cause) {
          const message = isUiError(cause)
            ? cause.message
            : 'That could not be done. Try again.';
          dispatch({ kind: 'failed', message });
        }
      }
    })();
  }, [state, onSignedIn]);

  // The whole window is the keyboard.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Tab') return;
      // Let the text inputs have their own typing; the reducer takes the rest.
      const target = event.target as HTMLElement | null;
      if (target?.tagName === 'INPUT' && !ALWAYS_OURS.has(event.key)) return;
      // The arrows move the mark, not the page.
      if (event.key === 'ArrowUp' || event.key === 'ArrowDown') event.preventDefault();
      dispatch({ kind: 'key', key: event.key });
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const onPad = useCallback((key: string) => {
    if (key === 'Backspace') dispatch({ kind: 'back' });
    else if (key !== '.') dispatch({ kind: 'digit', digit: key });
  }, []);

  const mode = state.mode;

  const problem = state.problem ? (
    <p className="mb-lock__problem" role="alert">
      {state.problem}
    </p>
  ) : null;

  const forgot =
    canRecover && mode.kind !== 'recover' && mode.kind !== 'recovered' ? (
      <Button variant="quiet" size="sm" onClick={() => dispatch({ kind: 'start-recovery' })}>
        Forgotten your PIN?
      </Button>
    ) : null;

  if (mode.kind === 'pin') {
    return (
      <div className="mb-lock" role="dialog" aria-modal="true" aria-label="Sign in">
        <div className="mb-lock__card mb-lock__card--split">
          <People
            people={state.people}
            typed={mode.typed}
            marked={mode.person}
            onType={(text) => dispatch({ kind: 'typed', text })}
            onChoose={(person) => dispatch({ kind: 'choose', person })}
          />
          <div className="mb-lock__pad">
            <Logo size="lg" />
            <SignIn person={mode.person} digits={mode.digits} busy={state.busy} onPad={onPad} />
            {problem}
            {forgot}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="mb-lock" role="dialog" aria-modal="true" aria-label="Sign in">
      <div className="mb-lock__card">
        <Logo size="lg" />
        {mode.kind === 'recovered' ? (
          <Recovered code={mode.freshCode} onDone={() => dispatch({ kind: 'done' })} />
        ) : (
          <Recover state={state} dispatch={dispatch} onPad={onPad} />
        )}
        {problem}
        {forgot}
      </div>
    </div>
  );
}

/** The PIN itself: four dots and a pad. */
function Pad({
  digits,
  busy,
  onPad,
  label,
}: {
  digits: string;
  busy: boolean;
  onPad: (key: string) => void;
  label: string;
}) {
  return (
    <>
      <div className="mb-lock__dots" aria-label={`${digits.length} of ${PIN_DIGITS} digits typed`}>
        {Array.from({ length: PIN_DIGITS }, (_, index) => (
          <span
            key={index}
            className={cx('mb-lock__dot', index < digits.length && 'mb-lock__dot--filled')}
          />
        ))}
      </div>

      {/* No decimal point: a PIN has none, and the key sat exactly where a thumb lands. */}
      <Keypad onPress={onPad} disabled={busy} dot={false} />
      <span className="mb-visually-hidden">{label}</span>
    </>
  );
}

/** The people down the side, one of them marked. */
function People({
  people,
  typed,
  marked,
  onType,
  onChoose,
}: {
  people: readonly PersonView[];
  typed: string;
  marked: PersonView | null;
  onType: (text: string) => void;
  onChoose: (person: PersonView) => void;
}) {
  const list = shown(people, typed);
  const rows = useRef<HTMLDivElement>(null);

  // The mark stays in view as the arrows move it.
  useEffect(() => {
    rows.current
      ?.querySelector<HTMLElement>('[aria-current="true"]')
      ?.scrollIntoView?.({ block: 'nearest' });
  }, [marked]);

  return (
    <div className="mb-lock__people-column">
      <h1 className="mb-lock__title">Who is at the counter?</h1>
      {people.length > 6 ? (
        <Input
          label="Name"
          value={typed}
          autoComplete="off"
          onChange={(event) => onType(event.target.value)}
        />
      ) : null}
      <Scroller inset className="mb-lock__people" ref={rows}>
        {list.length === 0 ? (
          <p className="mb-muted">
            {people.length === 0
              ? 'Nobody here has a PIN yet. Somebody who manages staff can set one.'
              : 'Nobody here goes by that. Clear the box to see everybody.'}
          </p>
        ) : (
          list.map((person) => (
            <Button
              key={person.id}
              wide
              list
              variant={person.lockedOut ? 'quiet' : 'secondary'}
              className="mb-lock__person"
              aria-current={person.id === marked?.id ? 'true' : undefined}
              onClick={() => onChoose(person)}
            >
              <span className="mb-lock__name">{person.name}</span>
              <span className="mb-lock__role">{person.lockedOut ?? person.role ?? ''}</span>
            </Button>
          ))
        )}
      </Scroller>
      <p className="mb-lock__hint">Up and down pick a name. The fourth digit signs you in.</p>
    </div>
  );
}

/** The marked person and their pad. */
function SignIn({
  person,
  digits,
  busy,
  onPad,
}: {
  person: PersonView | null;
  digits: string;
  busy: boolean;
  onPad: (key: string) => void;
}) {
  if (!person) {
    return <h1 className="mb-lock__title">Nobody can sign in yet</h1>;
  }
  return (
    <>
      <h1 className="mb-lock__title">{person.name}</h1>
      <p className={cx('mb-muted', person.lockedOut && 'mb-lock__problem')}>
        {person.lockedOut ?? person.role ?? ''}
      </p>
      <Pad
        digits={digits}
        busy={busy || person.lockedOut !== null}
        onPad={onPad}
        label={`${person.name}'s PIN`}
      />
    </>
  );
}

/** The way back in, as four screens. */
function Recover({
  state,
  dispatch,
  onPad,
}: {
  state: State;
  dispatch: (event: Event) => void;
  onPad: (key: string) => void;
}) {
  if (state.mode.kind !== 'recover') return null;
  const { step, code, person, newPin, again } = state.mode;
  const candidates = state.recoverable;

  const actions = (next: string) => (
    <div className="mb-lock__actions">
      <Button variant="quiet" onClick={() => dispatch({ kind: 'cancel' })} disabled={state.busy}>
        Back
      </Button>
      <Button variant="primary" onClick={() => dispatch({ kind: 'submit' })} disabled={state.busy}>
        {state.busy ? 'Setting…' : next}
      </Button>
    </div>
  );

  if (step === 'code') {
    return (
      <>
        <h1 className="mb-lock__title">Forgotten PIN</h1>
        {/* mb-layout-allow: the lock screen IS this sentence — there is nothing else on it to ask from */}
        <p className="mb-muted">
          Type the recovery code from the slip that printed when this shop was
          set up. It can only set a PIN for somebody who manages staff, and
          using it prints a new code.
        </p>
        <Input
          label="Recovery code"
          value={code}
          autoFocus
          autoComplete="off"
          spellCheck={false}
          placeholder="ABCDE-FGHJK"
          onChange={(event) => dispatch({ kind: 'typed', text: event.target.value })}
        />
        {actions('Next')}
      </>
    );
  }

  if (step === 'who') {
    return (
      <>
        <h1 className="mb-lock__title">Whose PIN?</h1>
        {/* mb-layout-allow: the lock screen IS this sentence — there is nothing else on it to ask from */}
        <p className="mb-muted">
          {/* Only the people Rust will accept. */}
          The recovery code sets a PIN for somebody who manages staff.
        </p>
        <div className="mb-lock__people">
          {candidates.length === 0 ? (
            <p className="mb-muted">
              Nobody here manages staff, so this code has no PIN to set. Ring
              support, with your licence key to hand.
            </p>
          ) : (
            candidates.map((candidate) => (
              <Button
                key={candidate.id}
                wide
                variant={person?.id === candidate.id ? 'primary' : 'secondary'}
                className="mb-lock__person"
                onClick={() => dispatch({ kind: 'choose', person: candidate })}
              >
                <span className="mb-lock__name">{candidate.name}</span>
                <span className="mb-lock__role">{candidate.role ?? ''}</span>
              </Button>
            ))
          )}
        </div>
        {actions('Next')}
      </>
    );
  }

  const typing = step === 'pin' ? newPin : again;
  return (
    <>
      <h1 className="mb-lock__title">
        {step === 'pin' ? 'A new PIN' : 'The same PIN again'}
      </h1>
      {/* mb-layout-allow: the lock screen IS this sentence — there is nothing else on it to ask from */}
      <p className="mb-muted">
        {step === 'pin'
          ? `${person?.name ?? 'This person'} will sign in with these ${PIN_DIGITS} digits.`
          : 'Type it a second time, so one slipped finger does not lock them out.'}
      </p>

      <Pad
        digits={typing}
        busy={state.busy}
        onPad={onPad}
        label={step === 'pin' ? 'The new PIN' : 'The new PIN again'}
      />

      {actions(step === 'pin' ? 'Next' : 'Set the PIN')}
    </>
  );
}

function Recovered({ code, onDone }: { code: string; onDone: () => void }) {
  return (
    <>
      <h1 className="mb-lock__title">Write this down</h1>
      {/* mb-layout-allow: the lock screen IS this sentence — there is nothing else on it to ask from */}
      <p className="mb-muted">
        This is the shop&rsquo;s new recovery code. The old one no longer works,
        and this one is shown here once and printed. Keep the slip somewhere
        only you can reach.
      </p>
      <p className="mb-lock__code">{code}</p>
      <Button variant="primary" wide onClick={onDone}>
        I have written it down
      </Button>
    </>
  );
}
