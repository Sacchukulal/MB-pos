//! **The trail, and the reason it can be believed.**
//!
//! > Audit **C4**: *"No audit trail on the counter. Nothing records who deleted
//! > an item, who changed a price, who reprinted a bill, who edited the bill
//! > counter, who changed a payment mode. The cloud admin panel has a full
//! > audit log; the till has none."* Its fix asks for a *"local,
//! > **tamper-evident** action log"*.
//!
//! # Append-only is not tamper-evident, and the difference is the whole point
//!
//! Migration 0001 carries two triggers that make `UPDATE` and `DELETE` on
//! `audit_log` abort. That stops this program and every accident inside it —
//! and it stops nobody at all with a SQLite browser, who can drop a trigger in
//! one statement.
//!
//! So each row also carries `hash = sha256(prev_hash ‖ its own fields)`.
//! Editing a row changes its hash; deleting one breaks the link *and* leaves a
//! gap in `seq`; reordering two breaks both. [`verify_chain`] reports **the
//! first `seq` where it breaks**, and the audit screen turns that into a
//! sentence with a date in it.
//!
//! That is evidence rather than prevention, and evidence is the honest goal: a
//! shop's own machine must be able to read the shop's own file, so nothing on
//! this counter can *stop* an owner with a hex editor. It can make sure they
//! cannot do it quietly.
//!
//! # One writer, so `seq` is just `MAX(seq) + 1`
//!
//! P04's `conn.rs` gives this database one writer and four readers. Sequencing
//! inside the writing transaction is therefore exact, and does not need the
//! `counters` machinery — a bill number has to survive being handed out and not
//! used, and an audit sequence never is.

