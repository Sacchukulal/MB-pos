import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Minus, Plus } from "lucide-react";
import Modal from "../../components/ui/Modal";
import { formatCurrency } from "../../utils/format";
import { SUB_TABLE_LETTERS } from "../../config/constants";
import { isSlotOccupied, occupiedOrdersForTable } from "./tableUtils";
import type { MenuItem, ProcessingOrder } from "../../types";

/* --------------------------- Quantity popup ------------------------ */

interface QtyPopupProps {
  item: MenuItem;
  onConfirm: (quantity: number) => void;
  onCancel: () => void;
}

export function QtyPopup({ item, onConfirm, onCancel }: QtyPopupProps) {
  const [qty, setQty] = useState<number | "">(1);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const t = setTimeout(() => inputRef.current?.focus(), 0);
    return () => clearTimeout(t);
  }, []);

  const confirm = () => onConfirm(qty === "" || qty < 1 ? 1 : Number(qty));

  const handleKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      confirm();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  };

  return (
    <Modal onClose={onCancel} width="360px" onKeyDown={handleKeyDown}>
      <h3 className="ui-modal-title">{item.name}</h3>
      <p className="text-secondary">Price: {formatCurrency(item.price)}</p>
      <div className="qty-controls">
        <button className="qty-step-btn" onClick={() => setQty((q) => Math.max(1, (typeof q === "number" ? q : 1) - 1))}>
          <Minus size={16} />
        </button>
        <input
          ref={inputRef}
          type="number"
          className="qty-input"
          value={qty}
          onChange={(e) => {
            const val = e.target.value;
            setQty(val === "" ? "" : Math.max(1, parseInt(val) || 1));
          }}
          onBlur={() => {
            if (qty === "" || qty < 1) setQty(1);
          }}
          onFocus={(e) => e.target.select()}
        />
        <button className="qty-step-btn" onClick={() => setQty((q) => (typeof q === "number" ? q : 0) + 1)}>
          <Plus size={16} />
        </button>
      </div>
      <button className="btn btn--primary btn--block" onClick={confirm}>
        Add to Cart
      </button>
    </Modal>
  );
}

/* -------------------------- Table number popup --------------------- */

interface TablePopupProps {
  value: string;
  onChange: (value: string) => void;
  onConfirm: () => void;
  onCancel: () => void;
}

export function TablePopup({ value, onChange, onConfirm, onCancel }: TablePopupProps) {
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const t = setTimeout(() => inputRef.current?.focus(), 0);
    return () => clearTimeout(t);
  }, []);

  const handleKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      onConfirm();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  };

  return (
    <Modal onClose={onCancel} width="360px" onKeyDown={handleKeyDown}>
      <h3 className="ui-modal-title">Enter Table Number</h3>
      <input
        ref={inputRef}
        type="text"
        className="input"
        style={{ margin: "var(--space-3) 0 var(--space-4)", fontSize: "var(--text-lg)", textAlign: "center" }}
        value={value}
        onChange={(e) => onChange(e.target.value.replace(/[^0-9]/g, ""))}
        placeholder="e.g. 5"
      />
      <button className="btn btn--primary btn--block" onClick={onConfirm}>
        Confirm &amp; Save
      </button>
    </Modal>
  );
}

/* --------------------- Occupied table / sub-table popup ------------ */

interface AlphabetPopupProps {
  baseTable: string;
  orders: ProcessingOrder[];
  onMergeIntoOrder: (order: ProcessingOrder) => void;
  onNewSubTable: (letter: string) => void;
  onBack: () => void;
}

/**
 * Grid of [existing order(s), then letters B..H] navigated with arrow keys.
 * Choosing an existing order merges the new items into it; a letter opens a
 * fresh sub-table like "6B".
 */
