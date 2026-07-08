# Magic Bill — Restaurant Billing System

A lightweight, offline-first desktop POS for restaurants, built with **Tauri v2 (Rust)** and **React 19**.

## 🚀 Features

*   **Billing (keyboard-first):** item search with ranked matching, quantity popup, KOTs with per-category
    printer routing and delta printing, table / sub-table (6, 6B…) workflow with order merging,
    Self Service / Table / Parcel order types with a lock, token & bill numbering with daily resets,
    Cash / Card / UPI / Credit payments.
*   **Thermal printing (ESC/POS):** fully configurable bill & KOT layouts (per-section fonts, bold,
    separators, visibility), store logo, UPI QR (dynamic with amount / static / off), test print,
    plain-text fallback when raw printing fails.
*   **Dashboard:** revenue/expense trend, category & payment analytics, top items, recent orders.
*   **Reports:** sales summary, day-wise, item sales with filters, GST tax report, expenses,
    recent bills with reprint & payment-mode editing — all printable and exportable to CSV.
*   **Credit customers (khata):** balances, partial payments, settlements, printable statements.
*   **Expenses & Staff** management.
*   **Theming:** Dark / Light / Ocean Blue / fully custom palette (auto-balanced for contrast).
*   **Licensing:** one-device hardware-bound license via Firebase, offline grace period.
*   **Auto-update:** signed updates from GitHub Releases.
*   **Local database:** SQLite in a user-chosen folder — works fully offline.

## 🛠️ Tech Stack

*   **Frontend:** React 19, TypeScript (strict), Vite
*   **Styling:** vanilla CSS on a token-based theme system (`src/theme/theme.css`)
*   **Shell:** Tauri v2 (Rust) — printing via the Win32 spooler
*   **Database:** SQLite via `@tauri-apps/plugin-sql`
*   **Charts / icons:** Recharts, lucide-react

## 📦 Setup

Prerequisites: Node.js 18+, Rust (`rustup`), and Visual Studio C++ Build Tools on Windows.

```bash
npm install
npm run tauri dev     # development (opens the app window)
npm run tauri build   # production installers → src-tauri/target/release/bundle/
```

## 📂 Project Structure

```
src/
├── app/            App shell: navigation, sidebar, onboarding, auto-update UI
├── components/ui/  Reusable UI kit (Modal, ConfirmDialog, Toast, StatCard, …)
├── config/         Constants, defaults, Firebase config
├── db/             SQLite client, migrations, repositories (the only SQL layer)
├── features/       One folder per screen (billing, reports, dashboard, settings, …)
├── hooks/          useSettings, useToast, usePlanGate, useUnsavedGuard
├── services/       printing/ (ESC/POS toolkit + templates), license/
├── styles/         base.css, ui.css, features.css
├── theme/          Design tokens + ThemeContext (custom palette derivation)
├── types/          Domain & settings types
└── utils/          Formatting, GST math, SQLite coercion
src-tauri/          Rust shell: printer enumeration, raw printing, device ID
```

Architecture rules and the full feature inventory live in `PROJECT_SPECIFICATION.md`;
the rebuild changelog is in `REBUILD_NOTES.md`; theming rules in `THEME_GUIDELINES.txt`.

## 🗄️ Database

On first launch the app asks for a folder; `restaurant.db` is created there and the path is
remembered. The schema is created/migrated automatically at startup (see `src/db/migrations.ts`) —
existing databases from older versions open unchanged.

## 🔧 Troubleshooting

*   **`sql.execute not allowed`** — ensure `src-tauri/capabilities/default.json` includes the
    `sql:default`, `sql:allow-load`, `sql:allow-execute`, `sql:allow-select` permissions.
*   **Print fails with a Win32 error** — the toast includes a hint (printer renamed, spooler
    stopped, access denied). Re-select the printer in Printer Settings and use Test Print.
*   **Build hangs** — delete `src-tauri/target` and rebuild.

## 📝 License

Proprietary / Private use.