use mb_core::{BusinessDay, StaffId, Timestamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// What happened. `&'static str` on purpose: an action is a value in the
/// program, not a sentence somebody typed, and the audit screen groups by it.
pub type AuditAction = &'static str;

/// Every action this build can write. Listed so the audit screen's filter can
/// be built from the list rather than from whatever it happens to find in a
/// shop's data — a filter built from the data cannot offer "voided a bill" to a
/// shop that has never voided one, which is the moment somebody needs it.
pub mod action {
    use super::AuditAction;

    pub const LOGIN_OK: AuditAction = "login.ok";
    pub const LOGIN_FAILED: AuditAction = "login.failed";
    pub const LOGOUT: AuditAction = "logout";
    pub const LOCKED: AuditAction = "locked";
    pub const DENIED: AuditAction = "denied";
    pub const PIN_SET: AuditAction = "pin.set";
    pub const RECOVERY_USED: AuditAction = "recovery.used";
    pub const RECOVERY_ISSUED: AuditAction = "recovery.issued";
    pub const STAFF_SAVED: AuditAction = "staff.saved";
    pub const STAFF_STATUS: AuditAction = "staff.status";
    pub const ROLE_SAVED: AuditAction = "role.saved";
    pub const BILL_SETTLED: AuditAction = "bill.settled";
    pub const BILL_VOIDED: AuditAction = "bill.voided";
    pub const BILL_REPRINTED: AuditAction = "bill.reprinted";
    pub const ORDER_CANCELLED: AuditAction = "order.cancelled";
    /// P14, scope 1.23 — the party changed seats.
    pub const ORDER_MOVED: AuditAction = "order.moved";
    /// P14, scope 1.22 — two tables became one bill.
    pub const ORDER_MERGED: AuditAction = "order.merged";
    /// P14, scope 1.21 — one table became two bills.
    pub const ORDER_SPLIT: AuditAction = "order.split";
    /// P15, scope 5.1 — money handed over against an account.
    pub const CREDIT_TAKEN: AuditAction = "credit.taken";
    /// P15 — an opening balance, a write-off or a correction.
    pub const CREDIT_ADJUSTED: AuditAction = "credit.adjusted";
    /// P15, scope 5.2 — somebody approved a bill past the limit.
    pub const CREDIT_LIMIT_OVERRIDDEN: AuditAction = "credit.limit_overridden";
    /// P16, scope 10.6 — money the shop spent. Audit B15: v1 could neither
    /// edit an expense nor account for an edit.
    pub const EXPENSE_SAVED: AuditAction = "expense.saved";
    pub const EXPENSE_DELETED: AuditAction = "expense.deleted";
    /// P16 — the float, a top-up, a payout, a bank drop.
    pub const CASH_MOVED: AuditAction = "cash.moved";
    pub const ITEM_VOIDED: AuditAction = "item.voided";
    pub const DISCOUNT_GIVEN: AuditAction = "discount.given";
    pub const DISCOUNT_REFUSED: AuditAction = "discount.refused";
    pub const PRICE_CHANGED: AuditAction = "price.changed";
    pub const SETTING_CHANGED: AuditAction = "setting.changed";
    pub const COUNTER_CHANGED: AuditAction = "counter.changed";
    pub const DRAWER_OPENED: AuditAction = "drawer.opened";
    pub const DAY_CLOSED: AuditAction = "day.closed";
    /// P18 — a locked day was opened again. **Its own action**, and never a
    /// second `DAY_CLOSED`: an owner asking "who unlocked Tuesday?" must be
    /// able to search for it.
    pub const DAY_REOPENED: AuditAction = "day.reopened";
    /// P19 — a phone was let onto the counter, or taken off it. Its own
    /// actions, because "who added that tablet?" is a question an owner asks
    /// months later and has to be able to search for.
    pub const DEVICE_PAIRED: AuditAction = "device.paired";
    pub const DEVICE_REVOKED: AuditAction = "device.revoked";
    /// P20 — something a phone asked the counter to do. "Who cancelled that
    /// item?" has to be answerable a month later; v1 kept two days.
    pub const INTENT_APPLIED: AuditAction = "intent.applied";
    pub const BACKUP_RESTORED: AuditAction = "backup.restored";
    /// P21 — the licence. **These are the shop's history and they go in the
    /// hash chain**, which is why this session adds no `licence_events` table:
    /// the audit trail already is that table, it is already append-only by
    /// trigger and tamper-evident by hash (D43), and a second history of the
    /// same events is a second answer to the same question.
    ///
    /// What is NOT here is the licence itself — that lives beside the config
    /// and never in the shop's database, because a backup is restored onto
    /// other machines (D85, and D27 before it).
    pub const LICENCE_ACTIVATED: AuditAction = "licence.activated";
    pub const LICENCE_DEACTIVATED: AuditAction = "licence.deactivated";
    pub const LICENCE_TRANSFERRED: AuditAction = "licence.transferred";
    pub const LICENCE_EMERGENCY: AuditAction = "licence.emergency";
    /// A licensing action that was refused — a wrong emergency code, a transfer
    /// inside its cooldown. Recorded because five of these in a row is the
    /// thing an owner would want to have been able to see afterwards.
    pub const LICENCE_REFUSED: AuditAction = "licence.refused";

    // --- P25, the stock book -------------------------------------------------
    //
    // **Only the things a person DECIDED are here.** A sale deducting rice is
    // not an audit row — it is the ledger, which already says who settled the
    // bill, and one audit row per material per bill would bury the trail this
    // table exists to be. What IS here is somebody changing a recipe, moving a
    // number by hand, or throwing food away.
    pub const MATERIAL_SAVED: AuditAction = "material.saved";
    pub const RECIPE_SAVED: AuditAction = "recipe.saved";
    /// **The one that needs watching.** An adjustment is how a real correction
    /// is made and also how a theft is covered up.
    pub const STOCK_ADJUSTED: AuditAction = "stock.adjusted";
    pub const STOCK_WASTED: AuditAction = "stock.wasted";
    /// Somebody recorded making a batch of a sub-recipe.
    pub const STOCK_PRODUCED: AuditAction = "stock.produced";
    /// The balance cache was rebuilt from the ledger (D114). Rare, and worth
    /// knowing about afterwards: it is what somebody does when a figure looks
    /// wrong, and the next question is always "what changed just before it".
    pub const STOCK_REBUILT: AuditAction = "stock.rebuilt";

    // --- P26, buying ---------------------------------------------------------
    //
    // The same rule as P25's: only what a person DECIDED. A purchase moving five
    // materials is not five rows; it is one, and the paper it points at names
    // every line.
    pub const SUPPLIER_SAVED: AuditAction = "supplier.saved";
    pub const PURCHASE_SAVED: AuditAction = "purchase.saved";
    /// **D125 — the only correction path a purchase has**, and therefore the row
    /// somebody will be asked about.
    pub const PURCHASE_CANCELLED: AuditAction = "purchase.cancelled";
    pub const PURCHASE_RETURNED: AuditAction = "purchase.returned";
    pub const SUPPLIER_PAID: AuditAction = "supplier.paid";
    pub const SUPPLIER_ADJUSTED: AuditAction = "supplier.adjusted";
    pub const ORDER_PLACED: AuditAction = "purchase.ordered";
    /// A count was started. Cheap, and it is what answers "who was in the store
    /// that night".
    pub const COUNT_OPENED: AuditAction = "count.opened";
    /// **The one that moves the book.** Approving a count is adjusting stock by
    /// hand at scale, so it is watched exactly as `stock.adjusted` is.
    pub const COUNT_APPROVED: AuditAction = "count.approved";
    pub const COUNT_ABANDONED: AuditAction = "count.abandoned";

    /// Every one of the above, for the screen's filter.
    pub const ALL: &[AuditAction] = &[
        LOGIN_OK,
        LOGIN_FAILED,
        LOGOUT,
        LOCKED,
        DENIED,
        PIN_SET,
        RECOVERY_USED,
        RECOVERY_ISSUED,
        STAFF_SAVED,
        STAFF_STATUS,
        ROLE_SAVED,
        BILL_SETTLED,
        BILL_VOIDED,
        BILL_REPRINTED,
        ORDER_CANCELLED,
        ORDER_MOVED,
        ORDER_MERGED,
        ORDER_SPLIT,
        CREDIT_TAKEN,
        CREDIT_ADJUSTED,
        CREDIT_LIMIT_OVERRIDDEN,
        EXPENSE_SAVED,
        EXPENSE_DELETED,
        CASH_MOVED,
        ITEM_VOIDED,
        DISCOUNT_GIVEN,
        DISCOUNT_REFUSED,
        PRICE_CHANGED,
        SETTING_CHANGED,
        COUNTER_CHANGED,
        DRAWER_OPENED,
        DAY_CLOSED,
        DAY_REOPENED,
        DEVICE_PAIRED,
        DEVICE_REVOKED,
        INTENT_APPLIED,
        BACKUP_RESTORED,
        LICENCE_ACTIVATED,
        LICENCE_DEACTIVATED,
        LICENCE_TRANSFERRED,
        LICENCE_EMERGENCY,
        LICENCE_REFUSED,
        MATERIAL_SAVED,
        RECIPE_SAVED,
        STOCK_ADJUSTED,
        STOCK_WASTED,
        STOCK_PRODUCED,
        STOCK_REBUILT,
        SUPPLIER_SAVED,
        PURCHASE_SAVED,
        PURCHASE_CANCELLED,
        PURCHASE_RETURNED,
        SUPPLIER_PAID,
        SUPPLIER_ADJUSTED,
        ORDER_PLACED,
        COUNT_OPENED,
        COUNT_APPROVED,
        COUNT_ABANDONED,
    ];

    /// What the owner reads, rather than the tag. UI_GUIDELINES §6.
    #[must_use]
    pub fn words(action: &str) -> &'static str {
        match action {
            LOGIN_OK => "Logged in",
            LOGIN_FAILED => "Wrong PIN",
            LOGOUT => "Logged out",
            LOCKED => "Screen locked",
            DENIED => "Was not allowed to",
            PIN_SET => "Set a PIN",
            RECOVERY_USED => "Used the recovery code",
            RECOVERY_ISSUED => "New recovery code printed",
            STAFF_SAVED => "Changed a staff member",
            STAFF_STATUS => "Changed who works here",
            ROLE_SAVED => "Changed a role",
            BILL_SETTLED => "Settled a bill",
            BILL_VOIDED => "Voided a bill",
            BILL_REPRINTED => "Reprinted a bill",
            ORDER_CANCELLED => "Cancelled an order",
            ORDER_MOVED => "Moved an order to another table",
            ORDER_MERGED => "Merged two tables into one bill",
            ORDER_SPLIT => "Split a bill",
            CREDIT_TAKEN => "Took a credit repayment",
            CREDIT_ADJUSTED => "Adjusted what a customer owes",
            CREDIT_LIMIT_OVERRIDDEN => "Approved a bill past the credit limit",
            EXPENSE_SAVED => "Recorded an expense",
            EXPENSE_DELETED => "Deleted an expense",
            CASH_MOVED => "Moved cash in or out of the drawer",
            ITEM_VOIDED => "Voided an item",
            DISCOUNT_GIVEN => "Gave a discount",
            DISCOUNT_REFUSED => "Tried to give too big a discount",
            PRICE_CHANGED => "Changed a price",
            SETTING_CHANGED => "Changed a setting",
            COUNTER_CHANGED => "Changed the bill counter",
            DRAWER_OPENED => "Opened the cash drawer",
            DAY_CLOSED => "Closed the day",
            DAY_REOPENED => "Opened a closed day again",
            DEVICE_PAIRED => "Added a phone to the counter",
            DEVICE_REVOKED => "Removed a phone from the counter",
            INTENT_APPLIED => "Changed an order from a phone",
            BACKUP_RESTORED => "Restored a backup",
            LICENCE_ACTIVATED => "Activated the licence",
            LICENCE_DEACTIVATED => "Deactivated the licence on this computer",
            LICENCE_TRANSFERRED => "Moved the licence to this computer",
            LICENCE_EMERGENCY => "Used an emergency unlock code",
            LICENCE_REFUSED => "A licensing action was refused",
            MATERIAL_SAVED => "Changed a material",
            RECIPE_SAVED => "Changed what a dish is made of",
            STOCK_ADJUSTED => "Adjusted stock by hand",
            STOCK_WASTED => "Recorded wastage",
            STOCK_PRODUCED => "Recorded making a batch",
            STOCK_REBUILT => "Rebuilt the stock balances from the ledger",
            SUPPLIER_SAVED => "Changed a supplier",
            PURCHASE_SAVED => "Entered a delivery",
            PURCHASE_CANCELLED => "Cancelled a delivery",
            PURCHASE_RETURNED => "Sent goods back to a supplier",
            SUPPLIER_PAID => "Paid a supplier",
            SUPPLIER_ADJUSTED => "Corrected what a supplier is owed",
            ORDER_PLACED => "Sent a purchase order",
            COUNT_OPENED => "Started counting the store",
            COUNT_APPROVED => "Approved a stock count",
            COUNT_ABANDONED => "Gave up on a stock count",
            _ => "Did something this version does not know about",
        }
    }
}

