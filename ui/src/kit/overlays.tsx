/** Modals, confirmations and toasts. */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
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
  /** What Enter does, for a dialog that has one answer — see `takesEnter`. */
  onEnter?: () => void;
  children?: ReactNode;
  actions?: ReactNode;
  /** One question and two buttons: the narrow box. */
  small?: boolean;
  wide?: boolean;
}

export function Modal({
  open,
  title,
  note,
  onClose,
  onEnter,
  children,
  actions,
  small,
  wide,
}: ModalProps) {
  const panel = useRef<HTMLDivElement>(null);
  const scrim = useRef<HTMLDivElement>(null);
  const shown = useLeaving(open, scrim);

  // Escape closes, Enter answers — the keyboard-first rule (§1) does not stop at the edge of
  // a modal.
  useEffect(() => {
    if (!open) return undefined;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' && event.key !== 'Enter') return;
      // One key, one dialog. A dialog opened over another one is further down the page than
      // the one it sits on, so the last one on the page is the one on top.
      const dialogs = document.querySelectorAll('.mb-modal');
      if (dialogs[dialogs.length - 1] !== panel.current) return;
      if (event.key === 'Escape') {
        // The dialog takes the key: a screen listening on the window must not also act on it.
        event.stopPropagation();
        onClose();
        return;
      }
      if (!onEnter || !takesEnter(document.activeElement)) return;
      // A form inside a dialog must not also submit on this key.
      event.preventDefault();
      event.stopPropagation();
      onEnter();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open, onClose, onEnter]);

  // Focus moves into the dialog ONCE, when it opens.
  useEffect(() => {
    if (!open) return;
    const first = panel.current?.querySelector<HTMLElement>(
      'input:not([type="hidden"]), select, textarea',
    );
    (first ?? panel.current)?.focus();
  }, [open]);

  if (shown === 'gone') return null;

  return (
    <div
      ref={scrim}
      className={cx('mb-overlay', shown === 'leaving' && 'mb-overlay--closing')}
      // Touch closes it too: "every popup closes by touch" (§1).
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={panel}
        className={cx('mb-modal', small && 'mb-modal--small', wide && 'mb-modal--wide')}
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

/**
 * Whether Enter is the dialog's to take. A box being typed into and a button being tabbed to
 * both have their own Enter, and taking the key from either of them would press the wrong
 * thing; anywhere else in the dialog the key belongs to the dialog.
 */
function takesEnter(focused: Element | null): boolean {
  if (!(focused instanceof HTMLElement)) return true;
  return !['INPUT', 'TEXTAREA', 'SELECT', 'BUTTON', 'A'].includes(focused.tagName);
}

/**
 * Whether a thing that can leave is still on the page.
 *
 * A dialog told to close stays mounted for as long as its exit motion runs, so the leaving can
 * be seen; the motion is the kit's, so this asks the element what is running rather than
 * knowing a duration. Where nothing runs — a test, reduced motion — it is gone at once.
 */
function useLeaving(
  open: boolean,
  element: React.RefObject<HTMLElement | null>,
): 'here' | 'leaving' | 'gone' {
  const [state, setState] = useState<'here' | 'leaving' | 'gone'>(open ? 'here' : 'gone');

  useEffect(() => {
    if (open) {
      setState('here');
      return undefined;
    }
    setState((was) => (was === 'here' ? 'leaving' : was));
    return undefined;
  }, [open]);

  useEffect(() => {
    if (state !== 'leaving') return undefined;
    const running = element.current?.getAnimations?.({ subtree: true }) ?? [];
    let cancelled = false;
    const gone = () => {
      if (!cancelled) setState('gone');
    };
    if (running.length === 0) {
      gone();
      return undefined;
    }
    void Promise.allSettled(running.map((a) => a.finished)).then(gone);
    return () => {
      cancelled = true;
    };
  }, [state, element]);

  return state;
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
      // The question has one answer, so Enter gives it — Esc is the other way out.
      onEnter={onConfirm}
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
 * With `text` it is a word and an arrow ("Card ▾") rather than a bare mark. The sheet is placed
 * against the WINDOW, not inside the row: a row lives in a scrolling table, and a sheet hung
 * inside it was cut off by the table's edge for the last rows. Near the bottom of the window
 * it opens upward by itself; `up` forces that.
 */
/** Below this much room under the button, the sheet opens upward instead. */
const SHEET_ROOM_REM = 12;
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
  const anchor = useRef<HTMLSpanElement>(null);
  const sheet = useRef<HTMLDivElement>(null);
  /** Where the button is when the sheet opens, and which way the sheet goes from it. */
  const [place, setPlace] = useState<{ x: number; y: number; up: boolean } | null>(null);
  /** Where the button is right now, and which way the sheet should go from it. */
  const measure = useCallback(() => {
    const box = anchor.current?.getBoundingClientRect();
    if (!box) return null;
    const rem = parseFloat(getComputedStyle(document.documentElement).fontSize);
    const flip = up ?? window.innerHeight - box.bottom < SHEET_ROOM_REM * rem;
    return { x: box.right, y: flip ? box.top : box.bottom, up: flip };
  }, [up]);
  useEffect(() => {
    if (!open) return undefined;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        setOpen(false);
      }
    };
    // The page moved under it: the sheet follows its button rather than hanging in the air.
    const onMove = () => setPlace(measure());
    document.addEventListener('keydown', onKey);
    document.addEventListener('scroll', onMove, true);
    window.addEventListener('resize', onMove);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('scroll', onMove, true);
      window.removeEventListener('resize', onMove);
    };
  }, [open, measure]);
  // The sheet reads its place from two custom properties, set here rather than as an inline
  // style: the kit's sheet rule owns everything else about how it looks.
  useLayoutEffect(() => {
    if (!open || !place || !sheet.current) return;
    sheet.current.style.setProperty('--rowmenu-x', `${place.x}px`);
    sheet.current.style.setProperty('--rowmenu-y', `${place.y}px`);
  }, [open, place]);
  const toggle = () => {
    if (open) {
      setOpen(false);
      return;
    }
    const at = measure();
    if (!at) return;
    setPlace(at);
    setOpen(true);
  };
  return (
    <span className={cx('mb-rowmenu', className)} ref={anchor}>
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
        onClick={toggle}
        icon={text === undefined ? <Icon name={icon} size="sm" /> : undefined}
      >
        {text}
        {text === undefined ? null : <Icon name={icon} size="sm" />}
      </Button>
      {open ? (
        <>
          <button
            type="button"
            className="mb-sheetscrim"
            aria-label="Close"
            onClick={() => setOpen(false)}
          />
          {/* A press inside lands on the button and then closes the sheet. */}
          <div
            ref={sheet}
            className={cx('mb-sheet mb-rowmenu__sheet', place?.up && 'mb-rowmenu__sheet--up')}
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
