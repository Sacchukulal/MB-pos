/**
 * **The customer's side of the counter** — P29, scope 7.8.
 *
 * A second window, on a second monitor, showing the bill as it is typed and
 * the total at the end. A customer watching their bill being made is a real
 * trust feature: it is the difference between "he charged me ₹640" and "I
 * watched it add up to ₹640".
 *
 * # It must never take the keyboard
 *
 * **A cashier who has to click back into the search box after every item will
 * unplug the display by Friday.** So this page has, deliberately, *nothing
 * focusable on it*: no button, no input, no link, no `tabIndex`. There is a
 * test that says so, and it is not a style rule — it is the condition on the
 * feature existing at all.
 *
 * The other half of that promise is in Rust: the window is built unfocused and
 * never asks for focus. Both halves are asserted, because either one alone
 * would leave the door open.
 *
 * # Nothing here is arithmetic
 *
 * Every figure arrives already formatted (R8). This file is a layout for text
 * somebody else wrote — which is also why it can be this short.
 */

import { useEffect, useState } from 'react';

import { Scroller } from '../kit';
import { subscribe } from '../ipc/call';

import './display.css';

type Line = { name: string; qty: string; amount: string };

export function Display() {
  const [title, setTitle] = useState('');
  const [lines, setLines] = useState<Line[]>([]);
  const [total, setTotal] = useState('');
  const [idle, setIdle] = useState(true);

  useEffect(() => {
    let stop: (() => void) | undefined;
    subscribe((message) => {
      if (message.kind !== 'customerBill') return;
      setTitle(message.title);
      setLines([...message.lines]);
      setTotal(message.total);
      setIdle(message.idle);
    })
      .then((off) => {
        stop = off;
      })
      .catch(() => {
        // A display that cannot attach shows the idle screen for ever, which
        // is the right failure: it is a sign in a shop, not a program.
      });
    return () => stop?.();
  }, []);

  return (
    <div className="mb-display">
      <h1 className="mb-display__shop">{title}</h1>

      {idle || lines.length === 0 ? (
        <p className="mb-display__welcome">Welcome</p>
      ) : (
        <>
          <Scroller inset className="mb-display__lines">
            {lines.map((line, index) => (
              <div className="mb-display__line" key={`${line.name}-${index}`}>
                <span className="mb-display__what">{line.name}</span>
                <span className="mb-display__qty">{line.qty}</span>
                <span className="mb-display__amount">{line.amount}</span>
              </div>
            ))}
          </Scroller>
          <div className="mb-display__total">
            <span>Total</span>
            <span className="mb-display__grand">{total}</span>
          </div>
        </>
      )}
    </div>
  );
}
