//! GST, and the tax that is not GST.
//!
//! Tax belongs to a line, never a bill (D3): one bill holds 5% food, an MRP
//! bottle taxed inside its price, and a beer outside GST.
//!
//! P33 replaced `TaxTreatment`, which answered two questions at once — "is the
//! tax inside the price?" and "what kind of supply is this?". Jammed together,
//! liquor had nowhere to put a rate, so `compute_line` charged zero and every
//! bar undercharged every drink. They are now [`PriceBasis`] and [`TaxKind`].
//!
//! [`GstAmounts`] and [`Vat`] are separate types on purpose: nothing here can
//! fold state VAT into a GST figure, so a return cannot report liquor as GST.

use crate::money::{Money, MoneyError};
use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, MoneyError>;

/// A tax rate in basis points: `500` is 5.00%, `250` is 2.50%.
///
/// Basis points, not a float — 2.5% and 0.25% are real rates and neither
/// survives binary floating point. No slab constants: a rate is data, and
/// `GST_12`/`GST_28` went stale when both slabs were abolished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaxRate(u32);

impl TaxRate {
    pub const ZERO: TaxRate = TaxRate(0);

    /// Rates above 100% are always a data-entry error, so they are refused.
    #[must_use]
    pub const fn from_basis_points(bp: u32) -> Option<Self> {
        if bp > 10_000 { None } else { Some(TaxRate(bp)) }
    }

    /// `5` -> 5%. The everyday case.
    #[must_use]
    pub const fn from_percent(percent: u32) -> Option<Self> {
        match percent.checked_mul(100) {
            Some(bp) => Self::from_basis_points(bp),
            None => None,
        }
    }

    #[must_use]
    pub const fn basis_points(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// `"5%"`, `"2.5%"`, `"18%"` — for the receipt and the summary table.
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "splitting basis points into whole and fractional percent"
    )]
    pub fn label(self) -> String {
        let whole = self.0 / 100;
        let frac = self.0 % 100;
        if frac == 0 {
            format!("{whole}%")
        } else if frac.is_multiple_of(10) {
            format!("{whole}.{}%", frac / 10)
        } else {
            format!("{whole}.{frac:02}%")
        }
    }
}

/// What kind of supply this is, in the law's terms — which box of a GST return
/// the value goes in. Not a rate and not a pricing convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaxKind {
    /// Normal GST at the line's rate. The everyday case.
    #[default]
    Gst,
    /// Inside GST at nil rate — in the return, with no tax.
    Exempt,
    /// Outside GST entirely — alcohol (Constitution, Art. 366(12A)). Not
    /// untaxed: it carries **state VAT** on its own channel, never in a GST
    /// return.
    OutsideGst,
    /// No tax of any kind — a deposit, a returnable container. Unlike
    /// [`TaxKind::Exempt`], not a supply in the return at all.
    Untaxed,
}

impl TaxKind {
    /// Does a line of this kind ever carry GST?
    #[must_use]
    pub const fn is_gst(self) -> bool {
        matches!(self, TaxKind::Gst)
    }

    /// Does a line of this kind carry state VAT?
    #[must_use]
    pub const fn is_vat(self) -> bool {
        matches!(self, TaxKind::OutsideGst)
    }
}

/// Is the tax already inside the price the shop typed?
///
/// Independent of [`TaxKind`], and it has to be: MRP is tax-inclusive by law,
/// so an MRP bottle and a tax-on-top dosa sit on the same bill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceBasis {
    /// Tax is added on top of the price. The dosa.
    #[default]
    Exclusive,
    /// Tax is already contained in the price and is worked backwards out of it.
    Inclusive,
}

impl PriceBasis {
    #[must_use]
    pub const fn is_inclusive(self) -> bool {
        matches!(self, PriceBasis::Inclusive)
    }
}

/// Everything about one line's tax, in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TaxSpec {
    pub kind: TaxKind,
    /// GST rate for [`TaxKind::Gst`], **state VAT rate** for
    /// [`TaxKind::OutsideGst`], zero otherwise. One field, because a line is
    /// never both.
    pub rate: TaxRate,
    pub basis: PriceBasis,
}

impl TaxSpec {
    /// The everyday line: GST at this rate, added on top of the price.
    #[must_use]
    pub const fn gst(rate: TaxRate) -> Self {
        TaxSpec { kind: TaxKind::Gst, rate, basis: PriceBasis::Exclusive }
    }

    /// GST at this rate, already inside the price.
    #[must_use]
    pub const fn gst_inclusive(rate: TaxRate) -> Self {
        TaxSpec { kind: TaxKind::Gst, rate, basis: PriceBasis::Inclusive }
    }

    /// Alcohol at a state VAT rate, priced tax-in — a bar quotes what is paid.
    #[must_use]
    pub const fn liquor(vat: TaxRate) -> Self {
        TaxSpec { kind: TaxKind::OutsideGst, rate: vat, basis: PriceBasis::Inclusive }
    }

    /// A nil-rated supply. It has a taxable value and no tax.
    #[must_use]
    pub const fn exempt() -> Self {
        TaxSpec { kind: TaxKind::Exempt, rate: TaxRate::ZERO, basis: PriceBasis::Exclusive }
    }

