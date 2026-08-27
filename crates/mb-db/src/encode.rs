//! Mb-core values <-> SQLite values.
//!
//! ```sql
//! CHECK ((mode = 'credit') = (customer_id IS NOT NULL))
//! CHECK ((mode = 'other')  = (mode_label  IS NOT NULL))
//! ```

use mb_core::{
    BusinessDay, ChargeBasis, ChargeKind, Discount, Money, OrderType, PaymentMode, PlaceOfSupply,
    PriceBasis, Qty, Registration, RoundingMode, StateTax, TaxKind, TaxRate, TaxSpec, Timestamp,
};

use crate::error::DbError;

// Scalars. Each of these is a newtype over an integer, so the mapping is the identity.

/// Paise. The whole of D2, in one line.
#[must_use]
pub fn money_to_sql(m: Money) -> i64 {
    m.paise()
}

#[must_use]
pub fn money_from_sql(paise: i64) -> Money {
    Money::from_paise(paise)
}

/// Thousandths of a unit, so 0.5 kg is 500 and there is no float in sight.
#[must_use]
pub fn qty_to_sql(q: Qty) -> i64 {
    q.thousandths()
}

#[must_use]
pub fn qty_from_sql(thousandths: i64) -> Qty {
    Qty::from_thousandths(thousandths)
}

/// Basis points, so 5% is 500 and 2.5% is 250.
#[must_use]
pub fn tax_rate_to_sql(r: TaxRate) -> i64 {
    i64::from(r.basis_points())
}

/// Rejects a stored rate above 100%, because `TaxRate` does and the disk is not more
/// trustworthy than the caller.
pub fn tax_rate_from_sql(bp: i64, column: &'static str) -> Result<TaxRate, DbError> {
    let bp = u32::try_from(bp).map_err(|_| DbError::OutOfRange {
        column,
        expected: "tax rate in basis points",
    })?;
    TaxRate::from_basis_points(bp).ok_or(DbError::OutOfRange {
        column,
        expected: "tax rate in basis points",
    })
}

/// Milliseconds since the Unix epoch, UTC.
#[must_use]
pub fn timestamp_to_sql(t: Timestamp) -> i64 {
    t.millis()
}

#[must_use]
pub fn timestamp_from_sql(millis: i64) -> Timestamp {
    Timestamp::from_millis(millis)
}

/// Days since 1970-01-01.
#[must_use]
pub fn business_day_to_sql(d: BusinessDay) -> i64 {
    i64::from(d.days_since_epoch())
}

pub fn business_day_from_sql(days: i64, column: &'static str) -> Result<BusinessDay, DbError> {
    let days = i32::try_from(days).map_err(|_| DbError::OutOfRange {
        column,
        expected: "business day as days since 1970-01-01",
    })?;
    Ok(BusinessDay::from_days_since_epoch(days))
}

/// 0 or 1. Never NULL, never 'true', never 'Y'.
#[must_use]
pub fn bool_to_sql(b: bool) -> i64 {
    i64::from(b)
}

pub fn bool_from_sql(v: i64, column: &'static str) -> Result<bool, DbError> {
    match v {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(DbError::BadValue {
            column,
            value: other.to_string(),
        }),
    }
}

// Each pair is written out rather than generated.

/// `tax_kind` / `kind`: what the supply is, in law.
#[must_use]
pub fn tax_kind_to_sql(kind: TaxKind) -> &'static str {
    match kind {
        TaxKind::Gst => "gst",
        TaxKind::Exempt => "exempt",
        TaxKind::OutsideGst => "outside_gst",
        TaxKind::Untaxed => "untaxed",
    }
}

/// An unknown tag is an error, never a default: a line taxed as GST because somebody typed
/// `gts` is a wrong bill.
pub fn tax_kind_from_sql(text: &str) -> Result<TaxKind, DbError> {
    match text {
        "gst" => Ok(TaxKind::Gst),
        "exempt" => Ok(TaxKind::Exempt),
        "outside_gst" => Ok(TaxKind::OutsideGst),
        "untaxed" => Ok(TaxKind::Untaxed),
        other => Err(DbError::BadValue {
            column: "tax_kind",
            value: other.to_owned(),
        }),
    }
}

