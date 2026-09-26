# LedgerPay S2 local audit fixture

This versioned FastAPI/PostgreSQL/Redis backend uses signed, local billing events and a
mock provider. No real account, purchase or subscription is touched. The seeded worker
commits a grant update before inserting its event receipt. The protected worker inserts
the unique receipt and updates the grant within one transaction.

Run `uv venv .venv && uv pip install --python .venv/bin/python -r requirements.txt`, then
`./run-local.sh`. The runner creates disposable PostgreSQL and Redis containers bound to
random localhost ports and removes them at exit. Tests use a separate database schema
and Redis list per case. `LEDGERPAY_DATABASE_URL` and `LEDGERPAY_REDIS_URL` can instead
point at disposable local services for direct `python -m unittest` runs.

`./run-swarm.sh` runs the opt-in daemon replay against the same disposable services.
It scripts K1–K3 as the first three workers, holds K4 until K1 exits, forwards K2's
duplicate-delivery discovery to K3 through the durable broker, and requires evidence
from all four jobs before scripted completion. This does not qualify a live harness.

The missing-Redis case is classified as an environment failure. The lost-ack case probes
the durable database state and reports `unknown_do_not_retry` when an update committed
but the receipt did not. Neither case counts as a passing retry-safety check.
