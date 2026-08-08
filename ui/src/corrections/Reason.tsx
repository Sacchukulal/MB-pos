/**
 * **One dialog, four callers** — void, cancel, void a line, reprint.
 *
 * The draft prompt asked for this and it was right: four dialogs would be four
 * places to forget the free-text box, four wordings of the same confirmation,
 * and four chances for one of them to stop being compulsory.
 *
 * # The confirmation says what will happen
 *
 * UI_GUIDELINES §6: *"a button says exactly what happens; the confirmation
 * echoes it."* So it is **"Void bill 0042 — ₹450.00"**, never "Confirm?" —
 * and the amount comes across preformatted from Rust (D39), because the one
 * moment you must not get a number wrong is the moment you are undoing one.
 *
 * # The reasons are the shop's
 *
 * They come from the `reasons` table, per kind. A list in this file would be a
 * support call from the first shop whose reasons are not ours.
 */

import { useEffect, useState } from 'react';

import { Button, Input, Modal, Radio } from '../kit';
import { call, isUiError } from '../ipc/call';
import type { ReasonView } from '../ipc/generated/ReasonView';

import './corrections.css';

export type ReasonKind = 'void' | 'cancel' | 'item_void' | 'reprint';

export interface ReasonDialogProps {
  kind: ReasonKind;
  /** "Void bill 0042 — ₹450.00". Already a whole sentence. */
  what: string;
  /** The button, in the same words. "Void the bill", not "OK". */
  confirmLabel: string;
  /** True when this action needs a manager's PIN as well (P12 item 4). */
  needsApproval?: boolean;
  approvers?: readonly { id: string; name: string }[];
  onCancel: () => void;
  onConfirm: (reason: string, approver?: { id: string; pin: string }) => void;
}

export function ReasonDialog({
  kind,
  what,
  confirmLabel,
  needsApproval,
  approvers = [],
  onCancel,
  onConfirm,
}: ReasonDialogProps) {
  const [choices, setChoices] = useState<readonly ReasonView[]>([]);
  const [chosen, setChosen] = useState<string>('');
  const [note, setNote] = useState('');
  const [approver, setApprover] = useState('');
  const [pin, setPin] = useState('');
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    call('reasons', { kind })
      .then((list) => {
        setChoices(list);
        // The first is chosen, so the common case is two keystrokes.
        setChosen(list[0]?.text ?? '');
      })
      .catch((cause) => {
        // A shop whose reason list will not load can still type one. Refusing
        // to let them correct a bill because a dropdown failed would be worse
        // than the thing being corrected.
        if (isUiError(cause)) setProblem(cause.message);
      });
  }, [kind]);

  const reason = [chosen, note.trim()].filter(Boolean).join(' — ');

  const confirm = () => {
    if (reason === '') {
      setProblem('Choose a reason, or type one.');
      return;
    }
    if (needsApproval && (approver === '' || pin === '')) {
      setProblem('This needs a manager: choose who, and have them type their PIN.');
      return;
    }
    onConfirm(
      reason,
      needsApproval ? { id: approver, pin } : undefined,
    );
  };

  return (
    <Modal open title={what} onClose={onCancel}>
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
        <div className="mb-approval">
          <p className="mb-muted">
            This one needs a manager. They type their own PIN — it is not stored
            and it is not remembered for the next one.
          </p>
          <div className="mb-reasons">
            {approvers.map((person) => (
              <Radio
                key={person.id}
                name="approver"
                label={person.name}
                checked={approver === person.id}
                onChange={() => setApprover(person.id)}
              />
            ))}
          </div>
          <Input
            label="Their PIN"
            type="password"
            inputMode="numeric"
            value={pin}
            onChange={(event) => setPin(event.target.value.replace(/[^0-9]/g, ''))}
          />
        </div>
      ) : null}

      {problem ? (
        <p className="mb-lock__problem" role="alert">
          {problem}
        </p>
      ) : null}

      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onCancel}>
          Leave it
        </Button>
        <Button variant="danger" onClick={confirm}>
          {confirmLabel}
        </Button>
      </div>
    </Modal>
  );
}
