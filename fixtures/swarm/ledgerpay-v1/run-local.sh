#!/bin/sh
set -eu

fixture_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
postgres=$(docker run --rm -d -e POSTGRES_PASSWORD=ledgerpay-test \
  -e POSTGRES_DB=ledgerpay -p 127.0.0.1::5432 postgres:16)
redis_container=""
cleanup() {
  if [ -n "$redis_container" ]; then docker stop "$redis_container" >/dev/null; fi
  docker stop "$postgres" >/dev/null
}
trap cleanup EXIT INT TERM
redis_container=$(docker run --rm -d -p 127.0.0.1::6379 redis:7.4)
pg_port=$(docker port "$postgres" 5432/tcp | sed 's/.*://')
redis_port=$(docker port "$redis_container" 6379/tcp | sed 's/.*://')
attempt=0
while [ "$attempt" -lt 30 ]; do
  if docker exec "$postgres" pg_isready -U postgres -d ledgerpay >/dev/null 2>&1 && \
     docker exec "$redis_container" redis-cli ping 2>/dev/null | grep -q PONG; then
    break
  fi
  attempt=$((attempt + 1))
  sleep 1
done
if [ "$attempt" -ge 30 ]; then
  echo 'LedgerPay fixture services did not become ready' >&2
  exit 1
fi
cd "$fixture_dir"
LEDGERPAY_DATABASE_URL="postgres://postgres:ledgerpay-test@127.0.0.1:$pg_port/ledgerpay" \
LEDGERPAY_REDIS_URL="redis://127.0.0.1:$redis_port/0" \
  .venv/bin/python -m unittest -v test_backend.py test_probe.py test_session.py
