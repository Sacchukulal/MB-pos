# Print audit — the real paper, 2026-08-23

> **FIXED.** Every fault below was repaired the same day, by P32 —
> `docs/PRINT_FIX_PROMPT.md` is the plan and its last section is what the build
> actually produced. This file is kept as written: it is the evidence, and the
> numbers in it are what the tests now hold down.

Written after the owner's photograph of a real KOT and a real bill from a real
install (`IMG_20260823_073224.jpg`): 80 mm roll, SADGURU logo, "laptop test",
bill 0008, one MASALA DOSE, TOKEN 3.

Every number below was **measured**, not guessed. The measuring was done by
building the same document the owner printed, laying it out with
`mb_print::layout`, and rasterising it with the real faces off
`C:\Windows\Fonts`. Every claim carries the file and the line.

---

## 0. The short answer

Nine things are wrong, and they are not nine separate bugs. They are **four
root causes** with nine faces:

| # | Root cause | What the owner sees |
|---|---|---|
| R1 | `Font::cell` picks the text size from the **width** of a character, never from the height that was asked for | Text prints about **half** the size the setting says. The preview shows the full size. |
| R2 | A separator is drawn as **repeated text characters**, not as a line | The rules print as widely spaced dots. The preview draws them solid. |
| R3 | The layout spends a **full row of paper** for every line whatever is drawn in it | A one-item bill is **117 mm** of paper and only **1.75 %** of it is ink |
| R4 | A `Document` is a **flat vertical list of blocks** | Nothing can ever sit beside anything else — so a logo can only be above the shop name, never left or right of it |

And on top of those, five values that reach the paper are simply the **wrong
value or missing**: the table id instead of the table name, no time on the
bill, no token on the kitchen ticket, no time on the kitchen ticket, no bill
number on the kitchen ticket.

---

## 1. The size setting is a lie — R1

### What was measured

`raster.rs:404 draw_text` and `raster.rs:335 draw_line` both work out the cell
like this:

```
cell_w = per_column * height / base_height      // 12 dots for a 24-dot line
cell   = font.cell(cell_w, height)
```

`font.rs:217 Font::cell(width, height)` then grows the point size **only until
the letter `M` fits inside `width`**. It never uses `height` except as a
ceiling that is never reached. On 80 mm paper `width` is 12 dots, and 12 dots
is a very small `M`.

Measured, on 80 mm paper, asking for the ordinary body size (24 dots, which the
settings screen calls **size 3**):

| Face | `M` actually drawn | Row of paper spent | Wasted |
|---|---|---|---|
| Magic Bill's own (IBM Plex Mono) | **13 dots** | 27 dots | 52 % |
| Times New Roman | **9 dots** | 27 dots | 67 % |
| Consolas | 14 dots | 27 dots | 48 % |
| Courier New | 11 dots | 27 dots | 59 % |

At the big size (48 dots — the shop name, TOKEN, TOTAL) it is the same
proportion: Times draws an 18-dot `M` in a 54-dot row.

**So the shop asks for 24 and gets 9 to 14.** That is the owner's sentence
*"the printed real page is completely different then the setting i set"*, and
it is still true after P31.

### And the preview does not do this

`ui/src/preview/receipt.css`:

```css
.mb-receipt__text { font-size: calc(var(--receipt-char) * var(--receipt-px, 24) / 24); }
```

The screen scales the **whole** requested height. The paper scales to whatever
`M` will fit in 12 dots. **The preview and the paper have two different size
models.** They were never going to agree, and no test compares them, because
the anti-drift test (`tests/antidrift.rs:38`) compares the text sink and the
PDF sink — which are both strings — and the raster sink only reports how many
dots it fired, never how big anything was.

### Five sizes on the dropdown do nothing

Measured on the real bill item table (name + qty + rate + amount on 80 mm):