/// One thing that happened, on its way to the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub at: Timestamp,
    pub business_day: BusinessDay,
    /// `None` for a failed login against a name nobody could match — there is
    /// genuinely no staff member, and inventing one would be a lie in the one
    /// table that must not contain any.
    pub staff_id: Option<StaffId>,
    pub action: AuditAction,
    pub entity: &'static str,
    pub entity_id: Option<String>,
    /// The ONE place JSON is allowed in this product (migration 0001 says so):
    /// the shape differs per action and nothing ever queries inside it.
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
}

impl AuditEntry {
    /// The common case: something happened and there was no "before".
    #[must_use]
    pub fn new(
        at: Timestamp,
        business_day: BusinessDay,
        staff_id: Option<StaffId>,
        action: AuditAction,
        entity: &'static str,
    ) -> AuditEntry {
        AuditEntry {
            at,
            business_day,
            staff_id,
            action,
            entity,
            entity_id: None,
            before: None,
            after: None,
        }
    }

    #[must_use]
    pub fn about(mut self, entity_id: impl Into<String>) -> AuditEntry {
        self.entity_id = Some(entity_id.into());
        self
    }

    /// **Before and after, which is draft T4 and audit C4's real ask.** An
    /// audit that says "somebody changed a price" and not "from ₹120 to ₹90" is
    /// an audit that cannot settle an argument.
    #[must_use]
    pub fn changed(mut self, before: serde_json::Value, after: serde_json::Value) -> AuditEntry {
        self.before = Some(before);
        self.after = Some(after);
        self
    }

