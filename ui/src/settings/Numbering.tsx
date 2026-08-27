/** The bill number and the token number. */

import { useCallback, useEffect, useState } from 'react';

import { Button, Card, Checkbox, Input, NumberInput, SectionHeader, Spinner, useToast } from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { CounterEdit } from '../ipc/generated/CounterEdit';
import type { CounterView } from '../ipc/generated/CounterView';
import type { NumberingView } from '../ipc/generated/NumberingView';

export function Numbering() {
  const [view, setView] = useState<NumberingView | null>(null);
  const toast = useToast();

  const load = useCallback(() => {
    if (!inApp()) return;
    call('numbering')
      .then(setView)
      .catch((cause) => {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      });
  }, [toast]);

  useEffect(load, [load]);

  const save = useCallback(
    async (edit: CounterEdit) => {
      try {
        setView(await call('save_counter', { edit }));
        toast.show('ok', 'Saved.');
      } catch (cause) {
        // The refusal is the feature here, so it is shown for a long time and in full: it
        // explains what a duplicate bill number does to a GST return, which is the whole reason
        // the rule exists.
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      }
    },
    [toast],
  );

  if (!view) return <Spinner label="Reading the numbering" />;

  return (
    <div className="mb-numbering">
      {view.counters.map((counter) => (
        <Counter key={counter.kind} counter={counter} onSave={save} />
      ))}
    </div>
  );
}

function Counter({
  counter,
  onSave,
}: {
  counter: CounterView;
  onSave: (edit: CounterEdit) => void;
}) {
  const [draft, setDraft] = useState<CounterEdit>(toEdit(counter));

  // The saved view is the truth; a fresh one replaces whatever was typed.
  useEffect(() => setDraft(toEdit(counter)), [counter]);

  const set = (patch: Partial<CounterEdit>) => setDraft({ ...draft, ...patch });
  const dirty = JSON.stringify(draft) !== JSON.stringify(toEdit(counter));

  return (
    <Card>
      <SectionHeader title={counter.label} note={counter.help} />

      {/* What the next one will actually say, as ONE sentence from Rust. */}
      <p className="mb-numbering__next">{counter.summary}</p>

      <div className="mb-numbering__fields">
        <Input
          label="Prefix"
          hint="Printed in front of the number. Leave it empty for none."
          value={draft.prefix}
          maxLength={8}
          onChange={(event) => set({ prefix: event.currentTarget.value })}
        />
        <NumberInput
          label="Pad with zeroes to"
          hint="Digits. 0 means no padding: bill 7 prints as 7, not 0007."
          value={String(draft.padWidth)}
          onChange={(event) => set({ padWidth: Number(event.currentTarget.value) || 0 })}
        />
        <NumberInput
          label="Start at"
          hint="What the series goes back to when it resets."
          value={String(draft.start)}
          onChange={(event) => set({ start: Number(event.currentTarget.value) || 1 })}
        />
        <NumberInput
          label="The next number"
          hint={
            counter.kind === 'bill'
              ? 'IT MUST NEVER GO BACKWARDS. Two bills with the same number is a GST return the department will reject.'
              : 'What the next customer will be called.'
          }
          value={String(draft.nextValue)}
          onChange={(event) => set({ nextValue: Number(event.currentTarget.value) || 1 })}
        />
      </div>

      <Checkbox
        label="Start again every day"
        checked={draft.resetDaily}
        onChange={(event) => set({ resetDaily: event.currentTarget.checked })}
      />

      <div className="mb-row mb-row--end">
        <Button variant="primary" small disabled={!dirty} onClick={() => onSave(draft)}>
          Save this counter
        </Button>
      </div>
    </Card>
  );
}

function toEdit(counter: CounterView): CounterEdit {
  return {
    kind: counter.kind,
    prefix: counter.prefix,
    padWidth: counter.padWidth,
    resetDaily: counter.resetDaily,
    start: counter.start,
    nextValue: counter.nextValue,
  };
}
