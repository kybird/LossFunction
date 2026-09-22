//! Persistence layer — SQLite (WAL) repository with audit-in-write-path.
//!
//! Conventions ported from the reference implementation:
//! - money crosses the boundary as Decimal, stored as INTEGER 1e-4 KRW;
//! - timestamps are app-supplied UTC ISO-8601 text (no DB clock defaults);
//! - every trading-state write records an audit row in the same transaction;
//! - numbered migrations tracked in `schema_migrations` (idempotent).

mod migrations;
mod money;

pub use money::{int_to_money, money_to_int};

use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use crate::domain::order::{Order, OrderStatus};
use crate::types::{OrderSide, OrderType, Price, Quantity, Symbol};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("invalid symbol in row: {0}")]
    BadSymbol(String),
    #[error("storage failure: {0}")]
    Internal(String),
}

/// Clone is cheap — the pool is reference-counted; spawned background tasks
/// (e.g. backfill) take their own handle.
#[derive(Clone)]
pub struct Repository {
    pool: SqlitePool,
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

/// Stored timestamps are app-supplied RFC3339 — a row that is not (corrupt
/// or foreign data) surfaces as a storage error, never a panic: reads must
/// not kill the process that owns the database.
fn parse_ts(raw: &str) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(raw)
        .map_err(|error| {
            StorageError::Internal(format!("invalid stored timestamp {raw:?}: {error}"))
        })
        .map(|parsed| parsed.with_timezone(&Utc))
}

