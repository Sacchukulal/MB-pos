# P33 — TAX. The whole thing, rebuilt from the core.

Read `docs/TAX_AUDIT_2026-08-24.md` first. It is the evidence; this is the plan.
Every fault named here was verified by reading the code, and every file and line
in this document was checked on 2026-08-24.

**The owner's instruction, 2026-08-24:**

> *"I want a full rework of tax, old things should not make problem later. Not
> just patching from above."*

That sentence decides the shape of everything below. This is not a bug list. It
is a **replacement of the tax model**, done from `mb-core` outward, with the old
model **deleted** rather than deprecated.

---

## How this prompt is to be worked

* **Build the whole thing, back to back, and report once.** Do not split it into
  an easy half now and a lengthy half later. If one phase turns out to be
  genuinely blocked, finish every other phase in full and say exactly what was
  left and why.
* **Phases are a dependency order, not a menu.** Phase 1 and Phase 2 must land
  together — the core types and the schema that stores them are one change. From
  Phase 3 on, each phase compiles and its tests pass before the next begins.
* **Delete, never deprecate.** No `#[deprecated]`, no `treatment_v2`, no "keep
  the old field for now". A compatibility shim is exactly the "old thing making
  problems later" the owner is ruling out. The old enum comes out of the tree.
* **Every fix needs a test that would fail with the fault restored.** The whole
  reason this round exists is that 596 Rust tests and 305 front-end tests all
  pass today while a bar undercharges every drink.
* **Nothing here is done until it has been seen on the real window.**
  `scripts/drive.mjs` drives the actual Tauri app over CDP. A tax screen that
  compiles is not a tax screen that works — P31 and P32 both proved that.
* **Money rules are unchanged and non-negotiable.** D7 still holds: no float
  arithmetic, no silent truncation, every fallible add returns `Result`. The new
  model must keep the two properties the old one already proves (§1.7).
* Plain English on screen. No paragraphs explaining GST to the owner — hover
  tips only. The owner is the developer; shopkeepers get short labels.

---

## The rulings this is built on

Settled by the audit and the owner's answers. Do not re-open them.

| Ruling | Decision |
|---|---|
| **No hardcoded slabs** | A rate is free entry, any value 0–100%. `GST_12` and `GST_28` constants are deleted. Slab lists in the UI are deleted. The owner's words: *"Do not hardcode the app to any tax slab."* |
| **Rate belongs to the item** | Through its tax class. Settings never owns a rate. |
| **Inclusive/exclusive belongs to the item too** | The owner's MRP objection settled this: a ₹20 MRP bottle is tax-inclusive on the same bill as a ₹100 tax-exclusive dosa. One shop-wide switch cannot express that bill. |
| **Settings owns who the shop IS** | Registration kind, GSTIN, state. Nothing else. |
| **Liquor gets a real tax** | State VAT, per item, on its own channel, never inside a GST total, never in GSTR-1. |
| **IGST is not for customer bills** | Restaurant service is always intra-state (IGST Act s.12(4)). The shop-wide place-of-supply setting is deleted. |
| **Old bills never change** | Crown jewel 4. The migration must leave every printed figure on every past bill exactly as it was. This is testable and must be tested. |

---

# Phase 1 — The tax model in `mb-core`

The core rework. Everything else is consequence.

## 1.1 The fault

`TaxTreatment` (`crates/mb-core/src/tax.rs:85`) has four values:

```rust
pub enum TaxTreatment { Exclusive, Inclusive, Exempt, NonGst }
```

**These are two unrelated questions jammed into one enum.**

* `Exclusive` / `Inclusive` answer *"is the tax inside the price I typed?"* — a
  **pricing convention**.
* `Exempt` / `NonGst` answer *"what kind of supply is this in law?"* — a **legal
  category**.

The damage this does is not theoretical:

1. **Liquor cannot have a rate.** The moment a line is `NonGst`, the enum has
   already decided it is neither inclusive nor exclusive of anything, so there is
   nowhere to put a 20% Karnataka VAT. `compute_line` (`tax.rs:189`) hard-codes
   `NonGst | Exempt => (net, Money::ZERO)`. A bar undercharges every drink.
2. **An exempt item cannot be priced inclusive**, which is meaningless but
   harmless — and a nil-rated item priced at MRP is a real thing.
3. **The four values are not orthogonal**, so no exhaustive `match` in the
   codebase is actually exhaustive over the real problem space.

## 1.2 What to build

Two enums, one struct. In `crates/mb-core/src/tax.rs`.

```rust
/// **What kind of supply this is, in the law's own terms.**
///
/// Not a rate and not a pricing convention — those are `TaxRate` and
/// `PriceBasis`. This is the question a GST return asks: which box does the
/// value of this line go in?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaxKind {
    /// Normal GST at the line's rate. The everyday case.
    #[default]
    Gst,
    /// Inside GST, at nil rate. Appears in the return with a taxable value and
    /// no tax.
    Exempt,
    /// **Outside GST entirely — alcohol** (Constitution, Article 366(12A)).
    ///
    /// It is NOT untaxed. It attracts state VAT at the line's rate — 20% in
    /// Karnataka, 25%/35% in Maharashtra — which is charged on its own channel
    /// and never enters a GST return.
    OutsideGst,
    /// Genuinely no tax of any kind. A deposit, a refundable container.
    Untaxed,
}

/// **Is the tax already inside the price the shop typed?**
///
/// Independent of `TaxKind`, and it has to be: an MRP-priced bottle of water is
/// tax-inclusive, an MRP-priced bottle of beer is VAT-inclusive, and a dosa is
/// tax-exclusive. All three can be on one bill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceBasis {
    /// Tax is added on top of the price. The dosa.
    #[default]
    Exclusive,
    /// Tax is already contained in the price and is worked backwards out of it.
    /// **MRP is inclusive of all taxes by law**, so every MRP item is this.
    Inclusive,
}

/// Everything about one line's tax, in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TaxSpec {
    pub kind: TaxKind,
    /// GST rate for `Gst`, **state VAT rate for `OutsideGst`**, zero for the
    /// other two. One field because a line has one rate; which tax it is a rate
    /// *of* is `kind`'s job.
    pub rate: TaxRate,
    pub basis: PriceBasis,
}
```

