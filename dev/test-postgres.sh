#!/bin/sh
# Runs the match-server behaviour suite against a disposable PostgreSQL cluster made with the mise-pinned binaries,
# then stops and removes the cluster. No machine-wide database is used or left behind. Arguments go to `cargo test`.
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/jade-pg.XXXXXX")"
PORT="${JADE_TEST_POSTGRES_PORT:-$((40000 + $$ % 20000))}"

cleanup() {
  mise exec -C "$REPO_ROOT" -- pg_ctl -D "$WORK/data" -m immediate stop >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

cd "$REPO_ROOT"
mise exec -- initdb -D "$WORK/data" -U jade --auth=trust -E UTF8 >/dev/null
# Loopback TCP only: no Unix socket, so the cluster's path length and permissions do not matter.
mise exec -- pg_ctl -D "$WORK/data" \
  -o "-p $PORT -c listen_addresses=127.0.0.1 -c unix_socket_directories=''" \
  -l "$WORK/postgres.log" -w start >/dev/null
JADE_TEST_POSTGRES_URL="postgres://jade@127.0.0.1:$PORT/postgres" \
  cargo test -p jade-match-server --test match_protocol_integration "$@"
