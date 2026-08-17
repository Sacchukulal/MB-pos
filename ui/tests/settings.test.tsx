/**
 * **The settings screen** — audit Part 3, and P17's T8.
 *
 * Rust proves what a setting is and what it may hold (`settings/tests.rs`,
 * `settings_tests.rs`). This proves the three claims that are the screen's own:
 *
 * 1. **the catalogue is the screen** — every control is drawn from what Rust
 *    sent, so a section this file has never heard of still renders;
 * 2. **the unsaved-changes guard offers Save / Discard / Cancel, and Save
 *    really saves before moving** (T8) — v1 lost edits silently;
 * 3. **a section a person may not change is read-only, not missing** — hiding
 *    it would leave the printer person wondering where the tax rates went.
 */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => true,
  isUiError: () => false,
}));

const { Settings } = await import('../src/settings/Settings');
const { ToastProvider } = await import('../src/kit');

import type { SettingsView } from '../src/ipc/generated/SettingsView';

const view: SettingsView = {
  hasShop: true,
  trouble: null,
  groups: [
    {
      code: 'store',
      label: 'Your shop',
      canEdit: true,
      settings: [
        {
          key: 'store.name',
          topic: 'Your shop',
          label: 'Shop name',
          help: 'Printed at the top of every bill.',
          control: 'words',
          value: 'Anna Kuteera',
          choices: [],
          min: null,
          max: null,
          unit: '',
          maxLen: 60,
        },
        {
          key: 'store.state_code',
          topic: 'Your shop',
          label: 'State',
          help: 'Which state you are in.',
          control: 'choice',
          value: '29',
          choices: [
            { value: '29', label: 'Karnataka' },
            { value: '32', label: 'Kerala' },
          ],
          min: null,
          max: null,
          unit: '',
          maxLen: 0,
        },
      ],
    },
    {
      code: 'receipt',
      label: 'The bill',
      canEdit: false,
      settings: [
        {
          key: 'receipt.show.token',
          topic: 'What goes on the bill',
          label: 'Print the token number',
          help: 'The big number the customer waits for.',
          control: 'tick',
          value: '1',
          choices: [],
          min: null,
          max: null,
          unit: '',
          maxLen: 0,
        },
        {
          key: 'receipt.logo_width_pct',
          topic: 'Your logo',
          label: 'Logo width',
          help: 'As a percentage of the paper width.',
          control: 'number',
          value: '40',
          choices: [],
          min: 10,
          max: 100,
          unit: '%',
          maxLen: 0,
        },
      ],
    },
  ],
};

function draw() {
  return render(
    <ToastProvider>
      <Settings />
    </ToastProvider>,
  );
}

beforeEach(() => {
  call.mockReset();
  call.mockImplementation((name: string) => {
    if (name === 'settings_all') return Promise.resolve(view);
    if (name === 'save_settings') {
      return Promise.resolve({ changed: [], settings: view });
    }
    if (name === 'search_settings') return Promise.resolve(['receipt.logo_width_pct']);
    if (name === 'settings_defaults_for') {
      return Promise.resolve([{ key: 'store.name', value: '' }]);
    }
    if (name === 'preview_settings') {
      return Promise.resolve({
        paper: '80 mm (3 inch)',
        notUsableYet: [],
        doc: {
          columns: 48,
          notes: [],
          lines: [
            { kind: 'text', text: 'Anna Kuteera', indent: 18, scale: 2, bold: true },
            { kind: 'rule', glyph: '-', width: 48, indent: 0 },
            { kind: 'text', text: 'Come back soon', indent: 17, scale: 1, bold: false },
          ],
        },
      });
    }
    return Promise.resolve(null);
  });
});

afterEach(cleanup);

