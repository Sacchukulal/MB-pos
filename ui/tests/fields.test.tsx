/** The two fields that have a shape. */

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { MoneyInput, PhoneInput, onlyAmount, onlyPhone } from '../src/kit';

afterEach(cleanup);

describe('an amount is digits and one dot', () => {
  it('drops letters as they are typed, rather than complaining afterwards', () => {
    // A field that lets you type "12ab" and refuses on save has already wasted the keystroke
    // and the trip to Rust.
    expect(onlyAmount('12ab')).toBe('12');
    expect(onlyAmount('abc')).toBe('');
    expect(onlyAmount('1 2 0')).toBe('120');
  });

  it('keeps a pasted amount instead of throwing it away', () => {
    expect(onlyAmount('₹1,200.50')).toBe('1200.50');
    expect(onlyAmount('Rs. 90')).toBe('90');
  });

  it('allows one dot and two paise, which is what Money::parse takes', () => {
    expect(onlyAmount('120.5')).toBe('120.5');
    expect(onlyAmount('120.50')).toBe('120.50');
    // A third place is `MoneyError::TooPrecise` in Rust.
    expect(onlyAmount('120.505')).toBe('120.50');
    // Fast typing, not a thrown-away number.
    expect(onlyAmount('12.5.7')).toBe('12.57');
  });

  it('lets an amount be cleared, because empty is not zero', () => {
    expect(onlyAmount('')).toBe('');
    expect(onlyAmount('.')).toBe('.');
  });

  /** The requirement, stated as a test. */
  it('never lets the rupee mark into the value', () => {
    const onChange = vi.fn();
    render(<MoneyInput label="Amount" value="" onChange={onChange} />);
    const box = screen.getByLabelText('Amount') as HTMLInputElement;

    fireEvent.change(box, { target: { value: '₹450' } });
    expect(onChange).toHaveBeenCalledWith('450');
    expect(onChange).not.toHaveBeenCalledWith(expect.stringContaining('₹'));
  });

  it('draws the mark beside the box and not inside it', () => {
    const { container } = render(
      <MoneyInput label="Amount" value="450" onChange={vi.fn()} />,
    );
    // What a person sees….
    expect(container.querySelector('.mb-adorned__mark')?.textContent).toBe('₹');
    // …and what travels to Rust.
    expect((screen.getByLabelText('Amount') as HTMLInputElement).value).toBe('450');
  });

  it('does not announce the mark to a screen reader', () => {
    // The label already says it is an amount; "rupees four fifty" before every field is noise.
    const { container } = render(
      <MoneyInput label="Amount" value="450" onChange={vi.fn()} />,
    );
    expect(container.querySelector('.mb-adorned__mark')?.getAttribute('aria-hidden')).toBe(
      'true',
    );
  });
});

describe('a phone is ten digits', () => {
  it('drops letters', () => {
    expect(onlyPhone('Ravi Kumar')).toBe('');
    expect(onlyPhone('98400abcde')).toBe('98400');
  });

  it('stops at ten', () => {
    expect(onlyPhone('98400112233445')).toBe('9840011223');
    expect(onlyPhone('9840011223')).toBe('9840011223');
  });

  it('understands a number pasted out of a contact list', () => {
    // Refusing a paste is how you make somebody retype what the program could have understood.
    for (const typed of ['+91 98400 11223', '+919840011223', '098400-11223']) {
      expect(onlyPhone(typed), typed).toBe('9840011223');
    }
  });

  it('does not eat a real number that begins 91', () => {
    expect(onlyPhone('9188776655')).toBe('9188776655');
  });

  it('keeps the country code out of the value', () => {
    const onChange = vi.fn();
    const { container } = render(
      <PhoneInput label="Phone" value="" onChange={onChange} />,
    );
    fireEvent.change(screen.getByLabelText('Phone'), {
      target: { value: '+91 98400 11223' },
    });
    expect(onChange).toHaveBeenCalledWith('9840011223');
    expect(container.querySelector('.mb-adorned__mark')?.textContent).toBe('+91');
  });

  it('will not hold an eleventh digit even by typing', () => {
    const onChange = vi.fn();
    render(<PhoneInput label="Phone" value="9840011223" onChange={onChange} />);
    const box = screen.getByLabelText('Phone') as HTMLInputElement;
    expect(box.maxLength).toBe(10);
    // And the filter agrees with the attribute, so a paste is caught too.
    fireEvent.change(box, { target: { value: '98400112239' } });
    expect(onChange).toHaveBeenCalledWith('9840011223');
  });
});
