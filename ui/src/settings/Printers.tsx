/**
 * **The printer setup** — audit Part 3's "Printer Settings", and scope 7.11.
 *
 * A printer is a record, not a scalar, so it does not come down the catalogue
 * with the other ninety settings and it gets its own screen. What it shares
 * with them is the rule: every decision — what a paper size may be, what a
 * network address has to look like, whether this printer may be removed — is
 * Rust's, and this file collects characters and draws what it is handed.
 */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Card,
  Checkbox,
  EmptyState,
  Input,
  Modal,
  SectionHeader,
  Select,
  Spinner,
  useToast,
} from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { PrinterEdit } from '../ipc/generated/PrinterEdit';
import type { PrinterRowView } from '../ipc/generated/PrinterRowView';
import type { PrintersView } from '../ipc/generated/PrintersView';

const PAPERS = [
  { value: '58', label: '58 mm (2 inch)' },
  { value: '80', label: '80 mm (3 inch)' },
  { value: '100', label: '100 mm (4 inch)' },
];

const KINDS = [
  { value: 'spooler', label: 'Through Windows' },
  { value: 'network', label: 'Over the network' },
  { value: 'serial', label: 'Serial cable' },
  { value: 'none', label: 'Not connected yet' },
];

const ROLES = [
  { value: 'both', label: 'Bills and kitchen tickets' },
  { value: 'bill', label: 'Bills only' },
  { value: 'kitchen', label: 'Kitchen tickets only' },
];

const ENGINES = [
  { value: 'raster', label: 'Graphics — prints exactly like the preview' },
  { value: 'text', label: "Text — the printer's own font, faster" },
];

function blank(): PrinterEdit {
  return {
    id: '',
    name: '',
    kind: 'spooler',
    address: '',
    paperMm: 80,
    isDefault: false,
    role: 'both',
    engine: 'raster',
    isBoldDark: false,
    canKickDrawer: false,
  };
}

