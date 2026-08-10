/**
 * **The set-up list** — P22, and it is deliberately not a wizard.
 *
 * D102: every step is derived from what is actually in the shop, so there is no
 * position to remember, skipping is implicit, and resuming is automatic. Each
 * step's button opens the screen that already does that job — P13 owns the
 * menu, P14 the tables, P17 the shop's details and the printers, P11 the staff.
 * There is no seventh editor here and there must never be one.
 *
 * **It is beside the till, never in front of it.** PERFORMANCE S5 gives three
 * minutes from installing to a printable bill; a form between a shopkeeper and
 * their first customer is how a product gets uninstalled. So this is a panel on
 * the billing screen that disappears when the shop is set up.
 */

import { useCallback, useEffect, useState } from 'react';

import { Button, Card, SectionHeader } from '../kit';
import { call, inApp } from '../ipc/call';
import type { SetupView } from '../ipc/generated/SetupView';

import './setup.css';

export function Setup({ onGoTo }: { onGoTo: (screen: string) => void }) {
  const [view, setView] = useState<SetupView | null>(null);

  const load = useCallback(() => {
    if (!inApp()) return;
    call('setup_list')
      .then(setView)
      .catch(() => {
        /* A counter that cannot say what is left still bills. */
      });
  }, []);

  useEffect(load, [load]);

  // Nothing left: the panel goes away rather than congratulating anybody every
  // morning for the rest of the shop's life.
  if (!view || view.finished) return null;

  /**
   * **Only what is left.**
   *
   * The first version listed every step, done ones included, on the theory
   * that seeing progress makes a list feel finishable. Looking at it on the
   * real screen settled it the other way: six rows with their reasons filled
   * the whole billing pane and pushed the table grid and the menu below the
   * fold — on the one screen a cashier looks at all day. Progress is a line;
   * the list is the work.
   */
  const left = view.steps.filter((step) => !step.done);
  const done = view.steps.length - left.length;

  return (
    <Card className="mb-setup">
      <SectionHeader title="Setting up" note={view.headline} />
      {done > 0 && (
        <p className="mb-setup__done">
          {done} of {view.steps.length} done.
        </p>
      )}
      <ul className="mb-setup__steps">
        {left.map((step) => (
          <li key={step.id} className="mb-setup__step">
            <span className="mb-setup__tick" aria-hidden="true">
              ○
            </span>
            <div className="mb-setup__body">
              <span className="mb-setup__title">{step.title}</span>
              <span className="mb-setup__why">{step.why}</span>
            </div>
            <Button small variant="quiet" onClick={() => onGoTo(step.goTo)}>
              Do it
            </Button>
          </li>
        ))}
      </ul>
    </Card>
  );
}
