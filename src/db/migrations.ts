import Database from "@tauri-apps/plugin-sql";

/**
 * Idempotent schema bootstrap + ordered migrations.
 *
 * The base schema (v1) reproduces the exact tables/columns of the pre-rebuild app so
 * existing restaurant.db files open unchanged. Later migrations are one-time fixes
 * tracked in schema_version.
 */

async function addColumn(db: Database, table: string, column: string, type: string, def: string) {
  try {
    await db.execute(`ALTER TABLE ${table} ADD COLUMN ${column} ${type} DEFAULT ${def}`);
  } catch {
    // Column already exists — expected on every startup after the first.
  }
}

async function baseSchema(db: Database) {
  await db.execute(`
    CREATE TABLE IF NOT EXISTS subscription (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      status TEXT, planId TEXT, subscriptionId TEXT,
      nextBillingDate TEXT, updatedAt TEXT, last_checked_date TEXT
    );
  `);
  await db.execute(`INSERT OR IGNORE INTO subscription (id) VALUES (1)`);

  await db.execute(`
    CREATE TABLE IF NOT EXISTS user_details (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      displayName TEXT, email TEXT, mobileNumber TEXT, restaurantName TEXT
    );
  `);
  await db.execute(`INSERT OR IGNORE INTO user_details (id) VALUES (1)`);

  await db.execute(`
    CREATE TABLE IF NOT EXISTS store_settings (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      hotel_name TEXT, address TEXT, phone_number TEXT, gst_number TEXT, fssai_number TEXT
    );
  `);
  await addColumn(db, "store_settings", "upi_id", "TEXT", "''");
  await addColumn(db, "store_settings", "merchant_name", "TEXT", "''");
  await addColumn(db, "store_settings", "payment_reference", "TEXT", "''");

  await db.execute(`
    CREATE TABLE IF NOT EXISTS printer_settings (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      printer_mode TEXT, default_printer TEXT, kot_printing_style TEXT,
      token_reset_daily BOOLEAN, token_starting_number INTEGER,
      bill_reset_daily BOOLEAN, bill_starting_number INTEGER,
      paper_size TEXT, print_bold BOOLEAN
    );
  `);
  await addColumn(db, "printer_settings", "paper_size", "TEXT", "'3inch'");
  await addColumn(db, "printer_settings", "print_bold", "BOOLEAN", "0");
  await addColumn(db, "printer_settings", "bill_prefix", "TEXT", "''");
  await addColumn(db, "printer_settings", "bill_current_number", "INTEGER", "0");
  await addColumn(db, "printer_settings", "token_current_number", "INTEGER", "100");
  await addColumn(db, "printer_settings", "token_print_size", "TEXT", "'Large'");
  await addColumn(db, "printer_settings", "last_reset_date", "TEXT", "''");
  await addColumn(db, "printer_settings", "kot_print_confirmation", "BOOLEAN", "0");
  await addColumn(db, "printer_settings", "bill_print_confirmation", "BOOLEAN", "0");
  await addColumn(db, "printer_settings", "disable_kot", "BOOLEAN", "0");

  await db.execute(`
    CREATE TABLE IF NOT EXISTS categories (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      name TEXT NOT NULL UNIQUE
    );
  `);

  await db.execute(`
    CREATE TABLE IF NOT EXISTS category_printers (
      category_id INTEGER PRIMARY KEY,
      printer_name TEXT,
      FOREIGN KEY (category_id) REFERENCES categories(id) ON DELETE CASCADE
    );
  `);

  await db.execute(`
    CREATE TABLE IF NOT EXISTS items (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      category_id INTEGER,
      name TEXT NOT NULL,
      price REAL NOT NULL,
      FOREIGN KEY (category_id) REFERENCES categories(id)
    );
  `);

  await db.execute(`
    CREATE TABLE IF NOT EXISTS bill_settings (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      footer_message TEXT,
      show_gst BOOLEAN DEFAULT 1,
      show_fssai BOOLEAN DEFAULT 1,
      show_address BOOLEAN DEFAULT 1,
      show_phone BOOLEAN DEFAULT 1,
      bill_font_size TEXT DEFAULT 'Medium'
    );
  `);
  const billCols: [string, string, string][] = [
    ["printer_size", "TEXT", "'3inch'"],
    ["header_font_family", "TEXT", "'monospace'"],
    ["header_font_size", "TEXT", "'16px'"],
    ["body_font_family", "TEXT", "'monospace'"],
    ["body_font_size", "TEXT", "'12px'"],
    ["footer_font_family", "TEXT", "'monospace'"],
    ["footer_font_size", "TEXT", "'12px'"],
    ["gst_enabled", "BOOLEAN", "1"],
    ["gst_type", "TEXT", "'Exclusive'"],
    ["show_cashier_name", "BOOLEAN", "1"],
    ["gst_percentage", "REAL", "5"],
    ["row_height", "TEXT", "'4px'"],
    ["logo_position", "TEXT", "'none'"],
    ["logo_size", "INTEGER", "50"],
    ["logo_opacity", "REAL", "0.2"],
    ["logo_base64", "TEXT", "''"],
    ["show_line_separators", "BOOLEAN", "1"],
    ["show_token", "BOOLEAN", "1"],
    ["sep_header", "BOOLEAN", "1"],
    ["sep_meta", "BOOLEAN", "1"],
    ["sep_token", "BOOLEAN", "1"],
    ["sep_table_header", "BOOLEAN", "1"],
    ["sep_table_body", "BOOLEAN", "1"],
    ["sep_subtotals", "BOOLEAN", "1"],
    ["sep_grand_total", "BOOLEAN", "1"],
    ["store_name_size", "TEXT", "'16px'"],
    ["address_size", "TEXT", "'12px'"],
    ["table_font_size", "TEXT", "'12px'"],
    ["total_font_size", "TEXT", "'12px'"],
    ["dynamic_upi_qr", "BOOLEAN", "0"],
    ["static_upi_qr", "BOOLEAN", "0"],
    ["no_qr_print", "BOOLEAN", "1"],
    ["search_match_mode", "TEXT", "'starts'"],
    ["global_font_family", "TEXT", "'monospace'"],
    ["store_name_bold", "BOOLEAN", "1"],
    ["address_bold", "BOOLEAN", "0"],
    ["table_bold", "BOOLEAN", "0"],
    ["total_bold", "BOOLEAN", "1"],
    ["footer_bold", "BOOLEAN", "0"],
  ];
  for (const [c, t, d] of billCols) await addColumn(db, "bill_settings", c, t, d);

  await db.execute(`
    CREATE TABLE IF NOT EXISTS kot_settings (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      header_font_family TEXT DEFAULT 'monospace',
      header_font_size TEXT DEFAULT '16px',
      body_font_family TEXT DEFAULT 'monospace',
      body_font_size TEXT DEFAULT '12px',
      row_height TEXT DEFAULT '4px 0',
      show_line_separators BOOLEAN DEFAULT 1
    );
  `);
  await db.execute(`INSERT OR IGNORE INTO kot_settings (id) VALUES (1)`);
  const kotCols: [string, string, string][] = [
    ["show_token", "BOOLEAN", "1"],
    ["sep_token", "BOOLEAN", "1"],
    ["sep_header", "BOOLEAN", "1"],
    ["sep_meta", "BOOLEAN", "1"],
    ["sep_table_header", "BOOLEAN", "1"],
    ["sep_table_body", "BOOLEAN", "1"],
    ["table_font_size", "TEXT", "'12px'"],
    ["show_kot_title", "BOOLEAN", "1"],
    ["show_bill_no", "BOOLEAN", "1"],
    ["show_order_type", "BOOLEAN", "1"],
    ["show_table", "BOOLEAN", "1"],
    ["show_date", "BOOLEAN", "1"],
    ["meta_font_size", "TEXT", "'12px'"],
    ["title_bold", "BOOLEAN", "1"],
    ["meta_bold", "BOOLEAN", "0"],
    ["items_bold", "BOOLEAN", "1"],
    ["meta_two_column", "BOOLEAN", "1"],
  ];
  for (const [c, t, d] of kotCols) await addColumn(db, "kot_settings", c, t, d);

  await db.execute(`
    CREATE TABLE IF NOT EXISTS staff (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      name TEXT NOT NULL, role TEXT, phone TEXT, pin TEXT
    );
  `);

  await db.execute(`
    CREATE TABLE IF NOT EXISTS expenses (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      description TEXT NOT NULL, amount REAL NOT NULL,
      category TEXT, date TEXT DEFAULT CURRENT_TIMESTAMP
    );
  `);

  const orderColumns = `
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cart_data TEXT, customer_name TEXT, customer_phone TEXT, payment_mode TEXT,
    subtotal REAL, gst REAL, total REAL,
    order_type TEXT DEFAULT 'Self Service', table_number TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
  `;
  await db.execute(`CREATE TABLE IF NOT EXISTS processing_orders (${orderColumns});`);
  await db.execute(`CREATE TABLE IF NOT EXISTS finalized_orders (${orderColumns});`);
  for (const table of ["processing_orders", "finalized_orders"]) {
    await addColumn(db, table, "customer_id", "INTEGER", "NULL");
    await addColumn(db, table, "token_number", "INTEGER", "NULL");
    await addColumn(db, table, "bill_number", "TEXT", "NULL");
  }
  // Cloud sync bookkeeping (outbox pattern): rows start unsynced and are pushed
  // to Supabase in batches by src/services/sync/billSync.ts.
  await addColumn(db, "finalized_orders", "synced", "INTEGER", "0");
  await addColumn(db, "finalized_orders", "sync_attempts", "INTEGER", "0");
  await addColumn(db, "finalized_orders", "last_sync_error", "TEXT", "''");

  await db.execute(`
    CREATE TABLE IF NOT EXISTS customers (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      name TEXT NOT NULL, phone TEXT,
      credit_balance REAL DEFAULT 0,
      created_at TEXT DEFAULT CURRENT_TIMESTAMP
    );
  `);

  await db.execute(`
    CREATE TABLE IF NOT EXISTS customer_payments (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      customer_id INTEGER NOT NULL,
      amount REAL NOT NULL, payment_mode TEXT,
      date TEXT DEFAULT CURRENT_TIMESTAMP,
      FOREIGN KEY(customer_id) REFERENCES customers(id) ON DELETE CASCADE
    );
  `);
}

