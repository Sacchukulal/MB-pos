# P32 — PRINTING. What actually comes out of the printer.

> **BUILT — 2026-08-23.** This file is the prompt it was built from; the
> results are at the end, under *What was built*. Where the build learned
> something the prompt did not know, the difference is recorded there rather
> than edited into the plan above it.

**Owner round 10, 2026-08-23.** Written against the owner's photograph of a
real KOT and a real bill (`IMG_20260823_073224.jpg`), and against the measured
audit in `docs/PRINT_AUDIT_2026-08-23.md`. Read that file first. Every number
quoted below was measured, not estimated.

---

## How this prompt is to be worked

* **Build the whole thing, back to back, and report once.** Do not split it
  into an easy half now and a lengthy half later. If one part turns out to be
  genuinely blocked, finish every other part in full and say exactly what was
  left and why.
* **Nothing here is done until it has been seen on paper.** `scripts/drive.mjs`
  drives the real Tauri window over CDP; the print path is verified by watching
  the Windows spooler (`Get-PrintJob -PrinterName …`) while the app prints.
  "Queued" is not "printed".
* **Every fix below needs a test that would fail with the bug restored.** The
  whole reason this round exists is that the existing tests all passed while
  the paper was wrong. See §8 — it is not optional and it is not last.
* Plain English on screen. No paragraphs explaining things to the owner; hover
  tips only.

---

## The owner's rulings, 2026-08-23

These are decided. Do not re-open them.

| Ruling | Decision |
|---|---|
| **Logo** | A logo may sit **top, left, right or none**. On left/right the logo box is **30 % of the paper width** and the shop's name/address/phone/GSTIN take the other **70 %, centred inside their 70 %**. The logo is scaled up to fill its full 30 % width and is vertically centred against the text block. |
| **Page design** | **Full redesign, new defaults.** Target: the owner's one-item bill drops from 117 mm to about 65 mm. There are no shops in the field to protect. |
| **Item amount column** | **Amount = Rate × Qty, before tax.** The column adds up to the Subtotal. CGST/SGST are added below it. |
| **Text size** | The number a shop picks means **the height of a capital letter, in dots**. The letter is drawn at its **natural proportions — never squeezed, never stretched**. When a name does not fit the line at that size, **the name wraps to a second line**; the font size is never changed by the software to make something fit. |

The owner's exact words on the last one:

> *"of item name, it may be in one line or 2 line, i dont want to damage the
> legth width ratio, it should look perfectly in bill according the size, u can
> take 2 lines if item name is tooo long, then only, otherwise in the same
> line… so dont damage the design, font, styles etc."*

---

## Part 1 — The size model. `mb-print`

### 1.1 The fault

`font.rs:217 Font::cell(width, height)` grows the point size only until the
letter `M` fits inside `width`. On 80 mm paper `width` is 12 dots. `height` is
never used. Measured result at the ordinary body size (a request for 24 dots):

| Face | `M` drawn | Row of paper spent |
|---|---|---|
| Magic Bill's own (IBM Plex Mono) | 13 dots | 27 dots |
| Times New Roman | 9 dots | 27 dots |
| Consolas | 14 dots | 27 dots |
| Courier New | 11 dots | 27 dots |

The row is always `size + size/8` (`raster.rs:299 leading`) whatever is drawn
in it. So more than half of every row is white space by construction.

`layout.rs chars_across` computes characters-per-line as
`usable × 24 ÷ height`, which is a **guess**, not a measurement. The preview
scales the full requested height in CSS (`receipt.css .mb-receipt__text`). The
two have never agreed and nothing compares them.

### 1.2 What to build

Introduce **`mb_print::metrics::Metrics`** — the one place that turns "the shop
asked for size N in face F" into dots.

```rust
/// What one size, in one face, is worth in dots on this paper.
/// Computed once per job, handed to the layout and to every sink,
/// so the paper and the screen cannot answer differently.
pub struct Metrics { /* face + paper */ }

impl Metrics {
    /// The point size at which a capital letter is exactly `cap_dots` tall.
    /// Searched against the real glyph, not against the face's declared
    /// metrics: what matters is what rasterises.
    pub fn px_for_cap(&self, cap_dots: u16) -> f32;

    /// The width of one character at that size, in whole dots.
    /// Monospace: the single advance. Proportional: the widest digit,
    /// because a bill is a column of figures before it is anything else.
    pub fn advance(&self, cap_dots: u16) -> u32;

    /// Ascender to descender, in dots — the real height of the line's ink.
    pub fn ink_height(&self, cap_dots: u16) -> u32;

    /// The whole row: ink height plus leading. THIS is what the paper spends.
    pub fn row_height(&self, cap_dots: u16) -> u32;

    /// How many characters of this size fit across `usable_dots`.
    /// MEASURED. Never `usable * 24 / height`.
    pub fn chars_across(&self, usable_dots: u32, cap_dots: u16) -> usize;
}
```