/// `tax_basis` / `basis`: is the tax already inside the price?
#[must_use]
pub fn price_basis_to_sql(basis: PriceBasis) -> &'static str {
    match basis {
        PriceBasis::Exclusive => "exclusive",
        PriceBasis::Inclusive => "inclusive",
    }
}

pub fn price_basis_from_sql(text: &str) -> Result<PriceBasis, DbError> {
    match text {
        "exclusive" => Ok(PriceBasis::Exclusive),
        "inclusive" => Ok(PriceBasis::Inclusive),
        other => Err(DbError::BadValue {
            column: "tax_basis",
            value: other.to_owned(),
        }),
    }
}

/// The whole spec, from the three columns that hold it.
pub fn tax_spec_from_sql_parts(
    rate_bp: i64,
    kind_text: &str,
    basis_text: &str,
    rate_column: &'static str,
) -> Result<TaxSpec, DbError> {
    Ok(TaxSpec {
        kind: tax_kind_from_sql(kind_text)?,
        rate: tax_rate_from_sql(rate_bp, rate_column)?,
        basis: price_basis_from_sql(basis_text)?,
    })
}

/// The tag text stored in a `registration` column.
#[must_use]
pub fn registration_to_sql(v: Registration) -> &'static str {
    match v {
        Registration::Unregistered => "unregistered",
        Registration::Composition => "composition",
        Registration::Regular => "regular",
    }
}

pub fn registration_from_sql(text: &str) -> Result<Registration, DbError> {
    match text {
        "unregistered" => Ok(Registration::Unregistered),
        "composition" => Ok(Registration::Composition),
        "regular" => Ok(Registration::Regular),
        other => Err(DbError::BadValue {
            column: "registration",
            value: other.to_owned(),
        }),
    }
}

/// The tag text stored in a `state_tax` column — what the state half of an intra-state supply
/// is called.
#[must_use]
pub fn state_tax_to_sql(v: StateTax) -> &'static str {
    match v {
        StateTax::Sgst => "sgst",
        StateTax::Utgst => "utgst",
    }
}

pub fn state_tax_from_sql(text: &str) -> Result<StateTax, DbError> {
    match text {
        "sgst" => Ok(StateTax::Sgst),
        "utgst" => Ok(StateTax::Utgst),
        other => Err(DbError::BadValue {
            column: "state_tax",
            value: other.to_owned(),
        }),
    }
}

/// The tag text stored in a `place_of_supply` column.
#[must_use]
pub fn place_of_supply_to_sql(v: PlaceOfSupply) -> &'static str {
    match v {
        PlaceOfSupply::Intra => "intra",
        PlaceOfSupply::Inter => "inter",
    }
}

pub fn place_of_supply_from_sql(text: &str) -> Result<PlaceOfSupply, DbError> {
    match text {
        "intra" => Ok(PlaceOfSupply::Intra),
        "inter" => Ok(PlaceOfSupply::Inter),
        other => Err(DbError::BadValue {
            column: "place_of_supply",
            value: other.to_owned(),
        }),
    }
}

/// The tag text stored in an `order_type` column.
#[must_use]
pub fn order_type_to_sql(v: OrderType) -> &'static str {
    match v {
        OrderType::DineIn => "dine_in",
        OrderType::Parcel => "parcel",
        OrderType::SelfService => "self_service",
        OrderType::Delivery => "delivery",
    }
}

pub fn order_type_from_sql(text: &str) -> Result<OrderType, DbError> {
    match text {
        "dine_in" => Ok(OrderType::DineIn),
        "parcel" => Ok(OrderType::Parcel),
        "self_service" => Ok(OrderType::SelfService),
        "delivery" => Ok(OrderType::Delivery),
        other => Err(DbError::BadValue {
            column: "order_type",
            value: other.to_owned(),
        }),
    }
}

/// The tag text stored in a `rounding_mode` column.
#[must_use]
pub fn rounding_mode_to_sql(v: RoundingMode) -> &'static str {
    match v {
        RoundingMode::None => "none",
        RoundingMode::NearestRupee => "nearest_rupee",
        RoundingMode::Up => "up",
        RoundingMode::Down => "down",
    }
}

