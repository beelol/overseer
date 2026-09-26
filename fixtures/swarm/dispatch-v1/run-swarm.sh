#!/bin/sh
set -eu
fixture_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH= cd -- "$fixture_dir/../../.." && pwd)
container=$(docker run --rm -d -e POSTGRES_PASSWORD=dispatch-test -e POSTGRES_DB=dispatch -p 127.0.0.1::5432 postgres:16)
trap 'docker stop "$container" >/dev/null' EXIT INT TERM
port=$(docker port "$container" 5432/tcp | sed 's/.*://')
attempt=0
until docker exec "$container" pg_isready -U postgres -d dispatch >/dev/null 2>&1; do
  attempt=$((attempt + 1))
  if [ "$attempt" -ge 30 ]; then echo 'Dispatch PostgreSQL did not become ready' >&2; exit 1; fi
  sleep 1
done
cd "$repo_dir"
export DISPATCH_DATABASE_URL="postgres://postgres:dispatch-test@127.0.0.1:$port/dispatch"
cargo test --offline -p overseerd --test swarm_dispatch_incident -- --ignored --nocapture
