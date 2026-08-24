/**
 * Modals, confirmations and toasts — **audit F10, which is one component's
 * worth of finding:**
 *
 * > *"Confirmation dialogs are inconsistent between screens — some styled, some
 * > plain, some missing."*
 * > Fix: *"one confirm component everywhere."*
 *
 * So there is exactly one `Modal`, exactly one `ConfirmDialog` and exactly one
 * toast system. A screen that wants to ask a question uses these; there is
 * nothing else to use.
 *
 * # And a toast is never the only place a failure is reported
 *
 * That is audit D4's whole lesson — *"in a rush the cashier misses the toast
 * and the kitchen simply never gets the order"*. A toast says "that worked"; a
 * failure that matters gets a **persistent** indicator (see `shell/`), and the
 * print queue is the reason P07 built one.
 */

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
import { Icon } from './Icon';

export interface ModalProps {
  open: boolean;
  title: string;
  /**
   * What this dialog is for, as something you can **ask** for.
   *
   * An `InfoTip` beside the title, never a paragraph above the fields. Same
   * change and same reason as `SectionHeader` and `Panel` — the owner,
   * 2026-08-22: *"it makes the app look cluttered and un professional… make it
   * a kind of popup text, when hovered."*
   *
   * A dialog's title IS its heading, so this is where a dialog's explanation
   * belongs. Every one that used to open with a sentence of prose now opens
   * with the field somebody came to fill in.
   */
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

  // Escape closes — the keyboard-first rule (§1) does not stop at the edge of a
  // modal. This one needs the LATEST `onClose`, so it re-runs freely.
  useEffect(() => {
    if (!open) return undefined;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  // **Focus moves into the dialog ONCE, when it opens.** `[open]` and nothing
  // else, and the separation from the Escape listener above is the whole point.
  //
  // *The field, if there is one; the panel only if there is not.* Focusing the
  // panel unconditionally looked right and was not: it ran AFTER the browser
  // had honoured `autoFocus` on the first input, so it took the caret straight
  // back out again. Typing into a freshly opened dialog then went nowhere until
  // you pressed Tab — caught at P13 by opening "Add a size" and typing a name
  // that never arrived.
  //
  // **And then P21 found the other half of it, by driving a dialog with TWO
  // fields.** This used to be one effect with `[open, onClose]`, and every
  // caller in the product passes `onClose={() => setSomething(null)}` — a new
  // function on every render. So every keystroke re-ran the effect and dragged
  // the caret back to the FIRST input: typing a licence key and then a
  // verification code produced "MB-STUB-000123456" in one box and "1" in the
  // other.
  //
  // It had been latent since P13 because every dialog until now had one field,
  // where "put the focus back where it already is" is invisible. A memoised
  // `onClose` at each call site would also fix it, and would be a rule twenty
  // screens have to remember — D40 says those are the rules that erode.
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
  /**
   * **A third way out** — P17, and it is the unsaved-changes question.
   *
   * "Save / Discard / Cancel" is not two choices with a default; it is three,
   * and squeezing it into two means one of them is missing. It is here rather
   * than in the screen because the alternative is every screen with a save bar
   * inventing its own three-button dialog, which is audit F10 exactly.
   */
  otherLabel?: string;
  onOther?: () => void;
}

/**
 * The one confirmation in the product.
 *
 * UI_GUIDELINES §6: *"a button says exactly what happens; the confirmation
 * echoes it."* So `confirmLabel` is required and there is no default of "OK" —
 * a dialog that says "OK" has not told anybody anything.
 */
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

// ---------------------------------------------------------------------------
// Toasts — one system, stacked.
// ---------------------------------------------------------------------------

export type ToastTone = 'ok' | 'warn' | 'danger' | 'info';

export interface Toast {
  id: number;
  tone: ToastTone;
  message: string;
  detail?: string;
}

/**
 * **The actions only, and the list deliberately not.**
 *
 * The first version of this carried `toasts` as well, and it caused a real
 * feedback loop found by running the app: the context value changed on every
 * toast, so any `useCallback` that reported an error changed identity, so the
 * effect depending on it re-ran, so it errored again — and the screen filled
 * with hundreds of identical toasts in a few seconds.
 *
 * Keeping this value **stable for the life of the provider** makes that
 * impossible rather than unlikely. A screen that needs the list can only be a
 * screen re-implementing the toast display, and there is one of those.
 */
interface ToastApi {
  show: (tone: ToastTone, message: string, detail?: string) => void;
  dismiss: (id: number) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

/** How long a toast stays. Longer for anything that went wrong. */
const LINGER: Record<ToastTone, number> = {
  ok: 3_000,
  info: 4_000,
  warn: 6_000,
  danger: 8_000,
};

/** How many notes may be on screen at once. The rest are older news. */
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
        // The same message again is the same message: it re-times rather than
        // stacking. A key held down used to draw a tower of identical notes.
        const others = current.filter((t) => t.message !== message);
        return [...others, { id, tone, message, detail }].slice(-MOST_AT_ONCE);
      });
      window.setTimeout(() => dismiss(id), LINGER[tone]);
    },
    [dismiss],
  );

  // Depends only on the two callbacks, which are themselves stable — so this
  // value never changes and nothing downstream re-renders because a toast
  // appeared.
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
    // `polite`, not `assertive`: a cashier mid-keystroke must not be
    // interrupted by a screen reader announcing a success message.
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
            small
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

/**
 * **Say what came back, as loudly as it deserves** — the owner's round of
 * 22 Aug 2026.
 *
 * Every screen had written its own three lines of this, and every one of them
 * showed everything in red. So pressing the kitchen button a second time — when
 * the kitchen already has the food, and nothing at all has gone wrong — raised
 * the same alarm as a printer that had died. A counter where the harmless
 * message and the real one look identical is a counter where the cashier learns
 * to ignore both.
 *
 * **The engine decides the tone, not the screen.** `Tone` travels on the error
 * itself, from the file where the words were written. A list of codes kept here
 * in TypeScript would be a second place to change every time a message is added,
 * and within a month the two would disagree.
 */
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