    /// No tax of any kind.
    #[must_use]
    pub const fn untaxed() -> Self {
        TaxSpec { kind: TaxKind::Untaxed, rate: TaxRate::ZERO, basis: PriceBasis::Exclusive }
    }

    /// A rate on a kind that cannot carry one is a mistake, not a nuance.
    /// Reported rather than silently zeroed (D7, D15).
    #[must_use]
    pub const fn is_coherent(self) -> bool {
        match self.kind {
            TaxKind::Gst | TaxKind::OutsideGst => true,
            TaxKind::Exempt | TaxKind::Untaxed => self.rate.is_zero(),
        }
    }
}

/// **What kind of taxpayer this shop is.** The gate on the whole tax pipeline.
///
/// The idea the product was missing. It lives in `mb-core` because it is a
/// **rule**, not a settings value (R8, D1): "may this shop put GST on a bill?"
/// is the same class of question as "what does this class mean for this order?",
/// and rules do not live in SQL or in React.
///
/// Audit §3.2 is what it is for. `is_composition` used to change nothing but the
/// bill's title, so a composition dealer's bill printed **"BILL OF SUPPLY"** with
/// CGST and SGST lines underneath it — a contradiction in law, on real paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Registration {
    /// Below the registration threshold. No GSTIN, and **no GST may be charged
    /// or shown on anything**.
    Unregistered,
    /// Composition scheme. Has a GSTIN, pays its 5% out of its own margin, and
    /// **may not collect or display GST**. Issues a bill of supply.
    Composition,
    /// Regular. Collects GST and issues a tax invoice.
    #[default]
    Regular,
}

impl Registration {
    /// The gate: only a regular taxpayer may put GST on a customer's bill.
    #[must_use]
    pub const fn charges_gst(self) -> bool {
        matches!(self, Registration::Regular)
    }

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

    /// A registered shop has a GST number; an unregistered one must not claim
    /// to.
    #[must_use]
    pub const fn needs_gstin(self) -> bool {
        !matches!(self, Registration::Unregistered)
    }

    /// What the document is called. `None` for an unregistered shop — a title
    /// it has no right to is worse than none.
    #[must_use]
    pub const fn document_title(self) -> Option<&'static str> {
        match self {
            Registration::Regular => Some("TAX INVOICE"),
            Registration::Composition => Some("BILL OF SUPPLY"),
            Registration::Unregistered => None,
        }
    }
}

/// Where the supply happens, which decides how the GST is named.
///
/// Not a shop-wide setting: restaurant service is supplied where the food is
/// served (IGST Act s.12(4)), so a sit-down bill is always `Intra`. `Inter` is
/// for stock moved across a state line and for catering at an event elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceOfSupply {
    /// Same state as the outlet: the tax splits into CGST + SGST/UTGST.
    #[default]
    Intra,
    /// Another state: the whole tax is IGST.
    Inter,
}

/// What the state half of an intra-state supply is called.
///
/// Union territories without a legislature use **UTGST**. Delhi, Puducherry and
/// J&K have one, so they use SGST.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateTax {
    #[default]
    Sgst,
    Utgst,
}

impl StateTax {
    /// From the two-digit GST state code. Short, closed, set by law.
    #[must_use]
    pub fn for_state_code(code: &str) -> Self {
        match code.trim() {
            "04" | "26" | "31" | "35" | "38" => StateTax::Utgst,
            _ => StateTax::Sgst,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            StateTax::Sgst => "SGST",
            StateTax::Utgst => "UTGST",
        }
    }
}

/// One line's GST, already named. **Never contains VAT.**
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GstAmounts {
    /// CGST.
    pub central: Money,
    /// SGST, or UTGST in a union territory without a legislature. One field
    /// because it is one number; [`StateTax`] decides what it is called.
    pub state: Money,
    /// IGST.
    pub integrated: Money,
}

impl GstAmounts {
    pub fn total(self) -> Result<Money> {
        Money::try_sum([self.central, self.state, self.integrated])
    }

    #[allow(clippy::should_implement_trait, reason = "addition here must be able to fail (D7)")]
    pub fn add(self, other: Self) -> Result<Self> {
        Ok(GstAmounts {
            central: self.central.add(other.central)?,
            state: self.state.add(other.state)?,
            integrated: self.integrated.add(other.integrated)?,
        })
    }

    /// What is left after taking `other` out — P32. A printed bill separates
    /// tax added on top from tax already inside a price, or an inclusive line
    /// is counted twice.
    #[allow(clippy::should_implement_trait, reason = "subtraction here must be able to fail (D7)")]
    pub fn sub(self, other: Self) -> Result<Self> {
        Ok(GstAmounts {
            central: self.central.sub(other.central)?,
            state: self.state.sub(other.state)?,
            integrated: self.integrated.sub(other.integrated)?,
        })
    }

    #[must_use]
    pub fn is_zero(self) -> bool {
        self.central.is_zero() && self.state.is_zero() && self.integrated.is_zero()
    }
}

