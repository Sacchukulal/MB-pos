/**
 * **The lock screen** — audit C1's visible half.
 *
 * It covers the shell completely, and two things stay visible behind it on
 * purpose: the shop's name and the print-queue indicator. A bill that printed
 * wrong while the screen was locked is still the shop's problem (audit D4), and
 * a queue nobody can see is that finding all over again.
 *
 * # Identity first, then the PIN
 *
 * BACKEND-**D1**: v1 tried the typed PIN against *every* active staff row, so
 * with ten staff a random guess was ten times likelier to land. Here the
 * cashier says who they are — by tapping a name or typing their staff code —
 * and only then types a PIN, which is one verification against one row.
 *
 * # One screen for two shapes of shop
 *
 * Two people get two big buttons; thirty get a code box and a filtered list.
 * The same screen does both, because two screens is two things to keep working.
 */

import { useCallback, useEffect, useReducer, useRef } from 'react';

import { Button, Input, Keypad } from '../kit';
import { call, isUiError } from '../ipc/call';
import type { PersonView } from '../ipc/generated/PersonView';
import { MAX_PIN, MIN_PIN, initial, reduce, take, type State } from './keyboard';

import './auth.css';

export interface LockProps {
  people: readonly PersonView[];
  canRecover: boolean;
  /** Called when somebody got in. The shell reloads itself from Rust. */
  onSignedIn: () => void;
}

export function Lock({ people, canRecover, onSignedIn }: LockProps) {
  const [state, dispatch] = useReducer(reduce, undefined, initial);
  const runningSeq = useRef(0);

  useEffect(() => {
    dispatch({ kind: 'people', people, canRecover });
  }, [people, canRecover]);

  // **The commands ride in the state.** StrictMode double-invokes the reducer,
  // so performing them inside it would sign somebody in twice (P10 found this
  // the expensive way, with a beer at 440.00).
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

  // The whole window is the keyboard. There is nothing else on screen to focus.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Tab') return;
      // Let the text inputs have their own typing; the reducer takes the rest.
      const target = event.target as HTMLElement | null;
      if (target?.tagName === 'INPUT' && event.key !== 'Enter') return;
      dispatch({ kind: 'key', key: event.key });
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const onPad = useCallback((key: string) => {
    if (key === 'Backspace') dispatch({ kind: 'back' });
    else if (key !== '.') dispatch({ kind: 'digit', digit: key });
  }, []);

  return (
    <div className="mb-lock" role="dialog" aria-modal="true" aria-label="Sign in">
      <div className="mb-lock__card">
        {state.mode.kind === 'recovered' ? (
          <Recovered code={state.mode.freshCode} onDone={() => dispatch({ kind: 'done' })} />
        ) : state.mode.kind === 'recover' ? (
          <Recover state={state} dispatch={dispatch} />
        ) : state.mode.kind === 'pin' ? (
          <PinPad
            person={state.mode.person}
            digits={state.mode.digits}
            busy={state.busy}
            onPad={onPad}
            onBack={() => dispatch({ kind: 'back' })}
            onSubmit={() => dispatch({ kind: 'submit' })}
          />
        ) : (
          <Who
            people={state.people}
            typed={state.mode.typed}
            onType={(text) => dispatch({ kind: 'typed', text })}
            onChoose={(person) => dispatch({ kind: 'choose', person })}
          />
        )}

        {state.problem ? (
          <p className="mb-lock__problem" role="alert">
            {state.problem}
          </p>
        ) : null}

        {canRecover && state.mode.kind !== 'recover' && state.mode.kind !== 'recovered' ? (
          <Button
            variant="quiet"
            small
            onClick={() => dispatch({ kind: 'start-recovery' })}
          >
            Forgotten your PIN?
          </Button>
        ) : null}
      </div>
    </div>
  );
}

