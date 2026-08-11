//! **Units, packs and the cost of a unit** — P25, scope 4.2.
//!
//! # Inventory does not fail on stock arithmetic. It fails on units.
//!
//! Rice is bought in 25 kg bags, kept in kg and cooked in grams. Oil comes in a
//! 15 litre tin and a recipe uses 30 ml. Eggs arrive in trays of 30 and a dish
//! uses two. A recipe says *"200 g rice"*, a purchase says *"2 bags"*, and the
//! owner asks *"how many bags have I got left"*.
//!
//! Get this right and everything after it is addition. Get it wrong and every
//! number in the module is wrong, including the one the owner bought it for —
//! and unlike a bill, **nobody ever checks a stock report against anything**. A
//! bill that is wrong by ₹2 is found by the customer holding it in ninety
//! seconds; a stock report that is wrong by 8% is believed for a year.
//!
//! # D108 — three dimensions, one base unit each, and every other unit is that
//! material's own pack
//!
//! A material declares a [`Dimension`], and its base unit **follows from the
//! dimension and is not a choice**: gram, millilitre, piece. That is
//! deliberate. A shop that could pick "kg" for rice and "g" for masala has two
//! rice figures the day somebody converts one, and a session six months from
//! now has to guess which.
//!
//! The standard units of a dimension come free and are never typed —
//! kg = 1000 g, litre = 1000 ml, dozen = 12. Everything else is the **shop's
//! own pack, defined against the material**, because a bag is 25 kg of rice and
//! 50 kg of flour, and a crate is 24 bottles of one drink and 12 of another.
//! **A conversion that lives on the unit instead of on the material is the bug
//! this decision exists to prevent.**
//!
//! # All stock is [`Qty`] in base units
//!
//! `Qty` is already an `i64` count of thousandths, already checked, already
//! tested by P00. Thousandths of a gram is a milligram, which is finer than any
//! kitchen scale in India. **There is no second quantity type in this product**
//! — if something is missing, it belongs in `qty.rs` where the existing tests
//! are.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::money::{Money, MoneyError};
use crate::qty::Qty;

/// Something a unit could not express.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnitError {
    #[error("a pack has to hold more than nothing")]
    EmptyPack,
    #[error("that quantity is too large")]
    Overflow,
    #[error("`{0}` is not a unit this material is measured in")]
    UnknownUnit(String),
    #[error("`{0}` is already a unit of this material")]
    DuplicateUnit(String),
    #[error("a unit needs a name")]
    UnnamedUnit,
}

type Result<T> = std::result::Result<T, UnitError>;

/// What kind of thing a material is measured in.
///
/// Three, and there will only ever be three, because these are the three ways a
/// kitchen measures anything: it weighs it, it pours it, or it counts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Weight,
    Volume,
    Count,
}

impl Dimension {
    /// Every dimension, for the screens and for the tests.
    pub const ALL: &'static [Dimension] = &[Dimension::Weight, Dimension::Volume, Dimension::Count];

