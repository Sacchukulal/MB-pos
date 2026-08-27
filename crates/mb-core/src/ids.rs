//! The identity types.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Declares one id newtype.
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
id_type!(
    ModifierId,
    "Identifies a modifier (`extra cheese`, `no onion`)."
);
id_type!(CategoryId, "Identifies a menu category.");
id_type!(StaffId, "Identifies a staff member.");
id_type!(CustomerId, "Identifies a customer.");
id_type!(
    OrderId,
    "Identifies one order, through every state it passes."
);
id_type!(TableId, "Identifies a table.");
id_type!(
    MaterialId,
    "Identifies a raw or made material. Not an `ItemId`: an item is sold, a material is consumed."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_of_different_kinds_are_different_types() {
        // The point of the newtypes: this file would not compile if an ItemId could be compared
        // with a CategoryId.
        let item = ItemId::new("itm_7f3a");
        assert_eq!(item.as_str(), "itm_7f3a");
        assert_eq!(item.to_string(), "itm_7f3a");
        assert_eq!(ItemId::from("itm_7f3a"), item);
        assert!(ItemId::default().is_empty());
    }

    #[test]
    fn ids_serialise_as_bare_strings() {
        let json = serde_json::to_string(&ItemId::new("itm_1")).expect("serialises");
        assert_eq!(json, "\"itm_1\"");
        let back: ItemId = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(back, ItemId::new("itm_1"));
    }
}
