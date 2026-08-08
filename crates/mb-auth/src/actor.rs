//! **Who is doing this** — the value every guarded command carries.
//!
//! An `Actor` is not "the logged-in user". It is *the person this particular
//! action is attributed to*, which is why it is passed rather than looked up: a
//! bill settled at 9 pm by the cashier who came on at 6 is attributed to them
//! even though the order was opened by somebody else, and both facts end up in
//! the row (`orders.created_by`, `orders.settled_by`).

use mb_core::{DiscountPolicy, Money, StaffId};

use crate::error::AuthError;
use crate::permission::{Permission, PermissionSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub staff_id: StaffId,
    /// What goes on the bill (audit **C3** — v1 printed "Cashier: Admin" on
    /// every bill in the product, staff list or no staff list).
    pub name: String,
    pub role_id: Option<String>,
    /// For the screen only. **Never branch on it** — see [`Actor::can`].
    pub role_name: Option<String>,
    pub permissions: PermissionSet,
    /// Scope 1.12, and D18: the policy travels with the person and is checked
    /// by the caller, never inside `compute_bill`.
    pub max_discount_bp: Option<u32>,
    pub max_discount: Option<Money>,
}

impl Actor {
    #[must_use]
    pub fn can(&self, permission: Permission) -> bool {
        self.permissions.has(permission)
    }

    /// The refusal, as a `Result` so a command reads
    /// `actor.must(Permission::BillVoid)?;` and cannot forget the `?`.
    pub fn must(&self, permission: Permission) -> Result<(), AuthError> {
        if self.can(permission) {
            Ok(())
        } else {
            Err(AuthError::Denied {
                what: permission.what(),
                need: permission,
            })
        }
    }

    /// **Scope 1.12.** The role's limits, as P02's own type.
    ///
    /// D18 is the rule this exists to honour: *"discount policy is checked by
    /// the caller, never inside `compute_bill`, so an old bill recomputes
    /// identically after a role changes."* This function is that caller's
    /// source, and the reason it takes no bill: a policy that depended on the
    /// bill would be a policy that could not be checked before the discount was
    /// typed.
    #[must_use]
    pub fn discount_policy(&self) -> DiscountPolicy {
        // Someone who may not discount at all gets a policy of zero rather than
        // no policy — "no policy" reads as "unrestricted" everywhere else, and
        // that is the wrong way round to be wrong.
        if !self.can(Permission::BillDiscountLine) && !self.can(Permission::BillDiscountBill) {
            return DiscountPolicy::none();
        }

        let unrestricted = DiscountPolicy::unrestricted();
        DiscountPolicy {
            max_percent_bp: self.max_discount_bp.unwrap_or(unrestricted.max_percent_bp),
            max_amount: self.max_discount.or(unrestricted.max_amount),
            // Any discount at all is worth a reason once there is a limit to
            // exceed. A shop that wants a reason on every discount gets it in
            // P17; until then this mirrors the limit.
            reason_required_above_bp: self.max_discount_bp,
            reason_required_above_amount: self.max_discount,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(permissions: PermissionSet) -> Actor {
        Actor {
            staff_id: StaffId::new("staff_1"),
            name: "Rekha".to_owned(),
            role_id: Some("role_cashier".to_owned()),
            role_name: Some("Cashier".to_owned()),
            permissions,
            max_discount_bp: None,
            max_discount: None,
        }
    }

    #[test]
    fn must_refuses_with_a_sentence() {
        let waiter = actor([Permission::BillCreate].into_iter().collect());
        assert!(waiter.must(Permission::BillCreate).is_ok());
        let refused = waiter.must(Permission::BillVoid).expect_err("denied");
        assert_eq!(refused.to_string(), "you do not have permission to void a bill");
    }

    #[test]
    fn somebody_who_cannot_discount_gets_a_policy_of_zero() {
        // Not "no policy". `unrestricted()` is what an absent policy means
        // everywhere else in the product, so the absent case has to be the
        // strict one here.
        let waiter = actor([Permission::BillCreate].into_iter().collect());
        let policy = waiter.discount_policy();
        assert_eq!(policy.max_percent_bp, 0);
        assert_eq!(policy.max_amount, None);
    }

    #[test]
    fn a_limit_on_the_role_becomes_the_policy() {
        let mut manager = actor(
            [Permission::BillCreate, Permission::BillDiscountBill]
                .into_iter()
                .collect(),
        );
        manager.max_discount_bp = Some(1_000); // 10%
        manager.max_discount = Some(Money::from_paise(50_000)); // ₹500
        let policy = manager.discount_policy();
        assert_eq!(policy.max_percent_bp, 1_000);
        assert_eq!(policy.max_amount, Some(Money::from_paise(50_000)));
        assert_eq!(policy.reason_required_above_bp, Some(1_000));
    }

    #[test]
    fn the_owner_is_unrestricted() {
        let owner = actor(PermissionSet::everything());
        let policy = owner.discount_policy();
        assert_eq!(policy.max_percent_bp, DiscountPolicy::unrestricted().max_percent_bp);
    }
}
