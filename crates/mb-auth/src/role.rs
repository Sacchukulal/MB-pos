//! **Roles are bundles, and no rule is ever written against a role's name.**
//!
//! `if role == "Manager"` is a rule that a shop deletes by renaming a role to
//! "Supervisor" — silently, on a Tuesday, with nobody able to explain why the
//! discount stopped working. The check is always
//! [`Actor::can`](crate::Actor::can), and there is a workspace-wide test that
//! no role name appears in a comparison.
//!
//! The four presets below are a **starting point, not law.** `is_builtin` (the
//! column P04 wrote) means "cannot be deleted", not "cannot be edited" — a shop
//! whose waiters take payments should be able to say so without asking us.

use mb_core::Money;

use crate::error::AuthError;
use crate::permission::{Permission, PermissionSet};

/// A role as it is stored and edited: a name, a bundle, and the discount
/// ceiling that scope 1.12 hangs off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleShape {
    pub id: String,
    pub name: String,
    /// Cannot be deleted. Can be edited.
    pub is_builtin: bool,
    pub permissions: PermissionSet,
    /// Basis points, `None` for no limit. Basis points because that is what
    /// `DiscountPolicy` speaks and because a percentage stored as a float is
    /// the bug D7 forbids.
    pub max_discount_bp: Option<u32>,
    pub max_discount: Option<Money>,
}

impl RoleShape {
    /// The ceiling as a person reads it — `"10%"`, `"12.5%"`, or `None` for no
    /// limit.
    ///
    /// **Formatted here, not in TypeScript** (R8, D39). The money guard caught
    /// the first attempt doing `bp / 100` in a React file, which is the same
    /// mistake as formatting rupees there: one arithmetic rule, one language.
    #[must_use]
    pub fn percent_label(&self) -> Option<String> {
        self.max_discount_bp
            .and_then(mb_core::TaxRate::from_basis_points)
            .map(|rate| rate.label())
    }

    /// And back again, from whatever somebody typed into the box.
    ///
    /// Empty is `None`, which is "no limit" — not zero. Those mean opposite
    /// things, and a role limited to 0% (a waiter) is a real thing a shop sets.
    pub fn parse_percent(typed: &str) -> Result<Option<u32>, AuthError> {
        let typed = typed.trim().trim_end_matches('%').trim();
        if typed.is_empty() {
            return Ok(None);
        }
        let (whole, frac) = match typed.split_once('.') {
            Some((w, f)) => (w, f),
            None => (typed, ""),
        };
        // Two decimal places is a basis point; a third would be silently
        // dropped, and silently dropping a digit somebody typed is the kind of
        // thing they find out about from a customer.
        if frac.len() > 2 || !typed.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return Err(AuthError::BadPercent {
                typed: typed.to_owned(),
            });
        }
        let whole: u32 = if whole.is_empty() {
            0
        } else {
            whole.parse().map_err(|_| AuthError::BadPercent {
                typed: typed.to_owned(),
            })?
        };
        let frac: u32 = match frac.len() {
            0 => 0,
            1 => frac.parse::<u32>().map_err(|_| AuthError::BadPercent {
                typed: typed.to_owned(),
            })? * 10,
            _ => frac.parse().map_err(|_| AuthError::BadPercent {
                typed: typed.to_owned(),
            })?,
        };
        let bp = whole
            .checked_mul(100)
            .and_then(|w| w.checked_add(frac))
            .filter(|bp| *bp <= 10_000)
            .ok_or_else(|| AuthError::BadPercent {
                typed: typed.to_owned(),
            })?;
        Ok(Some(bp))
    }
}

/// The four a new shop starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolePreset {
    Owner,
    Manager,
    Cashier,
    Waiter,
}

