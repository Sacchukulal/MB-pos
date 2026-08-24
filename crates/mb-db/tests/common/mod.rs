//! A scratch database per test, and the fixtures the behaviour tests need.
//!
//! No `tempfile` dependency. A unique directory under the OS temp folder is
//! twenty lines here, and every dependency — even a dev one — is a line someone
//! has to justify (R6, scope 16.15).

// Shared by four test binaries, each of which uses a different subset.
#![allow(dead_code, reason = "shared by four test binaries, each using a different subset")]
// The clippy.toml exemption only reaches `#[test]` functions, and everything
// here is a plain helper. In a fixture `expect` IS the assertion: a scratch
// directory that cannot be created is the test failing, not a shop losing data.
#![allow(
    clippy::expect_used,
    reason = "test fixtures: expect is the assertion"
)]

pub mod shop;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use mb_db::{Db, DbConfig};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A directory that deletes itself when the test ends.
pub struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    pub fn new(label: &str) -> Scratch {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "mb-db-{label}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch directory");
        Scratch { dir }
    }

    pub fn db_path(&self) -> PathBuf {
        self.dir.join("shop.db")
    }

    pub fn config(&self) -> DbConfig {
        DbConfig::new(self.db_path())
    }

    pub fn open(&self) -> Db {
        Db::open(&self.config()).expect("open the shop's data file")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Best effort: a leftover scratch folder is untidy, not a failure.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The ids seeded by migration 0001.
pub const OUTLET: &str = "outlet_default";
pub const TERMINAL: &str = "terminal_default";

/// One staff row, so orders have somebody to belong to.
pub const STAFF_SQL: &str = "
    INSERT INTO staff (id, outlet_id, name, status, created_at, updated_at)
    VALUES ('staff_1', 'outlet_default', 'Cashier', 'active', 0, 0);
";

/// A section and a table, so a dine-in order can be opened.
pub const FLOOR_SQL: &str = "
    INSERT INTO sections (id, outlet_id, name) VALUES ('sec_1', 'outlet_default', 'Hall');
    INSERT INTO dining_tables (id, outlet_id, section_id, label)
    VALUES ('tbl_1', 'outlet_default', 'sec_1', '1');
";

/// A small menu, including a liquor line so a bar can be tested.
pub const MENU_SQL: &str = "
    INSERT INTO categories (id, outlet_id, name, created_at, updated_at)
    VALUES ('cat_food', 'outlet_default', 'Food', 0, 0),
           ('cat_bar',  'outlet_default', 'Bar',  0, 0);
    INSERT INTO items (id, outlet_id, category_id, name, unit_price, tax_rate_bp,
                       tax_kind, tax_basis, hsn, created_at, updated_at)
    VALUES ('itm_dosa',  'outlet_default', 'cat_food', 'Masala Dosa', 12000, 500,
            'gst', 'exclusive', '2106', 0, 0),
           ('itm_water', 'outlet_default', 'cat_food', 'Water',        2000, 1800,
            'gst', 'inclusive', '2201', 0, 0),
           ('itm_beer',  'outlet_default', 'cat_bar',  'Beer',        22000, 0,
            'outside_gst', 'inclusive', NULL, 0, 0);
";