impl Repository {
    /// Open (creating if needed) the database in WAL mode and apply
    /// pending migrations.
    pub async fn open(path: &str) -> Result<Self, StorageError> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| StorageError::Internal(error.to_string()))?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_millis(5_000))
            .foreign_keys(true);
        let pool = SqlitePool::connect_with(options).await?;
        let repository = Self { pool };
        repository.migrate().await?;
        Ok(repository)
    }

    pub async fn close(self) {
        self.pool.close().await;
    }

    pub async fn migrate(&self) -> Result<Vec<i64>, StorageError> {
        migrations::apply(&self.pool)
            .await
            .map_err(StorageError::Db)
    }

    async fn write(&self) -> Result<Transaction<'_, Sqlite>, StorageError> {
        Ok(self.pool.begin().await?)
    }

    // ── quotes ─────────────────────────────────────────────────────

    pub async fn insert_quote(
        &self,
        symbol: &Symbol,
        price: Price,
        quoted_at: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO quotes (symbol, price, quoted_at, received_at) \
             VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(symbol.as_str())
        .bind(money_to_int(price))
        .bind(quoted_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
        .bind(now_iso())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Latest stored price per symbol (mark-to-market display).
    pub async fn latest_quotes(&self) -> Result<Vec<(Symbol, Price)>, StorageError> {
        let rows = sqlx::query(
            "SELECT symbol, price FROM quotes \
             WHERE id IN (SELECT MAX(id) FROM quotes GROUP BY symbol)",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let symbol: &str = row.try_get("symbol")?;
                Ok((
                    Symbol::parse(symbol)
                        .map_err(|_| StorageError::BadSymbol(symbol.to_string()))?,
                    int_to_money(row.try_get::<i64, _>("price")?),
                ))
            })
            .collect()
    }

    // ── market data candles ────────────────────────────────────────

    /// Upsert completed OHLCV bars; idempotent on (symbol, timeframe, ts) —
    /// a backfill re-run corrects rows in place instead of duplicating them.
    /// Prices cross the money boundary (1e-4 KRW integers) like every other
    /// write. Returns the number of rows written (inserts + updates).
    pub async fn upsert_candles(
        &self,
        bars: &[crate::marketdata::Bar],
    ) -> Result<u64, StorageError> {
        let mut tx = self.pool.begin().await?;
        let mut written = 0u64;
        for bar in bars {
            let result = sqlx::query(
                "INSERT INTO candles (symbol, timeframe, ts, open, high, low, close, volume) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
                 ON CONFLICT (symbol, timeframe, ts) DO UPDATE SET \
                     open = excluded.open, high = excluded.high, \
                     low = excluded.low, close = excluded.close, \
                     volume = excluded.volume",
            )
            .bind(bar.symbol.as_str())
            .bind(bar.timeframe.as_str())
            .bind(
                bar.timestamp
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            )
            .bind(money_to_int(bar.open))
            .bind(money_to_int(bar.high))
            .bind(money_to_int(bar.low))
            .bind(money_to_int(bar.close))
            .bind(bar.volume)
            .execute(&mut *tx)
            .await?;
            written += result.rows_affected();
        }
        tx.commit().await?;
        Ok(written)
    }

    /// Newest candle timestamp per symbol, symbol-ascending — the status
    /// page's data-freshness display.
    pub async fn latest_candle_dates(&self) -> Result<Vec<(String, String)>, StorageError> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT symbol, MAX(ts) FROM candles GROUP BY symbol ORDER BY symbol")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    // ── watchlist (persistent, editable) ───────────────────────────

    /// Current watchlist, position-ordered.
    pub async fn watchlist(&self) -> Result<Vec<Symbol>, StorageError> {
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT symbol FROM watchlist ORDER BY position, symbol")
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|raw| Symbol::parse(&raw).map_err(|_| StorageError::BadSymbol(raw.clone())))
            .collect()
    }

    /// Insert if absent; false when it was already there (dup is a no-op).
    pub async fn add_to_watchlist(
        &self,
        symbol: &Symbol,
        source: &str,
    ) -> Result<bool, StorageError> {
        let inserted = sqlx::query(
            "INSERT INTO watchlist (symbol, position, added_at, source)              VALUES (?1, (SELECT COALESCE(MAX(position), 0) + 1 FROM watchlist), ?2, ?3)              ON CONFLICT (symbol) DO NOTHING RETURNING symbol",
        )
        .bind(symbol.as_str())
        .bind(now_iso())
        .bind(source)
        .fetch_optional(&self.pool)
        .await?;
        Ok(inserted.is_some())
    }

    /// Delete if present; false when it was not.
    pub async fn remove_from_watchlist(&self, symbol: &Symbol) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM watchlist WHERE symbol = ?1")
            .bind(symbol.as_str())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// First-boot seeding: only when the table is empty.
    pub async fn seed_watchlist(&self, symbols: &[Symbol]) -> Result<(), StorageError> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM watchlist")
            .fetch_one(&self.pool)
            .await?;
        if count > 0 {
            return Ok(());
        }
        for symbol in symbols {
            self.add_to_watchlist(symbol, "settings-seed").await?;
        }
        Ok(())
    }

    /// Upsert the suggested default for one strategy (walk-forward winner).
    pub async fn upsert_strategy_suggestion(
        &self,
        strategy_key: &str,
        params_label: &str,
        improvement: &str,
        metrics_json: &serde_json::Value,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO strategy_suggestions (strategy_key, params_label, improvement, metrics_json, updated_at)              VALUES (?1, ?2, ?3, ?4, ?5)              ON CONFLICT (strategy_key) DO UPDATE SET                  params_label = excluded.params_label,                  improvement = excluded.improvement,                  metrics_json = excluded.metrics_json,                  updated_at = excluded.updated_at",
        )
        .bind(strategy_key)
        .bind(params_label)
        .bind(improvement)
        .bind(metrics_json.to_string())
        .bind(now_iso())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// (strategy_key, params_label, improvement) for the strategy table.
    pub async fn strategy_suggestions(
        &self,
    ) -> Result<Vec<(String, String, String)>, StorageError> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT strategy_key, params_label, improvement FROM strategy_suggestions",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Total stored candle rows — backfill sanity check.
    pub async fn candle_count(&self) -> Result<i64, StorageError> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM candles")
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    /// Stored bars for one symbol/timeframe, oldest first.
    pub async fn daily_candles(
        &self,
        symbol: &Symbol,
        timeframe: crate::marketdata::Timeframe,
    ) -> Result<Vec<crate::marketdata::Bar>, StorageError> {
        let rows = sqlx::query(
            "SELECT symbol, ts, open, high, low, close, volume FROM candles \
             WHERE symbol = ?1 AND timeframe = ?2 ORDER BY ts ASC",
        )
        .bind(symbol.as_str())
        .bind(timeframe.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let raw: &str = row.try_get("symbol")?;
                Ok(crate::marketdata::Bar {
                    symbol: Symbol::parse(raw)
                        .map_err(|_| StorageError::BadSymbol(raw.to_string()))?,
                    timeframe,
                    timestamp: parse_ts(row.try_get("ts")?)?,
                    open: int_to_money(row.try_get::<i64, _>("open")?),
                    high: int_to_money(row.try_get::<i64, _>("high")?),
                    low: int_to_money(row.try_get::<i64, _>("low")?),
                    close: int_to_money(row.try_get::<i64, _>("close")?),
                    volume: row.try_get("volume")?,
                })
            })
            .collect()
    }

    // ── orders ─────────────────────────────────────────────────────

    pub async fn create_order(&self, order: &Order, mode: &str) -> Result<(), StorageError> {
        let mut tx = self.write().await?;
        let now = now_iso();
        sqlx::query(
            "INSERT INTO orders (client_order_id, symbol, side, order_type, \
             quantity, limit_price, status, mode, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        )
        .bind(order.client_order_id())
        .bind(order.symbol().as_str())
        .bind(match order.side() {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        })
        .bind(match order.order_type() {
            OrderType::Market => "market",
            OrderType::Limit => "limit",
        })
        .bind(order.quantity())
        .bind(order.limit_price().map(money_to_int))
        .bind(order.status().to_string())
        .bind(mode)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        audit(
            &mut tx,
            "order.created",
            order.client_order_id(),
            &serde_json::json!({
                "symbol": order.symbol().as_str(),
                "side": order.side(), "order_type": order.order_type(),
                "quantity": order.quantity(),
                "limit_price": order.limit_price().map(|p| p.to_string()),
                "status": order.status().to_string(), "mode": mode,
            }),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_order_status(
        &self,
        client_order_id: &str,
        status: OrderStatus,
        broker_order_id: Option<&str>,
    ) -> Result<bool, StorageError> {
        let mut tx = self.write().await?;
        let result = sqlx::query(
            "UPDATE orders SET status = ?2, \
             broker_order_id = COALESCE(?3, broker_order_id), updated_at = ?4 \
             WHERE client_order_id = ?1",
        )
        .bind(client_order_id)
        .bind(status.to_string())
        .bind(broker_order_id)
        .bind(now_iso())
        .execute(&mut *tx)
        .await?;
        let updated = result.rows_affected() == 1;
        if updated {
            audit(
                &mut tx,
                "order.status_changed",
                client_order_id,
                &serde_json::json!({
                    "status": status.to_string(),
                    "broker_order_id": broker_order_id,
                }),
            )
            .await?;
        }
        tx.commit().await?;
        Ok(updated)
    }

    pub async fn recent_orders(&self, limit: i64) -> Result<Vec<RecentOrder>, StorageError> {
        let rows = sqlx::query(
            "SELECT client_order_id, symbol, side, quantity, limit_price, status, \
             created_at FROM orders ORDER BY created_at DESC, rowid DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(RecentOrder {
                    client_order_id: row.try_get("client_order_id")?,
                    symbol: row.try_get::<String, _>("symbol")?,
                    side: row.try_get("side")?,
                    quantity: row.try_get("quantity")?,
                    limit_price: row
                        .try_get::<Option<i64>, _>("limit_price")?
                        .map(int_to_money),
                    status: row.try_get("status")?,
                    created_at: parse_ts(row.try_get("created_at")?)?,
                })
            })
            .collect()
    }

    // ── fills ──────────────────────────────────────────────────────

    /// Insert a fill (idempotent on its natural key); true when inserted.
    pub async fn record_fill(
        &self,
        client_order_id: &str,
        symbol: &Symbol,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        executed_at: DateTime<Utc>,
    ) -> Result<bool, StorageError> {
        let mut tx = self.write().await?;
        let inserted = sqlx::query(
            "INSERT INTO fills (client_order_id, symbol, side, quantity, price, executed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT DO NOTHING RETURNING id",
        )
        .bind(client_order_id)
        .bind(symbol.as_str())
        .bind(match side {
            crate::types::OrderSide::Buy => "buy",
            crate::types::OrderSide::Sell => "sell",
        })
        .bind(quantity)
        .bind(money_to_int(price))
        .bind(executed_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
        .fetch_optional(&mut *tx)
        .await?;
        if inserted.is_none() {
            tx.commit().await?;
            return Ok(false);
        }
        audit(
            &mut tx,
            "fill.recorded",
            client_order_id,
            &serde_json::json!({
                "symbol": symbol.as_str(), "side": side, "quantity": quantity,
                "price": price.to_string(),
            }),
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn recent_fills(&self, limit: i64) -> Result<Vec<RecentFill>, StorageError> {
        let rows = sqlx::query(
            "SELECT client_order_id, symbol, side, quantity, price, executed_at FROM fills \
             ORDER BY executed_at DESC, id DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(RecentFill {
                    client_order_id: row.try_get("client_order_id")?,
                    symbol: row.try_get("symbol")?,
                    side: row.try_get("side")?,
                    quantity: row.try_get("quantity")?,
                    price: int_to_money(row.try_get("price")?),
                    executed_at: parse_ts(row.try_get("executed_at")?)?,
                })
            })
            .collect()
    }

    /// Newest-last price series for one symbol (quotes table) — the status
    /// page sparkline source.
    pub async fn quote_history(
        &self,
        symbol: &Symbol,
        limit: i64,
    ) -> Result<Vec<Price>, StorageError> {
        let rows: Vec<i64> = sqlx::query_scalar(
            "SELECT price FROM quotes WHERE symbol = ?1 ORDER BY id DESC LIMIT ?2",
        )
        .bind(symbol.as_str())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut prices: Vec<Price> = rows.into_iter().map(int_to_money).collect();
        prices.reverse(); // oldest first, left-to-right reading order
        Ok(prices)
    }

    // ── positions ──────────────────────────────────────────────────

    pub async fn upsert_position(
        &self,
        symbol: &Symbol,
        quantity: Quantity,
        average_price: Price,
    ) -> Result<(), StorageError> {
        let mut tx = self.write().await?;
        sqlx::query(
            "INSERT INTO positions (symbol, quantity, average_price, as_of) \
             VALUES (?1, ?2, ?3, ?4) ON CONFLICT (symbol) DO UPDATE SET \
             quantity = excluded.quantity, \
             average_price = excluded.average_price, as_of = excluded.as_of",
        )
        .bind(symbol.as_str())
        .bind(quantity)
        .bind(money_to_int(average_price))
        .bind(now_iso())
        .execute(&mut *tx)
        .await?;
        audit(
            &mut tx,
            "position.updated",
            symbol.as_str(),
            &serde_json::json!({
                "quantity": quantity, "average_price": average_price.to_string(),
            }),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn positions(&self) -> Result<Vec<StoredPosition>, StorageError> {
        let rows =
            sqlx::query("SELECT symbol, quantity, average_price FROM positions ORDER BY symbol")
                .fetch_all(&self.pool)
                .await?;
        rows.iter()
            .map(|row| {
                Ok(StoredPosition {
                    symbol: row.try_get("symbol")?,
                    quantity: row.try_get("quantity")?,
                    average_price: int_to_money(row.try_get("average_price")?),
                })
            })
            .collect()
    }

    // ── audit ──────────────────────────────────────────────────────

    /// Persist an operational event (controls, lifecycle) to the audit log.
    pub async fn record_event(
        &self,
        event_type: &str,
        subject: &str,
        payload: &serde_json::Value,
    ) -> Result<(), StorageError> {
        let mut tx = self.write().await?;
        audit(&mut tx, event_type, subject, payload).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn recent_audit(&self, limit: i64) -> Result<Vec<AuditRow>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, event_type, subject, payload, occurred_at FROM audit_log \
             ORDER BY id DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut events: Vec<AuditRow> = rows
            .iter()
            .map(|row| {
                Ok(AuditRow {
                    event_type: row.try_get("event_type")?,
                    subject: row.try_get::<Option<String>, _>("subject")?,
                    payload: serde_json::from_str(row.try_get::<&str, _>("payload")?)
                        .unwrap_or(serde_json::Value::Null),
                    occurred_at: parse_ts(row.try_get("occurred_at")?)?,
                })
            })
            .collect::<Result<_, StorageError>>()?;
        events.reverse(); // oldest first for display
        Ok(events)
    }
}

async fn audit(
    tx: &mut Transaction<'_, Sqlite>,
    event_type: &str,
    subject: &str,
    payload: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_log (event_type, subject, payload, occurred_at) \
         VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(event_type)
    .bind(subject)
    .bind(payload.to_string())
    .bind(now_iso())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct RecentOrder {
    pub client_order_id: String,
    pub symbol: String,
    pub side: String,
    pub quantity: Quantity,
    pub limit_price: Option<Price>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RecentFill {
    pub client_order_id: String,
    pub symbol: String,
    /// "buy" | "sell" — display decomposition of OrderSide.
    pub side: String,
    pub quantity: Quantity,
    pub price: Price,
    pub executed_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct StoredPosition {
    pub symbol: String,
    pub quantity: Quantity,
    pub average_price: Price,
}

#[derive(Debug, Clone)]
pub struct AuditRow {
    pub event_type: String,
    pub subject: Option<String>,
    pub payload: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::order::RestoredOrder;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    async fn repo() -> (tempfile::TempDir, Repository) {
        let dir = tempfile::tempdir().unwrap();
        let repository = Repository::open(dir.path().join("test.db").to_str().unwrap())
            .await
            .unwrap();
        (dir, repository)
    }

    fn order(id: &str, status: OrderStatus) -> Order {
        Order::restored(RestoredOrder {
            client_order_id: id.into(),
            symbol: Symbol::parse("005930").unwrap(),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: 10,
            limit_price: Some(Decimal::from(79_000)),
            status,
            filled_quantity: 0,
        })
        .unwrap()
    }

    #[tokio::test]
    async fn migrations_are_idempotent() {
        let (_dir, repository) = repo().await;
        let again = repository.migrate().await.unwrap();
        assert!(again.is_empty());

        let (version,): (i64,) = sqlx::query_as("SELECT MAX(version) FROM schema_migrations")
            .fetch_one(&repository.pool)
            .await
            .unwrap();
        assert_eq!(version, 4); // latest registered migration

        let tables: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(&repository.pool)
        .await
        .unwrap();
        let names: Vec<String> = tables.into_iter().map(|(n,)| n).collect();
        for expected in [
            "quotes",
            "candles",
            "orders",
            "fills",
            "positions",
            "audit_log",
            "schema_migrations",
        ] {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[tokio::test]
    async fn order_lifecycle_audited() {
        let (_dir, repository) = repo().await;

        repository
            .create_order(&order("it-1", OrderStatus::Pending), "paper")
            .await
            .unwrap();
        assert!(repository
            .update_order_status("it-1", OrderStatus::Submitted, Some("ODNO-77"))
            .await
            .unwrap());
        assert!(!repository
            .update_order_status("nope", OrderStatus::Filled, None)
            .await
            .unwrap());

        let executed_at = Utc::now();
        assert!(repository
            .record_fill(
                "it-1",
                &Symbol::parse("005930").unwrap(),
                OrderSide::Buy,
                10,
                Decimal::from(79_000),
                executed_at,
            )
            .await
            .unwrap());
        assert!(
            !repository
                .record_fill(
                    "it-1",
                    &Symbol::parse("005930").unwrap(),
                    OrderSide::Buy,
                    10,
                    Decimal::from(79_000),
                    executed_at,
                )
                .await
                .unwrap() // natural key blocks duplicates
        );

        repository
            .upsert_position(&Symbol::parse("005930").unwrap(), 10, Decimal::from(79_000))
            .await
            .unwrap();
        let positions = repository.positions().await.unwrap();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].quantity, 10);
        assert_eq!(positions[0].average_price, Decimal::from(79_000));

        let events = repository.recent_audit(50).await.unwrap();
        let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(
            types,
            vec![
                "order.created",
                "order.status_changed",
                "fill.recorded",
                "position.updated"
            ]
        );

        let orders = repository.recent_orders(10).await.unwrap();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].status, "submitted");
        assert_eq!(orders[0].limit_price, Some(Decimal::from(79_000)));
    }

    #[tokio::test]
    async fn quotes_stored_with_scaled_money() {
        let (_dir, repository) = repo().await;
        let symbol = Symbol::parse("035420").unwrap();
        repository
            .insert_quote(&symbol, Decimal::from_str("41000.5").unwrap(), Utc::now())
            .await
            .unwrap();
        let quotes = repository.latest_quotes().await.unwrap();
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].1, Decimal::from_str("41000.5").unwrap());
    }

    #[tokio::test]
    async fn record_event_writes_audit() {
        let (_dir, repository) = repo().await;
        repository
            .record_event(
                "control.kill_switch",
                "operator",
                &serde_json::json!({"activate": true}),
            )
            .await
            .unwrap();
        let events = repository.recent_audit(10).await.unwrap();
        assert_eq!(events[0].event_type, "control.kill_switch");
        assert_eq!(events[0].payload["activate"], serde_json::json!(true));
    }

    /// A corrupted stored timestamp surfaces as a storage error, never a
    /// panic — reads must not kill the process owning the database
    /// (was: parse_ts `.expect`).
    #[tokio::test]
    async fn corrupted_timestamp_is_an_error_not_a_panic() {
        let (_dir, repository) = repo().await;
        repository.migrate().await.unwrap();

        sqlx::query(
            "INSERT INTO candles (symbol, timeframe, ts, open, high, low, close, volume)              VALUES ('005930', 'day', 'not-a-timestamp', 100, 100, 100, 100, 1)",
        )
        .execute(&repository.pool)
        .await
        .unwrap();

        let result = repository
            .daily_candles(
                &Symbol::parse("005930").unwrap(),
                crate::marketdata::Timeframe::Day,
            )
            .await;
        assert!(matches!(result, Err(StorageError::Internal(_))));
    }

    /// Watchlist persistence: seed-once, add (dup no-op), remove, order.
    #[tokio::test]
    async fn watchlist_add_remove_seed_semantics() {
        let (_dir, repository) = repo().await;
        repository.migrate().await.unwrap();

        let samsung = Symbol::parse("005930").unwrap();
        let naver = Symbol::parse("035420").unwrap();
        repository
            .seed_watchlist(&[samsung.clone(), naver.clone()])
            .await
            .unwrap();
        // Seeding again must not duplicate.
        repository
            .seed_watchlist(std::slice::from_ref(&samsung))
            .await
            .unwrap();
        assert_eq!(repository.watchlist().await.unwrap().len(), 2);

        // Duplicate add is a no-op (false).
        assert!(!repository.add_to_watchlist(&samsung, "web").await.unwrap());
        let kakao = Symbol::parse("035720").unwrap();
        assert!(repository.add_to_watchlist(&kakao, "web").await.unwrap());
        assert_eq!(repository.watchlist().await.unwrap().len(), 3);

        assert!(repository.remove_from_watchlist(&naver).await.unwrap());
        assert!(!repository.remove_from_watchlist(&naver).await.unwrap());
        let remaining = repository.watchlist().await.unwrap();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.contains(&kakao));
    }
}
