//! **The recovery slip — the shop's last way back into its own counter.**
//!
//! # Why this file did not exist until 2026-08-22
//!
//! It should have. `mb_auth::recovery` has said since it was written that the
//! code is *"Shown once, and **printed on the shop's own printer** — there is a
//! printer, and paper in a drawer is a better place for this than a
//! screenshot."* The audit log agreed: `recovery.issued` reads back to a
//! shopkeeper as *"New recovery code printed"*.
//!
//! Nobody printed it. The code was returned to the screen, shown once in a
//! dialog, and that was the whole of it — so a shop that pressed *I have
//! written it down* without a pen to hand had lost the only thing that gets it
//! back in, while its own history said a slip had come out. Found going through
//! the sign-in path after the owner asked, on 2026-08-22, for it to be gone
//! through properly.
//!
//! # What it has to be, and it is not a receipt
//!
//! This slip is read **months later, by somebody who is stuck**, off paper that
//! has been in a drawer. So:
//!
//! * **the code is the biggest thing on it**, in two groups of five, with a
//!   generous size rather than the body size a bill line gets;
//! * it says **what it is for and what to do with it** in the shop's own
//!   language, because the person reading it has forgotten why they kept it;
//! * it carries the **shop's name and the date**, so a slip found loose in a
//!   drawer belongs to something;
//! * it says the old code is dead, because a shop that keeps two slips will
//!   reach for the wrong one on the day it matters;
//! * and it does **not** say whose PIN it set. That name is a hint about who to
//!   attack, and the slip lives in a drawer rather than a safe.
//!
//! There is no barcode and no QR. A code that can be scanned off a photograph
//! of the slip is a code that does not need to be in the drawer.

use crate::doc::{Align, Document, Pattern, Style};
use crate::paper::Paper;
use crate::template::bill::Store;

/// Everything the slip says. Small on purpose — the whole point of this paper
/// is that it contains one secret and as little else as possible.
#[derive(Debug, Clone)]
pub struct RecoveryContext<'a> {
    /// Already grouped for reading aloud: `ABCDE-FGHJK`.
    /// [`mb_auth::recovery::RecoveryCode::to_print`] is what makes it.
    pub code: &'a str,
    /// The shop, so a slip in a drawer belongs to something. `None` on a first
    /// run, when the shop has a PIN before it has finished having a name.
    pub store: Option<&'a Store>,
    /// The day this was issued, as the shop writes dates — preformatted by the
    /// caller (D39: nothing in this crate formats a date or an amount).
    pub issued_on: &'a str,
    /// True when this replaced an earlier code, so the slip can say the old one
    /// is dead. False for a shop's first one, where saying so would be
    /// puzzling.
    pub replaces_an_older_code: bool,
}