| Setting shown | Dots asked | Dots actually used |
|---|---|---|
| size 1 | 16 | 16 |
| size 2 | 20 | 20 |
| size 3 | 24 | 24 |
| size 4 | 28 | 28 — **and the dish name wraps onto two lines** |
| size 5 | 32 | 32 — **wraps** |
| size 6 | 36 | **26** |
| size 7 | 40 | **26** |
| size 8 | 48 | **26** |
| size 9 | 60 | **26** |
| size 10 | 72 | **26** |

Sizes 6, 7, 8, 9 and 10 print **identically**. `layout.rs cap_to` computes the
cap as `room / comfortable`, and `comfortable` is the longest name in **this
bill's** table (`layout.rs columns_need`). So the same setting also prints **a
different size on a different bill** — a bill with "Idli" prints bigger than a
bill with "Paneer Butter Masala". Typography that changes bill to bill is not
typography.

### And the two engines disagree about sizes 1, 2, 4 and 5

`layout_for(doc, Grid::Cells)` snaps every size to a whole 24-dot cell
(`doc.rs snapped_to_cells`). Measured:

| Setting | Graphics engine | Text engine |
|---|---|---|
| 1 (16) | 16 px, 72 chars across | **24 px**, 48 chars across |
| 2 (20) | 20 px, 57 chars | **24 px**, 48 chars |
| 4 (28) | 28 px, name wraps | **24 px**, name does not wrap |
| 5 (32) | 32 px, name wraps | **24 px**, does not wrap |

`ipc.rs:720 preview_test_page` and `settings/sample.rs:199 bill_preview` both
call plain `layout()`, which is **always** `Grid::Dots`. A shop whose printer
is set to the Text engine is shown the graphics engine's bill. The preview does
not know which engine the printer uses.

---

## 2. The separators print as spaced dots — R2

`raster.rs:513 rule` draws the rule by writing the pattern character 48 times
through `draw_text` — one character per 12-dot cell.

Measured ink of one character inside its 12-dot cell, at the ordinary size:

| Face | `-` (Dashed, the default) | `.` | `_` | `=` |
|---|---|---|---|---|
| Magic Bill's own | **5 dots, 7-dot gap** | 3 dots, 9-dot gap | 9 dots, 3-dot gap | 9 dots, 3-dot gap |
| Times New Roman | **3 dots, 9-dot gap** | 1 dot, 11-dot gap | 6, 6 | 7, 5 |
| Consolas | 6, 6 | 4, 8 | **12, 0 — solid** | 10, 2 |
| Courier New | 8, 4 | 3, 9 | 11, 1 | 9, 3 |

That is exactly the row of spaced ticks in the photograph, on **both** the KOT
and the bill.

The preview draws the same rule as `line.glyph.repeat(line.width)` inside a CSS
monospace font (`Receipt.tsx`, the `rule` arm) — where a hyphen fills most of
its advance. **So the screen shows a near-solid rule and the paper prints
dots.** The comment in `receipt.css` even claims the two agree.

A separator on a bill is a **rule**, not text. It should be drawn as dots
across the paper, not as a character repeated.

---

## 3. The paper is eating the bill — R3

The owner's exact bill, rebuilt and measured (80 mm, default settings, no
logo):

```
32 laid-out lines
936 dots of paper = 117.0 mm

ink on the paper:
  Magic Bill's own    18 139 of 539 136 dots =  3.36 % black
  Times New Roman      9 461 of 539 136 dots =  1.75 % black
  Consolas            27 010 of 539 136 dots =  5.01 % black
```

Add the logo (~30 mm in the photo), the 3 feed lines before the cut
(`escpos.rs feed_lines: 3` ≈ 11 mm) and the cut itself, and the owner's
one-dosa bill is about **160 mm — six and a quarter inches of roll**.

### Where the 117 mm goes