impl RolePreset {
    pub const ALL: &'static [RolePreset] = &[
        RolePreset::Owner,
        RolePreset::Manager,
        RolePreset::Cashier,
        RolePreset::Waiter,
    ];

    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            RolePreset::Owner => "role_owner",
            RolePreset::Manager => "role_manager",
            RolePreset::Cashier => "role_cashier",
            RolePreset::Waiter => "role_waiter",
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            RolePreset::Owner => "Owner",
            RolePreset::Manager => "Manager",
            RolePreset::Cashier => "Cashier",
            RolePreset::Waiter => "Waiter",
        }
    }

    /// What the preset grants on day one.
    #[must_use]
    pub fn shape(self) -> RoleShape {
        let permissions: PermissionSet = match self {
            RolePreset::Owner => PermissionSet::everything(),

            // Everything except the two that let somebody quietly take the shop
            // over: making themselves an administrator, and walking off with —
            // or overwriting — the data.
            RolePreset::Manager => Permission::ALL
                .iter()
                .copied()
                .filter(|p| !matches!(p, Permission::StaffManage | Permission::BackupRun))
                .collect(),

            RolePreset::Cashier => [
                Permission::BillCreate,
                Permission::BillDiscountLine,
                Permission::BillReprint,
                Permission::DrawerOpen,
                Permission::CreditCollect,
            ]
            .into_iter()
            .collect(),

            // A waiter takes orders. Nothing about a waiter's job involves the
            // cash drawer, and C1 is a finding about exactly this.
            RolePreset::Waiter => [Permission::BillCreate].into_iter().collect(),
        };

        RoleShape {
            id: self.id().to_owned(),
            name: self.name().to_owned(),
            is_builtin: true,
            permissions,
            max_discount_bp: match self {
                RolePreset::Owner => None,
                RolePreset::Manager => Some(2_000), // 20%
                RolePreset::Cashier => Some(1_000), // 10%
                RolePreset::Waiter => Some(0),
            },
            max_discount: match self {
                RolePreset::Owner => None,
                RolePreset::Manager => Some(Money::from_paise(100_000)), // ₹1,000
                RolePreset::Cashier => Some(Money::from_paise(20_000)),  // ₹200
                RolePreset::Waiter => Some(Money::ZERO),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_owner_can_manage_staff_or_restore_a_backup() {
        for preset in RolePreset::ALL {
            let shape = preset.shape();
            let expected = *preset == RolePreset::Owner;
            assert_eq!(shape.permissions.has(Permission::StaffManage), expected);
            assert_eq!(shape.permissions.has(Permission::BackupRun), expected);
        }
    }

    #[test]
    fn a_waiter_cannot_open_the_drawer_or_see_the_takings() {
        // Audit C1 in one assertion: "anybody who walks behind the counter can
        // open Reports and see the whole day's cash".
        let waiter = RolePreset::Waiter.shape();
        assert!(waiter.permissions.has(Permission::BillCreate));
        assert!(!waiter.permissions.has(Permission::DrawerOpen));
        assert!(!waiter.permissions.has(Permission::ReportsView));
        assert_eq!(waiter.permissions.len(), 1);
    }

    #[test]
    fn every_preset_can_at_least_take_an_order() {
        for preset in RolePreset::ALL {
            assert!(
                preset.shape().permissions.has(Permission::BillCreate),
                "{} cannot bill",
                preset.name()
            );
        }
    }

    #[test]
    fn a_builtin_role_is_still_editable() {
        // The column means "cannot be deleted". A shop whose waiters take
        // payments must be able to say so without asking us.
        assert!(RolePreset::Cashier.shape().is_builtin);
    }

    #[test]
    fn a_ceiling_round_trips_through_the_screen() {
        // The R8 fix: this arithmetic used to be `bp / 100` in a React file,
        // and the money guard failed the build over it.
        for (typed, bp, label) in [
            ("10", 1_000_u32, "10%"),
            ("12.5", 1_250, "12.5%"),
            ("0.25", 25, "0.25%"),
            ("100", 10_000, "100%"),
            ("0", 0, "0%"),
            ("15%", 1_500, "15%"),
        ] {
            assert_eq!(
                RoleShape::parse_percent(typed),
                Ok(Some(bp)),
                "parsing {typed}"
            );
            let mut role = RolePreset::Cashier.shape();
            role.max_discount_bp = Some(bp);
            assert_eq!(role.percent_label().as_deref(), Some(label));
        }
    }

    #[test]
    fn no_limit_and_no_discount_are_not_the_same_thing() {
        assert_eq!(RoleShape::parse_percent("   "), Ok(None), "empty is no limit");
        assert_eq!(RoleShape::parse_percent("0"), Ok(Some(0)), "0 is no discount");

        let mut role = RolePreset::Owner.shape();
        assert_eq!(role.percent_label(), None);
        role.max_discount_bp = Some(0);
        assert_eq!(role.percent_label().as_deref(), Some("0%"));
    }

    #[test]
    fn a_percentage_that_makes_no_sense_is_refused() {
        for typed in ["ten", "10.125", "101", "1e3", "-5", "..", "10.5.5"] {
            assert!(
                RoleShape::parse_percent(typed).is_err(),
                "{typed} was accepted"
            );
        }
        // And the refusal says what to type instead.
        let err = RoleShape::parse_percent("ten").expect_err("refused");
        assert!(err.to_string().contains("12.5"), "{err}");
    }

    #[test]
    fn the_ids_are_distinct_and_stable() {
        let ids: std::collections::BTreeSet<&str> =
            RolePreset::ALL.iter().map(|p| p.id()).collect();
        assert_eq!(ids.len(), RolePreset::ALL.len());
    }
}
