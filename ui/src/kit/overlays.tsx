/** Modals, confirmations and toasts. */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';

import { isUiError } from '../ipc/call';
import { cx } from './cx';
import { Button } from './controls';
import { InfoTip } from './InfoTip';
import { Icon, type IconName } from './Icon';

export interface ModalProps {
  open: boolean;
  title: string;
  /** What this dialog is for, as something you can ask for. */
  note?: ReactNode;
  onClose: () => void;
  children?: ReactNode;
  actions?: ReactNode;
  wide?: boolean;
}

export function Modal({
  open,
  title,
  note,
  onClose,
  children,
  actions,
  wide,
}: ModalProps) {
  const panel = useRef<HTMLDivElement>(null);

  // Escape closes — the keyboard-first rule (§1) does not stop at the edge of a modal.
  useEffect(() => {
    if (!open) return undefined;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      // The dialog takes the key: a screen listening on the window must not also act on it.
      event.stopPropagation();
      onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  // Focus moves into the dialog ONCE, when it opens.
  useEffect(() => {
    if (!open) return;
    const first = panel.current?.querySelector<HTMLElement>(
      'input:not([type="hidden"]), select, textarea',
    );
    (first ?? panel.current)?.focus();
  }, [open]);

  if (!open) return null;

  return (
    <div
      className="mb-overlay"
      // Touch closes it too: "every popup closes by touch" (§1).
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={panel}
        className={cx('mb-modal', wide && 'mb-modal--wide')}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
      >
        <div className="mb-modal__head">
          <h2 className="mb-modal__title">{title}</h2>
          {note ? <InfoTip label={`About ${title}`}>{note}</InfoTip> : null}
        </div>
        <div className="mb-modal__body">{children}</div>
        {actions ? <div className="mb-modal__actions">{actions}</div> : null}
      </div>
    </div>
  );
}

export interface ConfirmDialogProps {
  open: boolean;
  title: string;
  body?: string;
  /** What the button says — and it says exactly what will happen (§6). */
  confirmLabel: string;
  cancelLabel?: string;
  destructive?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  /** A third way out. */
  otherLabel?: string;
  onOther?: () => void;
}

/** The one confirmation in the product. */
export function ConfirmDialog({
  open,
  title,
  body,
  confirmLabel,
  cancelLabel = 'Cancel',
  destructive,
  onConfirm,
  onCancel,
  otherLabel,
  onOther,
}: ConfirmDialogProps) {
  return (
    <Modal
      open={open}
      title={title}
      onClose={onCancel}
      actions={
        <>
          <Button onClick={onCancel}>{cancelLabel}</Button>
          {otherLabel && onOther ? (
            <Button onClick={onOther}>{otherLabel}</Button>
          ) : null}
          <Button
            variant={destructive ? 'danger' : 'primary'}
            onClick={onConfirm}
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      {body}
    </Modal>
  );
}

// Toasts — one system, stacked.

export type ToastTone = 'ok' | 'warn' | 'danger' | 'info';

export interface Toast {
  id: number;
  tone: ToastTone;
  message: string;
  detail?: string;
}

/** The actions only, and the list deliberately not. */
interface ToastApi {
  show: (tone: ToastTone, message: string, detail?: string) => void;
  dismiss: (id: number) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

/** How long a toast stays. */
const LINGER: Record<ToastTone, number> = {
  ok: 3_000,
  info: 4_000,
  warn: 6_000,
  danger: 8_000,
};

/** How many notes may be on screen at once. */
const MOST_AT_ONCE = 3;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const next = useRef(1);

  const dismiss = useCallback((id: number) => {
    setToasts((current) => current.filter((t) => t.id !== id));
  }, []);

  const show = useCallback(
    (tone: ToastTone, message: string, detail?: string) => {
      const id = next.current;
      next.current += 1;
      setToasts((current) => {
        // The same message again is the same message: it re-times rather than stacking.
        const others = current.filter((t) => t.message !== message);
        return [...others, { id, tone, message, detail }].slice(-MOST_AT_ONCE);
      });
      window.setTimeout(() => dismiss(id), LINGER[tone]);
    },
    [dismiss],
  );

  // Depends only on the two callbacks, which are themselves stable — so this value never
  // changes and nothing downstream re-renders because a toast appeared.
  const value = useMemo<ToastApi>(() => ({ show, dismiss }), [show, dismiss]);

  return (
    <ToastContext.Provider value={value}>
      {children}
      <ToastList toasts={toasts} onDismiss={dismiss} />
    </ToastContext.Provider>
  );
}

function ToastList({
  toasts,
  onDismiss,
}: {
  toasts: readonly Toast[];
  onDismiss: (id: number) => void;
}) {
  if (toasts.length === 0) return null;
  return (
    // `polite`, not `assertive`: a cashier mid-keystroke must not be interrupted by a screen
    // reader announcing a success message.
    <div className="mb-toasts" role="status" aria-live="polite">
      {toasts.map((toast) => (
        <div key={toast.id} className={`mb-toast mb-toast--${toast.tone}`}>
          <div className="mb-stack">
            <span>{toast.message}</span>
            {toast.detail ? (
              <span className="mb-field__hint">{toast.detail}</span>
            ) : null}
          </div>
          <Button
            variant="quiet"
            size="sm"
            onClick={() => onDismiss(toast.id)}
            aria-label="Dismiss"
          >
            <Icon name="x" size="sm" />
          </Button>
        </div>
      ))}
    </div>
  );
}

export function useToast(): ToastApi {
  const found = useContext(ToastContext);
  if (!found) throw new Error('useToast was called outside ToastProvider');
  return found;
}

/** Say what came back, as loudly as it deserves. */
export function useReport(): (cause: unknown) => void {
  const toast = useToast();
  return useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) {
        toast.show(
          cause.tone === 'notice' ? 'info' : 'danger',
          cause.message,
          cause.detail ?? undefined,
        );
        return;
      }
      // Not one of ours: a bug rather than a refusal, and always loud.
      toast.show('danger', String(cause));
    },
    [toast],
  );
}

