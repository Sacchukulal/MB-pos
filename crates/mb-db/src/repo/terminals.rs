//! **The tills** — P27, scope 11.1 and 11.2.
//!
//! > Audit **E5**: *"One counter per shop. Full stop."*
//!
//! # D135 — every terminal has its own series, and the series IS the terminal
//!
//! Till 1 issues `A/0001`, till 2 issues `B/0001`. The two series share no
//! value, so **no partition, clock skew, restart or race can produce one number
//! twice** — not "rarely", but structurally. And nothing on the billing path
//! asks anybody for anything: a number is claimed from this till's own counter
//! row, inside the settle transaction, exactly as it has been since P05.
//!
//! The design this replaces — a master handing out reserved blocks — has to
//! answer *"what happens when a block runs out mid-service"*, and both answers
//! are a round trip to a machine that may be off, or a till that stops taking
//! money. It also leaves a hole in the shop's bill book for every partly-used
//! block, and every hole is a question at an audit.
//!
//! # The one way it can still go wrong
//!
//! Two tills issuing under the SAME prefix. So a prefix is unique within the
//! outlet (a partial unique index on `counters`), and **a shop with more than
//! one till must give every till a prefix** — refused here, in words, naming
//! what to type. Two tills both printing bare numbers is the collision.
//!
//! # D139 — moving the master is a decision a person makes
//!
//! There is no election. Automatic failover between two machines on a shop's
//! WiFi is a split-brain generator: the switch reboots, each till decides the
//! other is dead, and the shop has two floors. [`TerminalRepo::make_master`] is
//! a person pressing a button, and the stamp it writes is how an old master
//! knows to stand down when it comes back.

use mb_core::Timestamp;
use rusqlite::{Transaction, params};

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

/// One till.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terminal {
    pub id: String,
    pub name: String,
    /// **Exactly one per outlet**, and it is a person's choice (D139).
    pub is_master: bool,
    /// **D135.** What this till's numbers print under. Empty is allowed only
    /// while the shop has one till.
    pub series_prefix: String,
    /// When it became the master. A later stamp elsewhere wins, which is how
    /// an old master stands down instead of arguing.
    pub master_since: Option<Timestamp>,
    pub last_seen_at: Option<Timestamp>,
    pub created_at: Timestamp,
}

impl Terminal {
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, at: Timestamp) -> Self {
        Terminal {
            id: id.into(),
            name: name.into(),
            is_master: false,
            series_prefix: String::new(),
            master_since: None,
            last_seen_at: None,
            created_at: at,
        }
    }
}

#[derive(Debug)]
pub struct TerminalRepo<'a> {
    tx: &'a Transaction<'a>,
}

const COLUMNS: &str =
    "id, name, is_master, series_prefix, master_since, last_seen_at, created_at";

