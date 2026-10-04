/** The More page: every screen that is not in the bar, down the left, and the one chosen. */

import type { ReactNode } from 'react';

import { cx, Rail, RailItem, RailToggle, Scroller, useRailState } from '../kit';
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
  const [collapsed, setCollapsed] = useRailState('more');
  return (
    <div className={cx('mb-railpage', collapsed && 'mb-railpage--collapsed')}>
      <div className={cx('mb-navrail', collapsed && 'mb-navrail--collapsed')}>
        <RailToggle collapsed={collapsed} onChange={setCollapsed} label="More menu" />
        <Scroller inset>
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
        </Scroller>
      </div>
      <div className="mb-more__body">{children}</div>
    </div>
  );
}
