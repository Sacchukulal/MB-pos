import { useState } from 'react';
import { Button, Checkbox, Modal, SearchField } from '../kit';
import type { ReportChoiceView } from '../ipc/generated/ReportChoiceView';

/** Apply a group of choices together so exports never use a half-finished selection. */
export function SelectionPicker({ label, choices, selected, onChange }: {
  label: string;
  choices: readonly ReportChoiceView[];
  selected: readonly string[];
  onChange: (ids: string[]) => void;
}) {
  const [draft, setDraft] = useState<string[] | null>(null);
  const [search, setSearch] = useState('');
  const summary = selected.length === 0 ? `All ${label.toLowerCase()}`
    : selected.length === 1 ? choices.find((choice) => choice.id === selected[0])?.label ?? '1 selected'
    : `${selected.length} selected`;
  const apply = () => { if (draft) onChange(draft); setDraft(null); };
  return <>
    <Button size="sm" aria-label={`Select ${label.toLowerCase()}`} onClick={() => { setDraft([...selected]); setSearch(''); }}>
      {label}: {summary}
    </Button>
    {draft !== null ? <Modal open title={`Select ${label.toLowerCase()}`} onClose={() => setDraft(null)}
      onEnter={apply} actions={<>
        <Button variant="quiet" onClick={() => setDraft(null)}>Cancel</Button>
        <Button variant="primary" onClick={apply}>Apply selection</Button>
      </>}>
      <div className="mb-stack">
        <SearchField what={`Search ${label.toLowerCase()}`} value={search} onChange={(event) => setSearch(event.target.value)} />
        <Button size="sm" aria-pressed={draft.length === 0} onClick={() => setDraft([])}>All {label.toLowerCase()}</Button>
        <p className="mb-muted">Choose one or more. No selection means all.</p>
        {choices.filter((choice) => choice.label.toLowerCase().includes(search.toLowerCase())).map((choice) => (
          <Checkbox key={choice.id} label={choice.label} checked={draft.includes(choice.id)}
            onChange={() => setDraft(draft.includes(choice.id) ? draft.filter((id) => id !== choice.id) : [...draft, choice.id])} />
        ))}
        {choices.length === 0 ? <p>No sales in this or the previous period.</p> : null}
      </div>
    </Modal> : null}
  </>;
}