| Block | Lines | Dots | mm | Share |
|---|---|---|---|---|
| Shop header (name, address, phone) | 3 | 108 | 13.5 | 12 % |
| **Separator rules** | **9** | **243** | **30.4** | **26 %** |
| Bill No / Date / Type / Table / Cashier | 5 | 135 | 16.9 | 14 % |
| TOKEN | 1 | 54 | 6.8 | 6 % |
| Item header + one item | 2 | 54 | 6.8 | 6 % |
| Subtotal / CGST / SGST | 3 | 81 | 10.1 | 9 % |
| TOTAL | 1 | 54 | 6.8 | 6 % |
| Tax summary (3 lines) | 3 | 81 | 10.1 | 9 % |
| Card | 1 | 27 | 3.4 | 3 % |
| Footer | 1 | 27 | 3.4 | 3 % |
| Blanks | 3 | 72 | 9.0 | 8 % |

**A quarter of the bill is horizontal rules**, and those rules print as dots.
The default `Separators` has **all seven turned on** (`settings.rs`), and the
bill draws two more (below the tax summary, below the payments) that are not
even settings — `bill.rs tax_summary` and `bill.rs payments` call
`doc.separator` with no toggle in front of them.

### Two more paper leaks

* `raster.rs:299 leading` adds `height / 8` under **every** line — 3 dots at
  the ordinary size, 6 dots at the big one. Over 32 lines that is 108 dots =
  13.5 mm, spent on air under text that is already only half the height of its
  row.
* `raster.rs:560 blank` adds a full 24-dot row with **no** leading, while a
  text line adds 27. So a blank line is a different height from a line of text,
  which no one intended and which makes "row height: relaxed" behave oddly.

---

## 4. The logo — R4

### What exists now

* `settings.rs LogoPosition` has exactly two values: `None` and `Top`.
* `bill.rs header` pushes the logo as `Block::Image` with
  `align: Align::Centre`, **hard-coded**. The `align` field on `Block::Image`
  is therefore dead — nothing can ever set it to left or right.
* `raster.rs:470 draw_image` positions from the **full paper width** and
  ignores `line.indent`, so the print offset (scope 7.11) moves all the text
  and leaves the logo where it was.
* The preview never shows the logo. `preview.rs` maps `Image` to
  `PreviewLine::Logo`, and `Receipt.tsx` draws the literal text `[ logo ]`.
  **A shop can never see how big its logo will be until it prints one.**

### Why left/right is not a small change

A `Document` is `Vec<Block>` (`doc.rs`), and `layout` walks it top to bottom
emitting one `LaidLine` per line. **There is no way to say "this and that are
side by side."** Putting the logo on the left with the shop name and address
taking the other 70 % needs a new block — a row that holds a picture in one
cell and a run of text lines in the other, with the raster sink drawing the
picture and the text into the same band of rows.

That is the single biggest piece of work in this list, and it is the one the
owner asked for by name.

### Two logo quality faults

1. `ui/src/settings/Logo.tsx` thresholds the picture to 1 bit at the **source**
   resolution, then `image.rs:167 scaled_to` shrinks that 1-bit picture with
   **nearest neighbour** at print time. Shrinking a 1-bit image by picking one
   dot in four throws away thin strokes — that is the ragged edge on SADGURU in
   the photo. The threshold should happen **after** resizing to the exact print
   width, or the resize should average and re-threshold.
2. `settings/sample.rs:171 SAMPLE_LOGO` is `&[8, 8, 0xFF, ...]`. It does **not**
   start with `MB1`, so `image.rs decode` refuses it and `raster.rs draw_image`
   skips it with a note. The comment calls it "three tiny black squares"; it is
   a picture that has never once been drawn. A shop with no logo therefore sees
   `[ logo ]` in the preview standing in for a logo that would not have printed
   anyway.

---

## 5. Wrong or missing values on the paper

### 5.1 The table id is printed instead of the table name — on both papers

The photo shows `Table  tbl_outlet_default_sec_mt48cjqf_h1_` on the bill and
the same string on the KOT. Rebuilt and confirmed, character for character.

