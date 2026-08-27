/** The settings screen. */

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
import type { SettingView } from '../src/ipc/generated/SettingView';
import type { PreviewLine } from '../src/ipc/generated/PreviewLine';

function line(
  text: string,
  extra: Partial<Extract<PreviewLine, { kind: 'text' }>> = {},
): PreviewLine {
  return {
    kind: 'text',
    text,
    indent: 0,
    row: 24,
    cap: 15,
    advance: 13,
    scale: 1,
    bold: false,
    segments: [{ text, width: text.length, align: 'centre' }],
    ...extra,
  };
}

/** One setting, with the boring parts filled in. */
function setting(
  key: string,
  label: string,
  control: string,
  value: string,
  extra: Partial<SettingView> = {},
): SettingView {
  return {
    key,
    topic: 'A topic',
    row: '',
    short: '',
    label,
    help: '',
    control,
    value,
    choices: [],
    min: null,
    max: null,
    unit: '',
    maxLen: 0,
    ...extra,
  };
}

const view: SettingsView = {
  hasShop: true,
  trouble: null,
  groups: [
    {
      code: 'store',
      label: 'Your shop',
      canEdit: true,
      settings: [
        setting('store.name', 'Shop name', 'words', 'Anna Kuteera', {
          topic: 'Your shop',
          help: 'Printed at the top of every bill.',
          maxLen: 60,
        }),
        setting('store.state_code', 'State', 'choice', '29', {
          topic: 'Your shop',
          help: 'Which state you are in.',
          choices: [
            { value: '29', label: 'Karnataka' },
            { value: '32', label: 'Kerala' },
          ],
        }),
      ],
    },
    {
      code: 'receipt',
      label: 'The bill',
      canEdit: false,
      settings: [
        setting('receipt.show.token', 'Print the token number', 'tick', '1', {
          topic: 'What goes on the bill',
          help: 'The big number the customer waits for.',
        }),
        setting('receipt.logo_width_pct', 'Logo width', 'number', '40', {
          topic: 'Your logo',
          help: 'As a percentage of the paper width.',
          min: 10,
          max: 100,
        }),
      ],
    },
    /** The section that designs a piece of paper, and the one the paper tests use. */
    {
      code: 'kitchen',
      label: 'The kitchen ticket',
      canEdit: true,
      settings: [
        setting('kitchen.footer', 'Ticket footer', 'words', 'Cook it hot', {
          topic: 'The last words',
          maxLen: 40,
        }),
        setting('kitchen.title.scale', 'Title size', 'choice', '15', {
          topic: 'Typeface and sizes',
          row: 'Title',
          short: 'Size',
          choices: [
            { value: '15', label: '4' },
            { value: '26', label: '8' },
          ],
        }),
        setting('kitchen.title.bold', 'Title in bold', 'tick', '1', {
          topic: 'Typeface and sizes',
          row: 'Title',
          short: 'Bold',
        }),
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
          dots: 576,
          columns: 44,
          millimetres: 64,
          engine: 'raster',
          notes: [],
          lines: [
            line('Anna Kuteera', { cap: 26, row: 40, advance: 22, scale: 2, bold: true }),
            {
              kind: 'rule',
              indent: 0,
              width: 576,
              row: 9,
              thickness: 1,
              strokes: 1,
              gap: 0,
              dash: null,
            },
            line('Come back soon'),
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
      screen.queryByRole('button', { name: /Reset this section/ }),
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

  /** A box put back where it started is not a change, so there is nothing to save. */
  it('drops the save bar when a setting is typed back to what is saved', async () => {
    draw();
    const name = await screen.findByLabelText('Shop name');

    fireEvent.change(name, { target: { value: 'Anna Kuteera Veg' } });
    expect(screen.getByText(/1 setting changed and not saved/)).toBeTruthy();

    fireEvent.change(name, { target: { value: 'Anna Kuteera' } });
    expect(screen.queryByText(/changed and not saved/)).toBeNull();
    expect(screen.queryByText('not saved')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Save' })).toBeNull();
  });

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

  /** And the matching is Rust's, so this proves the screen uses it. */
  it('searches across every section and says where each hit lives', async () => {
    draw();
    await screen.findByLabelText('Shop name');
    fireEvent.change(screen.getByPlaceholderText('Search every setting'), {
      target: { value: 'logo' },
    });

    await waitFor(() => expect(call).toHaveBeenCalledWith('search_settings', { text: 'logo' }));
    // The hit, and the section it came from — a result that does not say where it lives answers
    // the question once and not next time.
    expect(await screen.findByLabelText('Logo width')).toBeTruthy();
    // "The bill" twice: once in the section list, once above the hit saying where it lives.
    expect(screen.getAllByText('The bill')).toHaveLength(2);
  });

  /** The live preview. */
  it('draws the sample paper and redraws it as a setting is typed', async () => {
    draw();
    await screen.findByLabelText('Shop name');
    fireEvent.click(screen.getByRole('button', { name: /The kitchen ticket/ }));

    expect(await screen.findByText('Anna Kuteera')).toBeTruthy();
    // Which paper it DREW on, said above the paper itself.
    expect(screen.getByText('Sample · 80 mm (3 inch)')).toBeTruthy();

    call.mockClear();
    fireEvent.change(screen.getByLabelText('Ticket footer'), {
      target: { value: 'Cook it fast' },
    });

    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('preview_settings', {
        group: 'kitchen',
        edits: [{ key: 'kitchen.footer', value: 'Cook it fast' }],
      }),
    );
  });

  /** The shop's own details are not a piece of paper. */
  it('shows no paper and no roll width on the shop section', async () => {
    draw();
    await screen.findByLabelText('Shop name');

    expect(screen.queryByLabelText('Paper width')).toBeNull();
    expect(screen.queryByLabelText('Preview of what prints')).toBeNull();
    expect(call).not.toHaveBeenCalledWith('preview_settings', expect.anything());

    // And it is on the section that DOES design a piece of paper.
    fireEvent.click(screen.getByRole('button', { name: /The kitchen ticket/ }));
    expect(await screen.findByLabelText('Paper width')).toBeTruthy();
    expect(screen.getByLabelText('Preview of what prints')).toBeTruthy();
  });

  /** A size and its bold tick are one decision, so they are one line. */
  it('draws a size and its bold tick on one named line', async () => {
    draw();
    await screen.findByLabelText('Shop name');
    fireEvent.click(screen.getByRole('button', { name: /The kitchen ticket/ }));

    // Named once, by the name Rust gave the line.
    const line = (await screen.findByText('Title')).closest('.mb-settings__line');
    expect(line).toBeTruthy();

    // Both controls are on it, and both still answer to their full name.
    expect(line!.contains(screen.getByLabelText('Title size'))).toBe(true);
    expect(line!.contains(screen.getByLabelText('Title in bold'))).toBe(true);
    // The short words are what a person reads, not "Title size" again.
    expect(line!.textContent).toContain('Bold');
    expect(line!.textContent).not.toContain('Title size');

    // And editing either one marks the LINE, not a box inside it.
    fireEvent.click(screen.getByLabelText('Title in bold'));
    expect(line!.textContent).toContain('not saved');
  });

  it('asks for the KITCHEN sample on the kitchen section', async () => {
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
            dots: 576,
            columns: 44,
            millimetres: 24,
            engine: 'raster',
            notes: [],
            lines: [line('Anna Kuteera')],
          },
        });
      }
      return Promise.resolve(null);
    });
    draw();
    await screen.findByLabelText('Shop name');
    fireEvent.click(screen.getByRole('button', { name: /The kitchen ticket/ }));

    // The paper is still drawn.
    expect(await screen.findByText('Anna Kuteera')).toBeTruthy();
    // ...and the box that could not be used is named.
    expect(screen.getByText(/Not used yet: Logo width/)).toBeTruthy();
  });

  /** Reset is shown as unsaved edits, so it can be looked at and cancelled. */
  it('does not save a reset — it fills the boxes and waits', async () => {
    draw();
    await screen.findByLabelText('Shop name');
    fireEvent.click(screen.getByRole('button', { name: /Reset this section/ }));

    await waitFor(() => expect(screen.getByLabelText('Shop name')).toHaveValue(''));
    expect(call).not.toHaveBeenCalledWith('save_settings', expect.anything());
    expect(screen.getByText(/1 setting changed and not saved/)).toBeTruthy();
  });
});
