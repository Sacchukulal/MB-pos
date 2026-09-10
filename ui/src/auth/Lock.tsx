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
  /** The owner's name when the shop has one — Rust's `LockState::owner`, whose PIN a reset sets. */
  owner: string | null;
  /** Who signed in last at this counter, so the mark starts on them. */
  lastSignedIn: string | null;
  /** Called when somebody got in. */
  onSignedIn: () => void;
}

/** Keys the window takes even while a text box has focus. */
const ALWAYS_OURS = new Set(['Enter', 'ArrowUp', 'ArrowDown']);

export function Lock({ people, owner, lastSignedIn, onSignedIn }: LockProps) {
  const [state, dispatch] = useReducer(reduce, undefined, initial);
  const runningSeq = useRef(0);

  useEffect(() => {
    dispatch({ kind: 'people', people, owner, lastSignedIn });
  }, [people, owner, lastSignedIn]);

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
          } else {
            // Rust checks the proof, sets the owner's PIN and signs them in with it.
            await call('reset_owner_pin', { proof: command.proof, newPin: command.newPin });
          }
          dispatch({ kind: 'done' });
          onSignedIn();
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
    else if (key === 'Clear') dispatch({ kind: 'clear' });
    else if (key !== '.') dispatch({ kind: 'digit', digit: key });
  }, []);

  const mode = state.mode;

  const problem = state.problem ? (
    <p className="mb-lock__problem" role="alert">
      {state.problem}
    </p>
  ) : null;

  if (mode.kind === 'pin') {
    return (
      <div className="mb-lock" role="dialog" aria-modal="true" aria-label="Sign in">
        <div className="mb-lock__card mb-lock__card--split">
          <People
            people={state.people}
            typed={mode.typed}
            marked={mode.person}
            onType={(text) => dispatch({ kind: 'typed', field: 'name', text })}
            onChoose={(person) => dispatch({ kind: 'choose', person })}
          />
          <Scroller inset className="mb-lock__pad">
            <Logo size="lg" />
            <SignIn person={mode.person} digits={mode.digits} busy={state.busy} onPad={onPad} />
            {problem}
            {/* The owner's way back in. Staff PINs are the owner's to reset, from Staff. */}
            {state.owner !== null ? (
              <Button variant="quiet" size="sm" onClick={() => dispatch({ kind: 'start-reset' })}>
                Forgotten your PIN?
              </Button>
            ) : null}
          </Scroller>
        </div>
      </div>
    );
  }

  return (
    <div className="mb-lock" role="dialog" aria-modal="true" aria-label="Sign in">
      <div className="mb-lock__card">
        <Logo size="lg" />
        {/* The body scrolls on a short window; the card never runs off it. */}
        <Scroller inset className="mb-lock__body">
          <Reset state={state} dispatch={dispatch} onPad={onPad} />
          {problem}
        </Scroller>
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

/** The owner's way back in, as three screens: the proof, the new PIN, the same PIN again. */
function Reset({
  state,
  dispatch,
  onPad,
}: {
  state: State;
  dispatch: (event: Event) => void;
  onPad: (key: string) => void;
}) {
  if (state.mode.kind !== 'reset') return null;
  const { step, email, password, key, newPin, again } = state.mode;
  const owner = state.owner ?? 'The owner';

  const actions = (next: string) => (
    <div className="mb-lock__actions">
      <Button variant="quiet" onClick={() => dispatch({ kind: 'cancel' })} disabled={state.busy}>
        Back
      </Button>
      <Button variant="primary" onClick={() => dispatch({ kind: 'submit' })} disabled={state.busy}>
        {state.busy ? 'Checking…' : next}
      </Button>
    </div>
  );

  if (step === 'prove') {
    return (
      <>
        <h1 className="mb-lock__title">Forgotten PIN</h1>
        {/* mb-layout-allow: the lock screen IS this sentence — there is nothing else on it to ask from */}
        <p className="mb-muted">
          A new PIN for {owner}. Prove it is you with your Magic Bill account, or with the
          licence key from your dashboard. Staff PINs are set from the Staff screen.
        </p>
        <div className="mb-lock__proof">
          <Input
            label="Email"
            type="email"
            autoComplete="username"
            autoFocus
            value={email}
            placeholder="you@example.com"
            onChange={(event) => dispatch({ kind: 'typed', field: 'email', text: event.target.value })}
          />
          <Input
            label="Password"
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(event) =>
              dispatch({ kind: 'typed', field: 'password', text: event.target.value })
            }
          />
          <p className="mb-lock__or" role="separator">
            or the licence key
          </p>
          <Input
            label="Licence key"
            autoComplete="off"
            spellCheck={false}
            value={key}
            placeholder="MB-XXXX-XXXX-XXXX"
            onChange={(event) => dispatch({ kind: 'typed', field: 'key', text: event.target.value })}
          />
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
          ? `${owner} will sign in with these ${PIN_DIGITS} digits.`
          : 'Type it a second time, so one slipped finger does not lock you out.'}
      </p>

      <Pad
        digits={typing}
        busy={state.busy}
        onPad={onPad}
        label={step === 'pin' ? 'The new PIN' : 'The new PIN again'}
      />

      {actions(step === 'pin' ? 'Next' : 'Set the PIN and sign in')}
    </>
  );
}