* `bill.rs:206` prints `table.as_str()` — a `TableId`.
* `flows.rs:264` and `flows.rs:351` hand `state.table` (also the raw id) to the
  kitchen ticket.
* The shop's own name for the table **is** available: `flows.rs:931
  table_label` looks it up, and `kitchen.rs:333` uses it — but only for a toast
  and for the kitchen screen. Neither piece of paper gets it.
* The settings preview passes `table: Some("6")` (`sample.rs`), so **the
  preview cannot show this bug**. That is why it survived to a real install.

It also eats 36 of the 48 columns, and on 58 mm paper it would wrap onto two
lines.

### 5.2 The bill has no time of day

`bill.rs:194` prints `core.business_day` only — `2026-08-23`. A restaurant bill
without a time is not much of a record, and every v1 bill had one. There is a
timestamp on the order; nothing reads it for the paper.

### 5.3 The kitchen ticket has no token

`flows.rs:371` (the **Send to kitchen** button) and `corrections.rs:552` (the
cancellation slip) both pass `token: None`. `flows.rs:270` — the background
timer that catches an order the kitchen screen missed — passes the **real**
token.

So the same shop gets tickets with a token and tickets without one, depending
on which path printed it, and the ordinary button is the one that prints
without. `show_token` defaults to `true` and can never do anything on the
common path. The cook cannot match the food to the bill.

### 5.4 The kitchen ticket has no time and no bill number

`flows.rs:151` and `flows.rs:154` hard-code `bill_number: None` and
`time: None` in the **one** function every kitchen ticket goes through. Both
`show_time` and `show_bill_number` default to `true`.

And the settings preview passes `time: Some("19:40")` and
`bill_number: Some("SAMPLE")` (`sample.rs`) — so the owner sees a ticket on
screen with two lines the paper will never have.

### 5.5 The item Amount column does not agree with the Subtotal

The photo reads:

```
MASALA DOSE      1   100.00   105.00
Subtotal                      100.00
CGST                            2.50
SGST                            2.50
TOTAL                         105.00
```

`bill.rs wide_items` prints `line.gross_including_tax` in the Amount column —
the figure **with** tax in it. `Subtotal` is `bill.subtotal`, which is the sum
of line **gross before tax**. So the item column shows tax, the subtotal does
not, and then the tax is added again underneath.

Rate × Qty = 100.00 but the line says 105.00. A customer reading top to bottom
sees 105 → 100 → +5 → 105. The grand total is correct; the **presentation** is
not, and it is the part a customer and an auditor read.

Every Indian restaurant bill prints Amount = Rate × Qty, and the column sums to
the subtotal.

---

## 6. Everywhere else the preview and the paper disagree

| # | The preview shows | The paper prints |
|---|---|---|
| 1 | text at the full requested height | text at 33–52 % of it (§1) |
| 2 | a near-solid rule | spaced dots (§2) |
| 3 | the graphics-engine layout, always | the text engine's layout when the printer is set to Text (§1) |
| 4 | `[ logo ]` | a picture of some size nobody has seen |
| 5 | the UPI payload as text | a real QR square drawn by the printer |
| 6 | the bill number as text | real CODE 128 bars |
| 7 | table `6` | `tbl_outlet_default_sec_mt48cjqf_h1_` |
| 8 | a KOT with `Time 19:40` and `Bill SAMPLE` | a KOT with neither |
| 9 | a bill for a sample order with 3 items, a discount, a charge and a split payment | the shop's actual bill |
| 10 | nothing about the feed and the cut | 3 fed lines and a cut |

Number 9 is **audit D6, never fixed**. `ipc.rs:707` still says *"P09 will add
`preview_order(order_id)` beside this"*. It was never added — there is no
command anywhere in the product that previews the **real** bill for the **real**
order before it prints. A grep for `preview_order` finds only that comment.

---

## 7. Why none of the tests caught any of this

* `tests/antidrift.rs:38` compares the **text sink** and the **PDF sink**. Both
  produce strings. The raster sink — the one that actually prints — is not in
  it, because it produces dots and there is nothing to compare.
* `raster.rs` records `LineInk { dots }` per line, and the only assertion is
  `dots > 0`. Nothing checks **how big**, **how wide**, or **where**.
* `ui/tests/preview.test.tsx` asserts the preview carries the same *text* the
  layout produced. It cannot assert size, because the size model lives in CSS.
* `t11_every_setting_changes_the_output` (`antidrift.rs:255`) toggles settings
  through the **text** sink. Item sizes 6–10 all snap to the same multiplier on
  that sink, so the five dead sizes look alive to it.
* Every layout test uses the **default** size. The one-letter-per-line bug of
  P31 and the five dead sizes here are the same blind spot.

**The missing test is: rasterise the bill and measure the ink.** Cap height,
rule coverage, column edges, total paper length. Until that test exists, the
paper and the screen will drift again.

---

## 8. What else is missing around printing

Facts, each checked in the code.

| # | Missing | Evidence |
|---|---|---|
| 8.1 | **Preview of the real bill before printing** (audit D6) | no `preview_order` command exists |
| 8.2 | **Preview of the real kitchen ticket** | same |
| 8.3 | **A4 / PDF tax invoice for B2B** (scope 7.10) | `mb_print::pdf::to_pdf` is called from `reports.rs:1729` only. No printer can be set to A4. |
| 8.4 | **Time on the bill** | §5.2 |
| 8.5 | **Token / time / bill number on the KOT** | §5.3, §5.4 |
| 8.6 | **A KOT number of its own** | the kitchen ticket model has no running number; a cook cannot say "KOT 14" |
| 8.7 | **Waiter / steward name on the KOT and the bill** | only `Cashier` exists |
| 8.8 | **Number of covers (persons) on a dine-in bill** | not on the model |
| 8.9 | **"Tax Invoice" / "Bill of Supply" title** | GST wants the document titled; the bill has no title line |
| 8.10 | **Place of supply / state code on the bill** | `Store.state_code` is carried and never printed |
| 8.11 | **Amount in words** | not present |
| 8.12 | **Reprint mark on a kitchen ticket** | the bill has `Copy::Duplicate`; the KOT has nothing |
| 8.13 | **The print offset does not move the logo or the QR** | `raster.rs draw_image` and the `qr` arm ignore `indent` |
| 8.14 | **Logo left/right beside the shop name** | §4 — needs a new block type |
| 8.15 | **A separator that is a real line** | §2 |
| 8.16 | **Per-section spacing control** | `RowHeight` gives 0 or 1 blank lines and nothing else; there is no way to say "less air above the total" |
| 8.17 | **A way to see how long the bill will be** | the preview shows lines, never millimetres of roll |

---

## 9. What a good 80 mm bill should look like

For the fixing prompt to aim at. 80 mm = 72 mm printable = 576 dots.

```
   [ logo 30% ]  SADGURU BAKERY          <- one band: picture left, text right
                 12 MG Road, Jayanagar
                 Ph 98800 12345
                 GSTIN 29ABCDE1234F1ZW
  -------------------------------------  <- a real 1-dot rule, edge to edge
  TAX INVOICE                    TOKEN 3
  Bill 0008   23-08-2026 19:42   Dine In
  Table 6                   Cashier Ravi
  -------------------------------------
  Item                Qty   Rate  Amount
  -------------------------------------
  Masala Dose           1 100.00  100.00
  -------------------------------------
                  Subtotal        100.00
                  CGST 2.5%         2.50
                  SGST 2.5%         2.50
  =====================================
  TOTAL                           105.00
  =====================================
  Card                            105.00
        Thank you, visit again
```

Roughly **65 mm** instead of 117 mm, with text at the size that was asked for,
rules that are rules, and the meta block on two lines instead of five.

---

## 10. The questions that must be answered before any of this is built

Listed in the reply to the owner. No fixing prompt should be written until they
are answered, because four of them change the shape of the code.
