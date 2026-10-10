/** One dialog, four callers — void, cancel, void a line, reprint. */

import { useEffect, useRef, useState, type ReactNode } from 'react';

import { Button, Input, Modal, Radio } from '../kit';
import { call, isUiError } from '../ipc/call';
import type { ReasonView } from '../ipc/generated/ReasonView';
import { ApprovalFields } from './Approval';

import './corrections.css';

export type ReasonKind = 'void' | 'cancel' | 'item_void' | 'reprint';

export interface ReasonDialogProps {
  kind: ReasonKind;
  /** "Void bill 0042 — ₹450.00". */
  what: string;
  /** Longer context belongs in the body so the dialog title stays readable. */
  description?: string;
  /** The button, in the same words. */
  confirmLabel: string;
  /** True when this action needs a manager's PIN as well. */
  needsApproval?: boolean;
  approvers?: readonly { id: string; name: string }[];
  children?: ReactNode;
  disabled?: boolean;
  onCancel: () => void;
  onConfirm: (reason: string, approver?: { id: string; pin: string }) => void | Promise<void>;
}

export function ReasonDialog({
  kind,
  what,
  description,
  confirmLabel,
  needsApproval,
  approvers = [],
  onCancel,
  onConfirm,
  children,
  disabled = false,
}: ReasonDialogProps) {
  const [choices, setChoices] = useState<readonly ReasonView[]>([]);
  const [chosen, setChosen] = useState<string>('');
  const [note, setNote] = useState('');
  const [approver, setApprover] = useState('');
  const [pin, setPin] = useState('');
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inFlight = useRef(false);

  useEffect(() => {
    call('reasons', { kind })
      .then((list) => {
        setChoices(list);
        // The first is chosen, so the common case is two keystrokes.
        setChosen(list[0]?.text ?? '');
      })
      .catch((cause) => {
        // A shop whose reason list will not load can still type one.
        if (isUiError(cause)) setProblem(cause.message);
      });
  }, [kind]);

  const reason = [chosen, note.trim()].filter(Boolean).join(' — ');

  const confirm = async () => {
    if (inFlight.current || disabled) return;
    if (reason === '') {
      setProblem('Choose a reason, or type one.');
      return;
    }
    if (needsApproval && (approver === '' || pin === '')) {
      setProblem('This needs a manager: choose who, and have them type their PIN.');
      return;
    }
    inFlight.current = true;
    setBusy(true);
    try {
      await onConfirm(reason, needsApproval ? { id: approver, pin } : undefined);
    } catch (cause) {
      setProblem(isUiError(cause) ? cause.message : 'That could not be done. Try again.');
    } finally { inFlight.current = false; setBusy(false); }
  };

  return (
    <Modal open title={what} onClose={() => { if (!inFlight.current) onCancel(); }}>
      {description ? <p className="mb-muted">{description}</p> : null}
      {children}
      <div className="mb-reasons">
        {choices.map((choice) => (
          <Radio
            key={choice.id}
            name="reason"
            label={choice.text}
            checked={chosen === choice.text}
            onChange={() => {
              setChosen(choice.text);
              setProblem(null);
            }}
          />
        ))}
      </div>

      <Input
        label="Anything to add"
        hint="Optional, and it goes in the history with your name."
        value={note}
        onChange={(event) => setNote(event.target.value)}
      />

      {needsApproval ? (
        <ApprovalFields people={approvers} selected={approver} pin={pin}
          onSelect={setApprover} onPin={setPin} disabled={busy} />
      ) : null}

      {problem ? (
        <p className="mb-lock__problem" role="alert">
          {problem}
        </p>
      ) : null}

      <div className="mb-row mb-row--end">
        <Button variant="quiet" disabled={busy} onClick={onCancel}>
          Leave it
        </Button>
        <Button variant="danger" disabled={busy || disabled} onClick={() => void confirm()}>
          {busy ? 'Saving…' : confirmLabel}
        </Button>
      </div>
    </Modal>
  );
}
