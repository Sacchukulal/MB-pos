/**
 * **T3 — the UI kit.** Rendering, keyboard reach, disabled and error states.
 *
 * Vitest + Testing Library, and the one-line reason: Testing Library asserts on
 * what a cashier can see and reach — text, roles, labels — rather than on a
 * component's internals, so these tests survive a change to the look. Which is
 * exactly what this session is for.
 */

import { readFileSync } from 'node:fs';

import { render, screen, cleanup } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  Badge,
  Button,
  ConfirmDialog,
  EmptyState,
  Input,
  Modal,
  Money,
  Table,
} from '../src/kit';

afterEach(cleanup);

describe('Button', () => {
  it('is reachable and pressable by keyboard alone', async () => {
    // The keyboard-first rule (§1): "a cashier must be able to run a whole
    // shift without touching the mouse."
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Settle</Button>);

    await userEvent.tab();
    expect(screen.getByRole('button', { name: 'Settle' })).toHaveFocus();
    await userEvent.keyboard('{Enter}');
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('does not fire when disabled', async () => {
    const onClick = vi.fn();
    render(
      <Button onClick={onClick} disabled>
        Settle
      </Button>,
    );
    await userEvent.click(screen.getByRole('button'));
    expect(onClick).not.toHaveBeenCalled();
  });

  it('renders a real button, so it is announced as one', () => {
    render(<Button>Print</Button>);
    expect(screen.getByRole('button', { name: 'Print' })).toBeInTheDocument();
  });

  it('uses a semantic colour for a destructive action, never the accent', () => {
    // §2 rule 1: changing a shop's accent must never make "delete" look like
    // "save". The class is the check, because the colour is a token.
    render(<Button variant="danger">Void bill</Button>);
    expect(screen.getByRole('button')).toHaveClass('mb-button--danger');
  });
});

describe('Input', () => {
  it('ties its label to its field, so clicking the label focuses it', async () => {
    render(<Input label="Customer name" />);
    await userEvent.click(screen.getByText('Customer name'));
    expect(screen.getByLabelText('Customer name')).toHaveFocus();
  });

  it('announces an error and marks the field invalid', () => {
    render(<Input label="GSTIN" error="That is not a valid GSTIN." />);
    expect(screen.getByRole('alert')).toHaveTextContent('not a valid GSTIN');
    expect(screen.getByLabelText('GSTIN')).toHaveAttribute('aria-invalid', 'true');
  });

  it('shows the hint only while there is no error', () => {
    const { rerender } = render(<Input label="Phone" hint="Ten digits." />);
    expect(screen.getByText('Ten digits.')).toBeInTheDocument();
    rerender(<Input label="Phone" hint="Ten digits." error="Too short." />);
    expect(screen.queryByText('Ten digits.')).not.toBeInTheDocument();
  });
});

describe('Money', () => {
  it('renders exactly what Rust formatted, and computes nothing', () => {
    // R8 and D2. The paise ride along for anything that needs the integer;
    // what is SHOWN is the string `Money::to_plain_string` produced.
    render(<Money value={{ paise: 128_050n, text: '1,280.50' }} />);
    const shown = screen.getByText('1,280.50');
    expect(shown).toBeInTheDocument();
    expect(shown).toHaveAttribute('data-paise', '128050');
  });
});

describe('ConfirmDialog', () => {
  it('says exactly what will happen, on the button', async () => {
    // §6: "a button says exactly what happens; the confirmation echoes it."
    // There is deliberately no default of "OK".
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        open
        title="Void this bill?"
        confirmLabel="Void the bill"
        destructive
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Void the bill' }));
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  /**
   * **All three ways out are reachable** — P18.
   *
   * P17 added the third button and never put three long labels in one dialog.
   * Closing a day offers "Cancel / Close without printing / Close and print
   * the slip", which is wider than a 26rem modal, and the row did not wrap —
   * Cancel was pushed off the left edge and clipped. Found by looking at the
   * screen, which is why the fix is in `kit.css` and the guard is here.
   */
  it('keeps all three ways out when the labels are long', async () => {
    const onCancel = vi.fn();
    const onOther = vi.fn();
    render(
      <ConfirmDialog
        open
        title="Close the day?"
        body="Over by 874.00."
        confirmLabel="Close and print the slip"
        otherLabel="Close without printing"
        onConfirm={vi.fn()}
        onOther={onOther}
        onCancel={onCancel}
      />,
    );
    // Wrapping is what keeps the third button on the page. jsdom applies no
    // stylesheet, so the rule is checked in the stylesheet itself — the same
    // trick `contrast.test.ts` uses, and for the same reason.
    const css = readFileSync('src/kit/kit.css', 'utf8');
    const rule = css.slice(css.indexOf('.mb-modal__actions'));
    expect(rule.slice(0, rule.indexOf('}'))).toContain('flex-wrap: wrap');

    // And every one of the three does its own thing.
    await userEvent.click(screen.getByRole('button', { name: 'Close without printing' }));
    expect(onOther).toHaveBeenCalledOnce();
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it('closes on Escape', async () => {
    const onCancel = vi.fn();
    render(
      <ConfirmDialog
        open
        title="Void this bill?"
        confirmLabel="Void the bill"
        onConfirm={vi.fn()}
        onCancel={onCancel}
      />,
    );
    await userEvent.keyboard('{Escape}');
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it('is a modal dialog, so focus and screen readers stay inside it', () => {
    render(
      <ConfirmDialog
        open
        title="Void this bill?"
        confirmLabel="Void the bill"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.getByRole('dialog')).toHaveAttribute('aria-modal', 'true');
  });
});

describe('Table', () => {
  it('shows an empty state rather than an empty grid', () => {
    render(
      <Table
        columns={[{ key: 'a', header: 'A', render: () => null }]}
        rows={[]}
        rowKey={() => 'x'}
        empty={
          <EmptyState title="No bills yet" body="Settle one and it lands here." />
        }
      />,
    );
    expect(screen.getByText('No bills yet')).toBeInTheDocument();
  });

  it('right-aligns the columns that hold figures', () => {
    render(
      <Table
        columns={[
          { key: 'name', header: 'Item', render: (r: { name: string }) => r.name },
          {
            key: 'total',
            header: 'Total',
            numeric: true,
            render: () => '240.00',
          },
        ]}
        rows={[{ name: 'Dosa' }]}
        rowKey={(r) => r.name}
      />,
    );
    // §3: "a column of rupees that doesn't line up looks broken to a
    // shopkeeper."
    expect(screen.getByRole('columnheader', { name: 'Total' })).toHaveClass(
      'mb-numeric',
    );
  });
});

describe('Badge', () => {
  it('carries a word as well as a colour', () => {
    // §2 rule 2: "colour is never the only signal." Grey-scale the screen and
    // "Paid" still says paid.
    render(<Badge tone="ok">Paid</Badge>);
    expect(screen.getByText('Paid')).toBeInTheDocument();
  });
});

describe('a dialog and the caret (§1, keyboard-first)', () => {
  it('puts the caret in the first field rather than taking it back out', () => {
    render(
      <Modal open title="Add a size" onClose={vi.fn()}>
        <Input label="Name" autoFocus />
        <Input label="Price" />
      </Modal>,
    );
    // The panel used to win this race, so the first thing typed into a fresh
    // dialog went nowhere. Found by opening "Add a size", typing "Half", and
    // watching it vanish (P13).
    const name = screen.getByLabelText('Name') as HTMLInputElement;
    expect(document.activeElement).toBe(name);
  });

  it('still takes the focus itself when there is nothing to type into', () => {
    render(
      <Modal open title="Are you sure?" onClose={vi.fn()}>
        <p>Nothing to fill in.</p>
      </Modal>,
    );
    expect((document.activeElement as HTMLElement).getAttribute('role')).toBe('dialog');
  });
});
