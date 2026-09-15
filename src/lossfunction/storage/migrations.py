"""Numbered, idempotent schema migrations for SQLite.

Conventions (enforced at the repository boundary):
- Money: INTEGER in units of 1e-4 KRW (KIS's own price precision); Decimal
  round-trips exactly through scaleb(4).
- Timestamps: TEXT ISO 8601 (UTC) supplied by the application — no DB-side
  clock defaults, so ordering is lexicographic and deterministic.
- JSON: TEXT payloads encoded/decoded by the repository.

Each entry is applied at most once, tracked in `schema_migrations`.
Re-running `migrate()` on an up-to-date database is a no-op.
"""

import aiosqlite

MIGRATIONS: list[tuple[int, str]] = [
    (
        1,
        """
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
        """,
    ),
]


async def migrate(db: aiosqlite.Connection) -> list[int]:
    """Apply pending migrations; return the versions applied this run."""
    await db.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations ("
        " version INTEGER PRIMARY KEY,"
        " applied_at TEXT NOT NULL)"
    )
    await db.commit()

    cursor = await db.execute("SELECT version FROM schema_migrations")
    applied = {row[0] for row in await cursor.fetchall()}

    newly_applied: list[int] = []
    for version, sql in MIGRATIONS:
        if version in applied:
            continue
        await db.execute("BEGIN IMMEDIATE")
        try:
            await db.executescript(sql)
            await db.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?, datetime('now'))",
                (version,),
            )
            await db.commit()
        except Exception:
            await db.rollback()
            raise
        newly_applied.append(version)
    return newly_applied
