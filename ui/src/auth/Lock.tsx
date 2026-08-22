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
 *
 * # One pad, drawn once
 *
 * Sign-in and the two steps of the recovery flow all draw [`Pad`]. That is not
 * tidiness: the recovery flow's new-PIN step had *no* pad at all, only a row of
 * bullets counting digits there was no way to type, and it stayed that way
 * because nothing forced the two halves of this screen to look like each other.
 * One component means a change to how a PIN is entered cannot reach one flow
 * and miss the other.
 */

import { useCallback, useEffect, useReducer, useRef } from 'react';

import { Button, Input, Keypad } from '../kit';
import { call, isUiError } from '../ipc/call';
import type { PersonView } from '../ipc/generated/PersonView';
import {
  PIN_DIGITS,
  initial,
  reduce,
  take,
  type Event,
  type State,
} from './keyboard';

import './auth.css';

export interface LockProps {
  /** Who can sign in: active, and holding a PIN. */
  people: readonly PersonView[];
  /**
   * Who the recovery code may set a PIN for — Rust's `LockState::recoverable`,
   * which is **not** a subset of `people`. A manager with no PIN is on this
   * list and not on that one, and is precisely who the code is for.
   */
  recoverable: readonly PersonView[];
  canRecover: boolean;
  /** Called when somebody got in. The shell reloads itself from Rust. */
  onSignedIn: () => void;
}

export function Lock({ people, recoverable, canRecover, onSignedIn }: LockProps) {
  const [state, dispatch] = useReducer(reduce, undefined, initial);
  const runningSeq = useRef(0);

  useEffect(() => {
    dispatch({ kind: 'people', people, recoverable, canRecover });
  }, [people, recoverable, canRecover]);

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
      //
      // **This line is why the recovery flow could not be finished.** The
      // recovery-code box was on screen at the same time as the new PIN, and it
      // had focus, so every digit went into the box and none of them reached
      // the reducer. The flow is four steps now and the box shares a step with
      // nothing — see `keyboard.ts`. The rule itself is right and stays.
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

  const mode = state.mode;

  return (
    <div className="mb-lock" role="dialog" aria-modal="true" aria-label="Sign in">
      <div className="mb-lock__card">
        {mode.kind === 'recovered' ? (
          <Recovered code={mode.freshCode} onDone={() => dispatch({ kind: 'done' })} />
        ) : mode.kind === 'recover' ? (
          <Recover state={state} dispatch={dispatch} onPad={onPad} />
        ) : mode.kind === 'pin' ? (
          <PinPad
            person={mode.person}
            digits={mode.digits}
            busy={state.busy}
            onPad={onPad}
            onCancel={() => dispatch({ kind: 'cancel' })}
            onSubmit={() => dispatch({ kind: 'submit' })}
          />
        ) : (
          <Who
            people={state.people}
            typed={mode.typed}
            onType={(text) => dispatch({ kind: 'typed', text })}
            onChoose={(person) => dispatch({ kind: 'choose', person })}
          />
        )}

        {state.problem ? (
          <p className="mb-lock__problem" role="alert">
            {state.problem}
          </p>
        ) : null}

        {canRecover && mode.kind !== 'recover' && mode.kind !== 'recovered' ? (
          <Button variant="quiet" small onClick={() => dispatch({ kind: 'start-recovery' })}>
            Forgotten your PIN?
          </Button>
        ) : null}
      </div>
    </div>
  );
}

/**
 * **The PIN itself: four dots and a pad.**
 *
 * Dots, not digits. Somebody is always standing behind the counter.
 *
 * **Four of them, and only ever four.** It drew `MAX_PIN`, which was eight, so
 * a shop typing a four-digit PIN looked at eight circles with four filled —
 * which reads as a half-finished PIN every single time. The first fix drew
 * `Math.max(MIN_PIN, digits.length)`, and that still grew to eight the moment
 * somebody kept pressing, because the *rule* underneath was still a range. The
 * rule is one number now (`PIN_DIGITS`), so this can be written as one number
 * too, and there is no arithmetic left in it to be wrong.
 */
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
            className={[
              'mb-lock__dot',
              index < digits.length ? 'mb-lock__dot--filled' : '',
            ]
              .filter(Boolean)
              .join(' ')}
          />
        ))}
      </div>

      {/* No decimal point. A PIN has no decimal point, and the key sat exactly
          where a thumb lands. See `Keypad`. */}
      <Keypad onPress={onPad} disabled={busy} dot={false} />
      <span className="mb-visually-hidden">{label}</span>
    </>
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
  const shown =
    wanted === ''
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
  onCancel,
  onSubmit,
}: {
  person: PersonView;
  digits: string;
  busy: boolean;
  onPad: (key: string) => void;
  onCancel: () => void;
  onSubmit: () => void;
}) {
  return (
    <>
      <h1 className="mb-lock__title">{person.name}</h1>
      <p className="mb-muted">{person.role ?? ''}</p>

      <Pad digits={digits} busy={busy} onPad={onPad} label={`${person.name}'s PIN`} />

      <div className="mb-lock__actions">
        {/* **`cancel`, not `back`.** This button used to send the same event as
            Backspace, so it rubbed out one digit per press and only reached the
            staff list once the pad was empty — four taps to do what it says.
            The owner found that on a real install (2026-08-22). */}
        <Button variant="quiet" onClick={onCancel} disabled={busy}>
          Somebody else
        </Button>
        <Button variant="primary" onClick={onSubmit} disabled={busy}>
          {busy ? 'Checking…' : 'Sign in'}
        </Button>
      </div>
    </>
  );
}

/**
 * **The way back in, as four screens.**
 *
 * The owner, 2026-08-22: *"Forgotton pin also not working, i typed recovery
 * code, but cant even type new pin."*
 *
 * Everything used to be on one screen — the code box, the staff list and a row
 * of bullets standing in for the new PIN. There was no field and no pad behind
 * those bullets, and the code box held the keyboard focus, so a digit could
 * only ever land in the code box. The flow could be entered and not left.
 *
 * The steps are `code → who → pin → again`, one at a time. On the two pad steps
 * there is no text input on screen at all, which is what makes the digits
 * reachable rather than a rule that has to be remembered.
 */
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
        <p className="mb-muted">
          {/* Only the people Rust will accept. Offering the whole staff list
              meant the refusal arrived after the code had been spent. */}
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