    /// The base unit, which is not a choice. See D108.
    #[must_use]
    pub const fn base_unit(self) -> &'static str {
        match self {
            Dimension::Weight => "g",
            Dimension::Volume => "ml",
            Dimension::Count => "piece",
        }
    }

    /// The tag stored in the database, spelled the way serde spells it so the
    /// counter, the phone and the cloud agree (the schema's own rule).
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Dimension::Weight => "weight",
            Dimension::Volume => "volume",
            Dimension::Count => "count",
        }
    }

    /// What a person calls it on a screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Dimension::Weight => "Weight",
            Dimension::Volume => "Volume",
            Dimension::Count => "Count",
        }
    }

    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Dimension::ALL.iter().copied().find(|d| d.tag() == tag)
    }

    /// **The unit a price is quoted in.**
    ///
    /// A shopkeeper says *"₹40 a kilo"*, never *"₹0.04 a gram"* — which is what
    /// the base unit produces and what P25 shipped until somebody seeded a demo
    /// shop and read the column. Weight and volume price by the thousand;
    /// **count prices by the piece**, because "₹60 a dozen" is not how eggs are
    /// discussed at a counter even though a dozen is the standard unit.
    #[must_use]
    pub const fn price_unit(self) -> &'static str {
        match self {
            Dimension::Weight => "kg",
            Dimension::Volume => "l",
            Dimension::Count => "piece",
        }
    }

    /// The units that come free with the dimension, as `(name, whole base
    /// units)`. **Nobody types these** — a shop that has to tell the software
    /// that a kilo is a thousand grams has already been given a job it should
    /// not have.
    #[must_use]
    pub const fn standard_units(self) -> &'static [(&'static str, i64)] {
        match self {
            Dimension::Weight => &[("kg", 1_000)],
            Dimension::Volume => &[("l", 1_000)],
            // A dozen, because eggs. Not "tray" — a tray is 30 in one shop and
            // 12 in another, which makes it a PACK and not a standard unit, and
            // that difference is the whole of D108.
            Dimension::Count => &[("dozen", 12)],
        }
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One unit a particular material is spoken about in.
///
/// The base unit is a `Pack` too, holding exactly one base unit. That is not a
/// trick to save a branch — it means every conversion in this module goes
/// through the same two functions, so there is no "and also the base unit" case
/// for anybody to get wrong.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pack {
    /// "kg", "bag", "tin", "tray".
    pub name: String,
    /// How many base units one of these holds, as a [`Qty`] in base units.
    ///
    /// A bag of rice is `Qty::from_whole(25_000)` — twenty-five thousand grams.
    pub base_per_unit: Qty,
    /// True for the units that came from the dimension, false for the shop's
    /// own. A screen may not offer to delete a standard unit, and a shop cannot
    /// redefine a kilo.
    pub is_standard: bool,
}

impl Pack {
    /// A pack the shop defined.
    pub fn new(name: impl Into<String>, base_per_unit: Qty) -> Result<Self> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(UnitError::UnnamedUnit);
        }
        if !base_per_unit.is_positive() {
            return Err(UnitError::EmptyPack);
        }
        Ok(Pack { name: name.trim().to_owned(), base_per_unit, is_standard: false })
    }

    /// A unit that came from the dimension. Infallible on purpose: the inputs
    /// are the three compile-time constants in [`Dimension::standard_units`],
    /// so there is no failure for a caller to handle and no `expect` for one to
    /// trip over.
    fn standard(name: &str, whole_base_units: i64) -> Self {
        Pack {
            name: name.to_owned(),
            base_per_unit: Qty::from_thousandths(whole_base_units.saturating_mul(1_000)),
            is_standard: true,
        }
    }

    /// **The truth direction.** How many base units `typed` of this pack is.
    ///
    /// Two bags of rice at 25 kg → 50,000 g. Rounded half away from zero, which
    /// matters for nothing in practice (the bias is a microgram) and is here so
    /// that there is one rounding rule in the file rather than a truncation
    /// somebody has to remember.
    pub fn to_base(&self, typed: Qty) -> Result<Qty> {
        if !self.base_per_unit.is_positive() {
            return Err(UnitError::EmptyPack);
        }
        // i128 so the product of two i64 thousandths cannot overflow. Both are
        // already scaled by 1,000, so dividing by 1,000 once removes the extra
        // scale the multiplication introduced.
        ratio(
            i128::from(typed.thousandths()) * i128::from(self.base_per_unit.thousandths()),
            1_000,
        )
    }

    /// **The label direction.** `base` read back in this pack.
    ///
    /// 68,000 g of rice → 2.72 bags. This is what a screen shows and what the
    /// buy list says; it is never what anything computes with, because it can
    /// round (2.72 bags is not a thing you can carry).
    pub fn from_base(&self, base: Qty) -> Result<Qty> {
        if !self.base_per_unit.is_positive() {
            return Err(UnitError::EmptyPack);
        }
        ratio(
            i128::from(base.thousandths()) * 1_000,
            i128::from(self.base_per_unit.thousandths()),
        )
    }

    /// "2 bags", "180 g", "1.5 kg" — the quantity and the unit as one phrase.
    #[must_use]
    pub fn say(&self, typed: Qty) -> String {
        format!("{typed} {}", self.name)
    }
}

/// `numerator ÷ denominator`, rounded half away from zero, as a [`Qty`].
#[allow(
    clippy::integer_division,
    reason = "integer division IS the operation; `den / 2` is the standard \
              half-away-from-zero bias and is exact for both parities"
)]
fn ratio(numerator: i128, denominator: i128) -> Result<Qty> {
    if denominator == 0 {
        return Err(UnitError::EmptyPack);
    }
    let biased = if (numerator < 0) == (denominator < 0) {
        numerator + denominator / 2
    } else {
        numerator - denominator / 2
    };
    i64::try_from(biased / denominator).map(Qty::from_thousandths).map_err(|_| UnitError::Overflow)
}