**Why one `rate` field and not `gst_rate` plus `vat_rate`:** a line is never both
GST and VAT. Two fields would mean two ways to express the same line and a day
when they disagree — the same argument `Charge` already makes about not having a
`taxable: bool` beside its treatment (`charge.rs:47`).

## 1.3 The two tax channels must be different types

**This is the single most important decision in the phase.**

Today `TaxAmounts { cgst, sgst, igst }` is summed in a dozen places. If VAT is
added as a fourth field, every existing `total()` silently starts including it,
and a GST return would report liquor VAT as GST. That is a worse bug than the one
being fixed.

So VAT gets its **own type**, and the compiler stops them mixing:

```rust
/// GST, already named. Never contains VAT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GstAmounts {
    pub central: Money,
    /// SGST, or **UTGST** in a union territory without a legislature. One field
    /// because it is one number; `StateTax` decides what it is called.
    pub state: Money,
    pub integrated: Money,
}

/// **State VAT on alcohol. Not GST, and structurally cannot become GST.**
///
/// A newtype rather than a bare `Money` so that no `Money::try_sum` anywhere in
/// this workspace can fold it into a GST figure by accident. Getting it out
/// takes a deliberate `.into_money()`, which is a line a reviewer can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Vat(Money);

impl Vat {
    pub const ZERO: Vat = Vat(Money::ZERO);
    #[must_use] pub const fn new(amount: Money) -> Self { Vat(amount) }
    /// Deliberate, and named so it reads as a decision at the call site.
    #[must_use] pub const fn into_money(self) -> Money { self.0 }
    pub fn add(self, other: Self) -> Result<Self> { Ok(Vat(self.0.add(other.0)?)) }
    #[must_use] pub fn is_zero(self) -> bool { self.0.is_zero() }
}
```

`GstAmounts` is `TaxAmounts` renamed with `cgst`/`sgst`/`igst` becoming
`central`/`state`/`integrated`. **Rename it rather than keeping the old name**:
every call site then has to be visited, which is the point — a silent compile
after a change this size would mean something was missed.

Keep `add`, `sub`, `total`, `is_zero` exactly as they are (`tax.rs:120-160`).
`sub` is P32's and is still needed for the inclusive-tax split on the printed
bill.

## 1.4 SGST or UTGST

```rust
/// What the state half of an intra-state supply is called.
///
/// Chandigarh, Lakshadweep, Andaman & Nicobar, Ladakh, and Dadra & Nagar Haveli
/// and Daman & Diu have no legislature, so their half is **UTGST**. Delhi,
/// Puducherry and Jammu & Kashmir have one, so theirs is SGST. A shop in
/// Chandigarh currently prints the wrong word on every bill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateTax { #[default] Sgst, Utgst }

impl StateTax {
    /// From the two-digit GST state code. The list is short, closed, and set by
    /// law, so it lives here rather than in settings data.
    #[must_use]
    pub fn for_state_code(code: &str) -> Self {
        // 04 Chandigarh, 26 DNH & DD, 31 Lakshadweep, 35 A&N, 38 Ladakh
        match code {
            "04" | "26" | "31" | "35" | "38" => StateTax::Utgst,
            _ => StateTax::Sgst,
        }
    }
    #[must_use] pub const fn label(self) -> &'static str {
        match self { StateTax::Sgst => "SGST", StateTax::Utgst => "UTGST" }
    }
}
```

**Check the codes against the current CBIC state-code list while building.** They
are stable but they are not guessable, and 26 was merged in 2020.

## 1.5 What kind of taxpayer the shop is

This is the idea the app is missing entirely, and it belongs in core because it
is a **rule**, not a setting value (R8, D1).

```rust
/// **What kind of taxpayer this shop is.** The gate on the whole tax pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Registration {
    /// Below the threshold. No GSTIN, and **no GST may be charged or shown**.
    Unregistered,
    /// Composition scheme. Has a GSTIN, pays 5% out of its own margin, and
    /// **may not collect or display GST**. Issues a bill of supply.
    Composition,
    /// Regular. Collects GST, issues a tax invoice.
    #[default]
    Regular,
}

impl Registration {
    /// **The gate.** Only a regular taxpayer may put GST on a customer bill.
    #[must_use]
    pub const fn charges_gst(self) -> bool { matches!(self, Registration::Regular) }

    /// A composition dealer may not make inter-state supplies.
    #[must_use]
    pub const fn may_supply_interstate(self) -> bool {
        !matches!(self, Registration::Composition)
    }

    /// A composition dealer may not deal in alcohol at all.
    #[must_use]
    pub const fn may_sell_alcohol(self) -> bool {
        !matches!(self, Registration::Composition)
    }

    /// Tax invoice, bill of supply, or neither.
    #[must_use]
    pub const fn document_title(self) -> Option<&'static str> {
        match self {
            Registration::Regular => Some("TAX INVOICE"),
            Registration::Composition => Some("BILL OF SUPPLY"),
            Registration::Unregistered => None,
        }
    }
}
```

**VAT is deliberately not gated by `Registration`.** A liquor licence is a state
excise registration and has nothing to do with GST registration. An unregistered
shop can still owe VAT on alcohol. Gating them together would be a new bug.

## 1.6 `compute_line`, rewritten

```rust
pub fn compute_line(
    net: Money,
    spec: TaxSpec,
    place: PlaceOfSupply,
    state_tax: StateTax,
    registration: Registration,
) -> Result<TaxOutcome>
```

The rules, in order:

