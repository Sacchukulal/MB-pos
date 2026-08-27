/** The customer's side of the counter. */

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
        // A display that cannot attach shows the idle screen for ever, which is the right
        // failure: it is a sign in a shop, not a program.
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
