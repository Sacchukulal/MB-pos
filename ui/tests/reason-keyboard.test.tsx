import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';

vi.mock('../src/ipc/call', () => ({
  call: vi.fn(async () => [{ id: 'items', text: 'Wrong items' }, { id: 'price', text: 'Wrong price' }]),
  isUiError: () => false,
}));
import { ReasonDialog } from '../src/corrections/Reason';

afterEach(cleanup);

it('focuses the chosen reason, moves with arrows, and confirms once with Enter', async () => {
  const confirm = vi.fn(() => new Promise<void>(() => undefined));
  render(<ReasonDialog kind="void" what="Revert bill A/7" confirmLabel="Revert bill" onCancel={vi.fn()} onConfirm={confirm} />);
  const first = await screen.findByLabelText('Wrong items');
  await waitFor(() => expect(first).toHaveFocus());
  fireEvent.keyDown(first, { key: 'ArrowDown' });
  expect(screen.getByLabelText('Wrong price')).toBeChecked();
  expect(screen.getByLabelText('Wrong price')).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: 'ArrowUp' });
  expect(first).toBeChecked();
  fireEvent.keyDown(first, { key: 'Enter', repeat: true });
  expect(confirm).not.toHaveBeenCalled();
  fireEvent.keyDown(first, { key: 'Enter' });
  fireEvent.keyDown(first, { key: 'Enter' });
  expect(confirm).toHaveBeenCalledExactlyOnceWith('Wrong items', undefined);
});

it('confirms from the note and retains manager approval validation', async () => {
  const confirm = vi.fn();
  render(<ReasonDialog kind="void" what="Revert bill" confirmLabel="Revert bill" needsApproval
    approvers={[{ id: 'manager', name: 'Manager' }]} onCancel={vi.fn()} onConfirm={confirm} />);
  await screen.findByLabelText('Wrong items');
  const note = screen.getByLabelText('Additional note');
  fireEvent.change(note, { target: { value: 'Correct quantity' } });
  fireEvent.keyDown(note, { key: 'Enter' });
  expect(screen.getByRole('alert')).toHaveTextContent('This needs a manager');
  expect(confirm).not.toHaveBeenCalled();
});