1. **Decide whether this line is taxed at all, and by what.**
   * `Untaxed` → no tax, taxable = net.
   * `Exempt` → no tax, taxable = net, and the value goes to the exempt bucket.
   * `OutsideGst` → **VAT at `spec.rate`**, and the value goes to the non-GST
     bucket. Never touched by `registration`.
   * `Gst` → GST at `spec.rate`, **but only if `registration.charges_gst()`**.
     Otherwise no tax is charged and the taxable value is still recorded, because
     a composition dealer's turnover is what it pays its own 5% on.
2. **Split the price by `spec.basis`.** Unchanged from today, and the inclusive
   branch must stay exactly as it is (`tax.rs:205`): taxable is
   `net × 10000 / (10000 + rate)` and the tax is **the remainder**, never a second
   rounded multiplication. That is what makes an inclusive line total to its menu
   price exactly. **This applies to VAT as well as GST** — a VAT-inclusive MRP
   beer must total to its menu price the same way.
3. **Name the tax.** GST only. `Intra` → `central`/`state` via `halve_exact`
   (`tax.rs:213`), so the two halves always sum to the whole. `Inter` →
   `integrated`. VAT is never split.

```rust
pub struct TaxOutcome {
    pub taxable: Money,
    pub gst: GstAmounts,
    pub vat: Vat,
    pub gross: Money,
    pub spec: TaxSpec,
}
```

**A composition line still computes `taxable`.** That figure is what the shop's
own 5% liability is calculated from, and the day-close report needs it. It is
simply never printed on the customer's bill.

## 1.7 The two properties that must survive

These are already proved by property tests and must still pass, reworded for the
new types. If either breaks, the rework is wrong.

| Property | Where it is proved today |
|---|---|
| An inclusive line's `taxable + tax` equals its menu price **exactly**, for every amount and every rate | `tax.rs:357` — 2,000 amounts × 4 rates |
| `central + state` equals the whole GST **to the paisa**, for every amount | `tax.rs:377` — 3,000 amounts |

Add two more:

| New property | Why |
|---|---|
| A VAT-inclusive line's `taxable + vat` equals its menu price exactly | Same reason as the first, for the new channel. Run it over the same 2,000 amounts at 20%, 25% and 35% — the real Indian liquor VAT rates. |
| `Vat` can never be summed into `GstAmounts` | Compile-time. Write a `trybuild`-style comment or simply assert the types differ; the real guard is that `GstAmounts::add` takes `GstAmounts`. |

## 1.8 What gets deleted in Phase 1

* `TaxTreatment` — the enum and every `use` of it.
* `TaxRate::GST_12` and `TaxRate::GST_28` (`tax.rs:34,36`). Keep `GST_5` and
  `GST_18` **only if** they are used by tests; the shipping code must build rates
  from data. Prefer deleting all four and having tests say
  `TaxRate::from_percent(5).expect("5%")`.
* `TaxClass::by_order_type`, `OrderTypeRate`, `TaxClass::with_override`,
  `TaxClass::for_order_type` (`taxclass.rs:67-120`) — see Phase 5.4.

## 1.9 Tests for Phase 1

Named so a failure reads as a sentence.

```
a_bar_in_karnataka_charges_twenty_per_cent_vat_on_a_beer
vat_never_appears_in_a_gst_total
a_vat_inclusive_beer_totals_to_its_menu_price
a_composition_shop_computes_taxable_value_but_charges_no_gst
an_unregistered_shop_charges_no_gst_on_anything
an_unregistered_shop_still_charges_vat_on_liquor
a_chandigarh_shop_calls_the_state_half_utgst
a_karnataka_shop_calls_the_state_half_sgst
an_mrp_bottle_and_an_exclusive_dosa_on_one_bill_both_come_out_right
exempt_and_outside_gst_land_in_different_buckets
the_inclusive_property_still_holds_for_gst      (ported)
the_halving_property_still_holds                (ported)
the_inclusive_property_holds_for_vat            (new)
```

---

# Phase 2 — The database. Migration `0004_tax_rework.sql`

Lands with Phase 1. The core types and the columns that store them are one
change.

## 2.1 What the schema says today

`treatment TEXT CHECK (… IN ('exclusive','inclusive','exempt','non_gst'))`
appears **six times**, verified:

| Table | Line in `0001_initial.sql` | What it holds |
|---|---|---|
| `tax_classes` | 174 | the shop's vocabulary |
| `tax_class_rates` | 190 | the dead order-type override |
| `items` | 243 | the denormalised copy on a menu row |
| `order_lines` | 639 | **lines on an order that is open right now** |
| `bill_lines` | 785 | **frozen history — must not change** |
| `bill_charges` | 810 | a charge's own treatment |

Miss one and the migration fails at runtime on a real shop's data.

## 2.2 The rules the migration engine imposes

Read `crates/mb-db/src/migrate.rs` before writing a line of SQL.

* Migrations are a `const` array (`migrate.rs:45`), forward-only, and
  **checksummed**. `apply_all` refuses to run if a shipped migration's SQL has
  changed since it was applied. So `0001` may not be edited — everything goes in
  `0004`.
* Next version is **4**. Add the entry to `MIGRATIONS` with its `include_str!`.
* `crates/mb-db/tests/schema_rules.rs` will fail the build unless every new table
  is `STRICT`, every column is `TEXT` or `INTEGER`, booleans are `0/1` and
  `NOT NULL`, ids are `TEXT`, nothing auto-increments, and every root table
  carries `outlet_id NOT NULL`.
* SQLite cannot alter a `CHECK` constraint. Each of the six tables needs the
  **12-step table rebuild**: create the new table, copy, drop, rename, recreate
  indexes and triggers. Do it inside the migration's transaction.

## 2.3 The column changes

Replace `treatment` with two columns everywhere it appears:

```sql
kind  TEXT NOT NULL DEFAULT 'gst'
      CHECK (kind IN ('gst', 'exempt', 'outside_gst', 'untaxed')),
basis TEXT NOT NULL DEFAULT 'exclusive'
      CHECK (basis IN ('exclusive', 'inclusive')),
```

