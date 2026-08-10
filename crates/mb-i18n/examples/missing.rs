//! **What is still waiting for a person** — P23 §7.
//!
//! ```text
//! cargo run -p mb-i18n --example missing
//! ```
//!
//! A session can produce plausible Hindi and Kannada. It cannot tell a
//! plausible string from a wrong one — and "wrong word on a bill" includes
//! *void*, *refund*, *credit* and *day close*, words a shopkeeper is answerable
//! to a tax officer for.
//!
//! So this prints the gap as a NUMBER, and prints the rows, so a reviewer has a
//! list rather than a feeling. Redirect it to a file, hand that to somebody who
//! reads the language, and change `NeedsReview` to `Reviewed` as they go.

#![allow(
    clippy::print_stdout,
    reason = "a developer tool whose whole job is to print"
)]

use mb_i18n::catalogue::{CATALOGUE, State};
use mb_i18n::Language;

fn main() {
    let waiting: Vec<_> = CATALOGUE
        .iter()
        .filter(|entry| entry.state == State::NeedsReview)
        .collect();

    println!("Magic Bill — strings waiting for a reviewer");
    println!("===========================================\n");
    println!("{} of {} rows.\n", waiting.len(), CATALOGUE.len());

    if waiting.is_empty() {
        println!("Nothing is waiting. Update the master plan and RELEASE.md.");
        return;
    }

    println!("Give this to somebody who READS the language. Change the row's");
    println!("state to `Reviewed` in crates/mb-i18n/src/catalogue.rs as you go.\n");
    println!("The policy, before you start: where a term has no good local");
    println!("equivalent, the English word in the local script beats a coined");
    println!("one nobody uses — बिल, not a Sanskritised construction.\n");

    for language in [Language::Hindi, Language::Kannada] {
        println!("--- {} ({}) ---\n", language.endonym(), language.code());
        for entry in &waiting {
            println!("  {:<28} {}", entry.key, entry.en);
            println!("  {:<28} {}", "", entry.in_language(language));
            if let Some(plurals) = entry.plural {
                let (zero, other) = match language {
                    Language::Hindi => (plurals.zero_hi, plurals.other_hi),
                    Language::Kannada => (plurals.zero_kn, plurals.other_kn),
                    Language::English => (plurals.zero_en, plurals.other_en),
                };
                println!("  {:<28} 0: {zero}", "");
                println!("  {:<28} n: {other}", "");
            }
            println!();
        }
    }

    println!("Deliberately English, and not up for review:");
    for entry in CATALOGUE
        .iter()
        .filter(|e| e.state == State::DeliberatelyEnglish)
    {
        println!("  {:<28} {}", entry.key, entry.en);
    }
}