export function Printers() {
  const [view, setView] = useState<PrintersView | null>(null);
  const [editing, setEditing] = useState<PrinterEdit | null>(null);
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
    async <T,>(work: Promise<T>, then?: (value: T) => void) => {
      try {
        then?.(await work);
      } catch (cause) {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      }
    },
    [toast],
  );

  if (!view) return <Spinner label="Reading the printer setup" />;

  const chosen = view.printers.find((p) => p.isDefault) ?? view.printers[0] ?? null;
  const editorFor = (printer: PrinterRowView): PrinterEdit => ({
    id: printer.id,
    name: printer.name,
    kind: printer.kind,
    address: printer.address,
    paperMm: printer.paperMm,
    isDefault: printer.isDefault,
    role: printer.role,
    engine: printer.engine,
    isBoldDark: printer.isBoldDark,
    canKickDrawer: printer.canKickDrawer,
  });

  return (
    <div className="mb-printers">
      <div className="mb-settings__head">
        <h2 className="mb-settings__title">Printers</h2>
        <Button variant="primary" small onClick={() => setEditing(blank())}>
          Add a printer
        </Button>
      </div>

      {view.printers.length === 0 ? (
        <EmptyState
          title="No printer is set up"
          body="Add one, and the counter will still bill in the meantime — a shop with no printer spools its paper and prints it when one appears."
        />
      ) : null}

      {/*
        **WHERE BILLS PRINT — one question, at the top, in a dropdown.**

        The owner, 2026-08-17: *"the printer setting there is completely wrong
        flow is there, no default printer selection option… just make it very
        simple, just show option to select a default printer using drop down
        and its settting like test print etc."*

        They were describing a real hole. This screen was a flat list of
        equal-looking cards, and which one the shop actually prints on was a
        checkbox at the bottom of the add-a-printer dialog worded "Bills go
        here unless something says otherwise". So a shop added its printer,
        saw it listed, and kept printing to the stand-in — see
        `save_printer_on`, where the other half of that bug lived.

        The dropdown is the whole answer to "where do my bills come out?", and
        everything a person does next — test it, straighten it, change it —
        is directly underneath it rather than in a card they have to find.
      */}
      {chosen ? (
        <Card>
          <SectionHeader
            title="Where bills print"
            note="Kitchen tickets go here too, unless a category below says otherwise."
          />
          <Select
            label="This shop's printer"
            value={chosen.id}
            options={view.printers.map((p) => ({
              value: p.id,
              label: p.isStandIn ? `${p.name} — prints nothing` : `${p.name} · ${p.connection}`,
            }))}
            onChange={(event) =>
              void run(
                call('set_default_printer', { printerId: event.currentTarget.value }),
                (next) => {
                  setView(next);
                  toast.show('ok', 'Bills print there now.');
                },
              )
            }
          />

          {/* **The one warning worth interrupting somebody for.** A shop whose
              bills go to the stand-in prints nothing at all, and nothing else
              on this screen would tell them — the jobs queue, are marked done
              and are thrown away, exactly as that row promises. */}
          {chosen.isStandIn ? (
            <p className="mb-field__error" role="alert">
              <span aria-hidden="true">⚠</span>
              Nothing is printing. This is the stand-in every shop starts with —
              add your real printer, or choose it above.
            </p>
          ) : null}

          <PrinterSettings
            printer={chosen}
            onEdit={() => setEditing(editorFor(chosen))}
            onTest={() =>
              void run(call('print_sample_bill', { printerId: chosen.id }), () =>
                toast.show(
                  'info',
                  'A sample bill has gone to the printer. Look at the paper, then nudge it if it is off-centre.',
                ),
              )
            }
            onSlip={() =>
              void run(call('print_test_page', { printerId: chosen.id }), () =>
                toast.show(
                  'info',
                  'The slip is printing. The ruler on it is what the arrows below move.',
                ),
              )
            }
            onNudge={(dx, dy) =>
              void run(call('nudge_printer', { printerId: chosen.id, dxMm: dx, dyMm: dy }), setView)
            }
          />
        </Card>
      ) : null}

      {/* **Scope 3.1, and it does something now.**
          It was drawn only when `printers.length > 1 && routes.length > 0`, so
          a shop with one printer never saw it and could not discover it before
          buying a second — and, more to the point, `print_kitchen_ticket_on`
          never read what it saved. See `flows::routed_printer`.

          Shown whenever the shop has categories, with the reason it is greyed
          out said in words rather than by not being there. */}
      {view.routes.length > 0 ? (
        <Card>
          <SectionHeader
            title="Which printer each kind of food goes to"
            note="Anything not listed goes to the printer above."
          />
          {/* **The stand-in is not a station.** It is excluded from the count
              and from the options below: sending the tandoor's food to the
              printer whose whole job is to print nothing is never what
              somebody means, and offering it invites exactly the silent
              failure this round is about. */}
          {view.printers.filter((p) => p.role !== 'bill' && !p.isStandIn).length < 2 ? (
            <p className="mb-field__hint">
              This shop has one kitchen printer, so everything goes there. Add a
              second and you can send the tandoor's food to the tandoor.
            </p>
          ) : (
            <div className="mb-printers__routes">
              {view.routes.map((route) => (
                <Select
                  key={route.categoryId}
                  label={route.category}
                  value={route.printerId}
                  options={[
                    { value: '', label: 'The printer above' },
                    ...view.printers
                      .filter((p) => p.role !== 'bill' && !p.isStandIn)
                      .map((p) => ({ value: p.id, label: p.name })),
                  ]}
                  onChange={(event) =>
                    void run(
                      call('route_category', {
                        categoryId: route.categoryId,
                        printerId: event.currentTarget.value,
                      }),
                      (next) => {
                        setView(next);
                        toast.show('ok', `${route.category} tickets print there now.`);
                      },
                    )
                  }
                />
              ))}
            </div>
          )}
        </Card>
      ) : null}

      {/* Every printer the shop has, for adding, changing and removing. The
          one it prints on is above; this is the cupboard. */}
      {view.printers.length > 0 ? (
        <Card>
          <SectionHeader
            title="Every printer set up here"
            note={`${view.printers.length} in total`}
          />
          <div className="mb-printers__all">
            {view.printers.map((printer) => (
              <div className="mb-printers__row" key={printer.id}>
                <div className="mb-stack">
                  <span className="mb-printers__name">
                    {printer.name}
                    {printer.isDefault ? <Badge tone="accent">Bills print here</Badge> : null}
                  </span>
                  <span className="mb-field__hint">
                    {printer.connection} · {printer.paperMm} mm ·{' '}
                    {printer.role === 'bill'
                      ? 'bills'
                      : printer.role === 'kitchen'
                        ? 'kitchen tickets'
                        : 'bills and kitchen tickets'}
                    {printer.canKickDrawer ? ' · opens the drawer' : ''}
                  </span>
                </div>
                <div className="mb-row mb-row--end">
                  <Button small onClick={() => setEditing(editorFor(printer))}>
                    Change
                  </Button>
                  {printer.isStandIn ? null : (
                    <Button
                      small
                      variant="quiet"
                      onClick={() =>
                        void run(call('delete_printer', { id: printer.id }), (next) => {
                          setView(next);
                          toast.show('info', 'That printer has gone.');
                        })
                      }
                    >
                      Remove
                    </Button>
                  )}
                </div>
              </div>
            ))}
          </div>
        </Card>
      ) : null}

      <PrinterDialog
        edit={editing}
        windows={view.windows}
        onClose={() => setEditing(null)}
        onSave={(next) =>
          void run(call('save_printer', { edit: next }), (updated) => {
            setView(updated);
            setEditing(null);
            toast.show(
              'ok',
              'Saved. New jobs use it after the next restart — anything already waiting keeps its own printer.',
            );
          })
        }
      />
    </div>
  );
}

