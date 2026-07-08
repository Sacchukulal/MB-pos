# Magic Bill — Complete Project Specification & Rebuild Blueprint

> **Version:** 1.0 · **Date:** 2026-07-02 · **App version analyzed:** 0.2.6
> **Purpose:** Full inventory of the existing application (features, workflows, data,
> settings, bugs, debt) and the architecture plan for a clean rebuild that preserves
> **100% of existing functionality**.

---

## 1. Product Overview

Magic Bill is an offline-first **Windows desktop restaurant billing / POS application**
built with **Tauri v2 (Rust) + React 19 + TypeScript + SQLite** (via
`@tauri-apps/plugin-sql`). It targets Indian restaurants: thermal-printer (ESC/POS)
bills and KOTs, GST, UPI QR payments, token numbers, table/parcel/self-service orders,
credit (khata) customers, expenses, and sales analytics.

Cloud pieces: **Firebase Firestore** for license/subscription verification (one device
per license, hardware-bound), and the **Tauri updater** pulling from GitHub Releases.

### Tech stack (current)

| Layer | Technology |
|---|---|
| Shell | Tauri v2 (Rust), Windows-focused (winapi spooler printing) |
| UI | React 19 + TypeScript 5.8 + Vite 7 |
| DB | SQLite file `restaurant.db` in a user-chosen folder |
| Charts | Recharts |
| Icons | lucide-react (+ Tabler icons webfont — barely used) |
| Cloud | Firebase (Firestore only in practice) |
| Printing | Raw ESC/POS bytes → Win32 spooler (`WritePrinter`), text fallback via PowerShell `Out-Printer` |
| Updates | `@tauri-apps/plugin-updater` → GitHub Releases `latest.json` |

---

## 2. Complete Feature Inventory (MUST all be preserved)

### 2.1 First-run / bootstrap
- On first launch, a welcome screen forces the user to **select a database folder**
  (stored in `localStorage.dbFolderPath`); `restaurant.db` is created there.
- All tables are created/migrated on every startup (idempotent `CREATE TABLE IF NOT
  EXISTS` + `ALTER TABLE ADD COLUMN` wrapped in try/catch).
- Database folder can be changed later from General Settings (requires app restart).

### 2.2 Navigation shell
- Compact left icon sidebar: **Account (logo button), Dashboard, Billing, Expenses,
  Reports**, and a **Settings** toggle that slides out a secondary panel with:
  General Settings, Bill Settings, Printer Settings, Menu Management, Staff Management.
- **Unsaved-changes guard**: navigating away from a settings tab with dirty state shows
  a Save / Discard / Cancel dialog; “Save” invokes the active tab’s save function.
- Update chip in sidebar footer: shows current version, manual “Check updates”,
  “Update available vX.Y.Z” state, install modal with progress bar + relaunch,
  error + retry state, auto-check 5 minutes after launch raising a skippable snackbar,
  and a transient “you’re on the latest version” toast.

### 2.3 Billing screen (the core workflow)
- **Item search** with keyboard-first UX:
  - Match mode setting: *starts-with* or *contains* (contains results ranked:
    prefix match → word-start match → substring match).
  - Max 10 suggestions; ArrowUp/Down navigation with scroll-into-view; Enter opens
    quantity popup; Esc closes suggestions, then clears/starts a new order.
  - Typing a number that equals an active **table number** and pressing Enter loads
    that table’s processing order.
  - Enter on an empty search: prints KOT (if not yet printed) else completes the bill;
    if the cart is empty, loads the first processing order.
  - Global keydown handling (when no popup/input focused): ArrowLeft/Right cycles the
    order type (unless locked), ArrowUp/Down cycles through processing orders,
    Esc starts a new order. Any click outside inputs refocuses the search box.
- **Quantity popup** per item: +/- buttons, numeric input (auto-select on focus,
  min 1, blank→1), Enter confirms, Esc cancels.
- **Cart** (right sidebar): editable quantity per row (blank→0→restored to 1 on blur),
  remove item, per-line totals, subtotal / GST / grand total.
