//! The identity types.
//!
//! **They are text, not autoincrementing integers, and that is a decision with
//! a reason.**
//!
//! `FEATURE_SCOPE.md` 11.1 and 11.2 require two billing PCs in one shop, and
//! 11.4 requires several outlets under one owner. An autoincrement id collides
//! the moment two machines create a row at the same second, and there is no way
//! to repair that afterwards without renumbering history. P04 makes these UUID
//! text columns.
//!
//! Deciding it here costs one newtype per id. Discovering it at P27, with a
//! year of bills already written, costs a migration of every table that
//! references an item.
//!
//! They are newtypes rather than bare `String`s so that an item id can never be
//! passed where a category id belongs — the compiler catches it, and that class
//! of bug is invisible in a review.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Declares one id newtype. They differ only in their name, and their name is
/// the entire point, so a macro keeps them from drifting apart.
macro_rules! id_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            #[must_use]
            pub fn new(id: impl Into<String>) -> Self {
                $name(id.into())
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            #[must_use]
            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> Self {
                $name(id.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> Self {
                $name(id)
            }
        }
    };
}

id_type!(ItemId, "Identifies a menu item.");
id_type!(ModifierId, "Identifies a modifier (`extra cheese`, `no onion`).");
id_type!(CategoryId, "Identifies a menu category.");
id_type!(StaffId, "Identifies a staff member. P11 gives them roles and PINs.");
id_type!(CustomerId, "Identifies a customer. P15 gives them a credit ledger.");
id_type!(OrderId, "Identifies one order, through every state it passes.");
id_type!(TableId, "Identifies a table. P14 gives it a position on the floor.");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_of_different_kinds_are_different_types() {
        // The point of the newtypes: this file would not compile if an ItemId
        // could be compared with a CategoryId. The runtime assertion below is
        // only here so the test has something to run; the real check is that
        // `assert_eq!(ItemId::new("x"), CategoryId::new("x"))` does not build.
        let item = ItemId::new("itm_7f3a");
        assert_eq!(item.as_str(), "itm_7f3a");
        assert_eq!(item.to_string(), "itm_7f3a");
        assert_eq!(ItemId::from("itm_7f3a"), item);
        assert!(ItemId::default().is_empty());
    }

    #[test]
    fn ids_serialise_as_bare_strings() {
        // Transparent, so P04's database columns and P08's TypeScript types
        // see a plain string rather than a wrapper object.
        let json = serde_json::to_string(&ItemId::new("itm_1")).expect("serialises");
        assert_eq!(json, "\"itm_1\"");
        let back: ItemId = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(back, ItemId::new("itm_1"));
    }
}
