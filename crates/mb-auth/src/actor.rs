//! Who is doing this — the value every guarded command carries.

use mb_core::{DiscountPolicy, Money, StaffId};

use crate::error::AuthError;
use crate::permission::{Permission, PermissionSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub staff_id: StaffId,
    /// What goes on the bill.
    pub name: String,
    pub role_id: Option<String>,
    /// For the screen only.
    pub role_name: Option<String>,
    pub permissions: PermissionSet,
    pub max_discount_bp: Option<u32>,
    pub max_discount: Option<Money>,
}

impl Actor {
    #[must_use]
    pub fn can(&self, permission: Permission) -> bool {
        self.permissions.has(permission)
    }

    /// The refusal, as a `Result` so a command reads `actor.must(Permission::BillVoid)?;` and
    /// cannot forget the `?`.
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

    #[must_use]
    pub fn discount_policy(&self) -> DiscountPolicy {
        // Someone who may not discount at all gets a policy of zero rather than no policy — "no
        // policy" reads as "unrestricted" everywhere else, and that is the wrong way round to
        // be wrong.
        if !self.can(Permission::BillDiscountLine) && !self.can(Permission::BillDiscountBill) {
            return DiscountPolicy::none();
        }

        let unrestricted = DiscountPolicy::unrestricted();
        DiscountPolicy {
            max_percent_bp: self.max_discount_bp.unwrap_or(unrestricted.max_percent_bp),
            max_amount: self.max_discount.or(unrestricted.max_amount),
            // Any discount at all is worth a reason once there is a limit to exceed.
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
        assert_eq!(
            refused.to_string(),
            "you do not have permission to void a bill"
        );
    }

    #[test]
    fn somebody_who_cannot_discount_gets_a_policy_of_zero() {
        // Not "no policy". `unrestricted()` is what an absent policy means everywhere else in
        // the product, so the absent case has to be the strict one here.
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
        assert_eq!(
            policy.max_percent_bp,
            DiscountPolicy::unrestricted().max_percent_bp
        );
    }
}
