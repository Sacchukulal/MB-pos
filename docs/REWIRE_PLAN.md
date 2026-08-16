# P31 — the wiring round

**Why this exists.** Every session up to P30.5 proved its work with `cargo test`.
The tests are honest and they all pass — but a Rust command with a test and no
button is a feature the shop cannot use. The owner installed the build, used it,
and found six things wrong in the first sitting. This round is the answer:
**run the real Tauri window, drive every command against it, and close the gap
between "Rust can do it" and "the shop can reach it".**

## How this round is checked

Not by `cargo test`. By the running counter:

| | |
|---|---|
| the app | `cargo run -p magic-bill` with `APPDATA` pointed at a scratch folder — a genuinely fresh install, D55/D156 |
| the driver | WebView2's own CDP endpoint (`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`) |
| what it can do | invoke any command against the real backend, read the real DOM, click real buttons, screenshot the real window |

`scripts/drive.mjs` is that harness, checked in so the next session does not
build it again.

## What was found

### The six the owner named

| # | What | Where it actually is |
|---|---|---|
| 1 | Menu categories cannot be added, renamed or removed, and cannot be routed to a printer | `save_menu_category` registered, never called; `route_category` reachable only from Settings > Printers |
| 2 | The item column in the print preview collapses to one character | `mb-print/src/layout.rs` — `Block::Columns` asks `cap_scale` whether **four columns** fit, not whether the columns' **widths** fit |
| 3 | No way to choose a logo | `mb-print` renders one; nothing in the product ever sets one. `BillContext.logo` is `None` at every call site |
| 4 | Bill and KOT fonts: one face, and sizes named Normal / Large / Extra large | one `Font::builtin()` for the whole process; `SIZES` in `settings/catalog.rs` |
| 5 | The first-run shop folder cannot be browsed | `FirstRun.tsx` offers the default path and found databases, and no picker. There is no file-dialog dependency in the tree at all |
| 6 | Unwired commands | **29 of 226**, listed in `NOT_WIRED.md` |

### Found while auditing, and worse than some of the six

| What | Why it matters |
|---|---|
| **A cart line's quantity cannot be changed** | `cart_set_qty` is unwired. The only control on a line is ✕. Changing 2 dosas to 3 means deleting the line and typing it again — on the till, mid-service |
| **"Cancel order" does not cancel the order** | it calls `cart_clear`. `cancel_order` — the one that writes the reason, the audit row and the kitchen's cancellation — is never called. An order that reached the kitchen and was then cancelled leaves no trace (audit B5/B6) |
| **A line cannot be voided after the kitchen ticket** | `void_line` unwired. Same hole from the other side |
| **A barcode never appears in the on-screen preview** | `Receipt.tsx` has no `barcode` arm; the switch returns `undefined` and React draws nothing. Turn on `receipt.bill_barcode` and the preview lies about the paper |
| **The print queue panel is empty after a window reload** | it is filled only by a pushed event. `list_print_jobs` — the fetch-on-mount — is unwired |
| The free trial renews the day it starts | `licensing::cloud()` passes `today` as `renews_on` to the stub |

Everything else is sound. Every screen and every tab was opened against the live
app: **no console errors, no thrown errors, no stuck spinners, no error toasts.**

## The order of work

Worst first means: money and printed paper before convenience.

### Stage 1 — the printed bill is wrong

1. **`layout.rs`: `cap_scale` for `Block::Columns` must weigh the columns' widths.**
   A fill column has a minimum readable width; below it the scale comes down,
   which is what rule two (crown jewel 18) already says everywhere else.
   This is one bill-printing bug, and it wastes a roll of paper per bill.
2. **`Receipt.tsx`: the `barcode` arm.** Plus a test that the preview draws one
   line per laid line for **every** variant, so the next one added cannot be
   forgotten.

### Stage 2 — the till loses money or history

3. **`cart_set_qty`** on the cart line: − qty +.
4. **`cancel_order`** behind the Cancel order button, with the reason dialog
   `reasons('cancel')` already returns.
5. **`void_line`** on a line whose kitchen ticket has gone.

### Stage 3 — the five the owner asked for

6. **Menu categories** — add, rename, deactivate, and route to a printer, on
   the Menu screen where the person already is.