    #[must_use]
    pub fn with_after(mut self, after: serde_json::Value) -> AuditEntry {
        self.after = Some(after);
        self
    }
}

/// A row as it comes back out, with the two chain columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRow {
    pub id: String,
    pub seq: i64,
    pub at: i64,
    pub business_day: i64,
    pub staff_id: Option<String>,
    pub staff_name: Option<String>,
    pub action: String,
    pub entity: String,
    pub entity_id: Option<String>,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub prev_hash: Option<String>,
    pub hash: String,
}

impl AuditRow {
    /// This row, as the hash sees it.
    #[must_use]
    pub fn chained(&self) -> Chained<'_> {
        Chained {
            prev_hash: self.prev_hash.as_deref(),
            seq: self.seq,
            at: self.at,
            business_day: self.business_day,
            staff_id: self.staff_id.as_deref(),
            action: &self.action,
            entity: &self.entity,
            entity_id: self.entity_id.as_deref(),
            before_json: self.before_json.as_deref(),
            after_json: self.after_json.as_deref(),
        }
    }
}

/// Everything that goes into a row's hash.
///
/// A struct rather than ten arguments, and the ordering is load-bearing, so it
/// being a struct is also what stops somebody swapping two `Option<&str>`
/// parameters at a call site and producing a chain that verifies against
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chained<'a> {
    pub prev_hash: Option<&'a str>,
    pub seq: i64,
    pub at: i64,
    pub business_day: i64,
    pub staff_id: Option<&'a str>,
    pub action: &'a str,
    pub entity: &'a str,
    pub entity_id: Option<&'a str>,
    pub before_json: Option<&'a str>,
    pub after_json: Option<&'a str>,
}