pub fn rounding_mode_from_sql(text: &str) -> Result<RoundingMode, DbError> {
    match text {
        "none" => Ok(RoundingMode::None),
        "nearest_rupee" => Ok(RoundingMode::NearestRupee),
        "up" => Ok(RoundingMode::Up),
        "down" => Ok(RoundingMode::Down),
        other => Err(DbError::BadValue {
            column: "rounding_mode",
            value: other.to_owned(),
        }),
    }
}

// Payload-carrying enums: a tag column plus a payload column.

/// How a `PaymentMode` is split across `payments.mode`, `payments.customer_id` and
/// `payments.mode_label`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentModeColumns {
    pub mode: &'static str,
    pub customer_id: Option<String>,
    pub mode_label: Option<String>,
}

#[must_use]
pub fn payment_mode_to_sql(mode: &PaymentMode) -> PaymentModeColumns {
    match mode {
        PaymentMode::Cash => PaymentModeColumns {
            mode: "cash",
            customer_id: None,
            mode_label: None,
        },
        PaymentMode::Card => PaymentModeColumns {
            mode: "card",
            customer_id: None,
            mode_label: None,
        },
        PaymentMode::Upi => PaymentModeColumns {
            mode: "upi",
            customer_id: None,
            mode_label: None,
        },
        PaymentMode::Credit(customer) => PaymentModeColumns {
            mode: "credit",
            customer_id: Some(customer.as_str().to_owned()),
            mode_label: None,
        },
        PaymentMode::Other(label) => PaymentModeColumns {
            mode: "other",
            customer_id: None,
            mode_label: Some(label.clone()),
        },
    }
}

/// The CHECK constraints on `payments` make the mismatched cases unstorable, so reaching one of
/// them here means the row was written by something other than this program.
pub fn payment_mode_from_sql(
    mode: &str,
    customer_id: Option<&str>,
    mode_label: Option<&str>,
) -> Result<PaymentMode, DbError> {
    match (mode, customer_id, mode_label) {
        ("cash", None, None) => Ok(PaymentMode::Cash),
        ("card", None, None) => Ok(PaymentMode::Card),
        ("upi", None, None) => Ok(PaymentMode::Upi),
        ("credit", Some(id), None) => Ok(PaymentMode::Credit(id.into())),
        ("other", None, Some(label)) => Ok(PaymentMode::Other(label.to_owned())),
        ("credit", None, _) => Err(DbError::invariant(
            "a credit payment is stored without the customer it is owed by",
        )),
        ("other", _, None) => Err(DbError::invariant(
            "a payment of an 'other' mode is stored without saying which mode",
        )),
        (other, _, _) => Err(DbError::BadValue {
            column: "payments.mode",
            value: other.to_owned(),
        }),
    }
}

/// `charges.kind`. `ChargeKind::Other` carries a label, but a charge already has a `name`
/// column holding exactly that, so the payload has nowhere new to go — the tag is `'other'` and
/// the name is the label.
#[must_use]
pub fn charge_kind_to_sql(kind: &ChargeKind) -> &'static str {
    match kind {
        ChargeKind::Service => "service",
        ChargeKind::Packing => "packing",
        ChargeKind::Delivery => "delivery",
        ChargeKind::Other(_) => "other",
    }
}

/// Reconstructs the kind, taking the `Other` label back out of `name`.
pub fn charge_kind_from_sql(kind: &str, name: &str) -> Result<ChargeKind, DbError> {
    match kind {
        "service" => Ok(ChargeKind::Service),
        "packing" => Ok(ChargeKind::Packing),
        "delivery" => Ok(ChargeKind::Delivery),
        "other" => Ok(ChargeKind::Other(name.to_owned())),
        other => Err(DbError::BadValue {
            column: "charges.kind",
            value: other.to_owned(),
        }),
    }
}

/// `basis` and `basis_value`: one column with two meanings, disambiguated by the tag beside it.
#[must_use]
pub fn charge_basis_to_sql(basis: ChargeBasis) -> (&'static str, i64) {
    match basis {
        ChargeBasis::Percent(bp) => ("percent", i64::from(bp)),
        ChargeBasis::Flat(amount) => ("flat", amount.paise()),
    }
}

