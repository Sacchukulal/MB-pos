import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { More } from '../src/shell/More';

afterEach(() => { cleanup(); localStorage.clear(); });

it('folds navigation, retains named destinations, and remembers the choice after remounting', () => {
  localStorage.setItem('magicbill.rail.more', 'expanded');
  const onGo = vi.fn();
  const draw = () => render(<More screens={[{ id: 'menu', label: 'Menu', icon: 'book', render: () => null }]} current="menu" onGo={onGo}><p>Menu content</p></More>);
  const first = draw();
  fireEvent.click(screen.getByRole('button', { name: 'Collapse More menu' }));
  expect(screen.getByRole('button', { name: 'Expand More menu' })).toHaveAttribute('aria-expanded', 'false');
  fireEvent.click(screen.getByRole('button', { name: 'Menu' }));
  expect(onGo).toHaveBeenCalledWith('menu');
  expect(screen.getByRole('button', { name: 'Menu' })).toHaveAttribute('aria-current', 'page');
  first.unmount();
  draw();
  expect(screen.getByRole('button', { name: 'Expand More menu' })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Expand More menu' }));
  expect(localStorage.getItem('magicbill.rail.more')).toBe('expanded');
});