/// The link. Every field that a person could want to change is in it — if a
/// column is not hashed, editing that column is undetectable, so the only safe
/// default is "all of them".
///
/// `staff_name` is deliberately **not** here: it is a join, not a stored
/// column, and a shop correcting a spelling must not break its own history.
/// SHA-256 of some bytes.
///
/// P19 needs it for a certificate fingerprint, which is the number a phone
/// pins. It lives here because this crate is the only home of `sha2` (see the
/// module list in `lib.rs`), and a second `Sha256::new()` elsewhere would be
/// the start of a second cryptography surface nobody is reviewing.
#[must_use]
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

#[must_use]
pub fn chain_hash(f: &Chained<'_>) -> String {
    let Chained {
        prev_hash,
        seq,
        at,
        business_day,
        staff_id,
        action,
        entity,
        entity_id,
        before_json,
        after_json,
    } = *f;
    let mut hasher = Sha256::new();
    // Length-prefixed, so ("ab", "c") and ("a", "bc") are different messages.
    // Without this, moving one character between two fields would preserve the
    // hash, which is a small hole and a completely unnecessary one.
    let mut field = |bytes: &[u8]| {
        hasher.update(u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_be_bytes());
        hasher.update(bytes);
    };
    field(prev_hash.unwrap_or("").as_bytes());
    field(&seq.to_be_bytes());
    field(&at.to_be_bytes());
    field(&business_day.to_be_bytes());
    field(staff_id.unwrap_or("").as_bytes());
    field(action.as_bytes());
    field(entity.as_bytes());
    field(entity_id.unwrap_or("").as_bytes());
    field(before_json.unwrap_or("").as_bytes());
    field(after_json.unwrap_or("").as_bytes());
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        // The only way this write fails is an allocation failure, which is not
        // a case a counter can do anything about.
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Where the chain stopped making sense.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broken {
    pub seq: i64,
    pub why: BrokenWhy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokenWhy {
    /// A row's contents do not produce its own hash: it was edited.
    Edited,
    /// This row does not point at the previous one: something was removed or
    /// reordered between them.
    Unlinked,
    /// The sequence skips: a row was deleted.
    Gap,
}

impl std::fmt::Display for Broken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self.why {
            BrokenWhy::Edited => "was changed after it was written",
            BrokenWhy::Unlinked => "does not follow the one before it",
            BrokenWhy::Gap => "is missing an entry before it",
        };
        write!(f, "history entry {} {what}", self.seq)
    }
}

