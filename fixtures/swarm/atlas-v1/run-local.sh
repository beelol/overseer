#!/bin/sh
set -eu

fixture_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
container=$(docker run --rm -d \
  -e POSTGRES_PASSWORD=atlas-test -e POSTGRES_DB=atlas \
  -p 127.0.0.1::5432 postgres:16)
trap 'docker stop "$container" >/dev/null' EXIT INT TERM
port=$(docker port "$container" 5432/tcp | sed 's/.*://')
ready=0
attempt=0
while [ "$attempt" -lt 30 ]; do
  if docker exec "$container" pg_isready -U postgres -d atlas >/dev/null 2>&1; then
    ready=1
    break
  fi
  attempt=$((attempt + 1))
  sleep 1
done
if [ "$ready" -ne 1 ]; then
  echo 'Atlas fixture PostgreSQL did not become ready' >&2
  exit 1
fi
cd "$fixture_dir"
ATLAS_DATABASE_URL="postgres://postgres:atlas-test@127.0.0.1:$port/atlas" npm test