7. **The logo** — a native picker, PNG in, one-bit bitmap stored, shown in the
   preview and printed.
8. **Fonts** — a face for the bill and a face for the KOT, and sizes in px.
9. **The first run** — browse for a folder, and `use_existing_shop` for a
   database that is already there.

### Stage 4 — the rest of the 29

In `NOT_WIRED.md`, in its own severity order.

### Stage 5 — the gate

10. `cargo test` (all crates), `npm run check`, a clean `cargo clippy`.
11. **The same live drive again, from a fresh `APPDATA`**, plus a screenshot of
    every screen touched.
12. Commit, and push.

## Two things this round will NOT quietly do

* **Sizes in px, honestly.** A thermal printer's character cell comes from the
  paper: `dots ÷ columns`, which is 12 × 24 dots on both 58 mm and 80 mm. The
  sizes a printer can actually make are that cell times 1, 2 or 3 — **24 px,
  48 px, 72 px** — and those are what the setting will say instead of Normal /
  Large / Extra large. A free-text px box would be a box that lies: the width
  has to stay on the column grid or the text sink, the PDF sink, the raster
  sink and the preview stop agreeing, which is the drift `mb-print` exists to
  prevent (D1, D29).
* **Font choices without a bigger installer.** Five extra faces in `assets/` is
  ~700 KB against S4's 20 MB. The faces come from `C:\Windows\Fonts` by name
  instead — every one of them is on every Windows 10/11 machine, `Font::load`
  already takes bytes, and a face that is somehow missing falls back to the
  built-in with a line in the log rather than a counter that will not print.

---

# What happened

Every stage above is done. `NOT_WIRED.md` carries the item-by-item record; this
is what the round came to.

## The gate

| | |
|---|---|
| `cargo test --workspace` | **green** — 534 in `magic-bill`, and every crate |
| `cargo clippy --workspace --all-targets` | **clean**, no warnings |
| `npm run check` (typecheck, 5 lints, 249 tests) | **green** |
| `node scripts/audit-wiring.mjs` | `wiring: clean — all 230 commands are reachable from a screen` |
| the running window, from an empty `APPDATA` | every screen and every tab: no console error, no unhandled rejection, no stuck spinner, no error toast |

## Driven against the real counter, not only tested

A whole sequence, through `window.__TAURI_INTERNALS__.invoke` on a fresh
install: open a table, add three dosas, **step the quantity**, **type an exact
one**, **say how many guests**, ask **what each owes**, tell the kitchen,
**void a line the kitchen already has** (which prints the cancellation slip),
take cash, settle — and the bill is in Bills, the day adds up, the queue can be
read back, the logo is on the paper.

The bill printed in Consolas and the kitchen ticket in Courier New, which the
log says out loud:

```
printing with Consolas from C:\WINDOWS\Fonts\consola.ttf
printing with Courier New from C:\WINDOWS\Fonts\cour.ttf
```

## Three things this round changed that were not on the list

* **The layout bug was the worst thing found.** It is not a missing wire, it is
  a shop burning a roll of paper per bill, and it had been shipping since P06
  behind a setting no test ever moved.
* **Two commands were deleted rather than wired.** `list_printers` and
  `nudge_print_offset` were duplicates of P17's, and audit E6 is about exactly
  that. `print_test_page` was on the same list and turned out not to be a
  duplicate — it carries P07's alignment ruler and works with no shop — so it
  got a button instead.
* **`App::print` is now the only door to the queue**, so no future print path
  can forget the shop's typeface. `hygiene_tests` keeps it that way.

## What is still open, and said out loud

* **The cloud is still `mb_license::cloud::Stub`** (P32–P34), which is why the
  Account screen says "Anna's Kitchen" and a free trial renews the day it
  starts. That is Phase 8's, it is documented in `licensing.rs`, and this round
  deliberately did not paper over it.
* **`go_back_a_version` returns words, not an action.** It finds the kept
  installer and says where it is; launching a process that replaces the running
  one is Phase 8's other half (`updates.rs` says so). The screen shows the
  sentence rather than pretending.
* **The look is still a person's judgement.** The harness proves behaviour and
  can screenshot a window; whether the counter is *good* is the owner's call,
  and `UI_AFTER.md` is where that conversation lives.