/**
 * **What you do with the printer once you have chosen it**: prove it works,
 * straighten what it puts on the paper, change how it is set up.
 *
 * It sits directly under the dropdown rather than in a card of its own, because
 * *"select a default printer and its settting like test print etc."* is one
 * thought and used to be spread across a list of identical cards.
 */
function PrinterSettings({
  printer,
  onEdit,
  onTest,
  onSlip,
  onNudge,
}: {
  printer: PrinterRowView;
  onEdit: () => void;
  onTest: () => void;
  /** P07's alignment slip — see the button's own note. */
  onSlip: () => void;
  onNudge: (dx: number, dy: number) => void;
}) {
  return (
    <>
      <div className="mb-row">
        <Button onClick={onTest}>Print a sample bill</Button>
        {/* **The slip is not the sample bill, and P31 stopped pretending it
            was.** P07 built it with the ALIGNMENT RULER on it, which is what
            the four nudge buttons below are read against — a sample bill has
            no ruler, so nudging by looking at one is guesswork.

            It is also the only thing on this screen that prints with no shop
            open, which is exactly when somebody needs to know whether the
            printer works: a first run, and the minutes after a restore. */}
        <Button variant="quiet" onClick={onSlip}>
          Print the alignment slip
        </Button>
        <Button onClick={onEdit}>Change how it is set up</Button>
      </div>

      {/* **Scope 7.11.** Thermal printers disagree about where the first dot
          sits, so the same correct document comes out 2 mm off-centre on one
          model and centred on another. The owner corrects it from the paper,
          in millimetres — which is why there is no number to type. */}
      <div className="mb-printers__nudge">
        <span className="mb-field__hint">
          Off-centre on the paper? Print a sample, then nudge it. Now at{' '}
          {printer.offsetXMm >= 0 ? `+${printer.offsetXMm}` : printer.offsetXMm} mm across,{' '}
          {printer.offsetYMm >= 0 ? `+${printer.offsetYMm}` : printer.offsetYMm} mm down.
        </span>
        <div className="mb-row">
          <Button small onClick={() => onNudge(-1, 0)} aria-label="Move the print 1 mm left">
            ← 1 mm
          </Button>
          <Button small onClick={() => onNudge(1, 0)} aria-label="Move the print 1 mm right">
            1 mm →
          </Button>
          <Button small onClick={() => onNudge(0, -1)} aria-label="Move the print 1 mm up">
            ↑ 1 mm
          </Button>
          <Button small onClick={() => onNudge(0, 1)} aria-label="Move the print 1 mm down">
            ↓ 1 mm
          </Button>
        </div>
      </div>
    </>
  );
}

