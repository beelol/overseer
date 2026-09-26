"""Run one S2 job against an isolated real PostgreSQL schema and Redis list."""

import json
import os
import sys
import threading
from uuid import uuid4

import psycopg
from fastapi.testclient import TestClient

from backend import LedgerPay, create_app, sign_payload


def probe(job):
    database_url = os.environ["LEDGERPAY_DATABASE_URL"]
    redis_url = os.environ["LEDGERPAY_REDIS_URL"]
    namespace = f"ledgerpay_{job.replace('-', '_')}_{uuid4().hex}"
    with psycopg.connect(database_url, autocommit=True) as root:
        root.execute(f"CREATE SCHEMA {namespace}")
    backend = LedgerPay(database_url, redis_url, namespace)
    try:
        backend.seed()
        client = TestClient(create_app(backend))

        def send(event_id="evt-42", event_type="activation", version=1):
            raw, signature = sign_payload({"event_id": event_id,
                                           "subscription_id": "sub-1",
                                           "type": event_type, "version": version})
            return client.post("/webhooks/billing", content=raw,
                               headers={"X-LedgerPay-Signature": signature})

        if job == "k1":
            denied = client.post("/webhooks/billing", content=b"{}",
                                 headers={"X-LedgerPay-Signature": "invalid"})
            accepted = send()
            evidence = {"invalidSignatureStatus": denied.status_code,
                        "validSignatureStatus": accepted.status_code,
                        "eventCount": backend.event_count(),
                        "queuedEventIds": backend.queue.lrange(backend.queue_key, 0, -1)}
        elif job == "k2":
            send()
            backend.duplicate_delivery("evt-42")
            queued = backend.queue.lrange(backend.queue_key, 0, -1)
            backend.consume_one("seeded", lose_ack_after_update=True)
            evidence = {"queuedEventIds": queued,
                        "outcomeProbe": backend.probe_outcome("evt-42"),
                        "sourcePath": "backend.py::consume_one"}
        elif job == "k3":
            send()
            backend.duplicate_delivery("evt-42")
            outcomes = backend.consume_pair("seeded", threading.Barrier(2))
            evidence = {"eventId": "evt-42", "deliveries": outcomes,
                        "finalState": backend.state(),
                        "receiptCount": backend.receipt_count(),
                        "transactionBoundary": "update committed before unique receipt insert",
                        "sourcePath": "backend.py::consume_one"}
        elif job == "k4":
            send("evt-cancel", "cancellation", 2)
            cancellation = backend.consume_one("seeded")
            send("evt-old", "activation", 1)
            stale = backend.consume_one("seeded")
            evidence = {"cancellation": cancellation, "staleActivation": stale,
                        "finalState": backend.state()}
        elif job == "k3-protected":
            send()
            backend.duplicate_delivery("evt-42")
            outcomes = backend.consume_pair("protected", threading.Barrier(2))
            evidence = {"eventId": "evt-42", "deliveries": outcomes,
                        "finalState": backend.state(),
                        "receiptCount": backend.receipt_count(),
                        "transactionBoundary": "unique receipt and update in one transaction"}
        elif job == "k2-missing-redis":
            missing = LedgerPay(database_url, "redis://127.0.0.1:1/0", namespace)
            failed = TestClient(create_app(missing))
            raw, signature = sign_payload({"event_id": "evt-42",
                                           "subscription_id": "sub-1",
                                           "type": "activation", "version": 1})
            response = failed.post("/webhooks/billing", content=raw,
                                   headers={"X-LedgerPay-Signature": signature})
            evidence = {"webhookStatus": response.status_code,
                        "unavailableResource": response.json()["unavailable_resource"],
                        "finalState": backend.state()}
        else:
            raise ValueError(f"unknown S2 job {job}")
        return {"fixtureVersion": 1, "job": job, "namespace": namespace, "evidence": evidence}
    finally:
        try:
            backend.queue.delete(backend.queue_key)
        finally:
            with psycopg.connect(database_url, autocommit=True) as root:
                root.execute(f"DROP SCHEMA {namespace} CASCADE")


if __name__ == "__main__":
    print(json.dumps(probe(sys.argv[1]), sort_keys=True))