/// The slip, ready for the queue.
#[must_use]
pub fn recovery_document(paper: Paper, context: &RecoveryContext<'_>) -> Document {
    let mut doc = Document::new(paper);

    doc.text(
        context.store.map_or("MAGIC BILL", |s| s.name.as_str()),
        Style::BOLD,
        Align::Centre,
    );
    doc.text("RECOVERY CODE", Style::new(2, true), Align::Centre);
    doc.separator(Pattern::Double);
    doc.spacer(1);

    // **The one thing on this paper that matters.** Two lines of a bill's
    // height would be cheaper; this is read by somebody in a hurry, off paper
    // that has been in a drawer, and getting one character wrong costs them the
    // shop.
    doc.text(context.code, Style::new(2, true), Align::Centre);

    doc.spacer(1);
    doc.separator(Pattern::Dashed);

    doc.text(
        "Keep this somewhere only you can reach.",
        Style::BOLD,
        Align::Left,
    );
    doc.line("If the owner forgets their PIN, this code is the way back in.");
    doc.line("Type it on the sign-in screen under \"Forgotten your PIN?\".");
    doc.spacer(1);

    if context.replaces_an_older_code {
        // A shop that keeps both slips will reach for the wrong one on the day
        // it matters, and that day is always a bad one.
        doc.text("THE OLD CODE NO LONGER WORKS.", Style::BOLD, Align::Left);
        doc.line("Throw the old slip away.");
        doc.spacer(1);
    }

    doc.line("Using it prints a new code and this one stops working.");
    doc.line("It is not stored anywhere it can be looked up.");

    doc.spacer(1);
    doc.separator(Pattern::Dashed);
    doc.row("Issued", context.issued_on, Style::NORMAL);
    // Deliberately NOT whose PIN it set — see the note at the top of this file.

    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Block;
    use crate::paper::PaperKind;

    fn a_slip(replaces: bool) -> Document {
        let store = Store {
            name: "Anna's Kitchen".to_owned(),
            ..Store::default()
        };
        recovery_document(
            Paper::new(PaperKind::Mm80),
            &RecoveryContext {
                code: "ABCDE-FGHJK",
                store: Some(&store),
                issued_on: "22 August 2026",
                replaces_an_older_code: replaces,
            },
        )
    }

    fn words(doc: &Document) -> String {
        doc.blocks
            .iter()
            .map(|block| match block {
                Block::Text { content, .. } => content.clone(),
                Block::Row { left, right, .. } => format!("{left} {right}"),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_code_is_on_the_paper_and_it_is_the_biggest_thing_on_it() {
        let doc = a_slip(false);
        let printed = words(&doc);
        assert!(printed.contains("ABCDE-FGHJK"), "{printed}");

        // Whatever else changes about this slip, the code must not end up at
        // body size among the sentences. It is read off a drawer months later.
        let size = doc
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Text { content, style, .. } if content == "ABCDE-FGHJK" => Some(style.size),
                _ => None,
            })
            .expect("the code is a text block");
        let body = Style::NORMAL.size;
        assert!(size > body, "the code is {size} dots and the body is {body}");
    }

    #[test]
    fn it_names_the_shop_and_the_day() {
        let printed = words(&a_slip(false));
        assert!(printed.contains("Anna's Kitchen"), "{printed}");
        assert!(printed.contains("22 August 2026"), "{printed}");
    }

    /// **The slip must not say whose PIN it set.** It lives in a drawer, and a
    /// name on it is a hint about who to attack. There is no field for it in
    /// [`RecoveryContext`] on purpose; this test is what stops one being added
    /// without the argument being had again.
    #[test]
    fn it_does_not_name_a_person() {
        let printed = words(&a_slip(true)).to_lowercase();
        for hint in ["pin was set for", "for:", "staff", "owner:"] {
            assert!(!printed.contains(hint), "{hint:?} is on the slip: {printed}");
        }
    }

    #[test]
    fn a_replacement_says_the_old_slip_is_dead() {
        let first = words(&a_slip(false));
        let replacement = words(&a_slip(true));
        assert!(
            !first.contains("OLD CODE"),
            "a shop's first code has no old one to retire: {first}"
        );
        assert!(replacement.contains("THE OLD CODE NO LONGER WORKS."), "{replacement}");
        assert!(replacement.contains("Throw the old slip away."), "{replacement}");
    }

    #[test]
    fn it_prints_on_a_shop_that_has_no_name_yet() {
        // A PIN can be set before the shop profile is finished, and a slip that
        // panicked or came out blank there would be the worst possible moment.
        let doc = recovery_document(
            Paper::new(PaperKind::Mm58),
            &RecoveryContext {
                code: "MNPQR-STUVW",
                store: None,
                issued_on: "22 August 2026",
                replaces_an_older_code: false,
            },
        );
        let printed = words(&doc);
        assert!(printed.contains("MAGIC BILL"), "{printed}");
        assert!(printed.contains("MNPQR-STUVW"), "{printed}");
    }

    /// **No barcode and no QR, and that is a decision.** A code that can be
    /// scanned off a photograph of the slip is a code that did not need to be
    /// in the drawer.
    #[test]
    fn the_code_cannot_be_scanned_off_a_photograph() {
        let doc = a_slip(true);
        assert!(
            !doc.blocks
                .iter()
                .any(|b| matches!(b, Block::QrCode { .. } | Block::Barcode { .. })),
            "the slip carries a machine-readable copy of the code"
        );
    }
}