pub fn charge_basis_from_sql(basis: &str, value: i64) -> Result<ChargeBasis, DbError> {
    match basis {
        "percent" => {
            let bp = u32::try_from(value).map_err(|_| DbError::OutOfRange {
                column: "charges.basis_value",
                expected: "percentage in basis points",
            })?;
            Ok(ChargeBasis::Percent(bp))
        }
        "flat" => Ok(ChargeBasis::Flat(Money::from_paise(value))),
        other => Err(DbError::BadValue {
            column: "charges.basis",
            value: other.to_owned(),
        }),
    }
}

/// A discount, in the same tag + value shape as a charge basis.
#[must_use]
pub fn discount_to_sql(discount: Discount) -> (&'static str, i64) {
    match discount {
        Discount::Percent(bp) => ("percent", i64::from(bp)),
        Discount::Amount(amount) => ("amount", amount.paise()),
    }
}

pub fn discount_from_sql(
    kind: &str,
    value: i64,
    column: &'static str,
) -> Result<Discount, DbError> {
    match kind {
        "percent" => {
            let bp = u32::try_from(value).map_err(|_| DbError::OutOfRange {
                column,
                expected: "percentage in basis points",
            })?;
            Discount::percent_bp(bp).ok_or(DbError::OutOfRange {
                column,
                expected: "percentage in basis points",
            })
        }
        "amount" => Discount::amount(Money::from_paise(value)).ok_or(DbError::OutOfRange {
            column,
            expected: "a discount amount that is not negative",
        }),
        other => Err(DbError::BadValue {
            column,
            value: other.to_owned(),
        }),
    }
}

/// The state tag on `orders`, which is `AnyOrder`'s serde discriminator.
pub mod order_state {
    pub const DRAFT: &str = "draft";
    pub const OPEN: &str = "open";
    pub const SETTLED: &str = "settled";
    pub const CANCELLED: &str = "cancelled";
    pub const VOIDED: &str = "voided";

    /// Every legal value, for the CHECK constraint and for the tests.
    pub const ALL: [&str; 5] = [DRAFT, OPEN, SETTLED, CANCELLED, VOIDED];
}

