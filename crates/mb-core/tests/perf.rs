//! Performance budget B4 — decision D12.
//!
//! `docs/PERFORMANCE.md` §2.2:
//!
//! > **B4** `compute_bill`, 50 lines, mixed rates, discounts, charges —
//! > budget **200 µs**, ceiling **1 ms**, on the reference machine
//! > (i3, 4 GB RAM, 5400 rpm HDD, Windows 10).
//!
//! Run it with:
//!
//! ```text
//! cargo test --release --test perf -- --nocapture
//! ```
//!
//! **The assertion runs in release builds only.** A debug build of this crate
//! is 10-50× slower, and a test that fails on every `cargo test` is a test
//! everyone learns to ignore. In debug it still measures and prints, so the
//! number is always in front of you.
//!
//! No benchmark framework. Compiling criterion to time a 200 µs function is
//! not a trade worth making (rule R6); `Instant` is enough to catch the kind
//! of regression that matters — an accidental clone or an allocation inside
//! the loop, not a 3% drift.

// This is a measuring harness, not shipped code. `expect` here IS the
// assertion, and dividing an elapsed total by the number of runs is the
// measurement itself — the workspace denies both because of D7, which is about
// the money path. (The clippy.toml exemption only reaches `#[test]` functions,
// and the cart builder below is a plain helper.)
#![allow(
    clippy::expect_used,
    clippy::integer_division,
    reason = "a stopwatch: expect is the assertion, and elapsed / runs is the measurement"
)]

use mb_core::{
    Cart, Discount, ItemId, ItemSnapshot, Modifier, ModifierId, Money, PlaceOfSupply, Qty, TaxRate,
    TaxTreatment, bill::BillInput, compute_bill,
};
use std::time::Instant;

/// The budget and the ceiling, from `docs/PERFORMANCE.md`.
const BUDGET_NANOS: u128 = 200_000;
const CEILING_NANOS: u128 = 1_000_000;

/// A busy table: 50 lines, every treatment, fractional quantities, modifiers
/// and discounts scattered through it. Deliberately worse than a real bill —
/// a 50-line order is a large party, and most bills are under ten.
fn busy_cart() -> Cart {
    let rates = [TaxRate::GST_5, TaxRate::GST_12, TaxRate::GST_18, TaxRate::GST_28];
    let treatments = [
        TaxTreatment::Exclusive,
        TaxTreatment::Inclusive,
        TaxTreatment::Exempt,
        TaxTreatment::NonGst,
    ];

    let mut cart = Cart::new();
    for i in 0..50_usize {
        let snapshot = ItemSnapshot::new(
            ItemId::new(format!("itm_{i:03}")),
            format!("Item number {i}"),
            Money::from_paise(4_500 + i64::try_from(i).unwrap_or(0) * 137),
            rates[i % rates.len()],
        )
        .with_treatment(treatments[i % treatments.len()])
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
            cart.set_line_discount(index, Discount::percent_bp(500))
                .expect("sets");
        }
    }
    cart
}

#[test]
fn compute_bill_stays_within_budget_b4() {
    let cart = busy_cart();
    assert_eq!(cart.len(), 50, "the budget is written against 50 lines");

    let input = || {
        BillInput::new(&cart)
            .with_place_of_supply(PlaceOfSupply::Intra)
            .with_bill_discount(Discount::percent_bp(1_000).expect("valid"))
    };

    // Warm up: first call pays for page faults and branch prediction, and
    // measuring that would measure the machine rather than the code.
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

    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    // Printed as integers: the workspace denies float arithmetic outright (D2),
    // and there is no reason for a stopwatch to be the one exception.
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