- **GST engine**: enabled = `show_gst && gst_enabled`; type Exclusive
  (`gst = subtotal × p%`, total = subtotal + gst) or Inclusive
  (`gst = subtotal − subtotal/(1+p)`, total = subtotal); percentage 5/12/18.
- **Payment modes**: Cash / Card / UPI, plus **Credit Billing** mode with customer
  dropdown and inline “Add Customer” popup (name + 10-digit phone validation).
  Collapsible payment section header shows the current mode.
- **Order types**: Self Service / Table / Parcel segmented control.
  - **Order-type lock** toggle (persisted in localStorage): pins a type; new orders
    reset to the pinned type; arrow-key switching disabled while locked; mouse
    selection re-pins.
- **Table workflow**:
  - Table orders require a table number (numeric input popup, Enter/Esc keys).
  - If the table is occupied, an **alphabet popup** shows the existing order(s) (as
    merge targets) plus sub-table letters **B–H** in a 4-column keyboard-navigable
    grid; choosing an existing order **merges** the new items into it (and prints a
    delta KOT); choosing a letter creates e.g. table “6B”. Occupied letters disabled.
- **KOT printing**:
  - New order → inserts into `processing_orders` with assigned **token number** and
    **bill number** (prefix + counter), increments both counters.
  - Existing order → updates it and prints **only the delta** (new items / increased
    quantities vs. the loaded snapshot).
  - **Printer routing**: Single Printer mode (all to default) or Multiple Printers
    mode with **per-category printer mapping** (fallback: default printer).
  - **KOT style**: one combined ticket per printer, or **category-wise tickets**
    (with category name header).
  - Optional **KOT confirmation popup** (Enter = print, Esc = save without printing).
  - **Disable KOT** setting: button becomes “Save Order”, order goes straight to
    processing without printing.
  - Failure path: raw ESC/POS print → plain-text fallback → error toast (order is
    still saved).
- **Processing orders panel**: live list of open orders (bill no., table badge with
  blink animation, order type, total); click or ArrowUp/Down to load; “New (Esc)”
  button clears the workspace.
- **Checkout (“Complete Bill”)**:
  - Optional bill-confirmation popup (Enter = print, Esc = finalize without printing).
  - Finalizes to `finalized_orders` (deletes the processing order; keeps its
    created_at, bill no., token). Direct checkout without a KOT assigns token/bill
    numbers and increments counters itself.
  - **Credit** checkout adds the total to the customer’s `credit_balance`.
  - Prints the **bill receipt**: store header (name/address/phone/GSTIN/FSSAI with
    per-field visibility), bill no. + date, time + cashier, order type/table, token
    number (sized Normal/Large/Extra-Large), item table, subtotal, GST line (or
    “Includes ₹x GST” for inclusive), grand total (always bold), footer message,
    optional logo image (top), and **UPI QR** (dynamic = amount embedded; from
    `upi_id`, merchant name, optional transaction reference). Per-section font sizes,
    bold flags, and 7 individually-toggleable line separators all honored.
  - No default printer configured → clear toast telling the user how to fix it.
- **Token & bill counters**: daily reset (per setting) to configured starting numbers,
  triggered on first Billing load of a new day; current values editable in settings.
- **Subscription check on Billing load** (grace logic; see §2.10). NOTE: Billing is
  **not** blocked when expired (only search autofocus is skipped) — Dashboard and
  Reports are the gated screens. Preserve this behavior.

### 2.4 Dashboard
- Gated by subscription (expired → “Activate Plan” card).
- Time range selector: Today / 7 days / 30 days / All time (persisted).
- KPIs: Gross Revenue, Total Orders, Avg Order Value, Net Profit (revenue − expenses,
  red when negative).
- Charts (theme-token colored): Revenue-vs-Expenses area trend (hourly buckets for
  “today”, daily otherwise; single-point series duplicated to render), Sales-by-
  category donut (top 5), Payment-modes horizontal bars.
- Top 5 selling items (by qty) list; Recent orders table (last 6).

