#!/bin/sh
# Runs the match-server behaviour suite against a disposable PostgreSQL cluster made with the mise-pinned binaries,
# then stops and removes the cluster. No machine-wide database is used or left behind. Arguments go to `cargo test`.
#
# The cluster listens on loopback TCP only and requires a password generated for this run, so other local users cannot
# use it. It serves TLS with a throwaway certificate authority, so the suite also checks certificate verification.
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
umask 077

PASSWORD="$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')"
printf '%s' "$PASSWORD" > "$WORK/password"

# A throwaway certificate authority and a server certificate for 127.0.0.1.
mkdir "$WORK/tls"
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=jade-test-ca" \
  -keyout "$WORK/tls/ca.key" -out "$WORK/tls/ca.pem" 2>/dev/null
printf 'subjectAltName=IP:127.0.0.1,DNS:localhost\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n' \
  > "$WORK/tls/server.ext"
openssl req -newkey rsa:2048 -nodes -subj "/CN=127.0.0.1" \
  -keyout "$WORK/tls/server.key" -out "$WORK/tls/server.csr" 2>/dev/null
openssl x509 -req -in "$WORK/tls/server.csr" -CA "$WORK/tls/ca.pem" -CAkey "$WORK/tls/ca.key" -CAserial "$WORK/tls/ca.srl" -CAcreateserial \
  -days 1 -extfile "$WORK/tls/server.ext" -out "$WORK/tls/server.pem" 2>/dev/null
# An unrelated authority, which must not vouch for the server's certificate.
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=jade-unrelated-ca" \
  -keyout "$WORK/tls/other.key" -out "$WORK/tls/other.pem" 2>/dev/null

cd "$REPO_ROOT"
mise exec -- initdb -D "$WORK/data" -U jade --auth=scram-sha-256 --pwfile="$WORK/password" -E UTF8 >/dev/null
# Loopback TCP only: no Unix socket, so the cluster's path length and permissions do not matter. Certificate paths are
# relative to the data directory, so a temporary directory with spaces survives pg_ctl's own option parsing.
mise exec -- pg_ctl -D "$WORK/data" \
  -o "-p $PORT -c listen_addresses=127.0.0.1 -c unix_socket_directories='' -c ssl=on -c ssl_cert_file=../tls/server.pem -c ssl_key_file=../tls/server.key" \
  -l "$WORK/postgres.log" -w start >/dev/null
JADE_TEST_POSTGRES_URL="postgres://jade:$PASSWORD@127.0.0.1:$PORT/postgres" \
JADE_TEST_POSTGRES_CA="$WORK/tls/ca.pem" \
JADE_TEST_POSTGRES_OTHER_CA="$WORK/tls/other.pem" \
  cargo test -p jade-match-server --test match_protocol_integration "$@"
