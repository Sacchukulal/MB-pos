//! Orders that leave on a bike.

use mb_core::Timestamp;
use mb_core::businessday::BusinessDay;
use mb_core::money::Money;
use rusqlite::{Transaction, params};

use crate::encode;
use crate::error::DbError;

#[derive(Debug)]
pub struct DeliveryRepo<'a> {
    tx: &'a Transaction<'a>,
}

/// Where a delivery has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryState {
    Pending,
    Assigned,
    Out,
    Delivered,
    Failed,
}

impl DeliveryState {
    #[must_use]
    pub const fn as_sql(self) -> &'static str {
        match self {
            DeliveryState::Pending => "pending",
            DeliveryState::Assigned => "assigned",
            DeliveryState::Out => "out",
            DeliveryState::Delivered => "delivered",
            DeliveryState::Failed => "failed",
        }
    }

    pub fn from_sql(text: &str) -> Result<Self, DbError> {
        match text {
            "pending" => Ok(DeliveryState::Pending),
            "assigned" => Ok(DeliveryState::Assigned),
            "out" => Ok(DeliveryState::Out),
            "delivered" => Ok(DeliveryState::Delivered),
            "failed" => Ok(DeliveryState::Failed),
            other => Err(DbError::invariant(format!(
                "a delivery state of {other:?} is not one this program knows"
            ))),
        }
    }

    /// What may follow what.
    #[must_use]
    pub const fn may_become(self, next: DeliveryState) -> bool {
        matches!(
            (self, next),
            (DeliveryState::Pending, DeliveryState::Assigned)
                | (DeliveryState::Assigned, DeliveryState::Out)
                | (DeliveryState::Assigned, DeliveryState::Pending)
                | (DeliveryState::Out, DeliveryState::Delivered)
                | (DeliveryState::Out, DeliveryState::Failed)
        )
    }

    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            DeliveryState::Pending => "Waiting for a rider",
            DeliveryState::Assigned => "A rider has it",
            DeliveryState::Out => "On the way",
            DeliveryState::Delivered => "Delivered",
            DeliveryState::Failed => "Did not arrive",
        }
    }
}

/// One delivery, as a screen reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub order_id: String,
    pub bill_number: Option<String>,
    pub day: BusinessDay,
    pub address: Option<String>,
    pub rider_id: Option<String>,
    pub rider_name: Option<String>,
    pub state: DeliveryState,
    /// Why it did not arrive.
    pub failure: Option<String>,
    pub total: Money,
    /// Cash the rider took, or zero when it was paid at the counter or on a card.
    pub cash_collected: Money,
    pub customer_name: Option<String>,
    pub phone: Option<String>,
    /// The ORDER's state — `draft`, `open`, `settled`, `cancelled`, `voided`.
    pub order_state: String,
}

/// One rider's evening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiderDay {
    pub rider_id: String,
    pub rider_name: String,
    pub out: i64,
    pub delivered: i64,
    pub failed: i64,
    pub collected: Money,
    pub handed_back: Money,
    /// `collected − handed_back`, floored at zero.
    pub carrying: Money,
}

impl<'a> DeliveryRepo<'a> {
    #[must_use]
    pub fn new(tx: &'a Transaction<'a>) -> Self {
        DeliveryRepo { tx }
    }