/// **State VAT on alcohol. Not GST, and structurally cannot become GST.**
///
/// A newtype rather than a bare [`Money`] so that no `Money::try_sum` anywhere
/// in this workspace can fold it into a GST figure by accident. Taking it out
/// takes a deliberate [`Vat::into_money`], which is a line a reviewer can see
/// and a grep can find.
///
/// This is the whole reason a bar can be billed now: the old model had one tax
/// channel, so the only way to give liquor a rate would have been to put it in
/// the GST fields, and it would have been filed as GST.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Vat(Money);

impl Vat {
    pub const ZERO: Vat = Vat(Money::ZERO);

    #[must_use]
    pub const fn new(amount: Money) -> Self {
        Vat(amount)
    }

    /// Deliberate, and named so that it reads as a decision at the call site.
    #[must_use]
    pub const fn into_money(self) -> Money {
        self.0
    }

    #[allow(clippy::should_implement_trait, reason = "addition here must be able to fail (D7)")]
    pub fn add(self, other: Self) -> Result<Self> {
        Ok(Vat(self.0.add(other.0)?))
    }

    #[allow(clippy::should_implement_trait, reason = "subtraction here must be able to fail (D7)")]
    pub fn sub(self, other: Self) -> Result<Self> {
        Ok(Vat(self.0.sub(other.0)?))
    }

    #[must_use]
    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }
}

/// What one line contributes to the bill and to the return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TaxOutcome {
    /// What tax is charged on. Less than the price for an inclusive line.
    /// Computed even when no tax is charged: a composition dealer's own 5% is
    /// paid on this.
    pub taxable: Money,
    pub gst: GstAmounts,
    pub vat: Vat,
    /// `taxable + tax`. For an inclusive line this equals the line net exactly.
    pub gross: Money,
    pub spec: TaxSpec,
}

impl TaxOutcome {
    /// Every tax on this line as one figure. Only for what the customer pays,
    /// never for a GST return — it folds VAT in beside GST.
    pub fn tax_total(self) -> Result<Money> {
        self.gst.total()?.add(self.vat.into_money())
    }
}

/// Compute the tax for one line from its **already discounted** net amount.
///
/// The input is the net *after* line discount and after that line's share of any
/// bill-level discount — decision D4, and section 15(3)(a) of the CGST Act says
/// the same thing: a discount shown on the invoice reduces the taxable value.
/// Passing an undiscounted amount here produces tax on money the customer never
/// paid.
///
/// # The rules, in order
///
/// 1. **What tax, if any.** `Untaxed` charges nothing. `Exempt` charges nothing
///    but keeps a taxable value. `OutsideGst` charges **VAT** — and is never
///    gated by `registration`, because a liquor licence is a state excise
///    registration and has nothing to do with GST. `Gst` charges GST **only if
///    the shop is a regular taxpayer**.
/// 2. **Split the price by its basis.** For an inclusive line the tax is the
///    REMAINDER after the taxable value, never a second rounded multiplication,
///    so the line always totals to its menu price exactly. That holds for VAT as
///    well as GST.
/// 3. **Name the GST.** Intra splits with `halve_exact` so the two halves always
///    sum to the whole; inter is all IGST. VAT is never split and never named.
pub fn compute_line(
    net: Money,
    spec: TaxSpec,
    place: PlaceOfSupply,
    registration: Registration,
) -> Result<TaxOutcome> {
    // Step 1 — is there a rate to apply at all, and to which channel?
    let charged = match spec.kind {
        TaxKind::Untaxed | TaxKind::Exempt => TaxRate::ZERO,
        TaxKind::OutsideGst => spec.rate,
        // **The composition gate.** The taxable value below is still computed;
        // only the tax is refused.
        TaxKind::Gst if registration.charges_gst() => spec.rate,
        TaxKind::Gst => TaxRate::ZERO,
    };

    // Step 2 — split the price.
    let (taxable, amount) = if charged.is_zero() {
        (net, Money::ZERO)
    } else {
        match spec.basis {
            PriceBasis::Exclusive => (net, net.percent_bp(charged.basis_points())?),
            PriceBasis::Inclusive => {
                // taxable = net × 10000 / (10000 + rate)
                //
                // The tax is then the REMAINDER, not a second rounded
                // multiplication. That guarantees `taxable + tax == net`
                // exactly, so an inclusive-priced item always totals to its menu
                // price — which is the entire point of inclusive pricing.
                let denominator = i64::from(charged.basis_points()).saturating_add(10_000);
                let taxable = net.mul_ratio(10_000, denominator)?;
                let tax = net.sub(taxable)?;
                (taxable, tax)
            }
        }
    };

    // Step 3 — name it. VAT is never named and never split.
    let (gst, vat) = if spec.kind.is_vat() {
        (GstAmounts::default(), Vat::new(amount))
    } else {
        let named = match place {
            PlaceOfSupply::Intra => {
                // halve_exact so central + state is always the whole tax, to the
                // paisa.
                let (central, state) = amount.halve_exact();
                GstAmounts { central, state, integrated: Money::ZERO }
            }
            PlaceOfSupply::Inter => GstAmounts {
                central: Money::ZERO,
                state: Money::ZERO,
                integrated: amount,
            },
        };
        (named, Vat::ZERO)
    };

    Ok(TaxOutcome {
        taxable,
        gst,
        vat,
        gross: taxable.add(amount)?,
        spec,
    })
}