Rules:

* **Never distort a glyph.** No horizontal squeeze, no vertical stretch. The
  owner ruled on this.
* `row_height` = `ink_height + leading`, where leading is **12 % of the cap
  height, minimum 2 dots**. Not `size / 8` of a nominal size that is not what
  is drawn.
* A **blank line** is one `row_height` at the body size — the same height as a
  line of text. Today `raster.rs:560 blank` adds a bare 24 rows while a text
  line adds 27, and nothing intended that.

### 1.3 Thread `Metrics` through the layout

`layout_for(doc, grid)` becomes `layout_for(doc, grid, &metrics)`.

This is the whole point: **the layout stops guessing character widths and is
told them.** D29 survives — there is still exactly one layout and every sink
still renders what it produced. The layout simply now knows how wide a
character is, which is the fact it was inventing before.

* `chars_across` → `metrics.chars_across(...)`.
* `LaidLine` gains `row_dots: u32` — the height this line will occupy. Every
  sink reads it instead of recomputing. The preview reads it too.
* `Laid` gains `total_dots: u32` and a `total_mm()` helper. The preview shows
  it (see §7.6).

### 1.4 Remap the size ladder

`settings/catalog.rs SIZES` currently stores nominal dots `16, 20, 24, 28, 32,
36, 40, 48, 60, 72` behind labels `1..10`. They become **cap heights in dots**:

| Label | Cap dots | Chars across 80 mm (IBM Plex Mono) |
|---|---|---|
| 1 | 10 | 63 |
| 2 | 12 | 52 |
| 3 | **14** | **45** ← default body |
| 4 | 16 | 39 |
| 5 | 18 | 35 |
| 6 | 21 | 30 |
| 7 | 24 | 26 |
| 8 | 28 | 22 |
| 9 | 34 | 18 |
| 10 | 40 | 15 |

Fill in the right-hand column from the real measurement at build time — do not
hard-code the table above; it is an illustration. The **settings screen must
show the characters-per-line for the chosen size and face**, because that is
the trade the owner is making and it should not be a surprise on paper.