function PrinterDialog({
  edit,
  windows,
  onClose,
  onSave,
}: {
  edit: PrinterEdit | null;
  windows: readonly string[];
  onClose: () => void;
  onSave: (edit: PrinterEdit) => void;
}) {
  const [draft, setDraft] = useState<PrinterEdit>(blank());

  useEffect(() => {
    if (edit) setDraft(edit);
  }, [edit]);

  const set = (patch: Partial<PrinterEdit>) => setDraft({ ...draft, ...patch });

  return (
    <Modal
      open={edit !== null}
      title={edit?.id ? 'Change this printer' : 'Add a printer'}
      onClose={onClose}
      wide
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={() => onSave(draft)}>
            Save this printer
          </Button>
        </>
      }
    >
      <div className="mb-stack">
        <Input
          label="What you call it"
          hint="Counter, Kitchen, Bar — the name you will look for."
          value={draft.name}
          onChange={(event) => set({ name: event.currentTarget.value })}
        />
        <Select
          label="How it is connected"
          value={draft.kind}
          options={KINDS}
          onChange={(event) => set({ kind: event.currentTarget.value, address: '' })}
        />

        {draft.kind === 'spooler' ? (
          windows.length > 0 ? (
            <Select
              label="Which Windows printer"
              hint="Asked of Windows each time this screen opens, so one installed a minute ago is here."
              value={draft.address}
              options={[
                { value: '', label: 'Choose one' },
                ...windows.map((name) => ({ value: name, label: name })),
              ]}
              onChange={(event) => set({ address: event.currentTarget.value })}
            />
          ) : (
            <Input
              label="Which Windows printer"
              hint="Windows lists no printers on this machine. Type the name if you know it."
              value={draft.address}
              onChange={(event) => set({ address: event.currentTarget.value })}
            />
          )
        ) : null}

        {draft.kind === 'network' ? (
          <Input
            label="Address and port"
            hint="Like 192.168.1.50:9100. Most thermal printers use 9100."
            value={draft.address}
            onChange={(event) => set({ address: event.currentTarget.value })}
          />
        ) : null}

        {draft.kind === 'serial' ? (
          <Input
            label="Serial port"
            hint="Like COM3."
            value={draft.address}
            onChange={(event) => set({ address: event.currentTarget.value })}
          />
        ) : null}

        {/* **One paper size.** v1 had two, in two places, and they could
            disagree — and paper belongs to a printer, not to a shop. */}
        <Select
          label="Paper width"
          value={String(draft.paperMm)}
          options={PAPERS}
          onChange={(event) => set({ paperMm: Number(event.currentTarget.value) })}
        />
        <Select
          label="What it prints"
          value={draft.role}
          options={ROLES}
          onChange={(event) => set({ role: event.currentTarget.value })}
        />
        <Select
          label="How it prints"
          hint="Graphics is exactly what the preview shows. Text uses the printer's own font and is quicker on an old machine."
          value={draft.engine}
          options={ENGINES}
          onChange={(event) => set({ engine: event.currentTarget.value })}
        />
        <Checkbox
          label="Bold and dark"
          checked={draft.isBoldDark}
          onChange={(event) => set({ isBoldDark: event.currentTarget.checked })}
        />
        <Checkbox
          label="A cash drawer is plugged into this printer"
          checked={draft.canKickDrawer}
          onChange={(event) => set({ canKickDrawer: event.currentTarget.checked })}
        />
        {/* **It says "default" now, and it is not how you set one.**
            This was the only way to choose where bills print, worded "Bills go
            here unless something says otherwise" — a sentence that describes
            being the default without ever using the word, at the bottom of a
            dialog somebody is filling in for the first time. The dropdown at
            the top of the screen is the answer to that question now; this
            stays because setting it while adding a printer is convenient, and
            it is worded so that skipping it is safe. */}
        <Checkbox
          label="Make this the printer bills print on"
          checked={draft.isDefault}
          onChange={(event) => set({ isDefault: event.currentTarget.checked })}
        />
      </div>
    </Modal>
  );
}
