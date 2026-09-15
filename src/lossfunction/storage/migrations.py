"""Numbered, idempotent schema migrations.

Each entry is applied at most once, tracked in `schema_migrations`.
Re-running `migrate()` on an up-to-date database is a no-op.
"""

import asyncpg

MIGRATIONS: list[tuple[int, str]] = [
    (
        1,
        """
        CREATE TABLE quotes (
            id          bigserial PRIMARY KEY,
            symbol      varchar(6) NOT NULL,
            price       numeric(18, 4) NOT NULL,
            quoted_at   timestamptz NOT NULL,
            received_at timestamptz NOT NULL DEFAULT now()
        );
        CREATE INDEX quotes_symbol_time_idx ON quotes (symbol, quoted_at DESC);

        CREATE TABLE candles (
            symbol    varchar(6) NOT NULL,
            timeframe varchar(3) NOT NULL,
            ts        timestamptz NOT NULL,
            open      numeric(18, 4) NOT NULL,
            high      numeric(18, 4) NOT NULL,
            low       numeric(18, 4) NOT NULL,
            close     numeric(18, 4) NOT NULL,
            volume    bigint NOT NULL,
            PRIMARY KEY (symbol, timeframe, ts)
        );

        CREATE TABLE orders (
            client_order_id text PRIMARY KEY,
            broker_order_id text UNIQUE,
            symbol        varchar(6) NOT NULL,
            side          varchar(4) NOT NULL,
            order_type    varchar(6) NOT NULL,
            quantity      bigint NOT NULL,
            limit_price   numeric(18, 4),
            status        varchar(16) NOT NULL,
            mode          varchar(5) NOT NULL DEFAULT 'paper',
            created_at    timestamptz NOT NULL DEFAULT now(),
            updated_at    timestamptz NOT NULL DEFAULT now()
        );
        CREATE INDEX orders_status_idx ON orders (status);

        CREATE TABLE fills (
            id              bigserial PRIMARY KEY,
            client_order_id text NOT NULL REFERENCES orders (client_order_id),
            quantity        bigint NOT NULL,
            price           numeric(18, 4) NOT NULL,
            executed_at     timestamptz NOT NULL,
            UNIQUE (client_order_id, executed_at, quantity, price)
        );

        CREATE TABLE positions (
            symbol        varchar(6) PRIMARY KEY,
            quantity      bigint NOT NULL,
            average_price numeric(18, 4) NOT NULL,
            as_of         timestamptz NOT NULL DEFAULT now()
        );

        CREATE TABLE audit_log (
            id          bigserial PRIMARY KEY,
            event_type  varchar(64) NOT NULL,
            subject     varchar(128),
            payload     jsonb NOT NULL,
            occurred_at timestamptz NOT NULL DEFAULT now()
        );
        CREATE INDEX audit_subject_idx ON audit_log (subject, occurred_at DESC);
        """,
    ),
]


async def migrate(pool: asyncpg.Pool) -> list[int]:
    """Apply pending migrations; return the versions applied this run."""
    async with pool.acquire() as connection:
        await connection.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations ("
            " version integer PRIMARY KEY,"
            " applied_at timestamptz NOT NULL DEFAULT now())"
        )
        rows = await connection.fetch("SELECT version FROM schema_migrations")
        applied = {row["version"] for row in rows}

    newly_applied: list[int] = []
    for version, sql in MIGRATIONS:
        if version in applied:
            continue
        async with pool.acquire() as connection, connection.transaction():
            await connection.execute(sql)
            await connection.execute("INSERT INTO schema_migrations (version) VALUES ($1)", version)
        newly_applied.append(version)
    return newly_applied