/** One-time ordered migrations, tracked in schema_version. */
const MIGRATIONS: { version: number; run: (db: Database) => Promise<void> }[] = [
  {
    // Convert legacy locale-formatted reset date (DD/MM/YYYY) to ISO YYYY-MM-DD so
    // the daily counter reset stays accurate across the upgrade.
    version: 2,
    run: async (db) => {
      const rows = await db.select<{ last_reset_date: string }[]>(
        "SELECT last_reset_date FROM printer_settings WHERE id = 1"
      );
      const legacy = rows[0]?.last_reset_date || "";
      const m = legacy.match(/^(\d{2})\/(\d{2})\/(\d{4})$/);
      if (m) {
        await db.execute("UPDATE printer_settings SET last_reset_date = $1 WHERE id = 1", [
          `${m[3]}-${m[2]}-${m[1]}`,
        ]);
      }
    },
  },
];

export async function runMigrations(db: Database) {
  await baseSchema(db);

  await db.execute(`CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY);`);
  const rows = await db.select<{ version: number }[]>(
    "SELECT COALESCE(MAX(version), 1) AS version FROM schema_version"
  );
  const current = rows[0]?.version ?? 1;

  for (const migration of MIGRATIONS) {
    if (migration.version > current) {
      await migration.run(db);
      await db.execute("INSERT OR IGNORE INTO schema_version (version) VALUES ($1)", [migration.version]);
    }
  }
}