describe('the settings screen', () => {
  it('draws whatever the catalogue sent, in the control it asked for', async () => {
    draw();
    // Words.
    expect(await screen.findByLabelText('Shop name')).toHaveValue('Anna Kuteera');
    // A choice, with its options — and this file has never heard of "state".
    const state = screen.getByLabelText('State') as HTMLSelectElement;
    expect(state.value).toBe('29');
    expect(state.options).toHaveLength(2);
  });

  it('shows a section it may not change rather than hiding it', async () => {
    draw();
    await screen.findByLabelText('Shop name');
    fireEvent.click(screen.getByRole('button', { name: /The bill/ }));

    // Present, and disabled.
    const tick = (await screen.findByLabelText('Print the token number')) as HTMLInputElement;
    expect(tick.disabled).toBe(true);
    // And no reset button, because there is nothing this person may reset.
    expect(
      screen.queryByRole('button', { name: /back to standard/ }),
    ).toBeNull();
  });

  it('marks an edited setting and offers to save it', async () => {
    draw();
    const name = await screen.findByLabelText('Shop name');
    fireEvent.change(name, { target: { value: 'Anna Kuteera Veg' } });

    expect(screen.getByText('not saved')).toBeTruthy();
    expect(screen.getByText(/1 setting changed and not saved/)).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('save_settings', {
        edits: [{ key: 'store.name', value: 'Anna Kuteera Veg' }],
      }),
    );
  });

  /** **T8**, and all three ways out of it. */
  it('asks before leaving a section with unsaved changes', async () => {
    draw();
    const name = await screen.findByLabelText('Shop name');
    fireEvent.change(name, { target: { value: 'Changed' } });

    fireEvent.click(screen.getByRole('button', { name: /The bill/ }));
    expect(await screen.findByText('You have unsaved changes')).toBeTruthy();
    // Three ways out, not two.
    expect(screen.getByRole('button', { name: 'Stay here' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Discard and move' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Save and move' })).toBeTruthy();

    // Cancel leaves us where we were, with the edit intact.
    fireEvent.click(screen.getByRole('button', { name: 'Stay here' }));
    expect(screen.getByLabelText('Shop name')).toHaveValue('Changed');
  });

  it('SAVES before moving when asked to', async () => {
    draw();
    const name = await screen.findByLabelText('Shop name');
    fireEvent.change(name, { target: { value: 'Changed' } });
    fireEvent.click(screen.getByRole('button', { name: /The bill/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Save and move' }));

    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('save_settings', {
        edits: [{ key: 'store.name', value: 'Changed' }],
      }),
    );
    // And we did move.
    await screen.findByLabelText('Print the token number');
  });

  it('discards and moves when asked to, and does not save', async () => {
    draw();
    const name = await screen.findByLabelText('Shop name');
    fireEvent.change(name, { target: { value: 'Changed' } });
    fireEvent.click(screen.getByRole('button', { name: /The bill/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Discard and move' }));

    await screen.findByLabelText('Print the token number');
    expect(call).not.toHaveBeenCalledWith('save_settings', expect.anything());
  });

  /** **T9** — and the matching is Rust's, so this proves the screen uses it. */
  it('searches across every section and says where each hit lives', async () => {
    draw();
    await screen.findByLabelText('Shop name');
    fireEvent.change(screen.getByPlaceholderText('Search every setting'), {
      target: { value: 'logo' },
    });

    await waitFor(() => expect(call).toHaveBeenCalledWith('search_settings', { text: 'logo' }));
    // The hit, and the section it came from — a result that does not say where
    // it lives answers the question once and not next time.
    expect(await screen.findByLabelText(/Logo width/)).toBeTruthy();
    // "The bill" twice: once in the section list, once above the hit saying
    // where it lives. The second one is the point.
    expect(screen.getAllByText('The bill')).toHaveLength(2);
  });

  /**
   * **The live preview — audit D1.**
   *
   * The document is Rust's; this proves the screen asks for it with the
   * UNSAVED edits, which is the whole point of a live preview.
   */
  it('draws the sample paper and redraws it as a setting is typed', async () => {
    draw();
    await screen.findByLabelText('Shop name');
    expect(await screen.findByText('Anna Kuteera')).toBeTruthy();
    // **Which paper it DREW on**, said above the paper itself. Matched by the
    // whole sentence rather than by "80 mm": the paper-width picker added on
    // 2026-08-17 offers "3 inch (80 mm)" as an option, so a loose match now
    // finds two things and cannot tell the label from the picture.
    expect(screen.getByText('Sample · 80 mm (3 inch)')).toBeTruthy();

    call.mockClear();
    fireEvent.change(screen.getByLabelText('Shop name'), {
      target: { value: 'Anna Kuteera Veg' },
    });

    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('preview_settings', {
        group: 'store',
        edits: [{ key: 'store.name', value: 'Anna Kuteera Veg' }],
      }),
    );
  });

  it('asks for the KITCHEN sample on the kitchen section', async () => {
    const withKitchen: SettingsView = {
      ...view,
      groups: [...view.groups, { ...view.groups[1]!, code: 'kitchen', label: 'The kitchen ticket' }],
    };
    call.mockImplementation((name: string) => {
      if (name === 'settings_all') return Promise.resolve(withKitchen);
      if (name === 'preview_settings') {
        return Promise.resolve({
          paper: '80 mm (3 inch)',
          notUsableYet: [],
          doc: { columns: 48, notes: [], lines: [] },
        });
      }
      return Promise.resolve(null);
    });
    draw();
    await screen.findByLabelText('Shop name');
    fireEvent.click(screen.getByRole('button', { name: /The kitchen ticket/ }));

    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('preview_settings', {
        group: 'kitchen',
        edits: [],
      }),
    );
  });

  /** Half-typed is a normal state, and the screen says so rather than blanking. */
  it('says which box is not usable yet instead of blanking the paper', async () => {
    call.mockImplementation((name: string) => {
      if (name === 'settings_all') return Promise.resolve(view);
      if (name === 'preview_settings') {
        return Promise.resolve({
          paper: '80 mm (3 inch)',
          notUsableYet: ['Logo width'],
          doc: {
            columns: 48,
            notes: [],
            lines: [{ kind: 'text', text: 'Anna Kuteera', indent: 0, scale: 1, bold: false }],
          },
        });
      }
      return Promise.resolve(null);
    });
    draw();
    // The paper is still drawn...
    expect(await screen.findByText('Anna Kuteera')).toBeTruthy();
    // ...and the box that could not be used is named.
    expect(screen.getByText(/Not used yet: Logo width/)).toBeTruthy();
  });

  /** Reset is shown as unsaved edits, so it can be looked at and cancelled. */
  it('does not save a reset — it fills the boxes and waits', async () => {
    draw();
    await screen.findByLabelText('Shop name');
    fireEvent.click(screen.getByRole('button', { name: /back to standard/ }));

    await waitFor(() => expect(screen.getByLabelText('Shop name')).toHaveValue(''));
    expect(call).not.toHaveBeenCalledWith('save_settings', expect.anything());
    expect(screen.getByText(/1 setting changed and not saved/)).toBeTruthy();
  });
});