/// One row of the rate-wise summary printed on the bill and filed in GSTR-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RateSummaryRow {
    pub rate: TaxRate,
    pub taxable: Money,
    pub gst: GstAmounts,
}

/// One rate of state VAT — the liquor register, which is a different book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VatSummaryRow {
    pub rate: TaxRate,
    pub taxable: Money,
    pub vat: Vat,
}

/// Taxable value and tax, grouped by rate — what a GST return is built from.
///
/// Liquor is not in it; it is in [`TaxSummary::vat_rows`]. A bar keeps two
/// books and an excise officer reads the second one.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TaxSummary {
    /// Sorted by rate, so rows print in ascending order.
    ///
    /// A `Vec`, not a map: JSON object keys are strings, so a `BTreeMap<u32, _>`
    /// cannot round-trip through serde's tag buffering, which an
    /// internally-tagged enum like [`AnyOrder`](crate::order::AnyOrder) uses.
    rows: Vec<RateSummaryRow>,
    /// The same shape for state VAT, and deliberately a separate list.
    vat: Vec<VatSummaryRow>,
    /// Value of supplies outside GST. The base the VAT was charged on.
    pub non_gst_value: Money,
    /// Value of nil-rated supplies. Enters the return with zero tax.
    pub exempt_value: Money,
    /// Value of supplies carrying no tax of any kind.
    pub untaxed_value: Money,
}

