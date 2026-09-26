"""Persistent local S2 effect session for daemon-restart and lost-ack replay."""

import json
import os
import re
import sys
from uuid import uuid4

import psycopg
from fastapi.testclient import TestClient

from backend import LedgerPay, create_app, sign_payload


def run(action, namespace=None):
    database_url = os.environ["LEDGERPAY_DATABASE_URL"]
    redis_url = os.environ["LEDGERPAY_REDIS_URL"]
    if action == "init":
        if namespace is not None:
            raise ValueError("init creates its own namespace")
        namespace = f"ledgerpay_effect_{uuid4().hex}"
        with psycopg.connect(database_url, autocommit=True) as root:
            root.execute(f"CREATE SCHEMA {namespace}")
        backend = LedgerPay(database_url, redis_url, namespace)
        try:
            backend.seed()
            raw, signature = sign_payload({"event_id": "evt-42",
                                           "subscription_id": "sub-1",
                                           "type": "activation", "version": 1})
            response = TestClient(create_app(backend)).post(
                "/webhooks/billing", content=raw,
                headers={"X-LedgerPay-Signature": signature})
            if response.status_code != 202:
                raise RuntimeError(f"signed fixture ingress failed: {response.status_code}")
            backend.duplicate_delivery("evt-42")
            return {"namespace": namespace,
                    "queued": backend.queue.lrange(backend.queue_key, 0, -1)}
        except Exception:
            backend.queue.delete(backend.queue_key)
            with psycopg.connect(database_url, autocommit=True) as root:
                root.execute(f"DROP SCHEMA {namespace} CASCADE")
            raise
    if not namespace or not re.fullmatch(r"ledgerpay_effect_[0-9a-f]{32}", namespace):
        raise ValueError("invalid effect namespace")
    backend = LedgerPay(database_url, redis_url, namespace)
    if action == "deliver":
        return backend.consume_one("seeded", lose_ack_after_update=True)
    if action == "outcome":
        observed = backend.probe_outcome("evt-42")
        observed["queued"] = backend.queue.lrange(backend.queue_key, 0, -1)
        return observed
    if action == "cleanup":
        try:
            backend.queue.delete(backend.queue_key)
        finally:
            with psycopg.connect(database_url, autocommit=True) as root:
                root.execute(f"DROP SCHEMA {namespace} CASCADE")
        return {"cleaned": True}
    raise ValueError(f"unknown session action {action}")


if __name__ == "__main__":
    action = sys.argv[1]
    namespace = sys.argv[2] if len(sys.argv) > 2 else None
    print(json.dumps(run(action, namespace), sort_keys=True))