/**
 * The ⋯ that holds the rest of a row's actions. The commands do not change; the buttons do.
 *
 * With `text` it is a word and an arrow ("Card ▾") rather than a bare mark, and with `up` the
 * sheet opens above the button — for a menu near the bottom of the screen.
 */
export function RowMenu({
  label = 'More',
  children,
  size = 'sm',
  icon = 'more',
  text,
  pressed,
  up,
  className,
}: {
  /** What the row is, for the screen reader — "More for Masala Dosa". */
  label?: string;
  /** `Button`s. Each one closes the menu when pressed. */
  children: ReactNode;
  size?: 'sm' | 'md';
  /** The mark on the button. */
  icon?: IconName;
  /** A word beside the mark, when the button says what was chosen from it. */
  text?: ReactNode;
  /** Lit, like a pressed toggle — the choice in this menu is the one in force. */
  pressed?: boolean;
  /** Open the sheet above the button rather than below it. */
  up?: boolean;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open) return undefined;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open]);
  return (
    <span className={cx('mb-rowmenu', className)}>
      <Button
        variant={text === undefined ? 'quiet' : 'secondary'}
        size={size}
        iconOnly={text === undefined}
        className={text === undefined ? undefined : 'mb-rowmenu__trigger'}
        title={label}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-pressed={pressed}
        onClick={() => setOpen((was) => !was)}
        icon={text === undefined ? <Icon name={icon} size="sm" /> : undefined}
      >
        {text}
        {text === undefined ? null : <Icon name={icon} size="sm" />}
      </Button>
      {open ? (
        <>
          <button
            type="button"
            className="mb-rowmenu__scrim"
            aria-label="Close"
            onClick={() => setOpen(false)}
          />
          {/* A press inside lands on the button and then closes the sheet. */}
          <div
            className={cx('mb-rowmenu__sheet', up && 'mb-rowmenu__sheet--up')}
            role="menu"
            onClick={() => setOpen(false)}
          >
            {children}
          </div>
        </>
      ) : null}
    </span>
  );
}