/// Every unit one material is measured in: its dimension's standards, plus the
/// shop's own packs.
///
/// **The base unit is its own field and not the first element of a list.** It
/// has to be there, every conversion goes through it, and a `Vec` that is
/// "never empty" is an invariant somebody has to keep — this way the compiler
/// keeps it and no lookup in this file can fail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Units {
    pub dimension: Dimension,
    base: Pack,
    extra: Vec<Pack>,
}

impl Units {
    /// A material that has only what its dimension gives it.
    #[must_use]
    pub fn standard(dimension: Dimension) -> Self {
        Units {
            dimension,
            base: Pack::standard(dimension.base_unit(), 1),
            extra: dimension
                .standard_units()
                .iter()
                .map(|(name, whole)| Pack::standard(name, *whole))
                .collect(),
        }
    }

    /// Add one of the shop's own packs — "bag", "tin", "tray".
    pub fn with_pack(mut self, name: impl Into<String>, base_per_unit: Qty) -> Result<Self> {
        let pack = Pack::new(name, base_per_unit)?;
        if self.find(&pack.name).is_some() {
            return Err(UnitError::DuplicateUnit(pack.name));
        }
        self.extra.push(pack);
        Ok(self)
    }

    #[must_use]
    pub fn base_unit(&self) -> &'static str {
        self.dimension.base_unit()
    }

    /// The pack that IS the base unit.
    #[must_use]
    pub const fn base(&self) -> &Pack {
        &self.base
    }

    /// Every unit, base first.
    pub fn all(&self) -> impl Iterator<Item = &Pack> {
        std::iter::once(&self.base).chain(self.extra.iter())
    }

    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Pack> {
        let wanted = name.trim();
        self.all().find(|p| p.name.eq_ignore_ascii_case(wanted))
    }

    /// What a person typed, in base units.
    pub fn to_base(&self, typed: Qty, unit: &str) -> Result<Qty> {
        self.find(unit).ok_or_else(|| UnitError::UnknownUnit(unit.to_owned()))?.to_base(typed)
    }

    /// Base units read back in a named unit.
    pub fn from_base(&self, base: Qty, unit: &str) -> Result<Qty> {
        self.find(unit).ok_or_else(|| UnitError::UnknownUnit(unit.to_owned()))?.from_base(base)
    }

    /// **The unit a person should be shown this amount in**, and the amount.
    ///
    /// The biggest pack that at least one of fits into, so that 68,000 g of
    /// rice reads "2.72 bags" and 300 g of saffron reads "300 g". A buy list
    /// that tells a man going to the market to bring 50,000 g of rice is a buy
    /// list nobody uses.
    #[must_use]
    pub fn readable(&self, base: Qty) -> (Qty, &Pack) {
        let mut best = self.base();
        for pack in self.all() {
            if pack.base_per_unit <= base.abs() && pack.base_per_unit > best.base_per_unit {
                best = pack;
            }
        }
        let shown = best.from_base(base).unwrap_or(base);
        (shown, best)
    }

    /// "2.72 bags" — [`Units::readable`] as one phrase.
    #[must_use]
    pub fn say(&self, base: Qty) -> String {
        let (shown, pack) = self.readable(base);
        pack.say(shown)
    }
}

/// **What one base unit costs** — paise per 1,000 base units.
///
/// # Why 1,000 base units and not one
///
/// A gram of rice costs six paise; a millilitre of water costs a fraction of
/// one. Storing paise per base unit would round every cheap material to zero
/// and every cost report with it. Storing paise per *thousand* base units makes
/// the stored number the one an Indian shopkeeper already says out loud:
/// **₹60 a kilo is 6000**, and a person reading the column in a SQLite browser
/// at 11 pm sees the price they paid.
///
/// It is an `i64` of paise for the same reason every other money value in this
/// product is (D2). There is no floating point anywhere in this path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnitCost(i64);

impl UnitCost {
    pub const ZERO: UnitCost = UnitCost(0);

    #[must_use]
    pub const fn from_paise_per_thousand(paise: i64) -> Self {
        UnitCost(paise)
    }