    /// Say where a delivery is going, and who is taking it.
    #[allow(clippy::too_many_arguments, reason = "a delivery IS this many facts")]
    pub fn set_delivery(
        &self,
        outlet: &str,
        order_id: &str,
        address: Option<&str>,
        customer_id: Option<&str>,
        rider_id: Option<&str>,
        state: DeliveryState,
        failure: Option<&str>,
    ) -> Result<(), DbError> {
        let current: Option<String> = self
            .tx
            .query_row(
                "SELECT COALESCE(delivery_state, 'pending') FROM orders
                  WHERE outlet_id = ?1 AND id = ?2 AND order_type = 'delivery'",
                params![outlet, order_id],
                |r| r.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    DbError::invariant("that order is not a delivery on this counter".to_owned())
                }
                other => DbError::from(other),
            })?;
        let now = DeliveryState::from_sql(&current.unwrap_or_else(|| "pending".to_owned()))?;
        if now != state && !now.may_become(state) {
            return Err(DbError::invariant(format!(
                "a delivery that is \"{}\" cannot become \"{}\"",
                now.words(),
                state.words()
            )));
        }

        let failure = match (state, failure.map(str::trim).filter(|f| !f.is_empty())) {
            (DeliveryState::Failed, None) => {
                return Err(DbError::invariant(
                    "say why the delivery did not arrive".to_owned(),
                ));
            }
            (DeliveryState::Failed, some) => some,
            // Moving off `failed` — which today only a void does — clears the reason, because a
            // reason on a delivered order is a lie the CHECK would refuse anyway.
            _ => None,
        };

        self.tx.execute(
            "UPDATE orders
                SET delivery_address = COALESCE(?3, delivery_address),
                    customer_id      = COALESCE(?7, customer_id),
                    delivery_rider   = ?4,
                    delivery_state   = ?5,
                    delivery_failure = ?6
              WHERE outlet_id = ?1 AND id = ?2 AND order_type = 'delivery'",
            params![
                outlet,
                order_id,
                address,
                rider_id,
                state.as_sql(),
                failure,
                customer_id
            ],
        )?;
        Ok(())
    }

    /// One delivery, or nothing.
    pub fn delivery(&self, outlet: &str, order_id: &str) -> Result<Option<Delivery>, DbError> {
        Ok(self.list_where(Some(order_id), outlet, None)?.pop())
    }

    /// Every delivery on a day, newest first.
    pub fn deliveries_on(&self, outlet: &str, day: BusinessDay) -> Result<Vec<Delivery>, DbError> {
        self.list_where(None, outlet, Some(day))
    }

    /// One query for both readers, and the filters are BOUND rather than spliced: a `format!`
    /// that changes which parameters the statement has is how the two callers ended up passing
    /// a different number of them.
    fn list_where(
        &self,
        order_id: Option<&str>,
        outlet: &str,
        day: Option<BusinessDay>,
    ) -> Result<Vec<Delivery>, DbError> {
        // The cash figure is a sub-select, not a join.
        let mut stmt = self.tx.prepare(
            "SELECT o.id,
                    o.bill_number_formatted,
                    o.business_day,
                    COALESCE(o.delivery_address, c.address),
                    o.delivery_rider,
                    s.name,
                    COALESCE(o.delivery_state, 'pending'),
                    o.delivery_failure,
                    COALESCE(b.grand_total, 0),
                    COALESCE((SELECT SUM(p.amount + p.tip) FROM payments p
                               WHERE p.order_id = o.id AND p.mode = 'cash'), 0),
                    c.name,
                    c.phone,
                    o.state
               FROM orders o
          LEFT JOIN bills b ON b.order_id = o.id
          LEFT JOIN staff s ON s.id = o.delivery_rider
          LEFT JOIN customers c ON c.id = o.customer_id
              WHERE o.outlet_id = ?1 AND o.order_type = 'delivery'
                AND (?2 IS NULL OR o.business_day = ?2)
                AND (?3 IS NULL OR o.id = ?3)
           ORDER BY o.business_day DESC, o.created_at DESC",
        )?;
        let day_sql = day.map(encode::business_day_to_sql);
        let mut rows = stmt.query(params![outlet, day_sql, order_id])?;

        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(Delivery {
                order_id: row.get(0)?,
                bill_number: row.get(1)?,
                day: encode::business_day_from_sql(row.get(2)?, "orders.business_day")?,
                address: row.get(3)?,
                rider_id: row.get(4)?,
                rider_name: row.get(5)?,
                state: DeliveryState::from_sql(&row.get::<_, String>(6)?)?,
                failure: row.get(7)?,
                total: encode::money_from_sql(row.get(8)?),
                cash_collected: encode::money_from_sql(row.get(9)?),
                customer_name: row.get(10)?,
                phone: row.get(11)?,
                order_state: row.get(12)?,
            });
        }
        Ok(out)
    }

    /// The people who take orders out.
    pub fn riders(&self, outlet: &str) -> Result<Vec<(String, String)>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name FROM staff
              WHERE outlet_id = ?1 AND is_rider = 1 AND status <> 'left'
              ORDER BY name",
        )?;
        let mut rows = stmt.query(params![outlet])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
        }
        Ok(out)
    }

    pub fn set_rider_flag(
        &self,
        outlet: &str,
        staff_id: &str,
        is_rider: bool,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "UPDATE staff SET is_rider = ?3, updated_at = ?4
              WHERE outlet_id = ?1 AND id = ?2",
            params![
                outlet,
                staff_id,
                encode::bool_to_sql(is_rider),
                encode::timestamp_to_sql(at)
            ],
        )?;
        Ok(())
    }

    /// Money handed over the counter by a rider.
    #[allow(clippy::too_many_arguments, reason = "a ledger row IS this many facts")]
    pub fn record_handback(
        &self,
        outlet: &str,
        id: &str,
        rider_id: &str,
        amount: Money,
        at: Timestamp,
        day: BusinessDay,
        taken_by: Option<&str>,
        note: Option<&str>,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO rider_handbacks
                 (id, outlet_id, rider_id, business_day, amount, at, taken_by, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                outlet,
                rider_id,
                encode::business_day_to_sql(day),
                encode::money_to_sql(amount),
                encode::timestamp_to_sql(at),
                taken_by,
                note,
            ],
        )?;
        Ok(())
    }

    /// Every rider's evening. What went out, what arrived, what was collected, what came back,
    /// and what is still on a bike.
    pub fn rider_day(&self, outlet: &str, day: BusinessDay) -> Result<Vec<RiderDay>, DbError> {
        let day_sql = encode::business_day_to_sql(day);
        let mut stmt = self.tx.prepare(
            "SELECT s.id, s.name,
                    SUM(CASE WHEN o.delivery_state = 'out' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN o.delivery_state = 'delivered' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN o.delivery_state = 'failed' THEN 1 ELSE 0 END),
                    COALESCE(SUM(CASE WHEN o.delivery_state IN ('out', 'delivered')
                                      THEN (SELECT COALESCE(SUM(p.amount + p.tip), 0)
                                              FROM payments p
                                             WHERE p.order_id = o.id AND p.mode = 'cash')
                                      ELSE 0 END), 0)
               FROM orders o JOIN staff s ON s.id = o.delivery_rider
              WHERE o.outlet_id = ?1 AND o.business_day = ?2
                AND o.order_type = 'delivery'
           GROUP BY s.id
           ORDER BY s.name",
        )?;
        let mut rows = stmt.query(params![outlet, day_sql])?;

        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let rider_id: String = row.get(0)?;
            let collected = encode::money_from_sql(row.get(5)?);
            let handed: i64 = self.tx.query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM rider_handbacks
                  WHERE outlet_id = ?1 AND rider_id = ?2 AND business_day = ?3",
                params![outlet, rider_id, day_sql],
                |r| r.get(0),
            )?;
            let handed_back = encode::money_from_sql(handed);
            // Floored at zero: a rider who hands back more than they collected has made a
            // mistake somebody must look at, and it must never read as the shop being owed
            // money by its own till.
            let carrying = if handed_back >= collected {
                Money::ZERO
            } else {
                collected.sub(handed_back).unwrap_or(Money::ZERO)
            };

            out.push(RiderDay {
                rider_id,
                rider_name: row.get(1)?,
                out: row.get(2)?,
                delivered: row.get(3)?,
                failed: row.get(4)?,
                collected,
                handed_back,
                carrying,
            });
        }
        Ok(out)
    }
}
