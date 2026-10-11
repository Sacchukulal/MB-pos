import { useState, type ReactNode } from 'react';

import { Button, DateRangePicker, Modal, RowMenu } from '../kit';
import type { PeriodChoiceView } from '../ipc/generated/PeriodChoiceView';

/** The report rail supplies the visible title; the header contains only useful controls. */
export function ReportHeader({ title, children, summary, info, actions }: {
  title: string;
  children?: ReactNode;
  summary?: ReactNode;
  info?: ReactNode;
  actions?: ReactNode;
}) {
  const [showInfo, setShowInfo] = useState(false);
  return <header className="mb-reportheader">
    <h1 className="mb-visually-hidden">{title}</h1>
    {children}
    {summary || info || actions ? <div className="mb-reportheader__line">
      {summary ? <div className="mb-reportheader__summary">{summary}</div> : null}
      {info || actions ? <div className="mb-reportheader__actions">
        {info ? <Button size="sm" variant="secondary" onClick={() => setShowInfo(true)}>Report info</Button> : null}
        {actions}
      </div> : null}
    </div> : null}
    {info ? <Modal open={showInfo} title={title} small onClose={() => setShowInfo(false)}
      actions={<Button onClick={() => setShowInfo(false)}>Close</Button>}>{info}</Modal> : null}
  </header>;
}

/** Bills and reports share presentation, while keeping their existing independent date state. */
export function ReportPeriod({ periods, from, to, onChange }: {
  periods: readonly PeriodChoiceView[];
  from: string;
  to: string;
  onChange: (from: string, to: string) => void;
}) {
  return <div className="mb-reportheader__period">
    <div className="mb-reports__presets" role="group" aria-label="Report period">
      {periods.map((choice) => <Button key={choice.label} size="sm" variant="quiet"
        aria-pressed={choice.from === from && choice.to === to}
        onClick={() => onChange(choice.from, choice.to)}>{choice.label}</Button>)}
    </div>
    <DateRangePicker size="sm" from={from} to={to} onChange={onChange} />
  </div>;
}

export function ReportExport({ disabled, onExport }: {
  disabled?: boolean;
  onExport: (format: 'csv' | 'pdf') => void;
}) {
  return <RowMenu text="Export" label="Export" icon="chevron-down" keyboardNavigation disabled={disabled}>
    <Button role="menuitem" size="sm" variant="quiet" onClick={() => onExport('csv')}>CSV</Button>
    <Button role="menuitem" size="sm" variant="quiet" onClick={() => onExport('pdf')}>PDF</Button>
  </RowMenu>;
}