export function AlphabetPopup({ baseTable, orders, onMergeIntoOrder, onNewSubTable, onBack }: AlphabetPopupProps) {
  const [selectedIndex, setSelectedIndex] = useState(0);
  const occupied = occupiedOrdersForTable(orders, baseTable);
  const totalCells = occupied.length + SUB_TABLE_LETTERS.length;
  const COLS = 4;

  const confirmCell = (index: number) => {
    if (index < occupied.length) {
      onMergeIntoOrder(occupied[index]);
      return;
    }
    const letter = SUB_TABLE_LETTERS[index - occupied.length];
    if (!letter) return;
    if (!isSlotOccupied(orders, `${baseTable.trim()}${letter}`)) {
      onNewSubTable(letter);
    }
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      confirmCell(selectedIndex);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onBack();
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      setSelectedIndex((p) => (p + 1) % totalCells);
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      setSelectedIndex((p) => (p - 1 + totalCells) % totalCells);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((p) => (p + COLS) % totalCells);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((p) => (p - COLS + totalCells) % totalCells);
    }
  };

  const isExistingCell = selectedIndex < occupied.length;
  const selectedLabel = isExistingCell
    ? occupied[selectedIndex]?.table_number
    : SUB_TABLE_LETTERS[selectedIndex - occupied.length];

  return (
    <Modal onClose={onBack} width="380px" autoFocusCard onKeyDown={handleKeyDown}>
      <h3 className="ui-modal-title">Table Occupied</h3>
      <p className="ui-modal-message">Select a sub-table identifier for table {baseTable.trim()}</p>
      <div className="alpha-grid">
        {occupied.map((order, index) => (
          <div
            key={`occ-${order.id}`}
            className={`alpha-cell existing ${index === selectedIndex ? "selected" : ""}`}
            onClick={() => onMergeIntoOrder(order)}
            onMouseEnter={() => setSelectedIndex(index)}
            title={`Add new items to table ${order.table_number}`}
          >
            {order.table_number}
          </div>
        ))}
        {SUB_TABLE_LETTERS.map((letter, i) => {
          const cellIndex = occupied.length + i;
          const taken = isSlotOccupied(orders, `${baseTable.trim()}${letter}`);
          return (
            <div
              key={letter}
              className={`alpha-cell ${taken ? "occupied" : cellIndex === selectedIndex ? "selected" : ""}`}
              onClick={() => {
                if (!taken) onNewSubTable(letter);
              }}
              onMouseEnter={() => {
                if (!taken) setSelectedIndex(cellIndex);
              }}
            >
              {letter}
            </div>
          );
        })}
      </div>
      <button className="btn btn--primary btn--block" onClick={() => confirmCell(selectedIndex)}>
        {isExistingCell ? `Add to ${selectedLabel}` : `Confirm ${selectedLabel}`}
      </button>
    </Modal>
  );
}

/* --------------------- KOT / bill confirm popup --------------------- */

interface PrintConfirmPopupProps {
  title: string;
  message: string;
  /** Enter → print, Esc / "No" → proceed without printing. */
  onYes: () => void;
  onNo: () => void;
  onClose: () => void;
}

export function PrintConfirmPopup({ title, message, onYes, onNo, onClose }: PrintConfirmPopupProps) {
  const handleKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      onYes();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onNo();
    }
  };

  return (
    <Modal onClose={onClose} width="340px" autoFocusCard onKeyDown={handleKeyDown}>
      <h3 className="ui-modal-title text-center">{title}</h3>
      <p className="ui-modal-message text-center">{message}</p>
      <div style={{ display: "flex", gap: "var(--space-3)" }}>
        <button className="btn btn--primary" style={{ flex: 1 }} onClick={onYes}>
          Yes (Enter)
        </button>
        <button className="btn btn--ghost" style={{ flex: 1 }} onClick={onNo}>
          No (Esc)
        </button>
      </div>
    </Modal>
  );
}

/* ------------------------- Add customer popup ----------------------- */

interface AddCustomerPopupProps {
  onAdd: (name: string, phone: string) => void;
  onCancel: () => void;
}

export function AddCustomerPopup({ onAdd, onCancel }: AddCustomerPopupProps) {
  const [name, setName] = useState("");
  const [phone, setPhone] = useState("");
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const t = setTimeout(() => nameRef.current?.focus(), 0);
    return () => clearTimeout(t);
  }, []);

  return (
    <Modal onClose={onCancel} width="360px">
      <h3 className="ui-modal-title">Add Customer</h3>
      <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)", marginBottom: "var(--space-4)" }}>
        <input
          ref={nameRef}
          type="text"
          className="input"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Name"
        />
        <input
          type="text"
          className="input"
          value={phone}
          onChange={(e) => setPhone(e.target.value.replace(/\D/g, "").slice(0, 10))}
          placeholder="Phone Number (10 digits)"
        />
      </div>
      <button className="btn btn--primary btn--block" onClick={() => onAdd(name, phone)} disabled={!name.trim()}>
        Add Customer
      </button>
    </Modal>
  );
}