On `bill_lines` and `bill_charges`, add the VAT column beside the GST ones:

```sql
vat INTEGER NOT NULL DEFAULT 0,
```

On `bills`, beside `total_cgst`/`total_sgst`/`total_igst`:

```sql
total_vat   INTEGER NOT NULL DEFAULT 0,
state_tax   TEXT    NOT NULL DEFAULT 'sgst' CHECK (state_tax IN ('sgst', 'utgst')),
registration TEXT   NOT NULL DEFAULT 'regular'
             CHECK (registration IN ('unregistered', 'composition', 'regular')),
```

**Why `bills` freezes the registration and the state-tax name:** the same reason
it already freezes the place of supply. A shop that moves from composition to
regular next year must not have last year's bills reprint with a different title.
Crown jewel 4 applies to the shop's own status, not only to prices.

On `store_profile`, replace `is_composition INTEGER` with:

```sql
registration TEXT NOT NULL DEFAULT 'regular'
             CHECK (registration IN ('unregistered', 'composition', 'regular')),
```

and **drop `default_place_of_supply`** (audit §3.6).

## 2.4 The backfill — and the rule that governs it

```sql
UPDATE <table> SET
  kind  = CASE treatment WHEN 'exempt'  THEN 'exempt'
                         WHEN 'non_gst' THEN 'outside_gst'
                         ELSE 'gst' END,
  basis = CASE treatment WHEN 'inclusive' THEN 'inclusive'
                         ELSE 'exclusive' END;
```

**`vat` backfills to 0 on every historical row, and that is correct, not a
compromise.** Old bills charged no VAT because the old app charged none. Writing
a VAT figure onto a bill that never had one would forge a document. The liquor
fix applies to bills made from now on.

Say this out loud in the migration's comment. It is the kind of decision a future
session will otherwise "fix".

## 2.5 Tables that go

```sql
DROP TABLE tax_class_rates;
```

Audit §3.5: modelled, stored, saved, **never read by any caller**, and justified
by a belief about takeaway that is not current law. It goes with the code in
Phase 5.4.

## 2.6 The 12% classes already sitting in real shops

A shop set up before today has a `tax_packaged_12` class, because
`starting_classes()` (`taxclass.rs:134`, the 12% entry at `:144`) seeded it. The
12% slab was abolished on 22 September 2025.

**Do not silently change its rate.** A migration that rewrites 12 to 18 changes
what every packaged item costs, without the owner knowing. That is worse than the
stale class.

Instead:

1. Leave the rate exactly as it is. It is the shop's data.
2. Set `is_active = 0` on any seeded class whose id is `tax_packaged_12` **and**
   which no item points at. A class nobody uses does not need to survive.
3. If items do point at it, leave it fully alone and let Phase 4.6 show the owner
   a one-time notice naming the class and the count of items, with a link to the
   Menu screen.

Migrations move data. **Deciding a shop's prices is the owner's job.**

## 2.7 Tests for Phase 2

`crates/mb-db/tests/` — and one of these is the most important test in the whole
round:

```
every_old_bill_prints_exactly_the_same_after_the_migration
```

Build a database on the **old** schema with a spread of bills — exclusive,
inclusive, exempt, liquor, a discount, a charge, inter-state — record every
computed figure, run `apply_all`, read them back, and assert **not one paisa
moved**. That is crown jewel 4 turned into a test, and it is what "old things
should not make problem later" means concretely.

```
the_migration_maps_all_four_old_treatments
liquor_lines_keep_zero_vat_after_the_migration
an_unused_twelve_per_cent_class_is_retired
a_twelve_per_cent_class_that_items_use_is_left_alone
tax_class_rates_is_gone
the_schema_still_satisfies_every_rule_in_schema_rules
a_second_run_of_apply_all_changes_nothing
```

---

# Phase 3 — The bill pipeline. `crates/mb-core/src/bill.rs`

## 3.1 The fault

`Bill` and `BillLine` carry `TaxAmounts` and `TaxTreatment` and know nothing
about VAT or registration. `BillInput` (`bill.rs:39`) has no idea who the shop
is, so `is_composition` cannot reach the calculation even though the print
template reads it (`bill.rs:300` in `mb-print`) — which is exactly how a bill of
supply ends up with CGST lines printed under it.

## 3.2 What to build

`BillInput` gains three fields, all required:

```rust
pub registration: Registration,
pub state_tax: StateTax,
```

`BillInput::new` keeps its "plain bill" defaults, but a **builder call is
required for registration** — do not let it default to `Regular`, because a
default that charges tax is the wrong way round for a safety gate. Make
`BillInput::new` take it:

```rust
pub fn new(cart: &'a Cart, registration: Registration) -> Self
```

That one signature change forces every one of the five call sites
(`billing.rs:131,782`, `flows.rs:940,1403`, `settings/sample.rs:116`) to be
visited, which is the point.

`BillLine` gains `vat: Vat` and swaps `rate`/`treatment` for `spec: TaxSpec`.

`Bill` gains:

```rust
pub total_vat: Vat,
pub registration: Registration,
pub state_tax: StateTax,
```