/// Walk the chain and report the first break.
///
/// `rows` must be in `seq` order. The caller is the repository, which orders
/// them in SQL — doing it here as well would hide a repository bug behind a
/// sort.
pub fn verify_chain(rows: &[AuditRow]) -> Result<(), Broken> {
    let mut previous: Option<&AuditRow> = None;
    for row in rows {
        let expected = chain_hash(&row.chained());
        if expected != row.hash {
            return Err(Broken {
                seq: row.seq,
                why: BrokenWhy::Edited,
            });
        }
        match previous {
            None => {}
            Some(prev) => {
                if row.seq != prev.seq + 1 {
                    return Err(Broken {
                        seq: row.seq,
                        why: BrokenWhy::Gap,
                    });
                }
                if row.prev_hash.as_deref() != Some(prev.hash.as_str()) {
                    return Err(Broken {
                        seq: row.seq,
                        why: BrokenWhy::Unlinked,
                    });
                }
            }
        }
        previous = Some(row);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(count: i64) -> Vec<AuditRow> {
        let mut rows: Vec<AuditRow> = Vec::new();
        for seq in 1..=count {
            let prev = rows.last().map(|r: &AuditRow| r.hash.clone());
            let at = 1_700_000_000_000 + seq;
            let hash = chain_hash(&Chained {
                prev_hash: prev.as_deref(),
                seq,
                at,
                business_day: 20_260_808,
                staff_id: Some("staff_1"),
                action: action::BILL_SETTLED,
                entity: "bill",
                entity_id: Some("bill_1"),
                before_json: None,
                after_json: None,
            });
            rows.push(AuditRow {
                id: format!("aud_{seq}"),
                seq,
                at,
                business_day: 20_260_808,
                staff_id: Some("staff_1".to_owned()),
                staff_name: Some("Rekha".to_owned()),
                action: action::BILL_SETTLED.to_owned(),
                entity: "bill".to_owned(),
                entity_id: Some("bill_1".to_owned()),
                before_json: None,
                after_json: None,
                prev_hash: prev,
                hash,
            });
        }
        rows
    }

    #[test]
    fn an_untouched_chain_verifies() {
        assert_eq!(verify_chain(&chain(20)), Ok(()));
        assert_eq!(verify_chain(&[]), Ok(()), "a shop with no history is fine");
    }

    #[test]
    fn an_edited_row_is_caught() {
        // T6, first way: somebody changes who did it.
        let mut rows = chain(10);
        rows[4].staff_id = Some("staff_2".to_owned());
        assert_eq!(
            verify_chain(&rows),
            Err(Broken {
                seq: 5,
                why: BrokenWhy::Edited
            })
        );
    }

    #[test]
    fn a_deleted_row_is_caught() {
        // T6, second way — and this is the one triggers cannot help with,
        // because whoever deletes the row can drop the trigger first.
        let mut rows = chain(10);
        rows.remove(4);
        let broken = verify_chain(&rows).expect_err("a gap");
        assert_eq!(broken.seq, 6);
        assert_eq!(broken.why, BrokenWhy::Gap);
    }

    #[test]
    fn reordered_rows_are_caught() {
        // T6, third way.
        let mut rows = chain(10);
        rows.swap(3, 4);
        assert!(verify_chain(&rows).is_err());
    }

    #[test]
    fn a_rewritten_row_with_a_recomputed_hash_still_breaks_the_next_link() {
        // The interesting attack: somebody who knows how the hash is made
        // changes a row AND fixes its hash. The row itself now verifies, and
        // the NEXT row's prev_hash does not match. Which is the entire reason
        // it is a chain and not a per-row checksum.
        let mut rows = chain(6);
        rows[2].entity_id = Some("bill_999".to_owned());
        rows[2].hash = chain_hash(&rows[2].chained());
        assert_eq!(
            verify_chain(&rows),
            Err(Broken {
                seq: 4,
                why: BrokenWhy::Unlinked
            })
        );
    }

    #[test]
    fn moving_a_character_between_fields_changes_the_hash() {
        // Why the fields are length-prefixed.
        let base = Chained {
            prev_hash: None,
            seq: 1,
            at: 0,
            business_day: 0,
            staff_id: Some("ab"),
            action: "c",
            entity: "e",
            entity_id: None,
            before_json: None,
            after_json: None,
        };
        let a = chain_hash(&base);
        let b = chain_hash(&Chained {
            staff_id: Some("a"),
            action: "bc",
            ..base
        });
        assert_ne!(a, b);
    }

    #[test]
    fn a_hash_is_sixty_four_hex_characters() {
        let hash = chain_hash(&Chained {
            prev_hash: None,
            seq: 1,
            at: 0,
            business_day: 0,
            staff_id: None,
            action: "x",
            entity: "y",
            entity_id: None,
            before_json: None,
            after_json: None,
        });
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn correcting_a_staff_members_spelling_does_not_break_history() {
        // staff_name is a join, not a hashed column. A shop that fixes "Rekha"
        // to "Rekhaa" must not be told its books have been tampered with.
        let mut rows = chain(5);
        for row in &mut rows {
            row.staff_name = Some("Rekhaa".to_owned());
        }
        assert_eq!(verify_chain(&rows), Ok(()));
    }

    #[test]
    fn the_break_reads_as_a_sentence() {
        let broken = Broken {
            seq: 41,
            why: BrokenWhy::Edited,
        };
        assert_eq!(
            broken.to_string(),
            "history entry 41 was changed after it was written"
        );
    }

    #[test]
    fn every_action_has_words() {
        for action in action::ALL {
            let words = action::words(action);
            assert!(!words.contains('.'), "{words} is a tag, not words");
            assert!(
                !words.starts_with("Did something"),
                "{action} has no words of its own"
            );
        }
    }
}