impl<'a> TerminalRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        TerminalRepo { tx }
    }

    pub fn all(&self, outlet: &str) -> Result<Vec<Terminal>, DbError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM terminals WHERE outlet_id = ?1 ORDER BY created_at"
        );
        let mut stmt = self.tx.prepare(&sql)?;
        let rows = stmt.query_map([outlet], read)?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    pub fn find(&self, outlet: &str, id: &str) -> Result<Option<Terminal>, DbError> {
        let sql =
            format!("SELECT {COLUMNS} FROM terminals WHERE outlet_id = ?1 AND id = ?2");
        let mut stmt = self.tx.prepare(&sql)?;
        let mut rows = stmt.query_map(params![outlet, id], read)?;
        rows.next().transpose().map_err(DbError::from)
    }

    /// The till that owns the floor (D137).
    pub fn master(&self, outlet: &str) -> Result<Option<Terminal>, DbError> {
        Ok(self.all(outlet)?.into_iter().find(|t| t.is_master))
    }

    /// **Add or change a till, and keep its two counter rows in step.**
    ///
    /// The series prefix is a property of the TILL and seeds both of its
    /// counters, because tokens collide across tills exactly as bills do — two
    /// tills both handing a customer "token 5" is the same bug wearing a
    /// smaller hat.
    pub fn save(&self, outlet: &str, terminal: &Terminal, at: Timestamp) -> Result<(), DbError> {
        if terminal.name.trim().is_empty() {
            return Err(DbError::invariant("a till needs a name"));
        }
        let prefix = terminal.series_prefix.trim().to_owned();

        // **The gap the partial index cannot cover** (D135). A shop with more
        // than one till must give every till a prefix; two tills both printing
        // bare numbers is precisely the collision.
        let others: Vec<Terminal> =
            self.all(outlet)?.into_iter().filter(|t| t.id != terminal.id).collect();
        if !others.is_empty() && prefix.is_empty() {
            return Err(DbError::invariant(
                "this shop has more than one till, so each one needs its own short \
                 prefix — A, B, C — in front of its numbers. Without it two tills \
                 would print the same bill number.",
            ));
        }
        if let Some(clash) = others.iter().find(|t| !prefix.is_empty() && t.series_prefix == prefix)
        {
            return Err(DbError::invariant(format!(
                "`{prefix}` is already {}'s prefix. Give this till a different one.",
                clash.name
            )));
        }

        self.tx.execute(
            "INSERT INTO terminals
                 (id, outlet_id, name, is_master, series_prefix, master_since,
                  last_seen_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name,
                 series_prefix = excluded.series_prefix,
                 last_seen_at = excluded.last_seen_at",
            params![
                terminal.id,
                outlet,
                terminal.name.trim(),
                encode::bool_to_sql(terminal.is_master),
                prefix,
                terminal.master_since.map(encode::timestamp_to_sql),
                terminal.last_seen_at.map(encode::timestamp_to_sql),
                encode::timestamp_to_sql(terminal.created_at),
            ],
        )?;

        self.seed_counters(outlet, &terminal.id, &prefix)?;
        OutboxRepo::new(self.tx).enqueue(outlet, "terminals", &terminal.id, Op::Upsert, at)
    }

    /// Give a till its own counter rows, or move its prefix onto the ones it
    /// has.
    ///
    /// **One function owns this invariant.** The prefix that prints lives on
    /// `counters` — the numbering screen edits it there and the claim path
    /// reads it there — so a second writer would be a second answer to "what
    /// does this till print".
    fn seed_counters(&self, outlet: &str, terminal: &str, prefix: &str) -> Result<(), DbError> {
        // The defaults match migration 0001's seeded pair: a token resets
        // daily and is not padded; a bill runs on and is padded to four.
        for (kind, reset_daily, pad) in [("token", 1, 0), ("bill", 0, 4)] {
            self.tx.execute(
                "INSERT INTO counters
                     (outlet_id, terminal_id, kind, last_issued, start, reset_daily,
                      prefix, pad_width, last_reset_day)
                 VALUES (?1, ?2, ?3, NULL, 1, ?4, ?5, ?6, NULL)
                 ON CONFLICT (outlet_id, terminal_id, kind) DO UPDATE SET
                     prefix = excluded.prefix",
                params![outlet, terminal, kind, reset_daily, prefix, pad],
            )?;
        }
        Ok(())
    }

    /// **D139 — move the master. A person did this.**
    ///
    /// One statement clears every master in the outlet and a second sets this
    /// one, inside the caller's transaction, so there is never an instant when
    /// two tills both answer as master or none does.
    ///
    /// The old master is not consulted, and that is the point: the machine that
    /// failed is exactly the one that cannot hand over gracefully, so nothing
    /// in the handover may require it. When it comes back it sees a later
    /// `master_since` than its own and stands down.
    pub fn make_master(&self, outlet: &str, id: &str, at: Timestamp) -> Result<(), DbError> {
        if self.find(outlet, id)?.is_none() {
            return Err(DbError::invariant("that till is not on file"));
        }
        self.tx.execute(
            "UPDATE terminals SET is_master = 0, master_since = NULL WHERE outlet_id = ?1",
            [outlet],
        )?;
        self.tx.execute(
            "UPDATE terminals SET is_master = 1, master_since = ?3
              WHERE outlet_id = ?1 AND id = ?2",
            params![outlet, id, encode::timestamp_to_sql(at)],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "terminals", id, Op::Upsert, at)
    }

    /// A till was heard from. Cheap, and deliberately not an outbox row — the
    /// panel's "last seen" is not a fact anybody syncs.
    pub fn seen(&self, outlet: &str, id: &str, at: Timestamp) -> Result<(), DbError> {
        self.tx.execute(
            "UPDATE terminals SET last_seen_at = ?3 WHERE outlet_id = ?1 AND id = ?2",
            params![outlet, id, encode::timestamp_to_sql(at)],
        )?;
        Ok(())
    }

    /// **How many tills this shop has**, for the licence check at the door
    /// (D141) — never on the billing path.
    pub fn count(&self, outlet: &str) -> Result<u32, DbError> {
        let n: i64 = self.tx.query_row(
            "SELECT COUNT(*) FROM terminals WHERE outlet_id = ?1",
            [outlet],
            |row| row.get(0),
        )?;
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    }
}

fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Terminal> {
    Ok(Terminal {
        id: row.get(0)?,
        name: row.get(1)?,
        is_master: row.get::<_, i64>(2)? == 1,
        series_prefix: row.get(3)?,
        master_since: row.get::<_, Option<i64>>(4)?.map(encode::timestamp_from_sql),
        last_seen_at: row.get::<_, Option<i64>>(5)?.map(encode::timestamp_from_sql),
        created_at: encode::timestamp_from_sql(row.get(6)?),
    })
}
