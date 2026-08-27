/** The Magic Bill mark. One picture, one place; everything that shows the brand shows this. */

import mark from './logo.png';
import { cx } from './cx';

export interface LogoProps {
  /** sm sits in a title bar, md beside a heading, lg on a screen of its own. */
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}

export function Logo({ size = 'md', className }: LogoProps) {
  return (
    <img
      className={cx('mb-logo', `mb-logo--${size}`, className)}
      src={mark}
      alt="Magic Bill"
      draggable={false}
    />
  );
}
