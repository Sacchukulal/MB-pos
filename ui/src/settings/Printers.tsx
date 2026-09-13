/** The printer setup: the bill printer, its paper, and how kitchen tickets are cut. */

import { useCallback, useEffect, useState } from 'react';

import { Button, Card, Checkbox, Choice, Hint, SectionHeader, Select, Spinner, Table, useToast, type Column } from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { PrintersView } from '../ipc/generated/PrintersView';
import type { RouteView } from '../ipc/generated/RouteView';

const PAPERS = [
  { value: '58', label: '2 inch (58 mm)' },
  { value: '80', label: '3 inch (80 mm)' },
  { value: '100', label: '4 inch (100 mm)' },
];

/** The Windows printers as a pick list, with the one already chosen kept even if Windows lost it. */
function printerOptions(windows: readonly string[], chosen: string, none: string) {
  const names = chosen && !windows.includes(chosen) ? [chosen, ...windows] : windows;
  return [{ value: '', label: none }, ...names.map((name) => ({ value: name, label: name }))];
}

/** "+2" or "-1", for an offset. */
function signed(n: number): string {
  return n >= 0 ? `+${n}` : String(n);
}

export function Printers() {
  const [view, setView] = useState<PrintersView | null>(null);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    if (!inApp()) return;
    call('printer_setup').then(setView).catch(complain);
  }, [complain]);

  useEffect(load, [load]);

  const run = useCallback(
    async (work: Promise<PrintersView>, said?: string) => {
      try {
        setView(await work);
        if (said) toast.show('ok', said);
      } catch (cause) {
        complain(cause);
      }
    },
    [complain, toast],
  );

  if (!view) return <Spinner label="Reading the printer setup" />;

  const bill = view.printers.find((p) => p.isDefault) ?? null;
  const billName = bill && !bill.isStandIn ? bill.address : '';
  /** A category's printer, as the Windows name the shop picks from. */
  const windowsNameOf = (printerId: string) =>
    view.printers.find((p) => p.id === printerId && p.kind === 'spooler')?.address ?? '';
  const separate = view.kitchenMode === 'other';

  const routeColumns: Column<RouteView>[] = [
    { key: 'category', header: 'Category', render: (r) => r.category },
    {
      key: 'printer',
      header: 'Printer',
      render: (r) => (
        <Select
          aria-label={`Printer for ${r.category}`}
          value={windowsNameOf(r.printerId)}
          options={printerOptions(view.windows, windowsNameOf(r.printerId), 'The bill printer')}
          onChange={(event) =>
            void run(
              call('route_category_to', {
                categoryId: r.categoryId,
                windowsName: event.currentTarget.value,
              }),
              `${r.category} tickets print there now.`,
            )
          }
        />
      ),
    },
  ];

  return (
    <div className="mb-printers">
      <Card>
        <SectionHeader title="Printer" />
        <div className="mb-printers__fields">
          <Select
            label="Printer"
            value={billName}
            options={printerOptions(view.windows, billName, 'None yet')}
            onChange={(event) =>
              void run(
                call('choose_bill_printer', { windowsName: event.currentTarget.value }),
                event.currentTarget.value ? 'Bills print there now.' : 'No printer: bills print nothing.',
              )
            }
          />
          <Select
            label="Paper"
            value={String(bill?.paperMm ?? 80)}
            disabled={billName === ''}
            options={PAPERS}
            onChange={(event) =>
              void run(
                call('set_paper_size', { mm: Number(event.currentTarget.value) }),
                `Bills print on ${event.currentTarget.value} mm paper now.`,
              )
            }
          />
        </div>
        {bill && billName !== '' ? (
          <>
            <Checkbox
              label="Cash drawer on this printer"
              checked={bill.canKickDrawer}
              onChange={(event) => void run(call('set_drawer', { on: event.currentTarget.checked }))}
            />
            <div className="mb-printers__test">
              <Button
                onClick={() =>
                  void call('print_sample_bill', { printerId: bill.id })
                    .then(() => toast.show('info', 'A sample bill has gone to the printer.'))
                    .catch(complain)
                }
              >
                Print a sample bill
              </Button>
              <Button
                variant="quiet"
                onClick={() =>
                  void call('print_test_page', { printerId: bill.id })
                    .then(() => toast.show('info', 'The alignment slip is printing.'))
                    .catch(complain)
                }
              >
                Alignment slip
              </Button>
              <span className="mb-printers__nudge" role="group" aria-label="Nudge the print">
                <Hint>
                  {signed(bill.offsetXMm)} mm across, {signed(bill.offsetYMm)} mm down
                </Hint>
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
              </span>
            </div>
          </>
        ) : null}
      </Card>

      <Card>
        <SectionHeader title="Kitchen tickets" />
        <Choice
          label="Printers"
          value={separate ? 'other' : 'same'}
          options={[
            { value: 'same', label: 'Single printer' },
            { value: 'other', label: 'Separate printers' },
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
        {separate ? (
          view.routes.length === 0 ? (
            <p className="mb-muted">The menu has no categories yet.</p>
          ) : (
            <Table dense rows={view.routes} columns={routeColumns} rowKey={(r) => r.categoryId} />
          )
        ) : (
          <Choice
            label="Tickets"
            value={view.ticketStyle === 'category' ? 'category' : 'combined'}
            options={[
              { value: 'combined', label: 'One ticket for everything' },
              { value: 'category', label: 'One ticket per category' },
            ]}
            onPick={(style) =>
              void run(
                call('set_ticket_style', { style }),
                style === 'combined'
                  ? 'One kitchen ticket per order.'
                  : 'Each category prints its own ticket.',
              )
            }
          />
        )}
      </Card>
    </div>
  );
}
