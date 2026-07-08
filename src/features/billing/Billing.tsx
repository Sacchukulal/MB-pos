import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { CheckCircle, Printer, Search, X } from "lucide-react";
import { MAX_SEARCH_SUGGESTIONS, STORAGE_KEYS } from "../../config/constants";
import { formatCurrency } from "../../utils/format";
import { computeCartTotals } from "../../utils/gst";
import { useSettings } from "../../hooks/useSettings";
import { useToast } from "../../hooks/useToast";
import { usePlanGate } from "../../hooks/usePlanGate";
import * as menuRepo from "../../db/repositories/menuRepo";
import * as ordersRepo from "../../db/repositories/ordersRepo";
import * as customersRepo from "../../db/repositories/customersRepo";
import { claimOrderNumbers } from "../../db/repositories/settingsRepo";
import { printBill, printKot } from "../../services/printing/printService";
import { findTableOrder, occupiedOrdersForTable } from "./tableUtils";
import { AddCustomerPopup, AlphabetPopup, PrintConfirmPopup, QtyPopup, TablePopup } from "./popups";
import { CartPanel, PaymentPanel, ProcessingOrdersPanel } from "./panels";
import type { CartItem, Category, Customer, MenuItem, OrderType, ProcessingOrder } from "../../types";

interface BillingProps {
  dbReady: boolean;
}

function loadOrderTypeLock(): { locked: boolean; type: OrderType } {
  const locked = localStorage.getItem(STORAGE_KEYS.orderTypeLocked) === "true";
  const stored = localStorage.getItem(STORAGE_KEYS.lockedOrderType);
  const type: OrderType =
    stored === "Table" || stored === "Parcel" || stored === "Self Service" ? stored : "Self Service";
  return { locked, type };
}

