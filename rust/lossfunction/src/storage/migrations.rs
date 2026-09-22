//! Numbered, idempotent schema migrations (SQLite dialect).
//!
//! Same schema as the reference implementation: money INTEGER in 1e-4 KRW,
//! timestamps TEXT UTC ISO-8601 app-supplied, JSON TEXT payloads.

use sqlx::{Row, SqlitePool};

const MIGRATION_1: &str = r#"
CREATE TABLE quotes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol      TEXT NOT NULL,
    price       INTEGER NOT NULL,
    quoted_at   TEXT NOT NULL,
    received_at TEXT NOT NULL
);
CREATE INDEX quotes_symbol_time_idx ON quotes (symbol, quoted_at DESC);

CREATE TABLE candles (
    symbol    TEXT NOT NULL,
    timeframe TEXT NOT NULL,
    ts        TEXT NOT NULL,
    open      INTEGER NOT NULL,
    high      INTEGER NOT NULL,
    low       INTEGER NOT NULL,
    close     INTEGER NOT NULL,
    volume    INTEGER NOT NULL,
    PRIMARY KEY (symbol, timeframe, ts)
);

CREATE TABLE orders (
    client_order_id TEXT PRIMARY KEY,
    broker_order_id TEXT UNIQUE,
    symbol        TEXT NOT NULL,
    side          TEXT NOT NULL,
    order_type    TEXT NOT NULL,
    quantity      INTEGER NOT NULL,
    limit_price   INTEGER,
    status        TEXT NOT NULL,
    mode          TEXT NOT NULL DEFAULT 'paper',
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
CREATE INDEX orders_status_idx ON orders (status);

CREATE TABLE fills (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    client_order_id TEXT NOT NULL REFERENCES orders (client_order_id),
    quantity        INTEGER NOT NULL,
    price           INTEGER NOT NULL,
    executed_at     TEXT NOT NULL,
    UNIQUE (client_order_id, executed_at, quantity, price)
);

CREATE TABLE positions (
    symbol        TEXT PRIMARY KEY,
    quantity      INTEGER NOT NULL,
    average_price INTEGER NOT NULL,
    as_of         TEXT NOT NULL
);

CREATE TABLE audit_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type  TEXT NOT NULL,
    subject     TEXT,
    payload     TEXT NOT NULL,
    occurred_at TEXT NOT NULL
);
CREATE INDEX audit_subject_idx ON audit_log (subject, occurred_at DESC);
"#;

/// Fills gain the display columns symbol/side (they previously lived only
/// in the audit payload — the status page needs them directly).
const MIGRATION_2: &str = r#"
ALTER TABLE fills ADD COLUMN symbol TEXT NOT NULL DEFAULT '';
ALTER TABLE fills ADD COLUMN side TEXT NOT NULL DEFAULT '';
"#;

/// The watchlist becomes a first-class, editable, persistent state.
const MIGRATION_3: &str = r#"
CREATE TABLE watchlist (
    symbol    TEXT PRIMARY KEY,
    position  INTEGER NOT NULL DEFAULT 0,
    added_at  TEXT NOT NULL,
    source    TEXT NOT NULL DEFAULT 'manual'
);
"#;

const MIGRATIONS: &[(i64, &str)] = &[(1, MIGRATION_1), (2, MIGRATION_2), (3, MIGRATION_3)];

/// Apply pending migrations; returns versions applied this run.
pub async fn apply(pool: &SqlitePool) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_migrations (\
         version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)",
    )
    .execute(pool)
    .await?;

    let rows = sqlx::query("SELECT version FROM schema_migrations")
        .fetch_all(pool)
        .await?;
    let applied: Vec<i64> = rows
        .iter()
        .map(|row| -> i64 { row.try_get(0usize).unwrap_or_default() })
        .collect();

    let mut newly_applied = Vec::new();
    for (version, sql) in MIGRATIONS {
        if applied.contains(version) {
            continue;
        }
        let mut tx = pool.begin().await?;
        // executescript is not exposed by sqlx; batch statements one by one.
        for statement in sql.split(';') {
            let trimmed = statement.trim();
            if trimmed.is_empty() {
                continue;
            }
            sqlx::query(trimmed).execute(&mut *tx).await?;
        }
        sqlx::query(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, datetime('now'))",
        )
        .bind(version)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        newly_applied.push(*version);
    }
    Ok(newly_applied)
}