Size 3 is chosen as the default body size because it keeps roughly today's
density (45 characters vs today's 48) while making the letter **14 dots tall
instead of 13 — but honestly, and with the row shrinking from 27 dots to ~18**.
That row saving is where a third of the paper comes back.

`Style::size_from_wire` must migrate the old stored values: anything that is
one of the old nominal numbers maps to the nearest new cap height, so a shop
that had tuned its receipt does not have to tune it again.

### 1.5 Delete the automatic font shrinking

`layout.rs cap_scale` / `cap_to` currently reduce the table's font until the
content fits. Measured consequence: **sizes 6, 7, 8, 9 and 10 all print at 26
dots — five dropdown choices that do nothing** — and the cap is computed from
the longest name on *that* bill, so the same setting prints a different size on
a different bill.

The owner's ruling replaces it:

1. The font size the shop chose is **used**, always.
2. If the item name does not fit its column, **the name wraps** onto a second
   line, indented by two characters, with Qty / Rate / Amount staying on the
   first line and in their usual places. `lay_row` already wraps cells; what
   must go is the size reduction in front of it.
3. Auto-reduction survives **only** as a last resort: when the fixed columns
   plus one single character of name do not fit at all. Then, and only then,
   reduce, and raise `Note::ScaleCapped` so the preview says so in words.
4. Crown jewel 18 ("font sizes are capped automatically so text can never
   overflow the paper") is honoured by 3, not by 1 and 2. Update its wording in
   `layout.rs`'s own module docs so the next reader is not misled.

`MIN_FILL` and `columns_need`'s "comfortable" logic are deleted with it.

### 1.6 The optional extra: a condensed built-in face

Not required, and worth doing if the time is there. A thermal printer's own
Font A is 12 dots wide with a **17-dot** cap — it is a condensed design. IBM
Plex Mono is not condensed, which is why 48 columns can only ever give a 13-dot
cap without distortion. Shipping a condensed monospace as the built-in face
would give more letter height per column **with no distortion at all**, which
is the only honest way to get both. If it is done, it is a new file in
`crates/mb-print/assets/` and one line in `font.rs BUILTIN`; the size ladder in
§1.4 is then recomputed from it. **Do not squeeze IBM Plex Mono to fake it.**

---

## Part 2 — A separator is a line, not a row of characters

### 2.1 The fault

`raster.rs:513 rule` writes the pattern character across the paper through
`draw_text`. Measured ink of one character inside its 12-dot cell:

| Face | `-` (the default) | `.` | `_` | `=` |
|---|---|---|---|---|
| Magic Bill's own | 5 dots, **7-dot gap** | 3 dots, 9-dot gap | 9, 3 | 9, 3 |
| Times New Roman | 3 dots, **9-dot gap** | 1 dot, 11-dot gap | 6, 6 | 7, 5 |
| Consolas | 6, 6 | 4, 8 | 12, **0 — solid** | 10, 2 |
| Courier New | 8, 4 | 3, 9 | 11, 1 | 9, 3 |

That is the row of spaced ticks on both papers in the photograph. The preview
draws `glyph.repeat(width)` in a CSS monospace font, which comes out nearly
solid — so **the screen has always shown a line the printer has never
printed**.

### 2.2 What to build

`RasterSink::rule` draws **dots directly onto the canvas**. No font, no glyph,
no advance. The five `Pattern` values become five drawn rules:

| Pattern | Drawn as |
|---|---|
| `Solid` | one continuous row of dots, edge to edge, **1 dot** thick |
| `Dashed` | 6 dots ink, 4 dots gap, repeating, 1 dot thick, **starting and ending on ink** so both ends touch the edges |
| `Dotted` | 2 dots ink, 4 dots gap, 1 dot thick |
| `Bold` | one continuous row, **3 dots** thick |
| `Double` | two continuous 1-dot rows with a 3-dot gap between them |

The rule spans `usable` columns starting at `indent` — so **it honours the
print offset**, which the character version did by accident and the drawn one
must do on purpose.

The row a rule occupies is its own thickness plus 4 dots of air above and below
— not a whole text row. Today each rule costs 27 dots; solid becomes 9.

### 2.3 The other two sinks

* `escpos::encode_text` (the printer's own font path) keeps writing characters
  — it has no other option. That is correct and must be commented as such.
* `pdf.rs` draws a real line, matching the raster.
* The preview draws a CSS border, not repeated characters — see §7.2.

`PreviewLine::Rule` stops carrying `glyph` and starts carrying
`{ pattern, width_cols, thickness_dots }`.

---

## Part 3 — The page design. Target 65 mm.

### 3.1 Where the 117 mm goes today

| Block | Lines | Dots | mm | Share |
|---|---|---|---|---|
| Shop header | 3 | 108 | 13.5 | 12 % |
| **Separator rules** | **9** | **243** | **30.4** | **26 %** |
| Bill No / Date / Type / Table / Cashier | 5 | 135 | 16.9 | 14 % |
| TOKEN | 1 | 54 | 6.8 | 6 % |
| Item header + one item | 2 | 54 | 6.8 | 6 % |
| Subtotal / CGST / SGST | 3 | 81 | 10.1 | 9 % |
| TOTAL | 1 | 54 | 6.8 | 6 % |
| Tax summary | 3 | 81 | 10.1 | 9 % |
| Card | 1 | 27 | 3.4 | 3 % |
| Footer | 1 | 27 | 3.4 | 3 % |
| Blanks | 3 | 72 | 9.0 | 8 % |

### 3.2 The new bill, 80 mm

```
   [ logo 30% ]     SADGURU BAKERY            <- one band, §4
                  12 MG Road, Jayanagar
                     Ph 98800 12345
                  GSTIN 29ABCDE1234F1ZW
  ───────────────────────────────────────     <- a real 1-dot rule
  TAX INVOICE                       TOKEN 3
  Bill 0008    23-08-2026 19:42      Dine In
  Table 6                       Cashier Ravi
  ───────────────────────────────────────
  Item                  Qty    Rate   Amount
  ───────────────────────────────────────
  Masala Dose             1  100.00   100.00
  ───────────────────────────────────────
                     Subtotal         100.00
                     CGST 2.5%          2.50
                     SGST 2.5%          2.50
  ═══════════════════════════════════════
  TOTAL                              105.00
  ═══════════════════════════════════════
  Card                               105.00

           Thank you, visit again
```

### 3.3 The specific changes in `template/bill.rs`

1. **The meta block goes from five rows to two.** `Bill No`, `Date`, `Type`,
   `Table`, `Cashier` are five full-width `Row`s today. They become two
   `Columns` rows, three cells each. Saves 3 rows.
2. **`TAX INVOICE` / `BILL OF SUPPLY` title**, on the same line as the token,
   left and right. A GST document must be titled and today none is. A
   composition dealer gets `BILL OF SUPPLY`; a shop with no GSTIN gets no title
   line at all rather than a wrong one.
3. **`TOKEN n` stops being its own 48-dot centred line with a rule under it.**
   It rides on the title line, right-aligned, at the token size. Saves a row
   and a rule.
4. **The default separator set drops from 9 to 4:** below the header, below the
   column names, below the items, and a `Double` above and below the TOTAL.
   `below_meta`, `below_token`, `below_subtotals` and `below_grand_total`
   default **off**. All seven stay as settings — nothing is removed from the
   settings screen.
5. **The two rules the template draws with no setting in front of them**
   (`tax_summary` and `payments` each end with a bare `doc.separator`) become
   two new named toggles, `below_tax_summary` and `below_payments`, both
   defaulting **off**. A rule nobody can switch off is the same fault as a
   setting nobody reads.
6. **The tax summary gets a rate label on the tax lines** (`CGST 2.5%`), and
   the separate `Tax summary` block defaults **off** on 58 mm and 80 mm paper —
   its content is already on the CGST/SGST lines for a single-rate bill. It
   stays on by default for a bill with more than one rate, and it stays a
   setting. (Audit B11 is why the block exists; it is not being removed, only
   its default changed for the case where it repeats itself.)
7. **`doc.spacer(2 + s.row_height.gap())` at the end of `footer` becomes
   `doc.spacer(1)`.** The three fed lines before the cut (`escpos.rs
   JobOptions::feed_lines`) already clear the blade; two extra blank rows on
   top of them is 6 mm of roll on every bill of the day.
8. `RowHeight::Compact` must actually be compact: `section_gap` 0 **and** no
   blank above the total.

### 3.4 The same treatment for the kitchen ticket

`template/kitchen.rs`. The ticket in the photograph is four lines of content in
about 40 mm. It should be:

```
  ═══════════════════════════════════════
  KITCHEN                          KOT 14
  ═══════════════════════════════════════
  Table 6      Dine In     19:42   Bill 8
  ───────────────────────────────────────

   1  MASALA DOSE

  ───────────────────────────────────────
```

* Title and KOT number on one line.
* Table / type / time / bill number on **one** line, not four rows.
* The food stays big — that is the whole point of a KOT and must not be traded
  away for compactness.

---

## Part 4 — The logo band. `mb-print`

### 4.1 The fault

A `Document` is `Vec<Block>` and `layout` walks it top to bottom, one line at a
time. **Nothing can sit beside anything.** `bill.rs header` pushes the logo
with `align: Align::Centre` hard-coded, which makes `Block::Image`'s `align`
field dead. `settings.rs LogoPosition` has only `None` and `Top`.

### 4.2 What to build

A new block:

```rust
/// A picture and a run of text lines, side by side, in one band.
/// The ONLY block in this crate that is two-dimensional, and it exists
/// because a letterhead is two-dimensional and nothing else on a receipt is.
Band {
    image: Vec<u8>,
    /// Which side the picture is on.
    image_side: Align,        // Left or Right
    /// The picture's share of the paper width, as a percentage.
    image_pct: u8,            // 30 by the owner's ruling
    /// The text beside it. Each line keeps its own style and alignment.
    text: Vec<(String, Style, Align)>,
}
```

`LogoPosition` gains `Left` and `Right`. `settings/catalog.rs LOGO_POSITIONS`
gains the two choices.

`bill.rs header` becomes:

* `LogoPosition::None` → the shop's text lines as they are today.
* `LogoPosition::Top` → the image centred above the text, as today.
* `LogoPosition::Left` / `Right` → **one `Band`**: image on that side at
  `logo_width_pct` (default **30**), the shop's name/address/phone/GSTIN/FSSAI
  in the other 70 %, **each line centred inside its 70 %** (the owner's
  ruling).

### 4.3 Laying a band out

`layout` emits **one `LaidLine` per band**, carrying the image bytes, the image
box and the already-wrapped text lines with their own boxes — because a band's
height is `max(image height, text block height)` and only the layout may decide
where text breaks.

* The text is wrapped to the 70 % width **at each line's own size**, using
  `Metrics` (§1.3). A shop name too long for 70 % wraps rather than shrinking —
  same rule as an item name.
* The image is scaled so it **fills its 30 % width**, and its height follows
  its own aspect ratio. Never cropped, never stretched.
* If the scaled image is taller than the text block, the text is vertically
  centred against the image. If the text block is taller, the image is
  vertically centred against the text. The owner asked for the logo to "fit
  correctly on full size" — that means fill the 30 % width, never overflow it,
  and never be letterboxed inside it.
* A band that would be taller than a third of the paper's own width is a
  mistake upstream: cap it there and raise a note.

### 4.4 The three sinks

* **raster** — `draw_band`: blit the scaled image into its box, draw the text
  runs into theirs, both starting at the band's top row, both offset by
  `line.indent`. **Today `raster.rs:470 draw_image` ignores `indent`, so the
  print offset moves all the text and leaves the logo behind. Fix that here.**
  The same fix applies to the `qr` arm.
* **escpos text engine** — cannot draw a picture. It prints the text lines
  full-width and skips the image, which is what it does today, commented.
* **preview** — draws the real logo (§7.3) in a 30 % box with the text beside
  it, at the same proportions.

### 4.5 Two logo quality faults to fix while here

1. `ui/src/settings/Logo.tsx` thresholds the picture to one bit at the
   **source** resolution; `image.rs:167 scaled_to` then shrinks that 1-bit
   picture with **nearest neighbour** at print time. Picking one dot in four
   throws away thin strokes — that is the ragged edge on SADGURU in the photo.
   **Resize the greyscale first, then threshold**, at the exact dot width the
   logo will print at. The settings screen knows the paper width and the
   percentage, so it can do this and show the true result.
2. `settings/sample.rs:171 SAMPLE_LOGO` is `&[8, 8, 0xFF, …]`. It does not
   start with `MB1`, so `image.rs decode` refuses it and the raster sink skips
   it. The comment claims "three tiny black squares"; it has never once been
   drawn. Replace it with a real encoded `MB1` picture and add a test that
   decodes it.

---

## Part 5 — Values that are wrong or missing on the paper

Each of these is a one-line fix with a test. None is optional.

### 5.1 The table id is printed instead of the table's name — both papers

The photograph reads `Table  tbl_outlet_default_sec_mt48cjqf_h1_`. It eats 36
of 48 columns and on 58 mm paper it would wrap.

* `template/bill.rs:206` prints `table.as_str()` — a `TableId`.
* `src-tauri/src/flows.rs:264` and `:351` hand the raw `state.table` id to the
  kitchen ticket.
* The shop's own label **already exists**: `flows.rs:931 table_label`. It is
  used for a toast and by `kitchen.rs:333` for the kitchen screen, and reaches
  no piece of paper.

**Fix:** resolve the label at the call sites — the print crate must not touch
the database. `BillContext` and `KitchenContext` take the label, never the id.
Falling back to the id is wrong: fall back to nothing, because a bill with no
table line is honest and a bill with a database key on it is not.

**And the preview must be able to show this class of bug.** `settings/sample.rs`
passes `table: Some("6")`, which is why five sessions of previewing never
revealed it. See §7.5.

### 5.2 The bill has no time of day

`bill.rs:194` prints `core.business_day` only. Add the time beside the date on
the same line — `23-08-2026 19:42`, 24-hour. The order carries the timestamp;
the print crate owns no clock (D5/D19), so the caller formats it and passes it
in, exactly as the kitchen ticket's `time` field already works.

### 5.3 The kitchen ticket has no token

`flows.rs:371` (the **Send to kitchen** button) and `corrections.rs:552` (the
cancellation slip) both pass `token: None`. `flows.rs:270` — the background
timer — passes the real one. So the same shop gets tickets with and without a
token depending on which code path printed, and the ordinary button is the one
that prints without.

**Fix:** the token is resolved inside `queue_kitchen_lines`, once, from the
order — not passed in by three callers who can each forget. `show_token`
defaults to true and must be able to do something.

### 5.4 The kitchen ticket has no time and no bill number

`flows.rs:151` and `:154` hard-code `bill_number: None` and `time: None` inside
`queue_kitchen_lines` — the one function every kitchen ticket goes through.
Both `show_time` and `show_bill_number` default to **true**. Meanwhile
`settings/sample.rs` passes `Some("19:40")` and `Some("SAMPLE")` to the
preview, so the owner is shown two lines the paper will never carry.

**Fix:** fill both in from the order and the clock.

### 5.5 A kitchen ticket has no number of its own

A cook cannot say *"KOT 14"* because there is no such number. Add a per-day
running KOT number, claimed the same way the token and the bill number are
claimed (`mb_core::Claimed`), stored on the order, printed at the top right of
the ticket, and shown on the kitchen screen. **Ids must not come out of the
clock** — that lesson is already recorded from round 9.

### 5.6 The item Amount column does not agree with the Subtotal

`bill.rs wide_items` prints `line.gross_including_tax`; `Subtotal` is
`bill.subtotal`, the sum of line gross **before** tax. So the photograph reads
`105.00` on the item, `100.00` as Subtotal, then adds `2.50 + 2.50` again.
Rate × Qty = 100.00 but the line says 105.00.

**Fix, per the owner's ruling:** the Amount column prints the line's value
**before tax** — `gross` less both discounts, i.e. `line.net`. The column then
adds up to the Subtotal exactly.

Careful with the inclusive-priced line: for a line priced tax-inclusive,
`net == gross_including_tax`, so its Amount would then include tax while its
neighbours' do not, and the column would still not add up. **Print the
tax-exclusive taxable value for every line** (`line.taxable` for a taxable
line, `line.net` for a non-GST or exempt one), and make `Subtotal` the sum of
exactly what was printed. Add a test that sums the printed Amount column
character-for-character out of the rendered paper and asserts it equals the
printed Subtotal, for a bill containing an exclusive line, an inclusive line,
a non-GST line and a discount.

### 5.7 Smaller ones, all confirmed present

| Add | Where |
|---|---|
| `Place of supply` on the bill | `Store.state_code` is carried and never printed |
| A reprint mark on a kitchen ticket | the bill has `Copy::Duplicate`; the KOT has nothing |
| Waiter / steward name on KOT and bill | only `Cashier` exists today |
| Number of covers on a dine-in bill | not on the model; needs a field on the order |
| Amount in words under the total | a setting, default off |

---

## Part 6 — Nothing on this list may be a setting nobody reads

Round 2's lesson, which shipped three of these: **hunt for a stored value with
no consumer.** Before this round is called done, grep every field of
`ReceiptSettings` and `KitchenSettings` for a reader outside the settings
screen and its tests. `scripts/audit-wiring.mjs` proves a *command* has a
button; extend it, or write its sibling, so it proves a *stored setting* has a
consumer. The five dead item sizes in §1.5 and the two rules with no toggle in
§3.3(5) are exactly this fault and would have been caught by it.

---

## Part 7 — The preview must be the paper

Ten places where the screen and the paper disagree today. All ten close.

### 7.1 Same size model

The preview stops scaling `--receipt-px / 24` in CSS and starts rendering from
`Metrics`: each line arrives with its `row_dots`, its cap height and its
character advance, all computed in Rust. The CSS turns dots into screen pixels
with **one** multiplier for the whole page.

### 7.2 Rules are rules

`PreviewLine::Rule` carries `{ pattern, width_cols, thickness_dots }` and the
component draws a CSS border — solid, dashed, dotted, thick, double — matching
§2.2. It stops repeating a character.

### 7.3 The logo is the logo

`PreviewLine::Logo` carries the actual dots (`logo.rs` already has a `Dots`
type that crosses IPC for the settings screen — reuse it) and the component
draws them on a `<canvas>` at the size they will print. **A shop must be able
to see how big its logo is without printing one.**

### 7.4 QR and barcode look like themselves

The preview draws a real QR square and real CODE 128 bars, at the module size
and width the printer will use. Drawing the payload as text was defensible when
the preview was a stand-in; it is not defensible on the screen a shop tunes its
letterhead with.

### 7.5 The sample must be able to show a bug

`settings/sample.rs` passes `table: Some("6")`, `time: Some("19:40")`,
`bill_number: Some("SAMPLE")` — values the real paths do not supply. That is
why §5.1, §5.3 and §5.4 survived to a real install.

**The sample takes the same route as a real order.** Where a real print calls
`table_label`, the sample calls it too; where a real kitchen ticket resolves a
token, the sample resolves one. The sample may differ in its *data* and must
not differ in its *path*.

### 7.6 The preview says how much paper

Under the paper, in words: **"about 65 mm of roll"**, from `Laid::total_mm()`.
The owner's complaint began with paper being eaten; the screen should say how
much before it is.

### 7.7 The preview follows the printer's engine

`ipc.rs:720 preview_test_page` and `settings/sample.rs:199` both call plain
`layout()`, which is always `Grid::Dots`. A printer set to the **Text** engine
gets a different layout — measured: at sizes 1, 2, 4 and 5 the wrapping and the
size both differ. **Lay the preview out for `printer.effective_engine()`**, and
label the preview with which engine it is showing.

### 7.8 Preview the REAL bill — audit D6, never fixed

`ipc.rs:707` still says *"P09 will add `preview_order(order_id)` beside this"*.
It was never added. A grep for `preview_order` finds only that comment. Today
the only preview in the product is of a made-up sample.

Add:

* `preview_order(order_id)` → the real bill for the real order.
* `preview_kitchen(order_id)` → the real ticket that would print now.
* A **Preview** button on the billing screen beside the print button, and on
  the table tile's print mark. The dialog shows the paper and a **Print** button
  under it.

This is the single most valuable thing in Part 7 and it costs one command each,
because the sink already exists.

---

## Part 8 — The tests that must exist, or this all happens again

Every existing test passed while every fault above was on the paper. That is
the real defect.

| Test | Asserts |
|---|---|
| **T-ink** | Rasterise the standard bill in every shipped face. Assert the drawn cap height equals the size that was asked for, ±1 dot. Would have failed on 24→13. |
| **T-rule** | Rasterise a bill with each of the five patterns. Assert a `Solid` rule has ink in **every** column from the left edge to the right edge. Would have failed on 5-dots-in-12. |
| **T-length** | Assert the standard one-item bill rasterises to **no more than 70 mm** on 80 mm paper. A hard budget; if a change makes the bill longer, the test says so. |
| **T-sizes** | For every entry in `SIZES`, rasterise the item table and assert the drawn cap height is **strictly greater** than the entry below it. Would have failed on 6/7/8/9/10 all giving 26. |
| **T-wrap** | A 40-character dish name at size 8 wraps onto a second line with Qty/Rate/Amount intact on the first, and the **font size is unchanged**. The owner's ruling, as a test. |
| **T-sums** | Sum the printed Amount column out of the rendered characters and assert it equals the printed Subtotal. Covers exclusive, inclusive, non-GST and discounted lines (§5.6). |
| **T-preview** | Render the same `Laid` through the raster sink and through the preview's own view model and assert **the same cap heights, the same row heights, the same box edges and the same rule widths**. This is the anti-drift test the raster sink was never in. |
| **T-band** | A logo band on 58/80/100 mm: the image occupies exactly 30 % of the paper width, the text is centred in the remaining 70 %, and the band's height is `max(image, text)`. |
| **T-values** | A bill and a KOT built through the **real** call path carry the table's label and not its id, a time, a token and a KOT number. |
| **T-settings** | Every field of `ReceiptSettings` and `KitchenSettings` has a reader outside the settings screen (§6). |

`tests/antidrift.rs` T1 gains the raster sink. It cannot compare strings, so it
compares the **measurements** listed above.

---

## Part 9 — The settings screen

* `LOGO_POSITIONS` gains **At the left** and **At the right**.
* `SIZES` labels stay `1..10`; the values become cap heights (§1.4), and each
  choice shows **how many characters fit per line** at the current face and
  paper.
* Two new separator toggles (`below_tax_summary`, `below_payments`, §3.3(5)).
* The preview panel gains: the real logo, real QR, real barcode, the engine it
  is showing, and the roll length in millimetres.
* No paragraphs on screen. Hover tips.

---

## Part 10 — What must NOT change

* **D29 — one layout, many sinks.** `Metrics` is passed *into* the layout. No
  sink starts measuring text and deciding where to break it. If a sink ever
  wraps a line, audit D1 is back.
* **R2/D2 — one money path.** No template computes an amount. §5.6 changes
  *which* figure is printed, never how it is calculated. `Money::to_plain_string`
  stays the only formatter.
* **Crown jewel 2 — the delta KOT.** Nothing in Part 3.4 or Part 5 changes what
  the kitchen is told, only how it is printed.
* **Requirement 3 — a bill always comes out.** A logo that will not decode, a
  face that will not load, a band that will not fit: note it and print the bill.
  D37 stands.
* **Kannada is still not this.** A second face is not a shaper. `layout` still
  counts characters and `Metrics` measures them; neither is text shaping.

---

## Acceptance — the owner's checklist, on real paper

Print one bill and one KOT on the TVSE and check:

- [ ] The table line says **Table 6**, not `tbl_outlet_default_sec_…` — on both papers.
- [ ] The bill shows a **time** next to the date.
- [ ] The KOT shows a **TOKEN**, a **time**, a **bill number** and a **KOT number**.
- [ ] The separator lines are **solid lines**, not rows of dots.
- [ ] The item Amount column **adds up to the Subtotal**.
- [ ] Change the item size from 6 to 7 to 8 — the text on paper **gets bigger each time**.
- [ ] At a big size a long dish name **wraps onto a second line** and the letters are not squashed.
- [ ] Set the logo to **left**: it fills 30 % of the paper and the shop name is centred in the other 70 %.
- [ ] The one-item bill is about **65 mm**, not 160 mm.
- [ ] The preview on screen and the paper in your hand **look like the same document**.
- [ ] Press **Preview** on a real order before printing it, and see that bill.

---

# What was built — 2026-08-23

Every part above is done. `cargo test --workspace` 588 + 42 suites green,
`cargo clippy --workspace --all-targets` clean, `npm test` 309 green, every UI
lint clean, `audit-wiring` clean at 240 commands. **And it was driven on the
real Tauri window against a fresh install**, which is where the last four
defects came from.

## The numbers, measured on the owner's own bill

| | before | after |
|---|---|---|
| one-item bill, 80 mm | **117 mm** | **61 mm** |
| lines on that bill | 32 | 21 |
| separator rules | 9, at 27 dots each | 4, at 7–9 dots each |
| body capital, built-in face | 13 dots for a 24-dot setting | **15 dots for a size-4 setting** |
| body row | 27 dots | 24 dots |
| item amount vs subtotal | `105.00` over `100.00` | `100.00` over `100.00` |
| sizes that print differently | 5 of 10 | **10 of 10** |

## What the real app printed, at the end

```
      SADGURU BAKERY
  12 MG Road, Jayanagar, Bengaluru 560011
               Ph 9880012345
           GSTIN 29ABCDE1234F1ZW
------------------------------------------
                TAX INVOICE
         TOKEN 1
Bill 0001     2026-08-23 13:58       Dine In
Table T1                        Cashier Ravi
Item                    Qty    Rate   Amount
------------------------------------------
MASALA DOSE               2  100.00   200.00
MASALA DOSE               1  100.00   100.00
(no onion)
------------------------------------------
Subtotal                              300.00
CGST 5%                                 7.50
SGST 5%                                 7.50

TOTAL               315.00
------------------------------------------
Card                                  315.00
           Thank you, visit again
```

and the ticket:

```
         KITCHEN
------------------------------------------
         TOKEN 1
Table T1           Dine In             13:55
Bill 0001
------------------------------------------
   2 MASALA DOSE

   1 MASALA DOSE
     * no onion
------------------------------------------
```

`Table T1`, not `tbl_outlet_default_none_t1_`. A token on the ticket. A time on
both. The column adds up.

## What the build changed that the prompt did not foresee

1. **The row-height reference set had to be trimmed.** Measuring a row against
   `Å` made every line six dots taller than a receipt needs, for a letter no
   Indian bill prints and the ESC/POS code page cannot produce. `font::REFERENCE`
   is now the shapes a receipt actually contains.
2. **`Monochrome::scaled_to` was the logo's real damage**, not the threshold
   order in `Logo.tsx`. Shrinking a 1-bit picture with nearest neighbour throws
   away three quarters of every stroke; it averages now (`COVERAGE`).
3. **A dashed rule's last stroke has to be computed from its own index.** A
   fixed step lost the remainder and every dashed rule stopped five dots short
   of the paper.
4. **The size ladder had to be disjoint from every value ever stored** so
   `Style::from_stored` can tell a shop's old row from a new one exactly rather
   than guessing. That is why the numbers are 9, 11, 13, 15, 17, 19, 22, 26, 33,
   41 and not round ones.
5. **Four defects only the running app could show**, all found with
   `scripts/drive.mjs` against an empty `APPDATA`:
   * a bill previewed before settling printed `1970-01-01`;
   * it showed `TOKEN -` for a token that did not exist yet;
   * once the order was parked the preview ignored its real token and number;
   * the print queue's own list still named the table by its database id.
6. **`Order Ravi` under `Cashier Ravi`** — a one-person counter is one person,
   and the waiter line is dropped when it is the same name.

## What was added beyond the prompt

* **`reprint_kitchen_ticket`** — the whole order again, marked
  `*** REPRINT ***`, ledger untouched. Without it `KitchenContext::reprint`
  would have been a field nothing set, which is the fault pattern round 2 was
  about.
* **`bill_pdf`** — scope 7.10, the A4 invoice. The PDF sink had existed since
  P06 and **nothing but the report exporter ever called it**, so a B2B customer
  asking for an invoice got a three-inch till roll.
* **Migration 0003** — the `kot` counter, so a kitchen ticket has a running
  number of its own.
* **`Bill::tax_included` / `tax_added`** in mb-core, without which a bill that
  mixes inclusive and exclusive prices cannot both show amounts before tax and
  add up.

## Still not done, and deliberately

* **Number of covers and the waiter's name** print, but nothing on the billing
  screen sets a cover count yet — the field is on the order and no screen fills
  it. Printing is ready for it.
* **A condensed built-in typeface** (§1.6). The size model no longer needs one
  to be honest; it would buy more letter height per column and is a separate,
  optional change.
* **The native QR is still not moved by the print offset.** The printer's own
  encoder positions it and knows nothing about a millimetre correction; the
  code says so where it happens rather than pretending.
