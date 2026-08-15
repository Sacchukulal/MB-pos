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
 * their first customer is how a product gets uninstalled. So this is a strip on
 * the billing screen that disappears when the shop is set up.
 *
 * # P27.5 made it a STRIP rather than a panel, and looking at it is why
 *
 * P22 already found that listing every step filled the billing pane, and cut it
 * to only what is left. That was the right direction and it did not go far
 * enough: a shop with two steps outstanding still gave a quarter of its billing
 * screen — the screen a cashier looks at all day, for the life of the shop — to
 * a checklist about backups. And the steps that remain longest are the ones a
 * shop is least likely to ever do, so the panel is biggest exactly when it is
 * least useful.
 *
 * So it collapses. One line says how many are left and what they are; pressing
 * it opens the reasons. It starts open only while something that **matters
 * most** is outstanding — the shop's own details, its menu, its printer — and
 * closed once what is left is the optional tail. A shop that has really not set
 * itself up still gets shouted at; a working shop gets a line.
 */

import { useCallback, useEffect, useState } from 'react';

import { Button, Icon } from '../kit';
import { call, inApp } from '../ipc/call';
import type { SetupView } from '../ipc/generated/SetupView';

import './setup.css';

export function Setup({ onGoTo }: { onGoTo: (screen: string) => void }) {
  const [view, setView] = useState<SetupView | null>(null);
  const [open, setOpen] = useState<boolean | null>(null);

  const load = useCallback(() => {
    if (!inApp()) return;
    call('setup_list')
      .then(setView)
      .catch(() => {
        /* A counter that cannot say what is left still bills. */
      });
  }, []);

  useEffect(load, [load]);

  // Nothing left: the strip goes away rather than congratulating anybody every
  // morning for the rest of the shop's life.
  if (!view || view.finished) return null;

  const left = view.steps.filter((step) => !step.done);
  if (left.length === 0) return null;

  const done = view.steps.length - left.length;
  const urgent = left.some((step) => step.mattersMost);
  // `null` means nobody has pressed it yet, so the shop's own state decides.
  const isOpen = open ?? urgent;

  return (
    <section className={['mb-setup', urgent ? 'mb-setup--urgent' : ''].filter(Boolean).join(' ')}>
      <button
        type="button"
        className="mb-setup__line"
        aria-expanded={isOpen}
        onClick={() => setOpen(!isOpen)}
      >
        <Icon name={urgent ? 'warning' : 'info'} size="sm" className="mb-setup__icon" />
        <span className="mb-setup__says">
          {/* **Rust's sentence, not one built here.** §6: one place turns a
              machine state into words. It is also the half that matters most —
              it ends by saying the shop can take money in the meantime, which
              is the whole reason this is a strip and not a wizard (D102). */}
          <strong>{view.headline}</strong>
          {/* The names, on the same line, so the collapsed state still says
              WHAT is left — a count on its own is a number nobody can act on. */}
          <span className="mb-setup__names"> {left.map((s) => s.title).join(' · ')}</span>
        </span>
        <span className="mb-setup__count">{`${done} of ${view.steps.length} done`}</span>
        <Icon name={isOpen ? 'chevron-up' : 'chevron-down'} size="sm" />
      </button>

      {isOpen ? (
        <ul className="mb-setup__steps">
          {left.map((step) => (
            <li key={step.id} className="mb-setup__step">
              <div className="mb-setup__body">
                <span className="mb-setup__title">{step.title}</span>
                <span className="mb-setup__why">{step.why}</span>
              </div>
              <Button small variant="secondary" onClick={() => onGoTo(step.goTo)}>
                Do it
              </Button>
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}
