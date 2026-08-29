/** The UI kit. */

import { readFileSync } from 'node:fs';

import { render, screen, cleanup, fireEvent } from '@testing-library/react';
import { useState } from 'react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  Badge,
  Button,
  ConfirmDialog,
  EmptyState,
  Input,
  Logo,
  Modal,
  Money,
  Stepper,
  Table,
} from '../src/kit';

afterEach(cleanup);

/** − n +, once. */
describe('Stepper', () => {
  it('names both buttons after the thing being counted', () => {
    const onLess = vi.fn();
    const onMore = vi.fn();
    render(
      <Stepper label="Quantity of Masala Dosa" what="Masala Dosa" onLess={onLess} onMore={onMore}>
        <span className="mb-stepper__value">2</span>
      </Stepper>,
    );
    expect(screen.getByRole('group', { name: 'Quantity of Masala Dosa' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'One less Masala Dosa' }));
    fireEvent.click(screen.getByRole('button', { name: 'One more Masala Dosa' }));
    expect(onLess).toHaveBeenCalledTimes(1);
    expect(onMore).toHaveBeenCalledTimes(1);
  });

  it('can hold either end', () => {
    render(
      <Stepper label="People" what="person" onLess={vi.fn()} onMore={vi.fn()} lessDisabled>
        <span className="mb-stepper__value">2</span>
      </Stepper>,
    );
    expect(screen.getByRole('button', { name: 'One less person' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'One more person' })).toBeEnabled();
  });

  it('is the only stepper: no screen draws − + buttons of its own', () => {
    // A hand-rolled pair drifts in size the moment the kit's changes. The cart, "each pays" and
    // the kitchen all go through the one component.
    const files = ['billing/Billing.tsx', 'floor/Floor.tsx', 'kitchen/Kitchen.tsx'];
    for (const file of files) {
      const source = readFileSync(`src/${file}`, 'utf8');
      expect(source, `${file} draws its own minus button`).not.toMatch(
        /<Button[^>]*>\s*<Icon name="minus"/,
      );
    }
  });
});


describe('Button', () => {
  it('is reachable and pressable by keyboard alone', async () => {
    // The keyboard-first rule (§1): "a cashier must be able to run a whole shift without
    // touching the mouse.".
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
    // §2 rule 1: changing a shop's accent must never make "delete" look like "save".
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

  /** A hint is asked for, an error is told to you. */
  it('puts the hint in a tip beside the label, not a line under the box', () => {
    const { container, rerender } = render(<Input label="Phone" hint="Ten digits." />);

    // In a tip's bubble, reached by its own button — not printed under the box.
    expect(container.querySelector('.mb-field__hint')).toBeNull();
    const bubble = container.querySelector('.mb-tip__bubble[role="tooltip"]');
    expect(bubble?.textContent).toBe('Ten digits.');
    expect(screen.getByRole('button', { name: 'About Phone' })).toBeTruthy();

    // An error still prints, and the hint is still one hover away.
    rerender(<Input label="Phone" hint="Ten digits." error="Too short." />);
    expect(screen.getByRole('alert')).toHaveTextContent('Too short.');
    expect(container.querySelector('.mb-tip__bubble')?.textContent).toBe('Ten digits.');
  });
});

describe('Money', () => {
  it('renders exactly what Rust formatted, and computes nothing', () => {
    render(<Money value={{ paise: 128_050n, text: '1,280.50' }} />);
    const shown = screen.getByText('1,280.50');
    expect(shown).toBeInTheDocument();
    expect(shown).toHaveAttribute('data-paise', '128050');
  });
});

describe('ConfirmDialog', () => {
  it('says exactly what will happen, on the button', async () => {
    // "a button says exactly what happens; the confirmation echoes it." There is deliberately
    // no default of "OK".
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

  /** All three ways out are reachable. */
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
    // Wrapping is what keeps the third button on the page.
    const css = readFileSync('src/kit/kit.css', 'utf8');
    // The rule where it is DEFINED — `\n.x {` — not the first place the class is mentioned,
    // which can be a descendant selector further up the file.
    const rule = css.slice(css.indexOf('\n.mb-modal__actions {'));
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
          <EmptyState title="No bills yet" hint="Settle one and it lands here." />
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
    // "a column of rupees that doesn't line up looks broken to a shopkeeper.".
    expect(screen.getByRole('columnheader', { name: 'Total' })).toHaveClass(
      'mb-numeric',
    );
  });
});

describe('Badge', () => {
  it('carries a word as well as a colour', () => {
    // §2 rule 2: "colour is never the only signal." Grey-scale the screen and "Paid" still says
    // paid.
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
    const name = screen.getByLabelText('Name') as HTMLInputElement;
    expect(document.activeElement).toBe(name);
  });

  it('leaves the caret where the person put it when the dialog re-renders', () => {
    function TwoFields() {
      const [key, setKey] = useState('');
      const [code, setCode] = useState('');
      return (
        // The inline arrow is the point: this is what every call site does.
        <Modal open title="Enter your licence key" onClose={() => undefined}>
          <Input label="Licence key" value={key} onChange={(e) => setKey(e.target.value)} autoFocus />
          <Input label="Code" value={code} onChange={(e) => setCode(e.target.value)} />
        </Modal>
      );
    }
    render(<TwoFields />);

    const licence = screen.getByLabelText('Licence key') as HTMLInputElement;
    const code = screen.getByLabelText('Code') as HTMLInputElement;
    expect(document.activeElement).toBe(licence);

    // Move to the second field and type into it, one character at a time.
    code.focus();
    for (const character of '123456') {
      fireEvent.change(code, { target: { value: code.value + character } });
      expect(document.activeElement).toBe(code);
    }
    expect(code.value).toBe('123456');
    expect(licence.value).toBe('');
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

/** The brand mark. */
describe('Logo', () => {
  it('is the one picture, named, at the size asked for', () => {
    render(<Logo size="sm" />);
    const mark = screen.getByRole('img', { name: 'Magic Bill' });
    expect(mark).toHaveClass('mb-logo', 'mb-logo--sm');
    expect(mark.getAttribute('src')).toMatch(/logo/);
  });
});