### 2.5 Reports & Analytics
- Gated by subscription. Two main tabs:
- **Sales Overview** (date-range picker, defaults today):
  - *Sales Summary*: 7 stat cards (revenue, gross, GST, avg bill, orders+items,
    expenses, net profit), payment-method breakdown table (bills/share/amount),
    order-type breakdown, top-5 items table.
  - *Day-wise Sales*: stat cards (active days, avg/day, best day, total) + per-day
    table with totals footer.
  - *Item Sales*: filter by item, category, and search text; ranked table with % share
    and totals footer.
  - *Tax Report*: taxable value, CGST/SGST (GST÷2), totals; per-bill table.
  - *Expenses*: stat cards, by-category breakdown, full expense list.
  - *Recent Bills*: table with **inline payment-mode editing** and **reprint bill**
    per row.
  - Every report has **Print (thermal)** and **Export CSV**.
- **Credit Customers**:
  - Stat cards (total customers, with due, total outstanding).
  - Customer table: view / **edit name inline** / **delete** (nullifies orders’
    customer_id, deletes payments + customer, with confirm).
  - **Customer detail modal**: pending balance, print statement (thermal),
    **Settle All Due**, **record partial payment** (amount + mode), transaction
    history (bills + payments merged, date-range filtered) with per-bill reprint.

### 2.6 Expenses
- Add expense (description, amount, category from fixed list: Daily Needs, Salary,
  Rent, Utilities, Maintenance, Other); list with delete (confirm); running total.