function Who({
  people,
  typed,
  onType,
  onChoose,
}: {
  people: readonly PersonView[];
  typed: string;
  onType: (text: string) => void;
  onChoose: (person: PersonView) => void;
}) {
  const wanted = typed.trim().toLowerCase();
  const shown = wanted === ''
    ? people
    : people.filter(
        (p) =>
          p.name.toLowerCase().includes(wanted) ||
          (p.code ?? '').toLowerCase() === wanted,
      );

  return (
    <>
      <h1 className="mb-lock__title">Who is at the counter?</h1>
      {people.length > 6 ? (
        <Input
          label="Name or staff code"
          value={typed}
          autoFocus
          onChange={(event) => onType(event.target.value)}
        />
      ) : null}
      <div className="mb-lock__people">
        {shown.length === 0 ? (
          /* **Two different emptinesses, and they were saying the same thing**
             — P30.5. Typing a name that matches nobody used to answer "Nobody
             here has a PIN yet", which is a claim about the SHOP rather than
             about what was typed, and it is alarming as well as untrue. Found
             by typing a PIN into the search box by mistake. */
          <p className="mb-muted">
            {people.length === 0
              ? 'Nobody here has a PIN yet. Somebody who manages staff can set one.'
              : 'Nobody here goes by that. Clear the box to see everybody.'}
          </p>
        ) : (
          shown.map((person) => (
            <Button
              key={person.id}
              wide
              variant={person.lockedOut ? 'quiet' : 'secondary'}
              className="mb-lock__person"
              onClick={() => onChoose(person)}
            >
              <span className="mb-lock__name">{person.name}</span>
              <span className="mb-lock__role">
                {person.lockedOut ?? person.role ?? ''}
              </span>
            </Button>
          ))
        )}
      </div>
    </>
  );
}

function PinPad({
  person,
  digits,
  busy,
  onPad,
  onBack,
  onSubmit,
}: {
  person: PersonView;
  digits: string;
  busy: boolean;
  onPad: (key: string) => void;
  onBack: () => void;
  onSubmit: () => void;
}) {
  return (
    <>
      <h1 className="mb-lock__title">{person.name}</h1>
      <p className="mb-muted">{person.role ?? ''}</p>

      {/* Dots, not digits. Somebody is always standing behind the counter. */}
      <div className="mb-lock__dots" aria-label={`${digits.length} digits typed`}>
        {Array.from({ length: MAX_PIN }, (_, index) => (
          <span
            key={index}
            className={[
              'mb-lock__dot',
              index < digits.length ? 'mb-lock__dot--filled' : '',
              index === MIN_PIN - 1 ? 'mb-lock__dot--six' : '',
            ]
              .filter(Boolean)
              .join(' ')}
          />
        ))}
      </div>

      <Keypad onPress={onPad} disabled={busy} />

      <div className="mb-lock__actions">
        <Button variant="quiet" onClick={onBack}>
          Somebody else
        </Button>
        <Button variant="primary" onClick={onSubmit} disabled={busy}>
          {busy ? 'Checking…' : 'Sign in'}
        </Button>
      </div>
    </>
  );
}

function Recover({
  state,
  dispatch,
}: {
  state: State;
  dispatch: (event: import('./keyboard').Event) => void;
}) {
  if (state.mode.kind !== 'recover') return null;
  const { code, person, newPin } = state.mode;
  return (
    <>
      <h1 className="mb-lock__title">Forgotten PIN</h1>
      <p className="mb-muted">
        Type the recovery code from the slip that printed when this shop was set
        up. It can only set a PIN for somebody who manages staff, and using it
        prints a new code.
      </p>
      <Input
        label="Recovery code"
        value={code}
        autoFocus
        onChange={(event) => dispatch({ kind: 'typed', text: event.target.value })}
      />
      <div className="mb-lock__people">
        {state.people.map((candidate) => (
          <Button
            key={candidate.id}
            wide
            variant={person?.id === candidate.id ? 'primary' : 'secondary'}
            onClick={() => dispatch({ kind: 'choose', person: candidate })}
          >
            {candidate.name}
          </Button>
        ))}
      </div>
      <p className="mb-muted">
        New PIN: {'•'.repeat(newPin.length)} — type {MIN_PIN} to {MAX_PIN} digits.
      </p>
      <div className="mb-lock__actions">
        <Button variant="quiet" onClick={() => dispatch({ kind: 'back' })}>
          Back
        </Button>
        <Button variant="primary" onClick={() => dispatch({ kind: 'submit' })}>
          Set the PIN
        </Button>
      </div>
    </>
  );
}

function Recovered({ code, onDone }: { code: string; onDone: () => void }) {
  return (
    <>
      <h1 className="mb-lock__title">Write this down</h1>
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
