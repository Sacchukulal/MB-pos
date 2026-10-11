import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { afterEach, expect, it, vi } from 'vitest';
import { Button, DatePicker, DateRangePicker, Modal } from '../src/kit';

afterEach(cleanup);

it('does not leave an old committed date available while the user types an invalid replacement', async () => {
  function Example() {
    const [value, setValue] = useState('2024-02-14');
    return <><DatePicker label="Date" value={value} onChange={setValue} />
      <Button disabled={!value}>Save date</Button><output aria-label="Committed date">{value}</output></>;
  }
  const user = userEvent.setup(); render(<Example />);
  const input = screen.getByLabelText('Date');
  await user.clear(input); await user.type(input, '31-02-2024'); await user.tab();
  expect(input).toHaveValue('31-02-2024');
  expect(screen.getByRole('button', { name: 'Save date' })).toBeDisabled();
  expect(screen.getByLabelText('Committed date')).toHaveTextContent('');
  await user.clear(input); await user.type(input, '29-02-2024'); await user.tab();
  expect(input).toHaveValue('29-02-2024');
  expect(screen.getByLabelText('Committed date')).toHaveTextContent('2024-02-29');
});

it('validates real calendar dates and preserves ISO values while showing day-month-year', async () => {
  const change = vi.fn();
  render(<DatePicker label="Date" value="2024-02-28" onChange={change} />);
  const field = screen.getByLabelText('Date');
  expect(field).toHaveValue('28-02-2024');
  fireEvent.change(field, { target: { value: '31-02-2024' } });
  fireEvent.blur(field);
  expect(change).toHaveBeenLastCalledWith('');
  expect(field).toHaveAttribute('aria-invalid', 'true');
  expect(screen.getByRole('alert')).toHaveTextContent('Enter a valid date');
  fireEvent.change(field, { target: { value: '29/02/2024' } });
  expect(change).toHaveBeenLastCalledWith('2024-02-29');
  fireEvent.change(field, { target: { value: '' } });
  expect(change).toHaveBeenLastCalledWith('');
});

it('uses arrow keys across leap days, clamps month navigation, and restores the opener', async () => {
  const user = userEvent.setup();
  const change = vi.fn();
  render(<DatePicker label="Date" value="2024-01-31" onChange={change} />);
  const opener = screen.getByRole('button', { name: 'Choose date' });
  await user.click(opener);
  expect(document.activeElement).toHaveAccessibleName(/31 January 2024/);
  await user.keyboard('{PageDown}');
  expect(document.activeElement).toHaveAccessibleName(/29 February 2024/);
  await user.keyboard('{ArrowRight}');
  expect(document.activeElement).toHaveAccessibleName(/1 March 2024/);
  await user.keyboard('{Enter}');
  expect(change).toHaveBeenLastCalledWith('2024-03-01');
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(opener).toHaveFocus();
});

it('selects a neighbouring-month cell without changing the date under the pointer', async () => {
  const user = userEvent.setup();
  const change = vi.fn();
  render(<DatePicker label="Date" value="2024-02-14" onChange={change} />);
  await user.click(screen.getByRole('button', { name: 'Choose date' }));
  await user.click(screen.getByRole('gridcell', { name: /1 March 2024/ }));
  expect(change).toHaveBeenLastCalledWith('2024-03-01');
});

it('supports direct month/year selection and enforces date limits for both typing and clicks', async () => {
  const user = userEvent.setup();
  const change = vi.fn();
  render(<DatePicker label="Date" value="2024-02-14" min="2024-02-10" max="2025-04-20" required onChange={change} />);
  const field = screen.getByLabelText('Date');
  fireEvent.change(field, { target: { value: '09-02-2024' } });
  fireEvent.blur(field);
  expect(change).toHaveBeenLastCalledWith('');
  expect(screen.getByRole('alert')).toHaveTextContent('10-02-2024 or later');
  await user.click(screen.getByRole('button', { name: 'Choose date' }));
  expect(screen.getByRole('gridcell', { name: /, 9 February 2024/ })).toBeDisabled();
  expect(screen.queryByRole('button', { name: 'Clear' })).toBeNull();
  await user.click(screen.getByRole('button', { name: 'Choose month and year' }));
  fireEvent.change(screen.getByLabelText('Year'), { target: { value: '2025' } });
  await user.click(screen.getByRole('button', { name: 'April' }));
  await user.click(screen.getByRole('gridcell', { name: /20 April 2025/ }));
  expect(change).toHaveBeenLastCalledWith('2025-04-20');
});

it('clears optional dates and keeps disabled/read-only calendars closed', async () => {
  const user = userEvent.setup();
  const change = vi.fn();
  const { rerender } = render(<DatePicker label="Date" value="2024-02-14" onChange={change} />);
  await user.click(screen.getByRole('button', { name: 'Choose date' }));
  await user.click(screen.getByRole('button', { name: 'Clear' }));
  expect(change).toHaveBeenLastCalledWith('');
  rerender(<DatePicker label="Date" value="" onChange={change} disabled />);
  expect(screen.getByRole('button', { name: 'Choose date' })).toBeDisabled();
  rerender(<DatePicker label="Date" value="" onChange={change} readOnly />);
  expect(screen.getByRole('button', { name: 'Choose date' })).toBeDisabled();
});

it('gives multiple date ranges unique labels and independent values', () => {
  const first = vi.fn(); const second = vi.fn();
  render(<><DateRangePicker from="2024-01-01" to="2024-01-31" onChange={first} />
    <DateRangePicker from="2024-02-01" to="2024-02-29" onChange={second} /></>);
  const from = screen.getAllByLabelText('From');
  expect(from[0]!.id).not.toBe(from[1]!.id);
  fireEvent.change(from[1]!, { target: { value: '03-02-2024' } });
  expect(first).not.toHaveBeenCalled();
  expect(second).toHaveBeenCalledWith('2024-02-03', '2024-02-29');
});

it('keeps calendar keyboard navigation inside its parent dialog and closes one layer at a time', async () => {
  function Example() {
    const [open, setOpen] = useState(false);
    return <><Button onClick={() => setOpen(true)}>Open form</Button><Modal open={open} title="Form" onClose={() => setOpen(false)}>
      <DatePicker label="Date" value="2024-02-14" onChange={() => undefined} />
      <Button onClick={() => setOpen(false)}>Done</Button>
    </Modal></>;
  }
  const user = userEvent.setup(); render(<Example />);
  const opener = screen.getByRole('button', { name: 'Open form' });
  await user.click(opener);
  const calendar = screen.getByRole('button', { name: 'Choose date' });
  await user.click(calendar);
  const popup = screen.getByRole('dialog', { name: 'Date calendar' });
  within(popup).getByRole('button', { name: 'Close' }).focus();
  await user.tab();
  expect(popup.contains(document.activeElement)).toBe(true);
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('dialog', { name: 'Date calendar' })).toBeNull();
  expect(calendar).toHaveFocus();
  expect(screen.getByRole('dialog', { name: 'Form' })).toBeInTheDocument();
  await user.keyboard('{Escape}');
  expect(opener).toHaveFocus();
});