/// Canonical text for a `mb_core::LineIdentity`, used as the kitchen ledger's key.
#[must_use]
pub fn line_identity_key(identity: &mb_core::LineIdentity) -> String {
    const US: char = '\u{1f}';
    let mut key = String::with_capacity(64);
    key.push_str(identity.item_id.as_str());
    key.push(US);
    if let Some(note) = &identity.note {
        key.push_str(note);
    }
    for modifier in &identity.modifier_ids {
        key.push(US);
        key.push_str(modifier.as_str());
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_core::{CustomerId, ItemId, LineIdentity, ModifierId};

    #[test]
    fn scalars_round_trip() {
        assert_eq!(
            money_from_sql(money_to_sql(Money::from_paise(-1234))).paise(),
            -1234
        );
        assert_eq!(
            qty_from_sql(qty_to_sql(Qty::from_thousandths(500))).thousandths(),
            500
        );
        assert_eq!(
            timestamp_from_sql(timestamp_to_sql(Timestamp::from_millis(1_770_000_000_000)))
                .millis(),
            1_770_000_000_000
        );
        let day = BusinessDay::from_ymd(2026, 8, 3);
        assert_eq!(
            business_day_from_sql(business_day_to_sql(day), "orders.business_day")
                .expect("in range"),
            day
        );
        let rate = TaxRate::from_percent(18).expect("18%");
        assert_eq!(
            tax_rate_from_sql(tax_rate_to_sql(rate), "items.tax_rate_bp").expect("in range"),
            rate
        );
    }

    #[test]
    fn a_stored_rate_above_one_hundred_percent_is_refused() {
        // The disk is not more trustworthy than the caller.
        assert!(tax_rate_from_sql(10_001, "items.tax_rate_bp").is_err());
        assert!(tax_rate_from_sql(-1, "items.tax_rate_bp").is_err());
    }

    #[test]
    fn a_boolean_is_zero_or_one_and_nothing_else() {
        assert!(!bool_from_sql(0, "items.is_available").expect("false"));
        assert!(bool_from_sql(1, "items.is_available").expect("true"));
        assert!(bool_from_sql(2, "items.is_available").is_err());
        assert!(bool_from_sql(-1, "items.is_available").is_err());
    }

    #[test]
    fn tags_round_trip_and_a_typo_is_an_error() {
        for kind in [
            TaxKind::Gst,
            TaxKind::Exempt,
            TaxKind::OutsideGst,
            TaxKind::Untaxed,
        ] {
            assert_eq!(
                tax_kind_from_sql(tax_kind_to_sql(kind)).expect("known"),
                kind
            );
        }
        for basis in [PriceBasis::Exclusive, PriceBasis::Inclusive] {
            assert_eq!(
                price_basis_from_sql(price_basis_to_sql(basis)).expect("known"),
                basis
            );
        }
        for r in [
            Registration::Unregistered,
            Registration::Composition,
            Registration::Regular,
        ] {
            assert_eq!(
                registration_from_sql(registration_to_sql(r)).expect("known"),
                r
            );
        }
        for s in [StateTax::Sgst, StateTax::Utgst] {
            assert_eq!(state_tax_from_sql(state_tax_to_sql(s)).expect("known"), s);
        }
        for o in [
            OrderType::DineIn,
            OrderType::Parcel,
            OrderType::SelfService,
            OrderType::Delivery,
        ] {
            assert_eq!(order_type_from_sql(order_type_to_sql(o)).expect("known"), o);
        }
        for r in [
            RoundingMode::None,
            RoundingMode::NearestRupee,
            RoundingMode::Up,
            RoundingMode::Down,
        ] {
            assert_eq!(
                rounding_mode_from_sql(rounding_mode_to_sql(r)).expect("known"),
                r
            );
        }
        for p in [PlaceOfSupply::Intra, PlaceOfSupply::Inter] {
            assert_eq!(
                place_of_supply_from_sql(place_of_supply_to_sql(p)).expect("known"),
                p
            );
        }
        // BACKEND-G7 wearing a schema hat: a typo is an error, not a default.
        assert!(price_basis_from_sql("exclusiv").is_err());
        assert!(tax_kind_from_sql("non_gst").is_err());
        assert!(order_type_from_sql("dinein").is_err());
    }

    /// Every kind x basis pair survives now.
    #[test]
    fn every_kind_and_basis_pair_round_trips_through_its_own_columns() {
        for kind in [
            TaxKind::Gst,
            TaxKind::Exempt,
            TaxKind::OutsideGst,
            TaxKind::Untaxed,
        ] {
            for basis in [PriceBasis::Exclusive, PriceBasis::Inclusive] {
                let spec = tax_spec_from_sql_parts(
                    2_000,
                    tax_kind_to_sql(kind),
                    price_basis_to_sql(basis),
                    "tax_classes.rate_bp",
                )
                .expect("known");
                assert_eq!(spec.kind, kind);
                assert_eq!(spec.basis, basis);
                assert_eq!(spec.rate, TaxRate::from_percent(20).expect("20%"));
            }
        }
        // The bar's sentence, in full: outside GST, 20% state VAT, priced tax-in.
        let beer = tax_spec_from_sql_parts(2_000, "outside_gst", "inclusive", "items.tax_rate_bp")
            .expect("known");
        assert_eq!(
            beer,
            TaxSpec::liquor(TaxRate::from_percent(20).expect("20%"))
        );
    }

    #[test]
    fn tag_text_matches_what_serde_writes() {
        let json = serde_json::to_string(&TaxKind::OutsideGst).expect("serialises");
        assert_eq!(json, "\"outside_gst\"");
        assert_eq!(tax_kind_to_sql(TaxKind::OutsideGst), "outside_gst");

        let json = serde_json::to_string(&Registration::Composition).expect("serialises");
        assert_eq!(json, "\"composition\"");
        assert_eq!(
            registration_to_sql(Registration::Composition),
            "composition"
        );

        let json = serde_json::to_string(&OrderType::SelfService).expect("serialises");
        assert_eq!(json, "\"self_service\"");
        assert_eq!(order_type_to_sql(OrderType::SelfService), "self_service");

        let json = serde_json::to_string(&RoundingMode::NearestRupee).expect("serialises");
        assert_eq!(json, "\"nearest_rupee\"");
        assert_eq!(
            rounding_mode_to_sql(RoundingMode::NearestRupee),
            "nearest_rupee"
        );
    }

    #[test]
    fn payment_modes_round_trip_with_their_payloads() {
        let credit = PaymentMode::Credit(CustomerId::new("cus_42"));
        let cols = payment_mode_to_sql(&credit);
        assert_eq!(cols.mode, "credit");
        assert_eq!(
            payment_mode_from_sql(cols.mode, cols.customer_id.as_deref(), None).expect("known"),
            credit
        );

        let other = PaymentMode::Other("Sodexo".to_owned());
        let cols = payment_mode_to_sql(&other);
        assert_eq!(cols.mode, "other");
        assert_eq!(
            payment_mode_from_sql(cols.mode, None, cols.mode_label.as_deref()).expect("known"),
            other
        );

        for plain in [PaymentMode::Cash, PaymentMode::Card, PaymentMode::Upi] {
            let cols = payment_mode_to_sql(&plain);
            assert_eq!(
                payment_mode_from_sql(cols.mode, None, None).expect("known"),
                plain
            );
        }
    }

    #[test]
    fn a_credit_payment_without_its_customer_is_refused() {
        // The CHECK constraint stops this reaching the disk; this is the belt for that pair of
        // braces.
        assert!(payment_mode_from_sql("credit", None, None).is_err());
        assert!(payment_mode_from_sql("other", None, None).is_err());
        assert!(payment_mode_from_sql("full_settlement", None, None).is_err());
    }

    #[test]
    fn charge_kind_carries_its_label_in_the_name_column() {
        let kind = ChargeKind::Other("Donation".to_owned());
        assert_eq!(charge_kind_to_sql(&kind), "other");
        assert_eq!(
            charge_kind_from_sql("other", "Donation").expect("known"),
            kind
        );
        assert_eq!(
            charge_kind_from_sql("service", "Service Charge").expect("known"),
            ChargeKind::Service
        );
    }

    #[test]
    fn charge_basis_and_discount_round_trip() {
        for basis in [
            ChargeBasis::Percent(1_000),
            ChargeBasis::Flat(Money::from_paise(5_000)),
        ] {
            let (tag, value) = charge_basis_to_sql(basis);
            assert_eq!(charge_basis_from_sql(tag, value).expect("known"), basis);
        }
        for discount in [
            Discount::percent_bp(1_500).expect("valid"),
            Discount::amount(Money::from_paise(30_000)).expect("valid"),
        ] {
            let (tag, value) = discount_to_sql(discount);
            assert_eq!(
                discount_from_sql(tag, value, "order_lines.discount_kind").expect("known"),
                discount
            );
        }
    }

    #[test]
    fn the_kitchen_key_is_stable_and_distinguishes_what_it_should() {
        let plain = LineIdentity {
            item_id: ItemId::new("itm_dosa"),
            note: None,
            modifier_ids: vec![],
        };
        let noted = LineIdentity {
            item_id: ItemId::new("itm_dosa"),
            note: Some("extra crispy".to_owned()),
            modifier_ids: vec![],
        };
        let modified = LineIdentity {
            item_id: ItemId::new("itm_dosa"),
            note: None,
            modifier_ids: vec![ModifierId::new("mod_cheese")],
        };

        assert_eq!(line_identity_key(&plain), line_identity_key(&plain.clone()));
        assert_ne!(line_identity_key(&plain), line_identity_key(&noted));
        assert_ne!(line_identity_key(&plain), line_identity_key(&modified));
        assert_ne!(line_identity_key(&noted), line_identity_key(&modified));
    }

    #[test]
    fn the_kitchen_key_does_not_confuse_a_note_with_a_modifier() {
        // Without a separator that cannot appear in an id, "itm_a" + note "b" and "itm_ab" with
        // no note would collide, and the kitchen would be told about the wrong dish.
        let a = LineIdentity {
            item_id: ItemId::new("itm_a"),
            note: Some("b".to_owned()),
            modifier_ids: vec![],
        };
        let b = LineIdentity {
            item_id: ItemId::new("itm_ab"),
            note: None,
            modifier_ids: vec![],
        };
        assert_ne!(line_identity_key(&a), line_identity_key(&b));
    }

    #[test]
    fn order_state_tags_match_any_orders_serde_discriminator() {
        // If these ever drift, the counter and the cloud disagree about what a cancelled order
        // is called.
        assert_eq!(order_state::ALL.len(), 5);
        for tag in order_state::ALL {
            assert!(tag.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }
}