and `total_tax: TaxAmounts` becomes `total_gst: GstAmounts`, with
`tax_included`/`tax_added` (P32's split, `bill.rs:145-176`) becoming
`gst_included`/`gst_added`. **Keep that split — it is what makes the printed
column add up** — and give VAT the same treatment: `vat_included`/`vat_added`, or
the printed bill will double-count a VAT-inclusive beer exactly the way it
double-counted an inclusive dosa before P32.

## 3.3 The pipeline order does not change

Steps 1–8 (`bill.rs:241-394`) stay exactly as they are. In particular:

* **Discount stays before tax** (steps 2–3 at `bill.rs:262,274`, tax at
  `bill.rs:290`). Section 15(3)(a) of the CGST Act: a discount shown on the
  invoice reduces the taxable value. The app already gets this right.
* **Round-off stays on the grand total only** (step 7, `bill.rs:389`) and stays
  recorded as its own figure, so the printed lines always sum to the printed
  total.

The comment at the top of `bill.rs` saying a session that wants to reorder these
must delete a comment saying what it is doing — leave it, and do not reorder.

## 3.4 Charges

`Charge` (`charge.rs:40`) carries its own rate and treatment. Swap its
`tax_treatment: TaxTreatment` for `spec: TaxSpec`. A charge is never liquor, so
`OutsideGst` on a charge should be refused at construction rather than
represented — one fewer state to reason about.

## 3.5 Tests for Phase 3

```
a_composition_bill_has_a_taxable_value_and_no_gst
an_unregistered_bill_has_no_tax_lines_at_all
a_bill_with_food_and_beer_keeps_the_two_taxes_apart
the_grand_total_still_equals_subtotal_minus_discount_plus_charges_plus_tax_plus_round_off
a_vat_inclusive_line_does_not_double_count_its_vat
discount_still_reduces_the_taxable_value
```

The fourth is the P32 reconciliation identity extended with VAT, and it should be
a **property test** over generated bills, not a single example. `bill.rs:589`
already has a generator that makes random bills — extend it to emit liquor lines
and composition shops.

---

# Phase 4 — Settings. Who the shop is.

## 4.1 Delete the two dead settings

`src-tauri/src/settings/mod.rs:425,427` and `catalog.rs:746,749`.

* `tax.default_class_id` — never read. The Menu screen uses `classes[0]`
  (`ui/src/menu/Menu.tsx:184`).
* `tax.prices_include_tax` — never read, and it claims to decide the same thing
  as the tax class's basis.

Delete the fields, the catalog rows, and the `Tax` struct if nothing else is left
in it. **This is the specific thing the owner reported.** It is the smallest
change in this document and it should be done first, in its own commit, so the
fix is visible immediately.

## 4.2 Registration replaces `is_composition`

One picker where the flag was:

```
Tax registration
  ( ) Not registered      no GSTIN, no tax on any bill
  ( ) Composition scheme  GSTIN, no tax shown, bill of supply
  (•) Regular GST         GSTIN, tax shown, tax invoice
```

Use the existing `pick_text!` macro (see `catalog.rs:741` for the shape) with a
closed list. The help text is one sentence, no paragraph.

## 4.3 Delete the shop-wide place of supply

`catalog.rs:741`, `settings/mod.rs:242`, and the `default_place_of_supply` column
(Phase 2.3).

Audit §3.6: it is applied to every bill (`billing.rs:133`, `flows.rs:942`), and
for restaurant service it can only ever be `Intra`. Its help text also promises
*"A single bill can still be changed"* — a feature that does not exist anywhere in
the UI. Delete the setting and the sentence together.

`PlaceOfSupply` **stays in core**, because Phase 8 needs it for the two places
inter-state is real.

## 4.4 The composition declaration cannot be blank

`receipt.composition_note` (`catalog.rs:891`) is free text, and the template
prints nothing when it is empty (`bill.rs:772` in `mb-print`). A bill of supply
without the declaration is not a valid bill of supply.

* Ship the legal wording as the default: *"Composition taxable person, not
  eligible to collect tax on supplies."*
* When registration is `Composition`, refuse to save it empty — the settings
  validation layer already has `Invalid` for exactly this (`catalog.rs:1262` shows
  the pattern).

## 4.5 Refuse the combinations the law refuses

The settings screen already cross-checks GSTIN against state code
(`catalog.rs:1262`) — a genuinely good check almost no POS does. Add three more
in the same place, as `Invalid` results with a sentence saying why:

| Combination | Message |
|---|---|
| Composition + any item in an `OutsideGst` class | "A composition dealer cannot sell alcohol. Change the registration or move those items." |
| Composition + a GSTIN that is empty | "A composition dealer has a GST number. Enter it, or choose Not registered." |
| Regular or Composition + empty GSTIN | "A registered shop needs its GST number on every bill." |
| Unregistered + a GSTIN entered | "You have entered a GST number but chosen Not registered. One of the two is wrong." |

The owner asked whether to refuse or warn (audit §7.4). **Refuse.** A warning on a
settings screen is read once and dismissed; the bills go out for years.

## 4.6 The one-time notice about a 12% class

Phase 2.6 leaves a used 12% class alone. Tell the owner why, once, on the Menu
screen: *"The 12% GST slab ended on 22 September 2025. N items still use
'Packaged goods 12%'. Check what they should be now."* Dismissible, and it does
not come back.

## 4.7 Tests for Phase 4

```
the_dead_tax_settings_are_gone            (assert the keys are absent from the catalog)
a_composition_shop_cannot_save_an_empty_declaration
a_composition_shop_with_a_liquor_item_is_refused
an_unregistered_shop_with_a_gstin_is_refused
the_place_of_supply_setting_is_gone
```

The first one matters more than it looks: it is a test that a **setting nobody
reads cannot exist**. Consider generalising it — see Phase 10.

---

# Phase 5 — Menu. What things are.

## 5.1 The fault that will bite hardest

`ui/src/menu/Menu.tsx:472-480`:

```tsx
treatment: editing.treatment.includes('Outside')
  ? 'non_gst'
  : editing.treatment.includes('Exempt')
    ? 'exempt'
    : editing.treatment.includes('included')
      ? 'inclusive'
      : 'exclusive',
```

`TaxClassView.treatment` is a **human display string** — *"Added on top"*,
*"Outside GST"* (`src-tauri/src/menu.rs:43`). The screen sniffs that prose to
decide what to send back to Rust.

**Reword the label and liquor silently becomes GST-taxable.** Nothing fails, no
test catches it, and a bar starts charging 5% GST on beer, which is illegal in
the other direction.

This is the clearest example in the codebase of the thing the owner is asking to
be rid of — an old shortcut that will make a problem later.

## 5.2 What to build

`TaxClassView` carries the machine values **and** the display strings, and the
screen only ever sends back a machine value:

```rust
pub struct TaxClassView {
    pub id: String,
    pub name: String,
    /// Preformatted for display — "5%", "12.5%". R8: TypeScript divides nothing.
    pub rate: String,
    /// The rate again, as basis points, for the editor to send back unchanged.
    pub rate_bp: u32,
    /// Machine value: "gst" | "exempt" | "outside_gst" | "untaxed".
    pub kind: String,
    /// Machine value: "exclusive" | "inclusive".
    pub basis: String,
    /// What to show a person: "GST", "Exempt", "Outside GST (VAT)", "No tax".
    pub kind_label: String,
    pub basis_label: String,
    pub is_active: bool,
    pub items_using: i64,
}
```

Generate the TypeScript with `ts-rs` as today (`cargo test` regenerates and diffs
it, so the two sides cannot drift — `src-tauri/Cargo.toml` explains this).

**Better still, make the machine values a real type.** `kind: TaxKind` with
`#[derive(TS)]` gives TypeScript a union `"gst" | "exempt" | "outside_gst" |
"untaxed"` instead of `string`, and then a typo is a compile error in the UI. Do
this — it is the structural version of the fix, and `ts-rs` already supports it.

## 5.3 The class editor becomes complete

Today the editor can change a class's **name and rate only**; the treatment is
explicitly not editable, with a comment saying *"P17's settings screen owns it"*
(`Menu.tsx:472`). That screen's switch is one of the two dead settings from Phase
4.1. **So the treatment is editable nowhere in the product.**

The new editor has four fields:

| Field | Control | Note |
|---|---|---|
| Name | text | The seeded names carry the rate; changing one must prompt for the other, as it already does (`Menu.tsx:402`). |
| Rate | number, per cent | **Free entry, no slab list.** Accepts decimals — 2.5% is a real rate and `TaxRate` holds basis points exactly. |
| Kind | 4 choices | GST / Exempt / Outside GST (VAT) / No tax. |
| Price basis | 2 choices | Tax added on top / Tax already in the price. |

When kind is `Exempt` or `Untaxed`, disable the rate and force it to 0 — do not
merely hint it, as the current hint does (*"Liquor and exempt items stay at 0"*,
`Menu.tsx:455`).

When kind is `OutsideGst`, the rate's label changes to **"State VAT %"** and the
hint names the two facts that matter: it is not GST, and it will never appear in
a GST return.

## 5.4 Delete the order-type override

Audit §3.5. `TaxClass::by_order_type`, `OrderTypeRate`, `with_override`,
`for_order_type` (`taxclass.rs:67`), `TaxClassRepo::resolve`
(`repo/taxclass.rs:171`), the `tax_class_rates` table (Phase 2.5), and the save
path's override block (`repo/taxclass.rs:135-147`).

`resolve` has **no callers anywhere in the repository**. The feature is dead
weight, and its stated justification — *"some states tax the same dish differently
to take away"* — is not current law: parcel from a restaurant is restaurant
service at 5%, the same as dine-in.

If per-order-type rates are ever genuinely needed, they will be needed for a
reason we can state, and they can be built then against that reason.

## 5.5 The seeded classes

`starting_classes()` (`taxclass.rs:134`) becomes:

```rust
vec![
    class("tax_food_5",     "Restaurant food 5%",  500, Gst,        Exclusive),
    class("tax_goods_5",    "Packaged goods 5%",   500, Gst,        Exclusive),
    class("tax_goods_18",   "Packaged goods 18%", 1800, Gst,        Exclusive),
    class("tax_liquor",     "Liquor — state VAT",    0, OutsideGst, Inclusive),
    class("tax_exempt",     "Exempt",                0, Exempt,     Exclusive),
]
```

Changes from today: the 12% class is gone; a 5% goods class replaces it; liquor
is `OutsideGst` with a **rate of 0 and a name that says VAT**, because the rate is
state-specific and the shop must set it. Liquor defaults to `Inclusive` because
bar menus quote the price the customer pays.

**These are a starting point, not a list in the source that a support call has to
change** — the same argument the permissions and correction reasons already make.

## 5.6 The setup wizard's hardcoded slabs

`ui/src/setup/FirstRun.tsx:601-606` hardcodes a fourth copy of the slab list:

```tsx
{ value: 'tax_food_5',      label: 'Restaurant food 5%' },
{ value: 'tax_packaged_12', label: 'Packaged 12%' },      // slab abolished
{ value: 'tax_packaged_18', label: 'Packaged 18%' },
{ value: 'tax_liquor',      label: 'Liquor — outside GST' },
```

Replace with a `menu_tax_classes` call, so the wizard offers **the shop's actual
classes**. There must be exactly one list of tax classes in the product and it
must come from the database.

## 5.7 HSN and SAC

Audit §3.11. `hsn: Option<String>` is unvalidated free text.

* Validate the digit count: 2, 4, 6 or 8 digits, numeric. Reject anything else
  with a sentence.
* Add a shop-level **annual turnover band** to settings (up to ₹5 crore / above),
  and warn — not refuse — when an item's HSN is shorter than that band requires.
  Above ₹5 crore needs 6 digits; up to it, 4 on B2B and optional on B2C.
* Ship **SAC 996331** as the default for the restaurant-food class, since that is
  what a restaurant's own service is.

## 5.8 Tests for Phase 5

```
a_tax_class_round_trips_its_kind_without_going_through_a_label
changing_a_class_label_does_not_change_what_it_taxes     <- the §5.1 fault
an_exempt_class_cannot_be_given_a_rate
a_liquor_class_asks_for_a_vat_rate
editing_a_class_moves_every_item_that_uses_it
editing_a_class_does_not_touch_a_past_bill
editing_a_class_does_not_touch_a_line_on_an_open_order
the_wizard_offers_the_shops_own_classes
an_hsn_of_three_digits_is_refused
```

`changing_a_class_label_does_not_change_what_it_taxes` is the one that would fail
today. Write it first.

---

# Phase 6 — Print. `crates/mb-print`

## 6.1 The document must match the registration

`title_of` (`template/bill.rs:300`) reads `is_composition` and picks the title —
and that is **all** the flag does today. The tax block below it
(`bill.rs:712-719`) prints CGST/SGST regardless.

Now that Phase 3 puts `registration` on the `Bill` itself, the template reads it
from there — frozen with the bill, not from live settings — and:

* `Regular` → "TAX INVOICE", GST lines as today.
* `Composition` → "BILL OF SUPPLY", **no tax lines at all**, and the declaration
  printed, not optional.
* `Unregistered` → no title, no tax lines, no GSTIN row.

**The template must not be able to print a tax line on a bill of supply.** Make
it structural: have the bill hand the template a rendered tax block that is
already empty, rather than have the template decide. A renderer that makes a tax
decision is a second tax path (R2, D2).

## 6.2 The VAT line

Beside the GST rows:

```
Subtotal                              300.00
CGST 2.5%                               7.50
SGST 2.5%                               7.50
VAT 20%                                50.00        <- new, its own row
------------------------------------------
TOTAL                                 365.00
```

* Its own row, its own label carrying the rate, never merged into a GST figure.
* `Non-GST value` (`bill.rs:737`) stays — it is the taxable base the VAT was
  charged on, and an excise officer reads it.
* If the beer was priced VAT-inclusive, the VAT is a memo line, exactly as P32
  did for inclusive GST: `(includes VAT 50.00)`.

## 6.3 UTGST

`bill.rs:716` hardcodes `"SGST"`. Read the frozen `state_tax` off the bill and
print `StateTax::label()`.

## 6.4 The rate-wise summary

Rule 46 wants the rate and the tax amount per rate. `rate_label` (`bill.rs:783`)
already prints a single rate when the whole bill is at one rate, and nothing when
it is mixed. For a mixed bill, print the actual rate-wise table — the data is
already computed in `TaxSummary` and thrown away by the template.

Keep it behind the existing `receipt.show.*` settings so a 2-inch roll can turn
it off.

## 6.5 Tests for Phase 6

Golden files, in `crates/mb-print/tests/golden/`. The existing ones
(`bill-58mm.txt`, `bill-80mm.txt`, `bill-100mm.txt`, `bill-a4.txt`) all change,
which is expected and must be reviewed line by line rather than blessed.

New goldens:

```
bill-of-supply-80mm.txt          composition: title, declaration, no tax lines
unregistered-80mm.txt            no title, no GSTIN, no tax
bar-bill-80mm.txt                food at 5% and beer at 20% VAT on one bill
utgst-80mm.txt                   a Chandigarh shop
```

Plus a test that is not a golden:

```
a_bill_of_supply_can_never_contain_the_word_cgst
```

Render every composition permutation the generator can make and assert the string
`CGST` never appears. That is the illegal-document bug turned into a guard.

---

# Phase 7 — Reports

## 7.1 GST and VAT must never be added together

`Reports::tax_by_rate` (`repo/reports.rs:371`) sums `bl.cgst`, `bl.sgst`,
`bl.igst` grouped by `rate_bp`. Once liquor has a VAT rate, a naive version of
this would put a 20% VAT row in the middle of a GST report and a CA would file
it.

* `tax_by_rate` filters to `kind = 'gst'`. **Explicitly, in the SQL**, not by
  relying on VAT being in a different column.
* `tax_by_hsn` (`reports.rs:415`) does the same.
* New `Reports::vat_by_rate` for the liquor register.

## 7.2 The liquor register

Audit §2.4: a bar keeps two parallel ledgers, and an excise officer inspects the
liquor one separately. Minimum useful version:

* date, bill number, item, quantity, taxable value, VAT rate, VAT amount;
* a period total;
* exportable as CSV, because that is what gets handed to an accountant.

The owner asked how far to take this (audit §7.2). **Build the register in this
round; leave daily stock reconciliation for a later one** — that is a stock
feature, not a tax feature, and `mb-db` already has a stock module it belongs in.

## 7.3 Composition turnover

A composition dealer pays 5% on turnover. It never appeared on a bill, so the
only place the shop can see it is a report. Add the taxable turnover for the
period and the 5% it implies, with a line saying it is an estimate and the return
is the CA's job.

## 7.4 GSTR-1 export

The audit asked whether the on-screen report is enough (§7.3). The data is
already there — `tax_by_rate` and `tax_by_hsn` are exactly the two tables GSTR-1
is built from. **Add a CSV export of both** in this round; a JSON return-file
format is a separate piece of work with its own schema to track.

## 7.5 Tests for Phase 7

```
a_liquor_sale_never_appears_in_the_gst_report
a_liquor_sale_appears_in_the_vat_register
the_gst_report_and_the_bills_agree_to_the_paisa
a_composition_shops_report_shows_turnover_and_no_collected_tax
```

The third should be a property test over a generated month of bills.

---

# Phase 8 — Where inter-state is actually real

`PlaceOfSupply` survives Phase 4.3's deletion for two reasons, and both are
currently missing.

## 8.1 Stock moved between outlets in different states

`crates/mb-core/src/transfer.rs` carries **no tax at all**. A transfer between two
outlets of the same business in different states is a supply between **distinct
persons** under Schedule I of the CGST Act: it attracts **IGST**, needs a tax
invoice, and needs an e-way bill above ₹50,000.

This is the thing the owner was reaching for with *"a restaurant chain in many
states"*. It is real, and it is here — not on a customer's bill.

Build: a transfer between outlets whose `store_profile.state_code` differs
computes IGST at the item's rate, produces a document, and is flagged for an
e-way bill above the threshold.

## 8.2 Catering performed in another state

Under IGST Act s.12(4) the place of supply is where the service is performed. For
an outdoor catering order at an event in another state, that is the event's
state, and the supply is inter-state.

Build: `PlaceOfSupply` becomes a **per-order** field on a catering/event order
only, with the destination state chosen when the order is created. Never a shop
default, and never reachable from an ordinary dine-in or parcel bill.

## 8.3 Tests for Phase 8

```
a_transfer_within_one_state_carries_no_igst
a_transfer_across_a_state_line_carries_igst
a_transfer_above_fifty_thousand_is_flagged_for_an_eway_bill
a_dine_in_bill_can_never_be_inter_state
a_catering_order_in_another_state_is_inter_state
```

The fourth is the guard on audit §3.6 — it must be **impossible**, not merely
discouraged, to charge IGST on a dine-in bill.

---

# Phase 9 — Everything that reads tax, swept

Phases 1–8 change the model. This phase makes sure nothing was missed, because
`TaxTreatment` is referenced in **49 files** — verified — and the ones nobody
thinks about are where a rework rots.

## 9.1 The list

Work it as a checklist, not a search-and-replace.

| Area | Files |
|---|---|
| Core | `tax.rs`, `taxclass.rs`, `bill.rs`, `charge.rs`, `item.rs`, `transfer.rs`, `lib.rs` |
| Storage | `encode.rs` (`tax_treatment_to_sql`/`_from_sql` at `:174,186` become kind + basis), `repo/menu.rs`, `repo/menucsv.rs`, `repo/order.rs`, `repo/taxclass.rs`, `repo/reports.rs`, `repo/settings.rs`, `schema.rs` |
| Counter | `billing.rs` (19 mentions), `ipc.rs` (11), `menu.rs` (11), `flows.rs`, `search.rs`, `reports.rs`, `setup.rs`, `settings/*` |
| Print | `template/bill.rs`, `template/delivery.rs`, `testprint.rs`, `settings.rs`, `doc.rs` |
| Screens | `Menu.tsx`, `FirstRun.tsx`, `Settings.tsx`, `Billing.tsx`, `Totals.tsx`, `Receipt.tsx`, `Reports.tsx` |
| Generated | `TaxClassView.ts`, `TaxRowView.ts`, and anything `ts-rs` re-emits |
| Tests & fixtures | 20+ files including `tests/common/shop.rs`, `settings/sample.rs`, `look_demo.rs`, every `*_tests.rs` |

`crates/mb-db/src/repo/menucsv.rs` deserves naming: **CSV import writes tax
treatment from a spreadsheet column.** Its header, its parser and its error
messages all change, and a shop's existing import file will stop matching. Decide
the column names deliberately and document them in the import screen's help.

## 9.2 The sample bill and the demo

`settings/sample.rs:116` builds the bill the print preview shows, and
`look_demo.rs` builds the screens for design review. **P32's lesson was that a
preview whose sample takes a different path from the real print hides bugs.**

The sample must include a liquor line and be renderable as all three
registrations, or the preview cannot show the owner what a bill of supply looks
like before they print one.

## 9.3 Wiring

`node scripts/audit-wiring.mjs` currently passes at 240 commands. Every new
command — the VAT register, the CSV exports, the registration setting — needs a
button that reaches it. P31 found 29 commands with no button; do not add the
30th.

---

# Phase 10 — The guards, so this cannot come back

The owner's actual requirement is not "fix tax". It is *"old things should not
make problem later"*. That means leaving behind mechanisms, not just corrections.

## 10.1 A lint: no setting may be unread

Phase 4.1 deletes two settings that were drawn on screen and read by nothing. The
same thing will happen again unless something checks.

Write `scripts/check-settings-read.mjs`, in the style of `check-tokens.mjs` and
`check-layout.mjs` (no dependencies, fails the build):

* every key declared in `settings/catalog.rs` must appear at least once more in
  the tree, outside `catalog.rs` and `settings/mod.rs`;
* an exception list with a written reason, like the existing lints have.

This is the general form of the bug the owner reported. It is worth more than the
specific fix.

## 10.2 A lint: no tax slab in the source

Add to the same script, or its own:

* refuse a literal percentage tax rate in `ui/src/**` — the slab lists at
  `FirstRun.tsx:601` are exactly what must not come back;
* refuse a named rate constant in `mb-core` outside tests.

The owner's ruling was *"do not hardcode the app to any tax slab"*. A ruling that
is only in a document gets forgotten in nineteen sessions. `check-tokens.mjs`
already proved this works — nineteen sessions and not one raw colour crept back.

## 10.3 The reconciliation property, permanently

One property test, run over generated bills, asserting for every bill:

```
subtotal − discount + charges + gst_added + vat_added + round_off == grand_total
gst.central + gst.state + gst.integrated == total_gst
vat is never inside any gst figure
a bill of supply contains no tax
```

`bill.rs:589` already has the generator. Extend it with liquor, composition and
union territories, and run it over a few thousand bills in CI.

---

# What "done" means

Every one of these, before the round is reported:

* `cargo test --workspace` green, `cargo clippy --workspace --all-targets` clean.
* `npm run check` green — typecheck, all six lints, all tests.
* `node scripts/audit-wiring.mjs` clean.
* The new lints from Phase 10 in place and passing.
* **`every_old_bill_prints_exactly_the_same_after_the_migration` passing** — the
  crown-jewel-4 test from Phase 2.7.
* The app **driven on the real Tauri window** via `scripts/drive.mjs` against a
  fresh install: set up a shop each way (regular, composition, unregistered), add
  a food item and a liquor item, bill both, and look at the paper.
* Four bills printed and read by a person: a regular tax invoice, a bill of
  supply, an unregistered bill, and a bar bill with food and beer on it.

## Report once, at the end

Say what was built, what the numbers are before and after, and — following P32's
example — a section on **what the build learned that this prompt did not know**.
Where the plan was wrong, record the difference there rather than editing it into
the plan above.
