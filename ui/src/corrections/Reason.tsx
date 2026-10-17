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
  /** Wider presentation for bill returns; other callers keep their existing width. */
  wide?: boolean;
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
  wide = false,
}: ReasonDialogProps) {
  const [choices, setChoices] = useState<readonly ReasonView[]>([]);
  const [chosen, setChosen] = useState<string>('');
  const [note, setNote] = useState('');
  const [approver, setApprover] = useState('');
  const [pin, setPin] = useState('');
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inFlight = useRef(false);
  const reasonList = useRef<HTMLDivElement>(null);

  useEffect(() => {
    reasonList.current?.querySelector<HTMLInputElement>('input:checked')?.focus();
  }, [choices]);

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
    <Modal open title={what} wide={wide} onClose={() => { if (!inFlight.current) onCancel(); }}
      onEnter={() => void confirm()}
      actions={
        <>
          <Button variant="quiet" disabled={busy} onClick={onCancel}>
            Cancel
          </Button>
          <Button variant="danger" disabled={busy || disabled} onClick={() => void confirm()}>
            {busy ? 'Saving…' : confirmLabel}
          </Button>
        </>
      }
    >
      <div onKeyDown={(event) => {
        if (event.nativeEvent.isComposing || event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return;
        const target = event.target as HTMLElement;
        if (event.key === 'Enter' && !target.closest('button, textarea, select, [contenteditable="true"]')) {
          event.preventDefault();
          event.stopPropagation();
          if (!event.repeat) void confirm();
        }
        if ((event.key === 'ArrowDown' || event.key === 'ArrowUp') &&
          (reasonList.current?.contains(target) || target.dataset.reasonNote === 'true')) {
          event.preventDefault();
          event.stopPropagation();
          if (busy || disabled || choices.length === 0) return;
          const next = (choices.findIndex((choice) => choice.text === chosen) +
            (event.key === 'ArrowDown' ? 1 : -1) + choices.length) % choices.length;
          setChosen(choices[next]!.text);
          setProblem(null);
          reasonList.current?.querySelectorAll<HTMLInputElement>('input')[next]?.focus();
        }
      }}>
      {description ? <p className="mb-muted">{description}</p> : null}
      {children}
      <div className="mb-reasons" ref={reasonList}>
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
        data-reason-note="true"
        label="Additional note"
        hint="Optional. Recorded in history with your name."
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
      </div>

    </Modal>
  );
}
