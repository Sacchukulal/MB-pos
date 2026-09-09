/** The charts: shapes from Rust's figures. */

import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { Chart } from '../src/kit';
import type { ChartView } from '../src/ipc/generated/ChartView';

afterEach(cleanup);

const point = (label: string, value: string, note: string, share: number, hue = 0) => ({
  label,
  value,
  note,
  share,
  hue,
});

describe('Chart', () => {
  it('names a ranking row by its label, figure and note, and sizes the bar by its share', () => {
    const chart: ChartView = {
      id: 'items',
      title: 'Top selling',
      kind: 'bars',
      note: '',
      empty: 'Nothing sold.',
      points: [point('Masala Dosa', '1,200.00', '10 sold', 1000), point('Idli', '300.00', '5 sold', 250)],
    };
    const { container } = render(<Chart chart={chart} />);
    expect(screen.getByRole('heading', { name: 'Top selling' })).toBeTruthy();
    expect(screen.getByText('Masala Dosa')).toBeTruthy();
    expect(screen.getByText('10 sold')).toBeTruthy();
    const marks = container.querySelectorAll('.mb-chart__track .mb-chart__mark');
    expect(marks[0]?.getAttribute('width')).toBe('100%');
    expect(marks[1]?.getAttribute('width')).toBe('25%');
  });

  it('gives every slice of the whole a legend entry in its own colour', () => {
    const chart: ChartView = {
      id: 'payment',
      title: 'Payment modes',
      kind: 'donut',
      note: '',
      empty: 'Nothing sold.',
      points: [point('Cash', '900.00', '3 bills', 750, 1), point('UPI', '300.00', '1 bill', 250, 2)],
    };
    const { container } = render(<Chart chart={chart} />);
    expect(screen.getByText('Cash')).toBeTruthy();
    expect(screen.getByText('UPI')).toBeTruthy();
    expect(container.querySelector('.mb-chart__swatch.mb-chart__mark--hue-1')).toBeTruthy();
    expect(container.querySelectorAll('.mb-chart__slice').length).toBe(2);
  });

  it('says so when there is nothing to draw', () => {
    const chart: ChartView = {
      id: 'trend',
      title: 'Sales by hour',
      kind: 'columns',
      note: '',
      empty: 'Nothing sold.',
      points: [],
    };
    render(<Chart chart={chart} />);
    expect(screen.getByText('Nothing sold.')).toBeTruthy();
  });

  it('leads the columns with the tallest point, said in full', () => {
    const chart: ChartView = {
      id: 'trend',
      title: 'Sales by hour',
      kind: 'columns',
      note: '',
      empty: 'Nothing sold.',
      points: [point('9 am', '200.00', '2 bills', 200), point('1 pm', '1,000.00', '9 bills', 1000)],
    };
    render(<Chart chart={chart} />);
    expect(screen.getByText('1 pm · 1,000.00 · 9 bills')).toBeTruthy();
  });
});
