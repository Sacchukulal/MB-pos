# What the app looked like before P27.5

**The before.** Written on 2026-08-15, from the running app with a real shop
in it, before a line was changed. Screenshots in `docs/ui/before-*.png`.

The owner's words are the brief:

> *"my main concern is UI should be very very important … it should be very
> good and modern styling flow, and must look very neatly alighnes UI UX, now
> it looks here and there"*

and, eleven days earlier and unanswered, UI_GUIDELINES §5 — *"icons, alignment,
spacing — where the real app must beat the mockup"*.

---

## How this was looked at

Not from memory, and not from the code. `src-tauri/src/look_demo.rs` was
written first: an `#[ignore]`d seeder that fills a plausible Bengaluru
vegetarian restaurant at half past eight on a Saturday — 43 dishes in seven
categories, 46 tables in four sections, 14 of them busy at ages from 6 to 63
minutes, 30 settled bills in every payment mode, six credit customers with one
over their limit, and the evening's expenses.

**The first attempt to look at this without it found "Nothing here" on the
floor and an onboarding checklist on billing.** An empty screen cannot be
designed, and a design judged against `₹100.00` twelve times learns nothing
about a column of rupees — it is the ragged ones that show whether the numbers
line up.

---

## The five findings, measured in the tree

| # | What | Where |
|---|---|---|
| 1 | **There were no icons.** The left rail drew its whole navigation with Unicode glyphs — `▦` Floor, `☰` Credit, `⌁` Spends, `⬒` Stock, `⇩` Buying — so a shop got whatever font Windows substituted, at three different weights and vertical positions. No `Icon` component existed; `svg` appeared in 2 files out of 18,500 lines. | `shell/Shell.tsx` |
| 2 | **Spacing was legal but never decided.** 17 feature CSS files each chose their own steps off `--space-*`: account 1–5, billing 1,2,3,4,6,8, corrections 1,2,3, credit 2,3,4. All within the scale, so the lint was silent. Nobody had decided what a step MEANT. | every `*.css` |
| 3 | **14 screens each set their own page margin**, and they disagreed — `--space-3`, `--space-4`, `--space-5`. The left edge of the app moved as you walked through it. | every screen root |
| 4 | **Every screen invented its own page.** No `Page`, no `PageHeader`, no `Toolbar`, no `Panel` in the kit, so Stock's, Buying's, Menu's, Reports' and Settings' headers were five separate pieces of CSS meant to be the same thing. | `kit/` |
| 5 | **Elevation was arbitrary.** 7 of 17 CSS files used a shadow, on no principle. Nothing decided what is raised, so the screen had no depth and no reading order. | `kit/kit.css` |

---

## What running it found on top of those

| # | Screen | What was wrong | Fixed |
|---|---|---|---|
| 6 | Title bar | The **full database path** was printed across it — `C:\Users\SACCHU\AppData\Local\Temp\…\magicbill.db`, wrapped onto two lines. Developer output on a shopkeeper's screen. | ✅ the wordmark; the path is the tooltip and is on Health |
| 7 | Billing | **The set-up checklist owned a quarter of the screen** a cashier looks at all day, for the life of the shop — and it is biggest when it matters least, because the steps that survive longest are the optional ones. | ✅ a one-line strip that opens on press |
| 8 | Billing | **The set-up checklist was always wrong.** `setup::look` read `menu_items` and `is_active` — a table and a column that have never existed. The read errored, `unwrap_or_default()` swallowed it, and every shop was told it had done none of its set-up. Silent for nineteen sessions (R3). | ✅ fixed, with the test that would have caught it |
| 9 | Floor | **Two choices rendered as one choice.** Which room (section chips) and which tables (view tabs) were both `Button`s with the selected one filled solid accent — two identical highlighted pills, four inches apart, meaning different things. | ✅ a segmented control and a tab set |
| 10 | Floor | The occupancy line was a **full-width grey slab** with four phrases run together, giving a quiet fact the weight of a warning. | ✅ facts separated by rules, not a box |
| 11 | Reports | **Prose set in a monospace face.** The dashboard passed its explanatory sentence inside `.mb-stat__value`, which carries `--font-mono` so rupee columns align. So "What the till expects, before counting." rendered like a terminal. | ✅ `note` is its own parameter |
| 12 | Credit | **No page title at all**, and one row holding a search box, two view buttons and "Add a customer" — the thing that changes the SHOP and the thing that changes what you are LOOKING AT, same shape, same colour, eight pixels apart. | ✅ header, action, tabs |
| 13 | Credit, Spends, Bills | A trailing **actions column got an equal share of the table width**, leaving a wide empty channel between the last figure and the first button. | ✅ actions shrink to content |
| 14 | Everywhere | **The Windows default scrollbars.** Wide grey troughs with square thumbs and stepper arrows, staying light grey on the dark theme — three visible at once on a busy bill, all belonging to a different application. | ✅ themed, thin |
| 15 | Everywhere | Glyph arrows and marks used as icons: `▲ ▼ ✓ ● ✕ 🔒 🖨 ☾ ☀ ⌧`. | ✅ all drawn |
| 16 | Billing | Section names (`A/C ROOM`) were **uppercase and letterspaced at `--text-sm`**, shouting louder than the table numbers under them. | ✅ a quiet label with a hairline |
| 17 | Billing | The order-type switch put a **heavy solid-accent slab in the top-left corner** of the busiest screen, competing with the search box beside it all day. | ✅ a sunk track, the chosen one raised |
| 18 | Billing | The **tax breakdown was squeezed to about fifty pixels** and scrolled away — the block UI_GUIDELINES §4 calls "a feature, not a footer". | ✅ its floor raised to 9rem |
| 19 | Kit | **Two components claimed the class `.mb-topbar`** — the shell's title bar and billing's own order-type row. Nothing owned layout, so nothing noticed. | ✅ renamed `.mb-billbar` |
| 20 | Shell | The **rail scrolled** — 13 items did not fit at 768px, so "Billing" was clipped at the top and "Account" at the bottom. | ✅ gone; six across the top plus More |

---

## What is NOT in this list

Colour. The palette was refined (a neutral grey ramp, a deeper accent, a dark
theme built as its own thing rather than light inverted) — but **not one item
above is fixed by a colour**, and the owner's complaint was never about one.
See D144.
