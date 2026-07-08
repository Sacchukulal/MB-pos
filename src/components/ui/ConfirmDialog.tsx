import type { ReactNode } from "react";
import Modal from "./Modal";

interface ConfirmDialogProps {
  title: string;
  message: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  /** Swap keyboard semantics: Enter=confirm, Esc=cancel (default true). */
  keyboard?: boolean;
}

/** Styled replacement for window.confirm, with Enter/Esc keyboard handling. */
export default function ConfirmDialog({
  title,
  message,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  danger = false,
  onConfirm,
  onCancel,
  keyboard = true,
}: ConfirmDialogProps) {
  return (
    <Modal
      onClose={onCancel}
      width="420px"
      autoFocusCard
      showClose={false}
      onKeyDown={
        keyboard
          ? (e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                e.stopPropagation();
                onConfirm();
              } else if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                onCancel();
              }
            }
          : undefined
      }
    >
      <h3 className={`ui-modal-title ${danger ? "text-danger" : ""}`}>{title}</h3>
      <div className="ui-modal-message">{message}</div>
      <div className="ui-modal-actions">
        <button className="btn btn--ghost" onClick={onCancel}>
          {cancelLabel}
        </button>
        <button className={`btn ${danger ? "btn--danger" : "btn--primary"}`} onClick={onConfirm}>
          {confirmLabel}
        </button>
      </div>
    </Modal>
  );
}