export default function Billing({ dbReady }: BillingProps) {
  const { settings } = useSettings();
  const { toast } = useToast();
  const { plan } = usePlanGate(dbReady);

  /* ------------------------------ state ------------------------------ */
  const [allItems, setAllItems] = useState<MenuItem[]>([]);
  const [categories, setCategories] = useState<Category[]>([]);
  const [searchTerm, setSearchTerm] = useState("");
  const [selectedSuggestionIndex, setSelectedSuggestionIndex] = useState(-1);

  const [cart, setCart] = useState<CartItem[]>([]);
  const [originalCart, setOriginalCart] = useState<CartItem[]>([]);
  const [customerName, setCustomerName] = useState("");
  const [customerPhone, setCustomerPhone] = useState("");
  const [paymentMode, setPaymentMode] = useState("Cash");
  const [billingType, setBillingType] = useState<"Cash" | "Credit">("Cash");
  const [creditCustomers, setCreditCustomers] = useState<Customer[]>([]);
  const [selectedCustomerId, setSelectedCustomerId] = useState<number | null>(null);
  const [isPaymentOpen, setIsPaymentOpen] = useState(true);

  const [processingOrders, setProcessingOrders] = useState<ProcessingOrder[]>([]);
  const [activeOrderId, setActiveOrderId] = useState<number | null>(null);
  const [activeTokenNumber, setActiveTokenNumber] = useState<number | null>(null);
  const [activeBillNumber, setActiveBillNumber] = useState<string | null>(null);
  const [isKOTPrinted, setIsKOTPrinted] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);

  const initialLock = useMemo(loadOrderTypeLock, []);
  const [orderType, setOrderType] = useState<OrderType>("Self Service");
  const [orderTypeLocked, setOrderTypeLocked] = useState(initialLock.locked);
  const [lockedOrderType, setLockedOrderType] = useState<OrderType>(initialLock.type);
  const [tableNumber, setTableNumber] = useState("");

  // Popups
  const [qtyItem, setQtyItem] = useState<MenuItem | null>(null);
  const [isTablePopupOpen, setIsTablePopupOpen] = useState(false);
  const [isAlphabetPopupOpen, setIsAlphabetPopupOpen] = useState(false);
  const [isAddCustomerOpen, setIsAddCustomerOpen] = useState(false);
  const [isKotConfirmOpen, setIsKotConfirmOpen] = useState(false);
  const [isBillConfirmOpen, setIsBillConfirmOpen] = useState(false);
  // Set after a merge; fires the KOT print once merged state has committed.
  const [pendingMergePrint, setPendingMergePrint] = useState(false);

  const searchInputRef = useRef<HTMLInputElement>(null);
  const suggestionListRef = useRef<HTMLDivElement>(null);

  const anyPopupOpen =
    qtyItem !== null || isTablePopupOpen || isAlphabetPopupOpen || isAddCustomerOpen || isKotConfirmOpen || isBillConfirmOpen;

  /* --------------------------- data loading -------------------------- */

  const fetchProcessingOrders = useCallback(async () => {
    try {
      setProcessingOrders(await ordersRepo.listProcessingOrders());
    } catch (error) {
      console.error("Failed to fetch processing orders:", error);
    }
  }, []);

  const fetchCreditCustomers = useCallback(async () => {
    try {
      setCreditCustomers(await customersRepo.listCustomers());
    } catch (error) {
      console.error("Failed to fetch customers:", error);
    }
  }, []);

  useEffect(() => {
    if (!dbReady) return;
    (async () => {
      try {
        setAllItems(await menuRepo.listAllItems());
        setCategories(await menuRepo.listCategories());
        await fetchProcessingOrders();
        await fetchCreditCustomers();
      } catch (error) {
        console.error("Failed to load billing data:", error);
      }
    })();
  }, [dbReady, fetchProcessingOrders, fetchCreditCustomers]);

  // Autofocus the search box once the plan check confirms usability.
  useEffect(() => {
    if (plan.usable) {
      setTimeout(() => searchInputRef.current?.focus(), 0);
    }
  }, [plan.usable]);

  /* ------------------------------ totals ----------------------------- */

  const gstConfig = settings?.bill.gst ?? { enabled: true, type: "Exclusive" as const, percentage: 5 };
  const { subtotal, gst, total } = computeCartTotals(cart, gstConfig);

  /* ----------------------------- search ------------------------------ */

  const suggestions = useMemo(() => {
    const query = searchTerm.trim().toLowerCase();
    if (!query) return [];
    const matchMode = settings?.bill.searchMatchMode ?? "starts";

    let matched: MenuItem[];
    if (matchMode === "contains") {
      // Rank: 0 = prefix match, 1 = word-start match, 2 = inside a word.
      matched = allItems
        .map((item) => {
          const idx = item.name.toLowerCase().indexOf(query);
          if (idx === -1) return null;
          const score = idx === 0 ? 0 : item.name[idx - 1] === " " ? 1 : 2;
          return { item, score };
        })
        .filter((m): m is { item: MenuItem; score: number } => m !== null)
        .sort((a, b) => a.score - b.score)
        .map((m) => m.item);
    } else {
      matched = allItems.filter((item) => item.name.toLowerCase().startsWith(query));
    }
    return matched.slice(0, MAX_SEARCH_SUGGESTIONS);
  }, [searchTerm, allItems, settings?.bill.searchMatchMode]);

  useEffect(() => {
    setSelectedSuggestionIndex(suggestions.length > 0 ? 0 : -1);
  }, [suggestions]);

  // Keep the highlighted suggestion visible during keyboard navigation.
  useEffect(() => {
    if (selectedSuggestionIndex < 0 || !suggestionListRef.current) return;
    const item = suggestionListRef.current.children[selectedSuggestionIndex] as HTMLElement | undefined;
    item?.scrollIntoView({ block: "nearest" });
  }, [selectedSuggestionIndex]);

  /* ------------------------------ cart -------------------------------- */

  const addToCart = (item: MenuItem, quantity: number) => {
    setCart((prev) => {
      const existing = prev.find((i) => i.id === item.id);
      if (existing) {
        return prev.map((i) => (i.id === item.id ? { ...i, quantity: i.quantity + quantity } : i));
      }
      return [...prev, { ...item, quantity }];
    });
    setSearchTerm("");
    setIsKOTPrinted(false);
    searchInputRef.current?.focus();
  };

  const changeCartQuantity = (id: number, quantity: number) => {
    setCart((prev) => prev.map((i) => (i.id === id ? { ...i, quantity } : i)));
    if (quantity > 0) setIsKOTPrinted(false);
  };

  const fixCartQuantity = (id: number) => {
    setCart((prev) => prev.map((i) => (i.id === id && (!i.quantity || isNaN(i.quantity)) ? { ...i, quantity: 1 } : i)));
  };

  const removeFromCart = (id: number) => {
    setCart((prev) => prev.filter((i) => i.id !== id));
    setIsKOTPrinted(false);
  };

  const startNewOrder = useCallback(() => {
    setCart([]);
    setOriginalCart([]);
    setCustomerName("");
    setCustomerPhone("");
    setPaymentMode("Cash");
    setBillingType("Cash");
    setSelectedCustomerId(null);
    setTableNumber("");
    setActiveOrderId(null);
    setActiveTokenNumber(null);
    setActiveBillNumber(null);
    setIsKOTPrinted(false);
    // While locked, new orders always return to the pinned order type.
    if (orderTypeLocked) setOrderType(lockedOrderType);
    setTimeout(() => searchInputRef.current?.focus(), 0);
  }, [orderTypeLocked, lockedOrderType]);

  const loadProcessingOrder = useCallback((order: ProcessingOrder) => {
    const parsed = ordersRepo.parseCart(order.cart_data);
    setCart(parsed);
    setOriginalCart(ordersRepo.parseCart(order.cart_data));
    setCustomerName(order.customer_name || "");
    setCustomerPhone(order.customer_phone || "");
    if (order.payment_mode === "Credit") {
      setBillingType("Credit");
      setPaymentMode("Cash");
      setSelectedCustomerId(order.customer_id ?? null);
    } else {
      setBillingType("Cash");
      setPaymentMode(order.payment_mode || "Cash");
      setSelectedCustomerId(null);
    }
    setOrderType((order.order_type as OrderType) || "Self Service");
    setTableNumber(order.table_number || "");
    setActiveOrderId(order.id);
    setActiveTokenNumber(order.token_number ?? null);
    setActiveBillNumber(order.bill_number ?? null);
    setIsKOTPrinted(true);
    setTimeout(() => searchInputRef.current?.focus(), 0);
  }, []);

  /* --------------------------- order drafts --------------------------- */

  const buildDraft = (finalTableNumber: string): ordersRepo.OrderDraft => {
    let cName = customerName;
    let cPhone = customerPhone;
    let pMode = paymentMode;
    let customerId: number | null = null;
    if (billingType === "Credit" && selectedCustomerId) {
      const customer = creditCustomers.find((c) => c.id === selectedCustomerId);
      if (customer) {
        cName = customer.name;
        cPhone = customer.phone;
        pMode = "Credit";
      }
      customerId = selectedCustomerId;
    }
    return {
      cart,
      customerName: cName,
      customerPhone: cPhone,
      paymentMode: pMode,
      subtotal,
      gst,
      total,
      orderType,
      tableNumber: finalTableNumber,
      customerId,
    };
  };

  /** New items (or quantity increases) vs. the loaded order snapshot. */
  const computeKotDelta = (): CartItem[] => {
    if (!activeOrderId) return [...cart];
    const delta: CartItem[] = [];
    cart.forEach((item) => {
      const original = originalCart.find((o) => o.id === item.id);
      if (!original) delta.push(item);
      else if (item.quantity > original.quantity) delta.push({ ...item, quantity: item.quantity - original.quantity });
    });
    return delta;
  };

  /* ------------------------------- KOT -------------------------------- */

  const executePrintKOT = async (overrideTableNumber?: string, skipPrint = false) => {
    if (cart.length === 0) {
      toast("Cart is empty!");
      return;
    }
    if (isProcessing || !settings) return;
    setIsProcessing(true);

    try {
      const finalTableNumber = overrideTableNumber !== undefined ? overrideTableNumber : tableNumber;
      if (orderType === "Table" && !finalTableNumber) {
        setIsTablePopupOpen(true);
        return;
      }

      const draft = buildDraft(finalTableNumber);
      let tokenNumber = activeTokenNumber;
      let billNumber = activeBillNumber;
      const itemsToPrint = computeKotDelta();

      if (activeOrderId) {
        await ordersRepo.updateProcessingOrder(activeOrderId, draft);
      } else {
        // Claim token + bill numbers atomically from the DB (never stale UI state).
        const claimed = await claimOrderNumbers({ token: true, bill: true });
        tokenNumber = claimed.tokenNumber;
        billNumber = claimed.billNumber;
        const newId = await ordersRepo.insertProcessingOrder(draft, tokenNumber, billNumber);
        setActiveOrderId(newId);
        setActiveTokenNumber(tokenNumber);
        setActiveBillNumber(billNumber);
      }
      await fetchProcessingOrders();

      if (skipPrint) {
        toast("Order Moved to Processing");
        setOriginalCart([...cart]);
        startNewOrder();
        return;
      }

      if (itemsToPrint.length === 0) {
        toast("Order Saved (No new items to print KOT)");
        setOriginalCart([...cart]);
        startNewOrder();
        return;
      }

      const result = await printKot(settings, itemsToPrint, categories, {
        tokenNumber: tokenNumber ?? "?",
        billNumber: billNumber ?? "?",
        orderType,
        tableNumber: finalTableNumber,
      });

      if (result.ok) {
        toast("KOT(s) Printed & Order Saved!", "success");
      } else if (result.error === "NO_PRINTER") {
        toast("Order Saved (No printers configured for items)", "warning");
      } else {
        toast(`Order Saved, but KOT PRINT FAILED: ${result.error}`, "danger");
      }

      setOriginalCart([...cart]);
      startNewOrder();
    } catch (error) {
      console.error("Unexpected error while printing KOT:", error);
      toast("An error occurred. Check console for details.", "danger");
    } finally {
      setIsProcessing(false);
    }
  };

  const handlePrintKOT = (overrideTableNumber?: string) => {
    const finalTableNum = typeof overrideTableNumber === "string" ? overrideTableNumber : tableNumber;

    if (cart.length === 0) {
      toast("Cart is empty!");
      return;
    }
    if (orderType === "Table" && !finalTableNum) {
      setIsTablePopupOpen(true);
      return;
    }
    if (orderType === "Table" && finalTableNum) {
      setTableNumber(finalTableNum);
    }

    if (settings?.printer.disableKot) {
      executePrintKOT(finalTableNum, true);
    } else if (settings?.printer.kotConfirmation) {
      setIsKotConfirmOpen(true);
    } else {
      executePrintKOT(finalTableNum);
    }
  };

  /* ----------------------------- checkout ----------------------------- */

  const executeCheckout = async (skipPrint = false) => {
    if (cart.length === 0) {
      toast("Cart is empty!");
      return;
    }
    if (isProcessing || !settings) return;
    setIsProcessing(true);

    try {
      const draft = buildDraft(tableNumber);
      let billNumber = "";
      let tokenNumber = activeTokenNumber;
      let createdAt = new Date().toISOString();

      if (activeOrderId) {
        const order = await ordersRepo.getProcessingOrder(activeOrderId);
        if (order) {
          billNumber = order.bill_number ?? "";
          if (!tokenNumber && order.token_number) tokenNumber = order.token_number;
          createdAt = order.created_at;
        }
        await ordersRepo.insertFinalizedOrder(draft, billNumber, tokenNumber, createdAt);
        if (draft.paymentMode === "Credit" && draft.customerId) {
          await customersRepo.addToCreditBalance(draft.customerId, total);
        }
        await ordersRepo.deleteProcessingOrder(activeOrderId);
        await fetchProcessingOrders();
      } else {
        // Direct checkout without a KOT: claim fresh numbers.
        const claimed = await claimOrderNumbers({ token: true, bill: true });
        billNumber = claimed.billNumber;
        tokenNumber = claimed.tokenNumber;
        if (draft.paymentMode === "Credit" && draft.customerId) {
          await customersRepo.addToCreditBalance(draft.customerId, total);
        }
        await ordersRepo.insertFinalizedOrder(draft, billNumber, tokenNumber, createdAt);
      }

      if (skipPrint) {
        toast(`Checkout successful! Total: Rs. ${total.toFixed(2)}`, "success");
        startNewOrder();
        return;
      }

      const result = await printBill(settings, {
        cart,
        subtotal,
        gst,
        total,
        billNumber,
        tokenNumber,
        orderType,
        tableNumber,
        date: new Date(),
        gstPercentage: gstConfig.enabled ? gstConfig.percentage : undefined,
        gstInclusive: gstConfig.enabled && gstConfig.type === "Inclusive",
      });

      if (result.ok) {
        toast("Checkout successful! Bill printed.", "success");
      } else if (result.error === "NO_PRINTER") {
        toast("Checkout saved — but NO PRINTER is selected. Open Printer Settings → choose a Default Printer, then Save.", "warning");
      } else {
        toast(`Checkout saved, but PRINT FAILED: ${result.error}`, "danger");
      }
      startNewOrder();
    } catch (error) {
      console.error("Unexpected error during checkout:", error);
      toast("An error occurred during checkout. Check console.", "danger");
    } finally {
      setIsProcessing(false);
    }
  };

  const handleCheckout = () => {
    if (cart.length === 0) {
      toast("Cart is empty!");
      return;
    }
    if (settings?.printer.billConfirmation) {
      setIsBillConfirmOpen(true);
    } else {
      executeCheckout();
    }
  };

  /* -------------------------- table workflow -------------------------- */

  const handleTableConfirm = () => {
    const trimmed = tableNumber.trim();
    if (!trimmed) return;

    const occupied = occupiedOrdersForTable(processingOrders, trimmed).length > 0;
    if (occupied && !activeOrderId) {
      setIsTablePopupOpen(false);
      setIsAlphabetPopupOpen(true);
      return;
    }
    setIsTablePopupOpen(false);
    handlePrintKOT();
  };

  const handleNewSubTable = (letter: string) => {
    const newTableNum = `${tableNumber.trim()}${letter}`;
    setTableNumber(newTableNum);
    setIsAlphabetPopupOpen(false);
    handlePrintKOT(newTableNum);
  };

  /** Merge current cart into an existing order; only the new items print on the KOT. */
  const handleMergeIntoOrder = (order: ProcessingOrder) => {
    try {
      const existingCart = ordersRepo.parseCart(order.cart_data);
      const merged: CartItem[] = existingCart.map((i) => ({ ...i }));
      cart.forEach((newItem) => {
        const found = merged.find((m) => m.id === newItem.id);
        if (found) found.quantity += newItem.quantity;
        else merged.push({ ...newItem });
      });

      // originalCart = existing items, so the delta (new items) prints on the KOT.
      setOriginalCart(existingCart);
      setCart(merged);
      setCustomerName(order.customer_name || "");
      setCustomerPhone(order.customer_phone || "");
      if (order.payment_mode === "Credit") {
        setBillingType("Credit");
        setPaymentMode("Cash");
        setSelectedCustomerId(order.customer_id ?? null);
      } else {
        setBillingType("Cash");
        setPaymentMode(order.payment_mode || "Cash");
        setSelectedCustomerId(null);
      }
      setOrderType((order.order_type as OrderType) || "Table");
      setTableNumber(order.table_number || "");
      setActiveOrderId(order.id);
      setActiveTokenNumber(order.token_number ?? null);
      setActiveBillNumber(order.bill_number ?? null);
      setIsKOTPrinted(true);
      setIsAlphabetPopupOpen(false);
      setIsTablePopupOpen(false);
      // Defer the print until the state above has committed.
      setPendingMergePrint(true);
    } catch (error) {
      console.error("Failed to merge into processing order:", error);
      toast("Failed to add to existing order.", "danger");
    }
  };

  useEffect(() => {
    if (pendingMergePrint) {
      setPendingMergePrint(false);
      handlePrintKOT(tableNumber);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingMergePrint]);

  /* --------------------------- customers ------------------------------ */

  const handleAddCustomer = async (name: string, phone: string) => {
    if (!name.trim()) return;
    if (phone && phone.length !== 10) {
      toast("Phone number must be exactly 10 digits.", "warning");
      return;
    }
    try {
      const newId = await customersRepo.addCustomer(name.trim(), phone);
      await fetchCreditCustomers();
      setIsAddCustomerOpen(false);
      setSelectedCustomerId(newId);
      toast("Customer added successfully!", "success");
    } catch (error) {
      console.error("Failed to add customer:", error);
      toast("Failed to add customer.", "danger");
    }
  };

  /* --------------------------- order type ----------------------------- */

  const cycleOrderType = (direction: "left" | "right") => {
    const types: OrderType[] = ["Self Service", "Table", "Parcel"];
    const idx = types.indexOf(orderType);
    const next = direction === "left" ? (idx - 1 + types.length) % types.length : (idx + 1) % types.length;
    setOrderType(types[next]);
  };

  const selectOrderType = (type: OrderType) => {
    setOrderType(type);
    // While locked, mouse-selecting a tab re-pins the lock to it.
    if (orderTypeLocked) {
      setLockedOrderType(type);
      localStorage.setItem(STORAGE_KEYS.lockedOrderType, type);
    }
  };

  const toggleOrderTypeLock = () => {
    setOrderTypeLocked((prev) => {
      const next = !prev;
      localStorage.setItem(STORAGE_KEYS.orderTypeLocked, String(next));
      if (next) {
        setLockedOrderType(orderType);
        localStorage.setItem(STORAGE_KEYS.lockedOrderType, orderType);
      }
      return next;
    });
  };

  /* --------------------------- keyboard flows ------------------------- */

  const stepProcessingOrder = useCallback(
    (direction: 1 | -1) => {
      if (processingOrders.length === 0) return;
      const currentIndex = processingOrders.findIndex((o) => o.id === activeOrderId);
      const nextIndex =
        currentIndex === -1
          ? direction === 1
            ? 0
            : processingOrders.length - 1
          : (currentIndex + direction + processingOrders.length) % processingOrders.length;
      loadProcessingOrder(processingOrders[nextIndex]);
    },
    [processingOrders, activeOrderId, loadProcessingOrder]
  );

  // Global shortcuts (active when no popup is open and focus is outside inputs).
  useEffect(() => {
    const handleGlobalKeyDown = (e: globalThis.KeyboardEvent) => {
      if (anyPopupOpen) return;

      const target = e.target as HTMLElement;
      const isInput = ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);

      if (e.key === "Escape") {
        // The search input's own handler closes suggestions first.
        if (target.classList.contains("billing-search-input")) return;
        e.preventDefault();
        startNewOrder();
        return;
      }
      if (isInput) return;

      if (!orderTypeLocked && e.key === "ArrowLeft") {
        e.preventDefault();
        cycleOrderType("left");
      } else if (!orderTypeLocked && e.key === "ArrowRight") {
        e.preventDefault();
        cycleOrderType("right");
      }

      if (e.key === "ArrowDown") {
        e.preventDefault();
        stepProcessingOrder(1);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        stepProcessingOrder(-1);
      }
    };
    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => window.removeEventListener("keydown", handleGlobalKeyDown);
  });

  // Clicking empty space refocuses the search box (unless text is selected).
  useEffect(() => {
    const handleGlobalClick = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return;
      if (anyPopupOpen) return;
      if (window.getSelection()?.toString().length) return;
      searchInputRef.current?.focus();
    };
    window.addEventListener("click", handleGlobalClick);
    return () => window.removeEventListener("click", handleGlobalClick);
  });

  const handleSearchKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      if (suggestions.length > 0) {
        setSearchTerm("");
      } else {
        startNewOrder();
      }
      return;
    }

    if (!orderTypeLocked && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      e.preventDefault();
      cycleOrderType(e.key === "ArrowLeft" ? "left" : "right");
      return;
    }

    if (e.key === "Enter") {
      e.preventDefault();
      const trimmed = searchTerm.trim();

      // Typing an active table number loads that order.
      if (trimmed) {
        const tableOrder = findTableOrder(processingOrders, trimmed);
        if (tableOrder) {
          loadProcessingOrder(tableOrder);
          setSearchTerm("");
          return;
        }
      }

      if (suggestions.length > 0 && selectedSuggestionIndex >= 0) {
        setQtyItem(suggestions[selectedSuggestionIndex]);
      } else if (trimmed === "") {
        if (cart.length > 0) {
          if (!isKOTPrinted) handlePrintKOT();
          else handleCheckout();
        } else if (processingOrders.length > 0 && activeOrderId === null) {
          loadProcessingOrder(processingOrders[0]);
        }
      }
      return;
    }

    if (suggestions.length === 0) {
      if (searchTerm.trim() === "" && processingOrders.length > 0) {
        if (e.key === "ArrowDown") {
          e.preventDefault();
          stepProcessingOrder(1);
        } else if (e.key === "ArrowUp") {
          e.preventDefault();
          stepProcessingOrder(-1);
        }
      }
      return;
    }

    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedSuggestionIndex((prev) => (prev + 1) % suggestions.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedSuggestionIndex((prev) => (prev - 1 + suggestions.length) % suggestions.length);
    }
  };

  /* ------------------------------ render ------------------------------ */

  const disableKot = Boolean(settings?.printer.disableKot);

  return (
    <div className="billing-page">
      {/* Center: search */}
      <div className="billing-center">
        <div className="search-bar">
          <Search className="search-icon" size={20} />
          <input
            ref={searchInputRef}
            type="text"
            placeholder="Search items (Type name...)"
            value={searchTerm}
            onChange={(e) => {
              const val = e.target.value;
              if (val.startsWith(" ") && val.trim() === "") return;
              setSearchTerm(val);
            }}
            onKeyDown={handleSearchKeyDown}
            className="billing-search-input"
          />
          {searchTerm && (
            <button className="clear-search" onClick={() => setSearchTerm("")}>
              <X size={16} />
            </button>
          )}
          {suggestions.length > 0 && (
            <div className="suggestions-list" ref={suggestionListRef}>
              {suggestions.map((item, index) => (
                <div
                  key={item.id}
                  className={`suggestion-item ${index === selectedSuggestionIndex ? "selected" : ""}`}
                  onClick={() => setQtyItem(item)}
                  onMouseEnter={() => setSelectedSuggestionIndex(index)}
                >
                  <span className="item-name">{item.name}</span>
                  <span className="item-price">{formatCurrency(item.price)}</span>
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="billing-hint">
          <span className="kbd">Enter</span> add / print &nbsp;·&nbsp; <span className="kbd">↑</span>
          <span className="kbd">↓</span> orders &nbsp;·&nbsp; <span className="kbd">←</span>
          <span className="kbd">→</span> order type &nbsp;·&nbsp; <span className="kbd">Esc</span> new order
        </div>
      </div>

      {/* Middle: processing orders */}
      <ProcessingOrdersPanel
        orders={processingOrders}
        activeOrderId={activeOrderId}
        orderType={orderType}
        orderTypeLocked={orderTypeLocked}
        lockedOrderType={lockedOrderType}
        onSelectOrder={loadProcessingOrder}
        onNewOrder={startNewOrder}
        onToggleLock={toggleOrderTypeLock}
        onSelectOrderType={selectOrderType}
      />

      {/* Right: cart + payment + totals + actions */}
      <div className="billing-cart">
        <CartPanel
          cart={cart}
          onQuantityChange={changeCartQuantity}
          onQuantityBlur={fixCartQuantity}
          onRemove={removeFromCart}
        />

        <PaymentPanel
          open={isPaymentOpen}
          onToggleOpen={() => setIsPaymentOpen((v) => !v)}
          paymentMode={paymentMode}
          billingType={billingType}
          onSelectPayment={(mode) => {
            setPaymentMode(mode);
            setBillingType("Cash");
          }}
          onSelectCredit={() => {
            setBillingType("Credit");
            setPaymentMode("Cash");
          }}
          creditCustomers={creditCustomers}
          selectedCustomerId={selectedCustomerId}
          onSelectCustomer={setSelectedCustomerId}
          onAddCustomer={() => setIsAddCustomerOpen(true)}
        />

        <div className="bill-summary">
          <div className="summary-row">
            <span>Subtotal</span>
            <span>{formatCurrency(subtotal)}</span>
          </div>
          {gstConfig.enabled && (
            <div className="summary-row">
              <span>
                {gstConfig.type === "Inclusive"
                  ? `Included GST (${gstConfig.percentage}%)`
                  : `GST (${gstConfig.percentage}%)`}
              </span>
              <span>{formatCurrency(gst)}</span>
            </div>
          )}
          <div className="summary-row total">
            <span>Total Amount</span>
            <span>{formatCurrency(total)}</span>
          </div>
        </div>

        <div className="billing-actions">
          <button className="btn btn--ghost" onClick={() => handlePrintKOT()} disabled={isProcessing}>
            {disableKot ? <CheckCircle size={16} /> : <Printer size={16} />}
            {isProcessing ? "Processing..." : disableKot ? "Save Order" : "Print KOT"}
          </button>
          <button className="btn btn--primary" onClick={handleCheckout} disabled={isProcessing}>
            <CheckCircle size={16} />
            {isProcessing ? "Processing..." : "Complete Bill"}
          </button>
        </div>
      </div>

      {/* Popups */}
      {qtyItem && (
        <QtyPopup
          item={qtyItem}
          onConfirm={(quantity) => {
            addToCart(qtyItem, quantity);
            setQtyItem(null);
          }}
          onCancel={() => {
            setQtyItem(null);
            searchInputRef.current?.focus();
          }}
        />
      )}

      {isTablePopupOpen && (
        <TablePopup
          value={tableNumber}
          onChange={setTableNumber}
          onConfirm={handleTableConfirm}
          onCancel={() => {
            setIsTablePopupOpen(false);
            searchInputRef.current?.focus();
          }}
        />
      )}

      {isAlphabetPopupOpen && (
        <AlphabetPopup
          baseTable={tableNumber}
          orders={processingOrders}
          onMergeIntoOrder={handleMergeIntoOrder}
          onNewSubTable={handleNewSubTable}
          onBack={() => {
            setIsAlphabetPopupOpen(false);
            setIsTablePopupOpen(true);
          }}
        />
      )}

      {isKotConfirmOpen && (
        <PrintConfirmPopup
          title="Print KOT?"
          message="Are you sure you want to print the KOT?"
          onYes={() => {
            setIsKotConfirmOpen(false);
            executePrintKOT();
          }}
          onNo={() => {
            setIsKotConfirmOpen(false);
            executePrintKOT(undefined, true);
          }}
          onClose={() => setIsKotConfirmOpen(false)}
        />
      )}

      {isBillConfirmOpen && (
        <PrintConfirmPopup
          title="Print Bill?"
          message="Are you sure you want to print the final Bill?"
          onYes={() => {
            setIsBillConfirmOpen(false);
            executeCheckout();
          }}
          onNo={() => {
            setIsBillConfirmOpen(false);
            executeCheckout(true);
          }}
          onClose={() => setIsBillConfirmOpen(false)}
        />
      )}

      {isAddCustomerOpen && <AddCustomerPopup onAdd={handleAddCustomer} onCancel={() => setIsAddCustomerOpen(false)} />}
    </div>
  );
}
