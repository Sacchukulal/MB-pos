/** The More page: every screen that is not in the bar, down the left, and the one chosen. */

import type { ReactNode } from 'react';

import { Rail, RailItem } from '../kit';
import type { Screen } from './Shell';

export function More({
  screens,
  current,
  onGo,
  children,
}: {
  /** The screens behind More, in their order. */
  screens: readonly Screen[];
  current: string;
  onGo: (id: string) => void;
  /** The chosen screen, drawn. */
  children: ReactNode;
}) {
  return (
    <div className="mb-more">
      <Rail label="More screens" className="mb-more__rail">
        {screens.map((item) => (
          <RailItem
            key={item.id}
            icon={item.icon}
            current={item.id === current}
            onClick={() => onGo(item.id)}
          >
            {item.label}
          </RailItem>
        ))}
      </Rail>
      <div className="mb-more__body">{children}</div>
    </div>
  );
}