impl TaxSummary {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, outcome: TaxOutcome) -> Result<()> {
        match outcome.spec.kind {
            TaxKind::OutsideGst => {
                self.non_gst_value = self.non_gst_value.add(outcome.taxable)?;
                let bp = outcome.spec.rate.basis_points();
                let index = match self.vat.binary_search_by_key(&bp, |r| r.rate.basis_points()) {
                    Ok(found) => found,
                    Err(insert_at) => {
                        self.vat.insert(
                            insert_at,
                            VatSummaryRow { rate: outcome.spec.rate, ..VatSummaryRow::default() },
                        );
                        insert_at
                    }
                };
                let row = &mut self.vat[index];
                row.taxable = row.taxable.add(outcome.taxable)?;
                row.vat = row.vat.add(outcome.vat)?;
            }
            TaxKind::Exempt => {
                self.exempt_value = self.exempt_value.add(outcome.taxable)?;
            }
            TaxKind::Untaxed => {
                self.untaxed_value = self.untaxed_value.add(outcome.taxable)?;
            }
            TaxKind::Gst => {
                let bp = outcome.spec.rate.basis_points();
                let index = match self.rows.binary_search_by_key(&bp, |r| r.rate.basis_points()) {
                    Ok(found) => found,
                    Err(insert_at) => {
                        // Inserted in place, so `rows` stays in ascending rate
                        // order without ever being sorted.
                        self.rows.insert(
                            insert_at,
                            RateSummaryRow { rate: outcome.spec.rate, ..RateSummaryRow::default() },
                        );
                        insert_at
                    }
                };
                let row = &mut self.rows[index];
                row.taxable = row.taxable.add(outcome.taxable)?;
                row.gst = row.gst.add(outcome.gst)?;
            }
        }
        Ok(())
    }

    /// GST rows, in ascending rate order.
    pub fn rows(&self) -> impl Iterator<Item = &RateSummaryRow> {
        self.rows.iter()
    }

    /// VAT rows, ascending. A different book from `rows`.
    pub fn vat_rows(&self) -> impl Iterator<Item = &VatSummaryRow> {
        self.vat.iter()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
            && self.vat.is_empty()
            && self.non_gst_value.is_zero()
            && self.exempt_value.is_zero()
            && self.untaxed_value.is_zero()
    }

    /// Taxable value across every GST rate. Liquor, exempt and untaxed have
    /// their own figures.
    pub fn total_taxable(&self) -> Result<Money> {
        Money::try_sum(self.rows.iter().map(|r| r.taxable))
    }

    pub fn total_gst(&self) -> Result<GstAmounts> {
        self.rows
            .iter()
            .try_fold(GstAmounts::default(), |acc, row| acc.add(row.gst))
    }

    pub fn total_vat(&self) -> Result<Vat> {
        self.vat.iter().try_fold(Vat::ZERO, |acc, row| acc.add(row.vat))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rs(rupees: i64) -> Money {
        Money::from_paise(rupees * 100)
    }

    fn pc(percent: u32) -> TaxRate {
        TaxRate::from_percent(percent).expect("a real rate")
    }

    fn bp(basis_points: u32) -> TaxRate {
        TaxRate::from_basis_points(basis_points).expect("a real rate")
    }

    /// Every rate a real Indian restaurant or bar meets. **Not constants in the
    /// shipping code** — the owner ruled slabs out of the source, so the test
    /// carries its own list.
    const REAL_RATES: [u32; 6] = [500, 1_800, 2_000, 2_500, 3_500, 4_000];

    // ---- the everyday cases ------------------------------------------------

    #[test]
    fn exclusive_adds_tax_on_top() {
        let out = compute_line(rs(100), TaxSpec::gst(pc(5)), PlaceOfSupply::Intra, Registration::Regular)
            .expect("computes");
        assert_eq!(out.taxable, rs(100));
        assert_eq!(out.gst.central, Money::from_paise(250));
        assert_eq!(out.gst.state, Money::from_paise(250));
        assert_eq!(out.gst.integrated, Money::ZERO);
        assert_eq!(out.vat, Vat::ZERO);
        assert_eq!(out.gross, Money::from_paise(10_500));
    }

    #[test]
    fn inclusive_extracts_tax_and_always_totals_to_the_menu_price() {
        // ₹105 inclusive of 5% -> taxable ₹100, tax ₹5, gross back to ₹105.
        let out = compute_line(
            Money::from_paise(10_500),
            TaxSpec::gst_inclusive(pc(5)),
            PlaceOfSupply::Intra,
            Registration::Regular,
        )
        .expect("computes");
        assert_eq!(out.taxable, rs(100));
        assert_eq!(out.gst.total(), Ok(rs(5)));
        assert_eq!(out.gross, Money::from_paise(10_500));
    }

    #[test]
    fn interstate_is_all_igst() {
        let out = compute_line(rs(100), TaxSpec::gst(pc(18)), PlaceOfSupply::Inter, Registration::Regular)
            .expect("computes");
        assert_eq!(out.gst.central, Money::ZERO);
        assert_eq!(out.gst.state, Money::ZERO);
        assert_eq!(out.gst.integrated, rs(18));
    }

    // ---- liquor: the defect this rework exists for -------------------------

    /// **THE BUG P33 WAS BUILT FOR.**
    ///
    /// A ₹250 beer in Karnataka carries 20% state VAT. The old model computed
    /// `NonGst => Money::ZERO` and charged **nothing**, so every bar running
    /// Magic Bill undercharged every drink by a fifth.
    #[test]
    fn a_bar_in_karnataka_charges_twenty_per_cent_vat_on_a_beer() {
        // Priced tax-in, the way a bar menu quotes it: ₹250 all-in.
        let out = compute_line(
            rs(250),
            TaxSpec::liquor(pc(20)),
            PlaceOfSupply::Intra,
            Registration::Regular,
        )
        .expect("computes");

        assert!(!out.vat.is_zero(), "a beer with no VAT is the old bug, restored");
        // 250 × 100/120 = 208.33, VAT is the remainder = 41.67
        assert_eq!(out.taxable, Money::from_paise(20_833));
        assert_eq!(out.vat.into_money(), Money::from_paise(4_167));
        assert_eq!(out.gross, rs(250), "an inclusive price must total to itself");
    }

    #[test]
    fn liquor_never_puts_anything_in_a_gst_field() {
        for rate in [20, 25, 35] {
            let out = compute_line(
                rs(500),
                TaxSpec::liquor(pc(rate)),
                PlaceOfSupply::Intra,
                Registration::Regular,
            )
            .expect("computes");
            assert!(out.gst.is_zero(), "{rate}% VAT leaked into a GST field");
            assert_eq!(out.gst.total(), Ok(Money::ZERO));
        }
    }

    #[test]
    fn vat_never_appears_in_a_gst_total() {
        let mut summary = TaxSummary::new();
        for (net, spec) in [
            (rs(400), TaxSpec::gst(pc(5))),
            (rs(250), TaxSpec::liquor(pc(20))),
        ] {
            let out = compute_line(net, spec, PlaceOfSupply::Intra, Registration::Regular)
                .expect("computes");
            summary.add(out).expect("adds");
        }

        // The GST side sees only the food.
        assert_eq!(summary.total_taxable(), Ok(rs(400)));
        assert_eq!(summary.total_gst().and_then(GstAmounts::total), Ok(rs(20)));
        assert_eq!(summary.rows().count(), 1, "liquor must not be a GST rate");

        // The VAT side sees only the beer.
        assert_eq!(summary.vat_rows().count(), 1);
        assert!(!summary.total_vat().expect("sums").is_zero());
        assert_eq!(summary.non_gst_value, Money::from_paise(20_833));
    }

    #[test]
    fn a_bar_bill_keeps_two_books_at_once() {
        let mut summary = TaxSummary::new();
        for (net, spec) in [
            (rs(400), TaxSpec::gst(pc(5))),
            (rs(200), TaxSpec::gst(pc(18))),
            (rs(250), TaxSpec::liquor(pc(20))),
            (rs(300), TaxSpec::liquor(pc(35))),
            (rs(100), TaxSpec::exempt()),
            (rs(50), TaxSpec::untaxed()),
        ] {
            let out = compute_line(net, spec, PlaceOfSupply::Intra, Registration::Regular)
                .expect("computes");
            summary.add(out).expect("adds");
        }

        assert_eq!(summary.rows().count(), 2, "two GST rates");
        assert_eq!(summary.vat_rows().count(), 2, "two VAT rates, kept apart");
        assert_eq!(summary.total_taxable(), Ok(rs(600)), "GST taxable excludes liquor");
        assert_eq!(summary.exempt_value, rs(100));
        assert_eq!(summary.untaxed_value, rs(50));

        // The two VAT rates are in ascending order, like the GST rows.
        let vat: Vec<_> = summary.vat_rows().copied().collect();
        assert_eq!(vat[0].rate, pc(20));
        assert_eq!(vat[1].rate, pc(35));
    }

    // ---- registration: the composition gate --------------------------------

    #[test]
    fn a_composition_shop_computes_taxable_value_but_charges_no_gst() {
        let out = compute_line(
            rs(100),
            TaxSpec::gst(pc(5)),
            PlaceOfSupply::Intra,
            Registration::Composition,
        )
        .expect("computes");

        assert!(out.gst.is_zero(), "a composition dealer may not collect GST");
        assert_eq!(out.gross, rs(100), "the customer pays the menu price");
        assert_eq!(
            out.taxable,
            rs(100),
            "but the turnover is still known — it is what the shop's own 5% is paid on"
        );
    }

    #[test]
    fn an_unregistered_shop_charges_no_gst_on_anything() {
        for rate in REAL_RATES {
            let out = compute_line(
                rs(100),
                TaxSpec::gst(bp(rate)),
                PlaceOfSupply::Intra,
                Registration::Unregistered,
            )
            .expect("computes");
            assert!(out.gst.is_zero(), "{rate}bp was charged by an unregistered shop");
            assert_eq!(out.gross, rs(100));
        }
    }

    /// **VAT is not gated by GST registration, and that is deliberate.**
    ///
    /// A liquor licence is a state excise registration. It has nothing to do
    /// with whether the shop is registered under GST, so tying them together
    /// would be a new bug wearing the old one's clothes.
    #[test]
    fn an_unregistered_shop_still_charges_vat_on_liquor() {
        let out = compute_line(
            rs(250),
            TaxSpec::liquor(pc(20)),
            PlaceOfSupply::Intra,
            Registration::Unregistered,
        )
        .expect("computes");
        assert!(!out.vat.is_zero(), "state VAT does not depend on GST registration");
    }

    #[test]
    fn the_composition_gate_does_not_touch_an_inclusive_price() {
        // A composition shop prices tax-in by necessity — it cannot add tax.
        let out = compute_line(
            rs(100),
            TaxSpec::gst_inclusive(pc(5)),
            PlaceOfSupply::Intra,
            Registration::Composition,
        )
        .expect("computes");
        assert_eq!(out.gross, rs(100));
        assert_eq!(out.taxable, rs(100), "with no tax charged, nothing is extracted");
    }

    #[test]
    fn only_a_regular_taxpayer_charges_gst() {
        assert!(Registration::Regular.charges_gst());
        assert!(!Registration::Composition.charges_gst());
        assert!(!Registration::Unregistered.charges_gst());
    }

    #[test]
    fn the_law_that_composition_cannot_do_is_written_down() {
        assert!(!Registration::Composition.may_sell_alcohol());
        assert!(!Registration::Composition.may_supply_interstate());
        assert!(Registration::Regular.may_sell_alcohol());
        assert!(Registration::Regular.may_supply_interstate());
        assert!(!Registration::Unregistered.needs_gstin());
        assert!(Registration::Composition.needs_gstin());
    }

    #[test]
    fn each_registration_names_its_own_document() {
        assert_eq!(Registration::Regular.document_title(), Some("TAX INVOICE"));
        assert_eq!(Registration::Composition.document_title(), Some("BILL OF SUPPLY"));
        assert_eq!(
            Registration::Unregistered.document_title(),
            None,
            "a title it has no right to is worse than none"
        );
    }

    // ---- the state half's name ---------------------------------------------

    #[test]
    fn a_chandigarh_shop_calls_the_state_half_utgst() {
        // 04 Chandigarh, 26 DNH & DD, 31 Lakshadweep, 35 A&N, 38 Ladakh.
        for code in ["04", "26", "31", "35", "38"] {
            assert_eq!(StateTax::for_state_code(code), StateTax::Utgst, "state {code}");
            assert_eq!(StateTax::for_state_code(code).label(), "UTGST");
        }
    }

    #[test]
    fn a_karnataka_shop_calls_the_state_half_sgst() {
        // 29 Karnataka, 27 Maharashtra, 07 Delhi, 34 Puducherry, 01 J&K — the
        // last three are union territories WITH a legislature, so they are SGST.
        for code in ["29", "27", "07", "34", "01", "33", "36"] {
            assert_eq!(StateTax::for_state_code(code), StateTax::Sgst, "state {code}");
            assert_eq!(StateTax::for_state_code(code).label(), "SGST");
        }
    }

    #[test]
    fn an_unknown_or_blank_state_code_falls_back_to_sgst() {
        // A shop that has not entered its state yet must not print UTGST.
        for code in ["", "  ", "99", "xx"] {
            assert_eq!(StateTax::for_state_code(code), StateTax::Sgst);
        }
    }

    // ---- exempt, untaxed, and the difference --------------------------------

    #[test]
    fn exempt_and_outside_gst_land_in_different_buckets() {
        let mut summary = TaxSummary::new();
        for spec in [TaxSpec::exempt(), TaxSpec::liquor(pc(20)), TaxSpec::untaxed()] {
            let out = compute_line(rs(200), spec, PlaceOfSupply::Intra, Registration::Regular)
                .expect("computes");
            summary.add(out).expect("adds");
        }
        // Exempt is a nil-rated SUPPLY and is in the return. Liquor is not a
        // supply under GST at all. Untaxed is neither.
        assert_eq!(summary.exempt_value, rs(200));
        assert_eq!(summary.untaxed_value, rs(200));
        assert!(!summary.non_gst_value.is_zero());
        assert_eq!(summary.rows().count(), 0);
        assert_eq!(summary.total_taxable(), Ok(Money::ZERO));
    }

    #[test]
    fn an_exempt_line_carries_a_taxable_value_and_no_tax() {
        let out = compute_line(rs(200), TaxSpec::exempt(), PlaceOfSupply::Intra, Registration::Regular)
            .expect("computes");
        assert_eq!(out.taxable, rs(200));
        assert_eq!(out.tax_total(), Ok(Money::ZERO));
        assert_eq!(out.gross, rs(200));
    }

    // ---- the mixed bill the owner's MRP objection was about -----------------

    /// **The owner's ruling of 2026-08-24, as a test.**
    ///
    /// A ₹20 MRP bottle is tax-inclusive by law; a ₹100 dosa is tax-exclusive.
    /// One shop-wide switch could not express this bill, which is why the
    /// pricing basis lives on the line.
    #[test]
    fn an_mrp_bottle_and_an_exclusive_dosa_on_one_bill_both_come_out_right() {
        let dosa = compute_line(
            rs(100),
            TaxSpec::gst(pc(5)),
            PlaceOfSupply::Intra,
            Registration::Regular,
        )
        .expect("computes");
        let water = compute_line(
            rs(20),
            TaxSpec::gst_inclusive(pc(18)),
            PlaceOfSupply::Intra,
            Registration::Regular,
        )
        .expect("computes");

        assert_eq!(dosa.gross, Money::from_paise(10_500), "tax added on top");
        assert_eq!(water.gross, rs(20), "MRP is what the customer pays, full stop");
        assert!(water.taxable < rs(20), "the tax came out of the MRP");

        let mut summary = TaxSummary::new();
        summary.add(dosa).expect("adds");
        summary.add(water).expect("adds");
        assert_eq!(summary.rows().count(), 2, "two rates, two rows");
    }

    // ---- the properties. These are the money guarantees. --------------------

    /// Ported from the old model. For ANY amount and ANY rate, an inclusive
    /// line's taxable + tax is exactly what the customer was quoted.
    #[test]
    fn the_inclusive_property_still_holds_for_gst() {
        for paise in 1..2_000_i64 {
            for rate in REAL_RATES {
                let net = Money::from_paise(paise);
                let out = compute_line(
                    net,
                    TaxSpec::gst_inclusive(bp(rate)),
                    PlaceOfSupply::Intra,
                    Registration::Regular,
                )
                .expect("computes");
                assert_eq!(out.gross, net, "{paise} paise at {rate}bp");
                assert_eq!(
                    out.taxable.add(out.gst.total().expect("sums")),
                    Ok(net),
                    "{paise} paise at {rate}bp did not reconcile"
                );
            }
        }
    }

    /// **The new channel gets the same guarantee.** A VAT-inclusive beer must
    /// total to its menu price for every amount, at every real liquor rate.
    #[test]
    fn the_inclusive_property_holds_for_vat() {
        for paise in 1..2_000_i64 {
            for rate in [2_000_u32, 2_500, 3_500] {
                let net = Money::from_paise(paise);
                let out = compute_line(
                    net,
                    TaxSpec::liquor(bp(rate)),
                    PlaceOfSupply::Intra,
                    Registration::Regular,
                )
                .expect("computes");
                assert_eq!(out.gross, net, "{paise} paise at {rate}bp VAT");
                assert_eq!(
                    out.taxable.add(out.vat.into_money()),
                    Ok(net),
                    "{paise} paise at {rate}bp VAT did not reconcile"
                );
                assert!(out.gst.is_zero(), "VAT leaked into GST at {paise} paise");
            }
        }
    }

    /// Ported. CGST + SGST is always the whole tax, to the paisa.
    #[test]
    fn the_halving_property_still_holds() {
        for paise in 1..3_000_i64 {
            let out = compute_line(
                Money::from_paise(paise),
                TaxSpec::gst(pc(5)),
                PlaceOfSupply::Intra,
                Registration::Regular,
            )
            .expect("computes");
            let halves = out.gst.central.add(out.gst.state).expect("sums");
            assert_eq!(halves, out.gst.total().expect("sums"), "at {paise} paise");
            assert_eq!(out.taxable.add(halves), Ok(out.gross));
        }
    }

    /// Whatever the kind, the outcome reconciles: taxable plus every tax on the
    /// line equals the gross. Run across the whole cross-product.
    #[test]
    fn every_line_reconciles_whatever_its_kind() {
        let specs = [
            TaxSpec::gst(pc(5)),
            TaxSpec::gst_inclusive(pc(18)),
            TaxSpec::liquor(pc(20)),
            TaxSpec { kind: TaxKind::OutsideGst, rate: pc(25), basis: PriceBasis::Exclusive },
            TaxSpec::exempt(),
            TaxSpec::untaxed(),
        ];
        for paise in (1..5_000_i64).step_by(7) {
            for spec in specs {
                for place in [PlaceOfSupply::Intra, PlaceOfSupply::Inter] {
                    for reg in [
                        Registration::Regular,
                        Registration::Composition,
                        Registration::Unregistered,
                    ] {
                        let net = Money::from_paise(paise);
                        let out = compute_line(net, spec, place, reg).expect("computes");
                        assert_eq!(
                            out.taxable.add(out.tax_total().expect("sums")),
                            Ok(out.gross),
                            "{paise} paise, {spec:?}, {place:?}, {reg:?}"
                        );
                        if spec.basis.is_inclusive() {
                            assert_eq!(out.gross, net, "an inclusive line must total to its price");
                        }
                    }
                }
            }
        }
    }

    // ---- the vocabulary ------------------------------------------------------

    #[test]
    fn rate_labels_read_the_way_a_receipt_should() {
        assert_eq!(pc(5).label(), "5%");
        assert_eq!(pc(18).label(), "18%");
        assert_eq!(bp(250).label(), "2.5%");
        assert_eq!(bp(25).label(), "0.25%");
        assert_eq!(TaxRate::ZERO.label(), "0%");
    }

    #[test]
    fn absurd_rates_are_refused() {
        assert!(TaxRate::from_basis_points(10_001).is_none());
        assert!(TaxRate::from_percent(101).is_none());
        assert!(TaxRate::from_percent(u32::MAX).is_none());
        assert_eq!(TaxRate::from_percent(18), TaxRate::from_basis_points(1_800));
    }

    #[test]
    fn a_rate_on_a_kind_that_cannot_carry_one_is_incoherent() {
        assert!(TaxSpec::gst(pc(5)).is_coherent());
        assert!(TaxSpec::liquor(pc(20)).is_coherent());
        assert!(TaxSpec::exempt().is_coherent());
        assert!(TaxSpec::untaxed().is_coherent());
        // 5% exempt is two contradictory statements.
        assert!(
            !TaxSpec { kind: TaxKind::Exempt, rate: pc(5), basis: PriceBasis::Exclusive }
                .is_coherent()
        );
        assert!(
            !TaxSpec { kind: TaxKind::Untaxed, rate: pc(5), basis: PriceBasis::Exclusive }
                .is_coherent()
        );
    }

    #[test]
    fn the_kinds_answer_which_channel_they_are_on() {
        assert!(TaxKind::Gst.is_gst() && !TaxKind::Gst.is_vat());
        assert!(TaxKind::OutsideGst.is_vat() && !TaxKind::OutsideGst.is_gst());
        assert!(!TaxKind::Exempt.is_gst() && !TaxKind::Exempt.is_vat());
        assert!(!TaxKind::Untaxed.is_gst() && !TaxKind::Untaxed.is_vat());
    }

    #[test]
    fn every_new_type_round_trips_through_serde() {
        // D20: nothing reachable from an order may serialise with a non-string
        // map key, and everything here ends up inside an order.
        let spec = TaxSpec::liquor(pc(20));
        let json = serde_json::to_string(&spec).expect("serialises");
        assert_eq!(serde_json::from_str::<TaxSpec>(&json).expect("round trips"), spec);

        let out = compute_line(rs(250), spec, PlaceOfSupply::Intra, Registration::Regular)
            .expect("computes");
        let json = serde_json::to_string(&out).expect("serialises");
        assert_eq!(serde_json::from_str::<TaxOutcome>(&json).expect("round trips"), out);

        let mut summary = TaxSummary::new();
        summary.add(out).expect("adds");
        let json = serde_json::to_string(&summary).expect("serialises");
        assert_eq!(serde_json::from_str::<TaxSummary>(&json).expect("round trips"), summary);

        for reg in [Registration::Regular, Registration::Composition, Registration::Unregistered] {
            let json = serde_json::to_string(&reg).expect("serialises");
            assert_eq!(serde_json::from_str::<Registration>(&json).expect("round trips"), reg);
        }
        for st in [StateTax::Sgst, StateTax::Utgst] {
            let json = serde_json::to_string(&st).expect("serialises");
            assert_eq!(serde_json::from_str::<StateTax>(&json).expect("round trips"), st);
        }
    }

    #[test]
    fn vat_takes_a_deliberate_call_to_become_money() {
        let vat = Vat::new(Money::from_paise(4_167));
        assert_eq!(vat.into_money(), Money::from_paise(4_167));
        assert_eq!(vat.add(Vat::new(rs(1))).expect("sums").into_money(), Money::from_paise(4_267));
        assert!(Vat::ZERO.is_zero());
    }
}
