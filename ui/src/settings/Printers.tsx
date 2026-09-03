/** The printer setup: where bills print, and how kitchen tickets are cut. */

import { useCallback, useEffect, useState } from 'react';

import { Button, Card, Checkbox, SectionHeader, Select, Spinner, useToast } from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { PrintersView } from '../ipc/generated/PrintersView';

const PAPERS = [
  { value: '58', label: '58 mm (2 inch)' },
  { value: '80', label: '80 mm (3 inch)' },
  { value: '100', label: '100 mm (4 inch)' },
];

/** The Windows printers as a pick list, with the one already chosen kept even if Windows lost it. */
function printerOptions(windows: readonly string[], chosen: string, none: string) {
  const names = chosen && !windows.includes(chosen) ? [chosen, ...windows] : windows;
  return [{ value: '', label: none }, ...names.map((name) => ({ value: name, label: name }))];
}

export function Printers() {
  const [view, setView] = useState<PrintersView | null>(null);
  const toast = useToast();

  const load = useCallback(() => {
    if (!inApp()) return;
    call('printer_setup')
      .then(setView)
      .catch((cause) => {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      });
  }, [toast]);

  useEffect(load, [load]);

  const run = useCallback(
    async (work: Promise<PrintersView>, said?: string) => {
      try {
        setView(await work);
        if (said) toast.show('ok', said);
      } catch (cause) {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      }
    },
    [toast],
  );

  if (!view) return <Spinner label="Reading the printer setup" />;

  const bill = view.printers.find((p) => p.isDefault) ?? null;
  const billName = bill && !bill.isStandIn ? bill.address : '';
  /** A category's printer, as the Windows name the shop picks from. */
  const windowsNameOf = (printerId: string) =>
    view.printers.find((p) => p.id === printerId && p.kind === 'spooler')?.address ?? '';
  const separate = view.kitchenMode === 'other';
  const perCategory = view.ticketStyle === 'category';

  return (
    <div className="mb-printers">
      <SectionHeader title="Printers" sticky />

      {/* ONE: the printer. */}
      <Card>
        <SectionHeader title="Bills print on" />
        <Select
          label="Printer"
          value={billName}
          options={printerOptions(view.windows, billName, 'No printer yet — bills print nothing')}
          onChange={(event) =>
            void run(
              call('choose_bill_printer', { windowsName: event.currentTarget.value }),
              event.currentTarget.value ? 'Bills print there now.' : 'No printer: bills print nothing.',
            )
          }
        />
        {bill && !bill.isStandIn ? (
          <>
            <div className="mb-printers__fields">
              <Select
                label="Paper width"
                value={String(bill.paperMm)}
                options={PAPERS}
                onChange={(event) =>
                  void run(
                    call('set_paper_size', { mm: Number(event.currentTarget.value) }),
                    `Bills print on ${event.currentTarget.value} mm paper now.`,
                  )
                }
              />
              <Checkbox
                label="A cash drawer is plugged into this printer"
                checked={bill.canKickDrawer}
                onChange={(event) =>
                  void run(call('set_drawer', { on: event.currentTarget.checked }))
                }
              />
            </div>
            <div className="mb-row">
              <Button
                onClick={() =>
                  void call('print_sample_bill', { printerId: bill.id })
                    .then(() => toast.show('info', 'A sample bill has gone to the printer.'))
                    .catch((cause) => {
                      if (isUiError(cause)) toast.show('danger', cause.message);
                    })
                }
              >
                Print a sample bill
              </Button>
              <Button
                variant="quiet"
                onClick={() =>
                  void call('print_test_page', { printerId: bill.id })
                    .then(() => toast.show('info', 'The alignment slip is printing. Its ruler is what the arrows move.'))
                    .catch((cause) => {
                      if (isUiError(cause)) toast.show('danger', cause.message);
                    })
                }
              >
                Print the alignment slip
              </Button>
              <span className="mb-field__hint">
                Off-centre? Nudge it: now {signed(bill.offsetXMm)} mm across, {signed(bill.offsetYMm)} mm
                down.
              </span>
              {(
                [
                  ['← 1 mm', -1, 0, 'Move the print 1 mm left'],
                  ['1 mm →', 1, 0, 'Move the print 1 mm right'],
                  ['↑ 1 mm', 0, -1, 'Move the print 1 mm up'],
                  ['↓ 1 mm', 0, 1, 'Move the print 1 mm down'],
                ] as const
              ).map(([word, dx, dy, says]) => (
                <Button
                  key={word}
                  size="sm"
                  aria-label={says}
                  onClick={() =>
                    void run(call('nudge_printer', { printerId: bill.id, dxMm: dx, dyMm: dy }))
                  }
                >
                  {word}
                </Button>
              ))}
            </div>
          </>
        ) : null}
      </Card>

      {/* TWO: the kitchen tickets. */}
      <Card>
        <SectionHeader title="Kitchen tickets" />
        <Choice
          label="Which printer"
          value={separate ? 'other' : 'same'}
          options={[
            { value: 'same', label: 'The bill printer' },
            { value: 'other', label: 'Other printers, by category' },
          ]}
          onPick={(mode) =>
            void run(
              call('set_kitchen_mode', { mode }),
              mode === 'same'
                ? 'Kitchen tickets print with the bills.'
                : 'Choose a printer for each category below.',
            )
          }
        />
        <Choice
          label="How many tickets"
          value={perCategory ? 'category' : 'combined'}
          options={[
            { value: 'combined', label: 'One ticket for everything' },
            { value: 'category', label: 'One ticket per category' },
          ]}
          onPick={(style) =>
            void run(
              call('set_ticket_style', { style }),
              style === 'combined'
                ? 'One kitchen ticket per printer.'
                : 'Each category prints its own ticket.',
            )
          }
        />

        {separate ? (
          view.routes.length === 0 ? (
            <p className="mb-field__hint">The menu has no categories yet, so there is nothing to send apart.</p>
          ) : (
            <div className="mb-printers__routes">
              {view.routes.map((route) => (
                <Select
                  key={route.categoryId}
                  label={route.category}
                  value={windowsNameOf(route.printerId)}
                  options={printerOptions(
                    view.windows,
                    windowsNameOf(route.printerId),
                    'The bill printer',
                  )}
                  onChange={(event) =>
                    void run(
                      call('route_category_to', {
                        categoryId: route.categoryId,
                        windowsName: event.currentTarget.value,
                      }),
                      `${route.category} tickets print there now.`,
                    )
                  }
                />
              ))}
            </div>
          )
        ) : null}
      </Card>
    </div>
  );
}

/** A choice of two, as two buttons: the one in force is filled. */
function Choice({
  label,
  value,
  options,
  onPick,
}: {
  label: string;
  value: string;
  options: readonly { value: string; label: string }[];
  onPick: (value: string) => void;
}) {
  return (
    <div className="mb-field">
      <span className="mb-field__label">{label}</span>
      <div className="mb-row" role="group" aria-label={label}>
        {options.map((option) => (
          <Button
            key={option.value}
            variant={option.value === value ? 'primary' : 'secondary'}
            aria-pressed={option.value === value}
            onClick={() => option.value !== value && onPick(option.value)}
          >
            {option.label}
          </Button>
        ))}
      </div>
    </div>
  );
}

/** "+2" or "-1", for an offset. */
function signed(n: number): string {
  return n >= 0 ? `+${n}` : String(n);
}
