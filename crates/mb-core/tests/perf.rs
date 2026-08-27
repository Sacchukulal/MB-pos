//! ```text
//! cargo test --release --test perf -- --nocapture
//! ```

// This is a measuring harness, not shipped code.
#![allow(
    clippy::expect_used,
    clippy::integer_division,
    reason = "a stopwatch: expect is the assertion, and elapsed / runs is the measurement"
)]

use mb_core::{
    Cart, Charge, ChargeKind, Discount, DiscountEntry, ItemId, ItemSnapshot, Modifier, ModifierId,
    Money, PlaceOfSupply, Qty, Registration, RoundingMode, TaxRate, TaxSpec, bill::BillInput,
    compute_bill,
};
use std::time::Instant;

const BUDGET_NANOS: u128 = 200_000;
const CEILING_NANOS: u128 = 1_000_000;

/// A busy table: 50 lines, every treatment, fractional quantities, modifiers and discounts
/// scattered through it.
fn busy_cart() -> Cart {
    let pc = |percent: u32| TaxRate::from_percent(percent).expect("a real rate");
    let rates = [pc(5), pc(12), pc(18), pc(28)];
    // Every shape a line can take, so the budget is measured against the worst realistic bill.
    let specs = [
        TaxSpec::gst(pc(5)),
        TaxSpec::gst_inclusive(pc(18)),
        TaxSpec::liquor(pc(20)),
        TaxSpec::exempt(),
        TaxSpec::untaxed(),
    ];
    let mut cart = Cart::new();
    for i in 0..50_usize {
        let snapshot = ItemSnapshot::new(
            ItemId::new(format!("itm_{i:03}")),
            format!("Item number {i}"),
            Money::from_paise(4_500 + i64::try_from(i).unwrap_or(0) * 137),
            rates[i % rates.len()],
        )
        .with_tax(specs[i % specs.len()])
        .with_hsn("996331");

        let modifiers = if i % 5 == 0 {
            vec![Modifier::new(
                ModifierId::new("mod_extra"),
                "Extra portion",
                Money::from_paise(2_500),
            )]
        } else {
            vec![]
        };

        // Fractional on every third line, so the rounding path is exercised.
        let qty = if i % 3 == 0 {
            Qty::from_thousandths(500 + i64::try_from(i).unwrap_or(0) * 7)
        } else {
            Qty::from_whole(1 + i64::try_from(i % 4).unwrap_or(0)).unwrap_or(Qty::ONE)
        };

        let index = cart
            .add(snapshot, qty, Some(format!("note {i}")), modifiers)
            .expect("adds");

        if i % 4 == 0 {
            cart.set_line_discount(index, Discount::percent_bp(500).map(DiscountEntry::new))
                .expect("sets");
        }
    }
    cart
}

/// The three charges a real bill carries, each with its own rate.
fn charges() -> Vec<Charge> {
    let pc = |percent: u32| TaxRate::from_percent(percent).expect("a real rate");
    vec![
        Charge::percent(ChargeKind::Service, "Service Charge", 1_000, pc(18)),
        Charge::flat(
            ChargeKind::Packing,
            "Packing",
            Money::from_paise(2_000),
            pc(5),
        ),
        Charge::flat(
            ChargeKind::Delivery,
            "Delivery",
            Money::from_paise(4_000),
            pc(18),
        ),
    ]
}

#[test]
fn compute_bill_stays_within_budget_b4() {
    let cart = busy_cart();
    assert_eq!(cart.len(), 50, "the budget is written against 50 lines");
    let charges = charges();

    let input = || {
        BillInput::new(&cart, Registration::Regular)
            .with_place_of_supply(PlaceOfSupply::Intra)
            .with_charges(&charges)
            .with_rounding(RoundingMode::NearestRupee)
            .with_bill_discount(DiscountEntry::new(
                Discount::percent_bp(1_000).expect("valid"),
            ))
    };

    // Warm up: first call pays for page faults and branch prediction, and measuring that would
    // measure the machine rather than the code.
    for _ in 0..200 {
        let bill = compute_bill(input()).expect("computes");
        std::hint::black_box(&bill);
    }

    const RUNS: u128 = 2_000;
    let started = Instant::now();
    for _ in 0..RUNS {
        let bill = compute_bill(input()).expect("computes");
        std::hint::black_box(&bill);
    }
    let per_call = started.elapsed().as_nanos() / RUNS;

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    // Printed as integers: the workspace denies float arithmetic outright, and there is no
    // reason for a stopwatch to be the one exception.
    println!(
        "\nB4  compute_bill, 50 lines : {per_call} ns/call  ({}.{:03} µs)  [{profile}]\n    budget {BUDGET_NANOS} ns, ceiling {CEILING_NANOS} ns",
        per_call / 1_000,
        per_call % 1_000
    );
    if per_call > BUDGET_NANOS && !cfg!(debug_assertions) {
        println!("    OVER BUDGET but inside the ceiling — see PERFORMANCE.md §3.4");
    }

    // Debug builds measure and print but do not assert; see the module docs.
    if !cfg!(debug_assertions) {
        assert!(
            per_call <= CEILING_NANOS,
            "compute_bill took {per_call} ns per call, over the {CEILING_NANOS} ns ceiling for \
             budget B4. Look for an allocation or a clone in the per-line loop before optimising \
             the arithmetic — 50 lines of i64 maths is not what costs a millisecond."
        );
    }
}