    #[must_use]
    pub const fn paise_per_thousand(self) -> i64 {
        self.0
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// What the shop actually says: "a bag costs ₹1,500".
    pub fn from_pack_price(price: Money, pack: &Pack) -> Result<Self> {
        if !pack.base_per_unit.is_positive() {
            return Err(UnitError::EmptyPack);
        }
        // price is paise per pack; the pack is `base_per_unit` base units, held
        // in thousandths. paise per 1,000 base units is therefore
        // price × 1,000 × 1,000 ÷ thousandths.
        let scaled = i128::from(price.paise()).checked_mul(1_000_000).ok_or(UnitError::Overflow)?;
        let out = ratio(scaled, i128::from(pack.base_per_unit.thousandths()))?;
        Ok(UnitCost(out.thousandths()))
    }

    /// What one base unit of a made material costs, from what a batch cost and
    /// how much it yielded (D111).
    ///
    /// "This batch cost ₹840 and made 4 kg of gravy" is 21 paise a gram.
    pub fn from_batch(total: Money, yielded: Qty) -> Result<Self> {
        if !yielded.is_positive() {
            return Err(UnitError::EmptyPack);
        }
        let scaled = i128::from(total.paise()).checked_mul(1_000_000).ok_or(UnitError::Overflow)?;
        Ok(UnitCost(ratio(scaled, i128::from(yielded.thousandths()))?.thousandths()))
    }

    /// Back the other way, for a screen that wants to say "₹1,500 a bag".
    pub fn per_pack(self, pack: &Pack) -> std::result::Result<Money, MoneyError> {
        Money::from_paise(self.0).mul_ratio(pack.base_per_unit.thousandths(), 1_000_000)
    }

    /// **What `qty` of this material is worth**, rounded exactly once.
    ///
    /// 180 g of rice at ₹60 a kilo: 6000 × 180,000 ÷ 1,000,000 = 1080 paise.
    pub fn cost_of(self, qty: Qty) -> std::result::Result<Money, MoneyError> {
        Money::from_paise(self.0).mul_ratio(qty.thousandths(), 1_000_000)
    }

    /// **D118 — a material's cost is a weighted average of what actually came
    /// in**, never a "current price" field somebody typed in 2019.
    ///
    /// A stale typed price is the single most common way a food-cost report
    /// lies, and the lie is invisible: the report looks right, adds up, and is
    /// forty per cent out.
    ///
    /// When the holding is zero or negative there is nothing to average
    /// against — a negative balance is a bookkeeping state, not a stock of
    /// goods with a cost — so the incoming price simply becomes the cost.
    pub fn blend(self, holding: Qty, incoming: Qty, incoming_cost: UnitCost) -> Result<Self> {
        if !incoming.is_positive() {
            return Ok(self);
        }
        if !holding.is_positive() {
            return Ok(incoming_cost);
        }
        let have = i128::from(holding.thousandths());
        let coming = i128::from(incoming.thousandths());
        let total = have.checked_add(coming).ok_or(UnitError::Overflow)?;
        let value = i128::from(self.0)
            .checked_mul(have)
            .and_then(|v| {
                i128::from(incoming_cost.0).checked_mul(coming).and_then(|w| v.checked_add(w))
            })
            .ok_or(UnitError::Overflow)?;
        Ok(UnitCost(ratio(value, total)?.thousandths()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rice: base unit gram, and the shop buys it in 25 kg bags.
    fn rice() -> Units {
        Units::standard(Dimension::Weight)
            .with_pack("bag", Qty::from_whole(25_000).expect("in range"))
            .expect("a new unit")
    }

    #[test]
    fn the_base_unit_is_not_a_choice() {
        // D108. A shop that could pick kg for rice and g for masala has two
        // rice figures the day somebody converts one.
        assert_eq!(Dimension::Weight.base_unit(), "g");
        assert_eq!(Dimension::Volume.base_unit(), "ml");
        assert_eq!(Dimension::Count.base_unit(), "piece");
        for d in Dimension::ALL {
            assert_eq!(Dimension::from_tag(d.tag()), Some(*d));
            assert_eq!(Units::standard(*d).base().base_per_unit, Qty::ONE);
        }
    }

    #[test]
    fn t1_two_bags_in_forty_plates_out() {
        // T1, and the worked example the owner is shown at the end of the
        // session. Two 25 kg bags in, forty plates at 180 g each out.
        let rice = rice();
        let bought = rice.to_base(Qty::from_whole(2).expect("in range"), "bag").expect("converts");
        assert_eq!(bought, Qty::from_whole(50_000).expect("in range"), "2 bags is 50,000 g");

        let used_per_plate = rice.to_base(Qty::from_whole(180).expect("in range"), "g").expect("g");
        let used = used_per_plate.thousandths() * 40;
        let left = Qty::from_thousandths(bought.thousandths() - used);

        // The same number read three ways, which is the whole of D108.
        assert_eq!(left, Qty::from_whole(42_800).expect("in range"), "42,800 g");
        assert_eq!(rice.from_base(left, "kg").expect("kg"), Qty::from_thousandths(42_800), "42.8 kg");
        assert_eq!(rice.from_base(left, "bag").expect("bag"), Qty::from_thousandths(1_712), "1.712 bags");
        // And what a screen would actually say.
        assert_eq!(rice.say(left), "1.712 bag");
    }

    #[test]
    fn a_pack_belongs_to_the_material_not_to_the_unit() {
        // A bag is 25 kg of rice and 50 kg of flour. If the conversion lived on
        // the unit, one of these two numbers would be wrong for ever.
        let flour = Units::standard(Dimension::Weight)
            .with_pack("bag", Qty::from_whole(50_000).expect("in range"))
            .expect("a new unit");
        let one = Qty::ONE;
        assert_eq!(rice().to_base(one, "bag").expect("rice"), Qty::from_whole(25_000).expect("ok"));
        assert_eq!(flour.to_base(one, "bag").expect("flour"), Qty::from_whole(50_000).expect("ok"));
    }

    #[test]
    fn eggs_are_counted_and_a_tray_is_the_shops_own_word() {
        let eggs = Units::standard(Dimension::Count)
            .with_pack("tray", Qty::from_whole(30).expect("in range"))
            .expect("a new unit");
        assert_eq!(eggs.to_base(Qty::from_whole(3).expect("ok"), "tray").expect("t"),
                   Qty::from_whole(90).expect("ok"));
        // A dozen came free; a tray did not, because a tray is 30 here and 12
        // somewhere else.
        assert_eq!(eggs.to_base(Qty::ONE, "dozen").expect("d"), Qty::from_whole(12).expect("ok"));
        assert!(eggs.find("dozen").expect("present").is_standard);
        assert!(!eggs.find("tray").expect("present").is_standard);
    }

    #[test]
    fn a_unit_is_refused_rather_than_guessed() {
        let rice = rice();
        assert!(matches!(rice.to_base(Qty::ONE, "sack"), Err(UnitError::UnknownUnit(_))));
        assert!(matches!(
            Pack::new("bag", Qty::ZERO),
            Err(UnitError::EmptyPack)
        ));
        assert!(matches!(Pack::new("  ", Qty::ONE), Err(UnitError::UnnamedUnit)));
        assert!(matches!(
            rice.with_pack("BAG", Qty::from_whole(1).expect("ok")),
            Err(UnitError::DuplicateUnit(_))
        ));
    }

    #[test]
    fn readable_picks_the_unit_a_person_would_use() {
        let rice = rice();
        // Half a bag reads as a bag; a spoonful reads as grams.
        assert_eq!(rice.say(Qty::from_whole(30_000).expect("ok")), "1.2 bag");
        assert_eq!(rice.say(Qty::from_whole(2_500).expect("ok")), "2.5 kg");
        assert_eq!(rice.say(Qty::from_whole(180).expect("ok")), "180 g");
        // Nothing left is still a sentence, and it is in the base unit.
        assert_eq!(rice.say(Qty::ZERO), "0 g");
        // A negative holding still reads in a sensible unit rather than
        // collapsing to grams, because a shop that has gone 30 kg negative
        // needs to see that at a glance.
        assert_eq!(rice.say(Qty::from_whole(-30_000).expect("ok")), "-1.2 bag");
    }

    #[test]
    fn cost_is_the_number_a_shopkeeper_already_says() {
        // ₹60 a kilo.
        let rice = rice();
        let per_kg = UnitCost::from_pack_price(
            Money::from_paise(6_000),
            rice.find("kg").expect("present"),
        )
        .expect("converts");
        assert_eq!(per_kg.paise_per_thousand(), 6_000, "₹60/kg stores as 6000");

        // 180 g of it.
        assert_eq!(
            per_kg.cost_of(Qty::from_whole(180).expect("ok")),
            Ok(Money::from_paise(1_080))
        );

        // The same price expressed as a bag.
        let per_bag = UnitCost::from_pack_price(
            Money::from_paise(150_000),
            rice.find("bag").expect("present"),
        )
        .expect("converts");
        assert_eq!(per_bag, per_kg, "₹1,500 a 25 kg bag IS ₹60 a kilo");
        assert_eq!(per_bag.per_pack(rice.find("bag").expect("p")), Ok(Money::from_paise(150_000)));
    }

    #[test]
    fn a_cheap_material_does_not_round_to_free() {
        // The reason the cost is per 1,000 base units. Salt at ₹20 a kilo is
        // two paise a gram; per base unit it would round to 2 and be 0% wrong,
        // but water at ₹5 for 20 litres is 0.025 paise a millilitre, which per
        // base unit rounds to zero and makes every recipe using it free.
        let water = Units::standard(Dimension::Volume);
        let cost = UnitCost::from_pack_price(
            Money::from_paise(500),
            water.find("l").expect("present"),
        )
        .expect("converts");
        assert_eq!(cost.paise_per_thousand(), 500, "₹5 a litre");
        // 20 ml of it is a real, non-zero cost.
        assert_eq!(cost.cost_of(Qty::from_whole(20).expect("ok")), Ok(Money::from_paise(10)));
    }

    #[test]
    fn d118_the_cost_is_a_weighted_average_of_what_came_in() {
        // 10 kg at ₹60, then 10 kg at ₹80, is ₹70 — not ₹80, and not still ₹60.
        let ten_kg = Qty::from_whole(10_000).expect("ok");
        let sixty = UnitCost::from_paise_per_thousand(6_000);
        let eighty = UnitCost::from_paise_per_thousand(8_000);
        assert_eq!(sixty.blend(ten_kg, ten_kg, eighty), Ok(UnitCost::from_paise_per_thousand(7_000)));

        // Unequal amounts weight correctly: 30 kg at ₹60 plus 10 kg at ₹80.
        let thirty_kg = Qty::from_whole(30_000).expect("ok");
        assert_eq!(
            sixty.blend(thirty_kg, ten_kg, eighty),
            Ok(UnitCost::from_paise_per_thousand(6_500))
        );

        // Nothing on the shelf: the new price simply IS the price. Averaging
        // against a negative balance would invent a cost from a bookkeeping
        // state rather than from goods.
        assert_eq!(sixty.blend(Qty::ZERO, ten_kg, eighty), Ok(eighty));
        assert_eq!(
            sixty.blend(Qty::from_whole(-5_000).expect("ok"), ten_kg, eighty),
            Ok(eighty)
        );
        // Nothing came in: nothing changes.
        assert_eq!(sixty.blend(ten_kg, Qty::ZERO, eighty), Ok(sixty));
    }

    #[test]
    fn the_truth_direction_round_trips_and_the_label_direction_is_allowed_not_to() {
        // This is D109 as arithmetic. `to_base` is the truth and is exact for
        // anything a person can type; `from_base` is a LABEL and rounds,
        // because a bag is 25,000 g and there is no way to write one gram of it
        // in bags. That asymmetry is precisely why both numbers are stored.
        let rice = rice();
        for typed in [Qty::from_thousandths(1), Qty::ONE, Qty::from_thousandths(2_500)] {
            let base = rice.to_base(typed, "bag").expect("to base");
            assert_eq!(rice.from_base(base, "bag").expect("back"), typed, "{typed} bags");
        }
        // One milligram, read in bags, is nothing — and reading it back gives
        // nothing. Believing that number would lose the gram, which is why the
        // ledger holds base units and never the label.
        let one_thousandth_of_a_gram = Qty::from_thousandths(1);
        assert_eq!(rice.from_base(one_thousandth_of_a_gram, "bag").expect("to bags"), Qty::ZERO);

        let absurd = Units::standard(Dimension::Weight)
            .with_pack("world", Qty::from_thousandths(i64::MAX))
            .expect("a new unit");
        assert!(matches!(
            absurd.to_base(Qty::from_thousandths(i64::MAX), "world"),
            Err(UnitError::Overflow)
        ));
    }
}