### 2.7 Menu Management
- Two-pane: categories list (add w/ duplicate check, inline rename, delete w/ confirm
  + cascade item delete warning) and items in selected category (add with
  Enter-to-price-field flow, inline edit of name/price/**category move** with
  duplicate checks, delete w/ confirm). Optimistic UI updates with refetch.

### 2.8 Staff Management
- Add staff (name, role: Manager/Waiter/Chef/Cashier, 10-digit phone), list, delete.
  (Schema has an unused `pin` column reserved for future login.)

### 2.9 Settings screens
- **General**: theme picker (Dark / Light / Ocean Blue / Custom with live-derived
  palette editor: mode + base tint + accent, auto-balanced tokens, reset), store info
  (hotel name, address, phone, GSTIN, FSSAI), UPI payment (UPI ID, merchant name,
  payment reference), database folder display + change.
- **Bill Settings** (with live **bill preview** and **KOT preview** panels):
  - Receipt format: preview width (3/4/5 inch), row height (compact/standard/relaxed).
  - Billing search match mode (starts/contains).
  - Global font family (applies to bill + KOT; writes header/body/footer families).
  - Per-section size steppers + bold: hotel name, address/meta, item table, totals,
    footer (bill) and title, details, items (KOT).
  - 7 bill separators + 5 KOT separators, individually toggleable.
  - Content visibility: token, GSTIN, FSSAI, address, phone, cashier (bill);
    KOT title, token, bill no., order type, table, date, 2-column meta packing (KOT).
  - UPI QR: Dynamic / Static / No QR (radio).
  - GST: enable, type (Inclusive/Exclusive), percentage (5/12/18).
  - Logo: position (none/top), image upload (base64), size % slider.
  - Footer message.
- **Printer Settings**: printer mode (single/multiple), KOT style, default printer
  dropdown (native printer enumeration) + **Test Print** + **Refresh List**, paper
  size (2/3/4 inch), bold ESC/POS toggle, per-category printer mapping grid,
  KOT/bill confirmation toggles, disable-KOT toggle, token numbering (daily reset,
  start, current, print size), bill numbering (daily reset, prefix, start, current).

### 2.10 Account / Licensing
- Activation screen: enter license key (Firestore `users/{key}` doc) with
  “Restore previous key” shortcut.
- **One-device enforcement**: license binds to a hardware ID (SMBIOS UUID, fallback
  MAC, from a Rust command); already-bound-elsewhere → hard block with device name;
  heartbeat `device.lastSeen` refresh; App-level sync also detects a transferred
  binding and locks the device back to activation.
- Logged-in view: hero card with initials avatar, status chip
  (ACTIVE / GRACE PERIOD / EXPIRED / LOCKED), plan chip, Sync and Deactivate buttons,
  identity panel (name/business/mobile/email), plan panel (next billing date, days
  left / grace days, contextual alerts, Renew link), device-lock panel with ID.
- Subscription snapshot cached in SQLite (`subscription` table) for offline checks;
  `user_details` cached similarly.
- **Grace/tamper logic** (used by Dashboard, Reports, Billing): expired unless
  `now ≤ nextBillingDate + 10 days`; clock rollback detection via monotonically
  advancing `last_checked_date` (now < lastChecked ⇒ treated as expired).

### 2.11 Auto-update
- Manual + delayed automatic check; modal-driven download with byte progress;
  relaunch on completion; signed updates (minisign pubkey in tauri.conf.json).

### 2.12 Native (Rust) commands
- `get_printers` (PowerShell `Get-Printer` / `lpstat`), `print_receipt_raw`
  (Win32 spooler RAW datatype with descriptive Win32 error hints; `lp -o raw` on
  Unix), `print_receipt_text` (PowerShell `Out-Printer` via temp file; `lp`),
  `get_device_id` (SMBIOS UUID with placeholder-UUID rejection, MAC fallback),
  and an unused `greet` sample.

---

## 3. Database Structure (current)

All tables live in one SQLite file. Singleton tables use `id INTEGER PRIMARY KEY CHECK (id = 1)`.

| Table | Purpose | Columns (effective, after migrations) |
|---|---|---|
| `store_settings` (singleton) | Business identity | hotel_name, address, phone_number, gst_number, fssai_number, upi_id, merchant_name, payment_reference |
| `bill_settings` (singleton) | Receipt/KOT appearance + misc | footer_message, show_gst, show_fssai, show_address, show_phone, **bill_font_size (dead)**, printer_size, header/body/footer_font_family+size, gst_enabled, gst_type, show_cashier_name, gst_percentage, row_height, logo_position/size/**opacity (dead)**/base64, show_line_separators, show_token, sep_header/meta/token/table_header/table_body/subtotals/grand_total, store_name_size, address_size, table_font_size, total_font_size, dynamic_upi_qr, **static_upi_qr (stored, never read by print code)**, no_qr_print, search_match_mode, global_font_family, store_name/address/table/total/footer_bold |
| `kot_settings` (singleton) | KOT appearance | header/body font family+size, row_height, show_line_separators, show_token, sep_token/header/meta/table_header/table_body, table_font_size, show_kot_title/bill_no/order_type/table/date, meta_font_size, title/meta/items_bold, meta_two_column |
| `printer_settings` (singleton) | Hardware + numbering | printer_mode, default_printer, kot_printing_style, token_reset_daily, token_starting_number, token_current_number, token_print_size, bill_reset_daily, bill_starting_number, bill_current_number, bill_prefix, last_reset_date, paper_size, print_bold, kot_print_confirmation, bill_print_confirmation, disable_kot |
| `categories` | Menu categories | id, name (UNIQUE) |
| `items` | Menu items | id, category_id FK, name, price |
| `category_printers` | Category→printer routing | category_id PK/FK, printer_name |
| `processing_orders` | Open orders (KOT stage) | id, cart_data (JSON), customer_name/phone, payment_mode, subtotal, gst, total, order_type, table_number, customer_id, token_number, bill_number, created_at |
| `finalized_orders` | Completed bills | same shape as processing_orders |
| `customers` | Credit customers | id, name, phone, credit_balance, created_at |
| `customer_payments` | Credit repayments | id, customer_id FK, amount, payment_mode, date |
| `expenses` | Expense entries | id, description, amount, category, date |
| `staff` | Team members | id, name, role, phone, pin (unused) |
| `subscription` (singleton) | Cached license state | status, planId, subscriptionId, nextBillingDate, updatedAt, last_checked_date |
| `user_details` (singleton) | Cached account info | displayName, email, mobileNumber, restaurantName (created lazily by Account.tsx) |

**Cart JSON shape** (inside `cart_data`): `{ id, category_id, name, price, quantity }[]` — a denormalized snapshot (prices frozen at sale time; item renames don’t rewrite history). This is a good property; keep it.

**localStorage keys**: `dbFolderPath`, `orderTypeLocked`, `lockedOrderType`, `dashboardTimeRange`, `magicbill_license_key`, `magicbill_license_key_history`, `magicbill_theme`, `magicbill_custom_colors`.

**Firestore** (`users/{licenseKey}`): `displayName, email, mobileNumber, restaurantName, subscription{status, planId, id, nextBillingDate, updatedAt}, device{id, name, platform, boundAt, lastSeen}`. Rules file `firestore.rules` in repo.

---

## 4. Bugs & Defects Found

### Functional bugs
1. **Reprint QR never prints** — `Reports.tsx` checks `billSettings?.no_qr_print === false` on the **raw SQLite row** (0/1, never boolean `false`), so the UPI QR is silently skipped on every reprint. Billing coerces to booleans first, so it works there. Root cause: no single coercion layer.
2. **Reprint ≠ original bill** — `handleReprintBill` uses legacy `header_font_size` logic and ignores per-section sizes/bold, separator toggles, cashier/GSTIN/FSSAI visibility, and logo. A reprinted bill looks different from the original.
3. **Tax Report wrong for Inclusive GST** — “Taxable Value” = `sum(subtotal)`, but with inclusive GST the stored subtotal already includes GST; taxable value should be `subtotal − gst`. CGST/SGST derived from an inflated base.
4. **Regex injection in table lookup** — `new RegExp("^" + tableNumber + "[A-Z]?$")` built from user input (also in `getOccupiedOrdersForTable`). Input is digit-filtered in the popup, but the search-box “load table by number” path passes arbitrary text into `.match()` in `handleTableConfirm`’s siblings; malformed input can throw.
5. **First KOT of a table order can print without the table number** — `executePrintKOT` uses `finalTableNumber` for the DB write but the KOT meta line reads the **stale `tableNumber` state** (`show_table && tableNumber`), so a sub-table letter chosen a moment ago (state not yet committed) may be missing from the printed ticket.
6. **`static_upi_qr` has no effect** — the print path only distinguishes `dynamic_upi_qr` (adds `&am=`) vs `no_qr_print`; Static behaves identical to Dynamic-minus-amount only because the amount param is skipped — the UI presents three modes but only two exist in code. (Preserve the three-way UI; wire static = QR without amount, dynamic = with amount, which is the evident intent.)
7. **Paper-size settings conflict** — `bill_settings.printer_size` (3/4/**5**inch, drives only the preview) vs `printer_settings.paper_size` (**2**/3/4inch, drives actual line width). Two settings for one concept, with non-overlapping option sets; the preview can show a size the printer path doesn’t support.
8. **Daily-reset date is locale-dependent** — `last_reset_date` stores `toLocaleDateString('en-GB')`; if OS locale formatting ever differs, resets misfire. Should be ISO `YYYY-MM-DD`.
9. **Counter race / stale-state risks** — token/bill counters are read from React state and incremented via SQL after use; rapid operations (guarded only by `isProcessing`) and multi-window use could duplicate numbers. Also `token_current_number || 100` and `bill_current_number || 1` fallbacks silently mask a `0` counter (0 is falsy → treated as 100/1).
10. **`show_gst` doubles as two settings** — it toggles GSTIN visibility in the header **and** participates in enabling GST math (`isGstEnabled = show_gst && gst_enabled`). Turning off the GSTIN header line disables GST calculation entirely.
11. **ESC/POS strip fallback is lossy** — the regex `/[\x1B\x1D][^a-zA-Z0-9]*[a-zA-Z0-9]/g` used to clean text for the fallback printer can eat legitimate characters and misses multi-byte commands.
12. **Dashboard memo missing `timeRange` dep** — `dashboardData` uses `timeRange` for bucketing but doesn’t list it as a dependency (works today only because `orders` changes with it).
13. **Billing screen accessible when plan expired** — subscriptionStatus is computed and used *only* to decide autofocus; there is no gate. If intentional (billing keeps working offline), document it; the dead-looking state machine invites accidental “fixes”.
14. **No way to void/delete a processing order** — once a KOT is saved the only exit is Complete Bill; a mistaken order stays in the list forever (functional gap worth flagging; adding a void action is a UX improvement, not a behavior change to existing flows).
15. **“Full Settlement” recorded as a payment_mode** — pollutes payment-mode analytics for customer payments.
16. **`alert()`** used for DB-init failure; blocking and unstyled.
17. **Global click handler steals focus** to the search input on any non-input click on the Billing page — interferes with text selection/buttons in edge cases (it does check selection, but not e.g. focusable custom controls).

### Security / hygiene issues
18. **Tauri updater signing private key committed** — in `package.json` (`tauri` script env), `.env`, `tauri.key`, `temp_priv.txt`, `temp_priv_no_bom.txt`, `test_key.key`. Anyone with repo access can sign malicious updates. **Rotate the key and purge from history.**
19. **CSP is `null`** in `tauri.conf.json`.
20. Firebase config hardcoded in source (acceptable for Firebase web keys, but should live in one config module / env).
21. Legacy branding: Rust prints doc name “EasyBill Receipt”, temp file `easy_bill_receipt.txt`.

### Dead code / repo cruft
- `src/components/Settings.tsx` — entire tabbed settings screen, never imported by App.
- `greet` Rust command; `auth` export in `firebase.ts` (Firebase Auth initialized, never used).
- `bill_settings.bill_font_size`, `logo_opacity` (UI removed, column remains), `kot_settings.body_font_family/size` (superseded by per-section sizes), `logo_position` values `bottom`/`watermark` handled in `buildPrintData` but not offered by UI.
- `Reports.tsx` prop `onRequireAuth` never used.
- Repo root litter: `original_account.tsx`, `easy_bill_backup.patch` (974 KB), `extract.txt`, `bump_styles.cjs`, `fix_bill_settings.cjs`, `fix_panels.cjs`, `fix_printer_settings.cjs`, `fix_staff_menu.cjs`, `replace_css.cjs`, `set_secret.cjs`, `set_new_secret.cjs`, `stitch.cjs`, `swap.cjs`, `swap.js`, `update_fg_colors.cjs`, key/temp files, `src/theme/components.css_debug`.
- `public/assets/**` — an entire leftover HTML-template asset tree (its own theme.css, chart.js, swiper.js, choice.js, etc.) shipped in `dist` but unused by the React app.
- Heavy/global deps of dubious use: `bootstrap` JS bundle imported in `main.tsx` (no Bootstrap components in code), `simplebar` global import, `@tabler/icons-webfont` (lucide is the icon system).

### Duplicated logic (the biggest debt)
| Duplicated thing | Copies |
|---|---|
| Subscription/grace/tamper check | Billing, Reports, Dashboard (3 near-identical blocks) + variants in App and Account |
| `getLineWidth`, `padLeft`, `padRight` | Billing + Reports |
| `generateESCPOSImage` (canvas → GS v 0 raster) | Billing + Reports (byte-identical logic) |
| `buildPrintData` | Billing + Reports (Billing’s adds logo support) |
| UPI QR string + append-before-cut splice | Billing + Reports |
| Token-size ESC/POS switch (`\x1D\x21…`) | 3 places |
| SQLite boolean coercion (`x !== 0 && x !== false && x !== "0"`) | ~60 inline occurrences across Billing/BillSettings + a stray `isOn()` helper |
| Toast state + 3s auto-hide effect | 7 components, each its own |
| “Plan expired” card markup | Dashboard + Reports (inline SVG copies) |
| Expired-plan/grace period constant (10 days) | 4 places |
| Settings dirty-check + `setTriggerSave` wiring | GeneralSettings, BillSettings, PrinterSettings (same boilerplate ×3) |
| Save (“UPDATE if exists else INSERT”) pattern with 44 positional params | BillSettings (and smaller versions elsewhere) — extremely fragile (`$39` reused for 4 columns as a sync hack) |
| Bill text layout | Billing checkout vs Reports reprint (diverged — see bug 2) |
| Hardcoded fallbacks (“3inch”, 48 cols, token 100, GST 5, “Thank you! Visit again.”, “Cashier: Admin”) | scattered everywhere |

### UI/UX issues observed
- Massive inline `style={{…}}` objects everywhere (hundreds), fighting the token
  system; App.css is 2,877 lines with several generations of class systems coexisting
  (`modern-*`, `sx-*`, `rep-*`, `settings-*`, `dash-*`).
- Billing layout uses a hardcoded `416px` sidebar and ad-hoc flex; popups are four
  hand-rolled variants of the same modal.
- “Cashier: Admin” is hardcoded on receipts even though Staff Management exists.
- Inconsistent confirmations: window.confirm (Expenses, Staff, Reports-customers) vs
  styled modal (Menu) vs none.
- Inconsistent currency/number formatting (`toFixed(2)` vs `toLocaleString('en-IN')`).
- Inconsistent empty/loading states (spinners, text, blinking icons).
- Window starts 800×600 then maximized; title bar is default.

---

## 5. Required Settings — Canonical Consolidated Model

The rebuild centralizes all configuration into **one typed settings service** with four
persisted groups (same SQLite tables, cleaned):

1. **Store profile**: name, address, phone, gstNumber, fssaiNumber, upi { id, merchantName, reference }.
2. **Receipt design** (bill + KOT sub-objects): paperSize (**single** setting: 2/3/4 inch — drives preview *and* print), fontFamily (global), per-section { size, bold }, separators map, visibility map, rowHeight, logo { position, base64, sizePct }, footerMessage, qrMode (`dynamic | static | none`), gst { enabled, type, percentage }, gstinVisibleOnHeader (**split from gst.enabled** — fixes bug 10 while defaulting to current combined behavior).
3. **Printing & numbering**: printerMode, defaultPrinter, kotStyle, categoryPrinterMap, printBold, confirmations { kot, bill }, disableKot, token { resetDaily, start, current, printSize }, bill { resetDaily, prefix, start, current }, lastResetDate (ISO).
4. **App preferences** (localStorage): theme, customColors, dbFolderPath, orderTypeLock, dashboardTimeRange, license keys.

Boolean coercion happens **once** at the repository boundary (SQLite int → boolean);
components only ever see typed objects. Defaults live in a single `config/defaults.ts`.

---

## 6. Recommended Architecture (rebuild)

```
src/
├── app/                    # Shell: App, routing (tab state), Sidebar, UpdateManager
├── config/                 # constants.ts (paper widths, payment modes, order types,
│                           #   grace period, currency), defaults.ts, firebase.ts
├── db/
│   ├── client.ts           # Database.load, single connection
│   ├── migrations.ts       # numbered migrations + schema_version table
│   └── repositories/       # settingsRepo, menuRepo, ordersRepo, customersRepo,
│                           #   expensesRepo, staffRepo, subscriptionRepo
├── services/
│   ├── printing/
│   │   ├── escpos.ts       # command constants, styled(), padding, line width
│   │   ├── image.ts        # raster + QR encoding (one copy)
│   │   ├── templates/      # billTemplate, kotTemplate, reportTemplate,
│   │   │                   #   statementTemplate — all settings-driven; reprint
│   │   │                   #   uses billTemplate (fixes divergence)
│   │   └── printService.ts # route jobs, raw→text fallback, timeouts
│   ├── license/            # deviceId, firestore sync, grace/tamper logic (one copy)
│   └── update/             # updater state machine
├── features/               # One folder per screen: billing/, dashboard/, reports/,
│   │                       #   expenses/, menu/, staff/, settings/, account/
│   │                       # each = components + hooks + logic for that feature
├── components/ui/          # Button, Modal, ConfirmDialog, Toast(+provider),
│                           # Field/Input/Select, Table, Tabs, StatCard, EmptyState,
│                           # Spinner, Badge, DateRangePicker
├── hooks/                  # useToast, useSettings, useLicenseGate, useKeyboardNav,
│                           # useUnsavedChanges
├── theme/                  # theme.css (tokens — keep, it's good), ThemeContext (keep)
├── types/                  # domain models (Order, CartItem, Settings groups…)
└── utils/                  # formatCurrency, dates (ISO helpers), sqliteBool
```

**Principles**
- UI ↔ business logic ↔ data strictly separated; components never run SQL or emit
  ESC/POS bytes.
- Every duplicated block in §4 becomes exactly one module.
- Migrations: keep the existing idempotent bootstrap **plus** a `schema_version`
  table so future changes are ordered; existing databases must open unchanged.
- Strict TypeScript (`strict: true`), no `any` in domain code.
- All user-visible strings/formats via shared helpers (currency, dates).
- Inline styles eliminated; one consistent class system on top of the existing token
  contract (theme.css is already well designed — keep tokens, rewrite component CSS).
- Rust side: keep the 4 real commands, drop `greet`, rename doc/temp-file branding,
  set a real CSP.
- Repo hygiene: delete all root cruft; **rotate the updater signing key** and move it
  to CI secrets; remove bootstrap/simplebar/tabler deps unless a real usage is found.

---

## 7. UI/UX Refresh Plan (same features, modern skin)

- **Design system**: keep the token contract (`--bg-*`, `--text-*`, `--accent`,
  semantic colors) and the 4 themes incl. custom palette derivation. Rebuild component
  CSS as a small set of primitives (buttons, cards, inputs, tables, modals, toasts,
  tabs) used everywhere — no more 5 parallel class families.
- **Consistency passes**: one modal component (replaces 6 hand-rolled popups), one
  toast system (top-right, stacked, semantic variants), one confirm dialog (replaces
  window.confirm), one table style, one empty/loading state pattern, uniform
  ₹ formatting (`en-IN`, 2 decimals) everywhere.
- **Billing screen**: keep the exact keyboard model (it’s the product’s soul), fix
  the fluid 3-zone layout (search+suggestions / processing orders / cart+pay),
  responsive at smaller widths, subtle motion on order cards and modals.
- **Settings**: consistent section cards, sticky save bar with dirty indicator
  (replaces per-form buttons + keeps the navigation guard).
- **Dashboard/Reports**: aligned stat-card grid, consistent chart styling from chart
  tokens, proper table density.
- Micro-animations: modal fade/scale, toast slide, tab transitions — CSS only,
  120–200 ms, respects `prefers-reduced-motion`.

---

## 8. Optional Enhancements (non-breaking, post-parity)

These add usability without altering existing behavior — implement only after parity:
1. Void/cancel a processing order (with confirm) — closes gap #14.
2. Cashier selection at billing time from Staff (falls back to “Admin”), wiring the
   existing staff data to the existing “Cashier:” receipt line.
3. Keyboard-shortcut help overlay (`?`).
4. Database backup/restore (copy `restaurant.db` with timestamp) from General Settings.
5. Expense date editing + inline edit.
6. Search by item short-code (prefix in item name).
7. Update changelog display in the update modal.

---

## 9. Rebuild Plan & Parity Checklist

**Phased execution** (each phase leaves the app runnable):
1. Foundation — repo cleanup, config/constants, db client + repositories +
   migrations, settings service, theme kept, UI kit.
2. Shell — sidebar, navigation, unsaved-changes guard, update manager.
3. Billing — search/cart/KOT/checkout/tables/credit (highest risk; port keyboard
   behavior verbatim, then test against §2.3 line by line).
4. Print service — templates for bill/KOT/report/statement; verify against current
   output byte-for-byte where settings match (fixing only bugs 1, 2, 5, 6).
5. Dashboard, Reports, Expenses, Menu, Staff.
6. Settings screens + Account/licensing.
7. QA sweep against this document’s §2 inventory; `tsc && vite build` clean;
   manual print tests.

**Acceptance criteria**
- Every §2 feature verified present and behaving identically (bug fixes from §4
  functional list are the only sanctioned behavior changes, each noted in commit).
- Existing `restaurant.db` files open and work without data loss.
- Zero duplicated blocks from §4’s table; zero inline hex colors outside print
  previews; zero `any` in domain code; no dead files.
