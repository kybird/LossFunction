#!/usr/bin/env bash
# Local development PostgreSQL for integration tests (no Docker needed).
#
#   scripts/dev_postgres.sh start    # create (first run) + start on 127.0.0.1:5433
#   scripts/dev_postgres.sh stop     # stop the server
#   scripts/dev_postgres.sh status   # is it listening?
#
# Binaries live in the project-local conda env .pgenv/ (gitignored), data in
# .pgdata/ (gitignored). Tests default to
# postgresql://postgres@127.0.0.1:5433/lossfunction_test (LOSSFUNCTION_TEST_DSN
# overrides).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PGBIN="$ROOT/.pgenv16/Library/bin"
PGDATA="$ROOT/.pgdata16"
PORT=5433
DB=lossfunction_test

command_available() {
  [ -x "$PGBIN/pg_ctl.exe" ] || [ -x "$PGBIN/pg_ctl" ]
}

ensure_binaries() {
  if command_available; then
    return
  fi
  echo "[dev_postgres] installing project-local PostgreSQL 16 (conda-forge)..."
  conda create -y -p "$ROOT/.pgenv16" -c conda-forge "postgresql=16"
}

case "${1:-status}" in
  start)
    ensure_binaries
    if [ ! -f "$PGDATA/PG_VERSION" ]; then
      echo "[dev_postgres] initializing data directory..."
      "$PGBIN/initdb" -D "$PGDATA" -U postgres -A trust -E UTF8
    fi
    # Start with a minimal PATH: backends inherit the server environment, and
    # DLLs from unrelated tools on a busy dev PATH crashed them with
    # 0xC0000142 (see doc/raw/2026-09-14.md Case 7).
    env \
      PATH="$(cygpath -w "$PGBIN");C:\\Windows\\System32;C:\\Windows" \
      SYSTEMROOT='C:\Windows' \
      TEMP="$(cygpath -w "$TMPDIR")" \
      TMP="$(cygpath -w "$TMPDIR")" \
      USERPROFILE="$USERPROFILE" \
      "$PGBIN/pg_ctl" -D "$PGDATA" -l "$PGDATA/server.log" -o "-p $PORT" start || true
    sleep 2
    if ! "$PGBIN/psql" -h 127.0.0.1 -p "$PORT" -U postgres -lqt >/dev/null 2>&1; then
      echo "[dev_postgres] server did not come up; see $PGDATA/server.log" >&2
      exit 1
    fi
    "$PGBIN/createdb" -h 127.0.0.1 -p "$PORT" -U postgres "$DB" 2>/dev/null || true
    echo "[dev_postgres] ready on 127.0.0.1:$PORT (db: $DB)"
    ;;
  stop)
    "$PGBIN/pg_ctl" -D "$PGDATA" stop || true
    ;;
  status)
    if netstat -an | grep -q "127.0.0.1:$PORT.*LISTENING"; then
      echo "[dev_postgres] running on 127.0.0.1:$PORT"
    else
      echo "[dev_postgres] not running"
    fi
    ;;
  *)
    echo "usage: $0 {start|stop|status}" >&2
    exit 2
    ;;
esac
