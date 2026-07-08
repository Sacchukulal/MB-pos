import { useEffect, useRef, type KeyboardEvent, type ReactNode } from "react";
import { X } from "lucide-react";

interface ModalProps {
  onClose?: () => void;
  children: ReactNode;
  /** Max width of the card, e.g. "420px". */
  width?: string;
  heavy?: boolean;
  /** Key handler on the card — used by keyboard-driven billing popups. */
  onKeyDown?: (e: KeyboardEvent<HTMLDivElement>) => void;
  /** Focus the card itself on open (for popups driven purely by keys). */
  autoFocusCard?: boolean;
  showClose?: boolean;
  className?: string;
}

/** The one modal used everywhere: overlay + animated card + optional close button. */
export default function Modal({
  onClose,
  children,
  width = "440px",
  heavy = false,
  onKeyDown,
  autoFocusCard = false,
  showClose = true,
  className = "",
}: ModalProps) {
  const cardRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (autoFocusCard) {
      const t = setTimeout(() => cardRef.current?.focus(), 0);
      return () => clearTimeout(t);
    }
  }, [autoFocusCard]);

  return (
    <div
      className={`modal-overlay ${heavy ? "modal-overlay--heavy" : ""}`}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose?.();
      }}
    >
      <div
        ref={cardRef}
        className={`ui-modal-card ${className}`}
        style={{ maxWidth: width }}
        tabIndex={autoFocusCard ? 0 : undefined}
        onKeyDown={onKeyDown}
      >
        {showClose && onClose && (
          <button className="ui-modal-close icon-btn" onClick={onClose} aria-label="Close" tabIndex={-1}>
            <X size={18} />
          </button>
        )}
        {children}
      </div>
    </div>
  );
}
