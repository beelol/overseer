import os
import threading
import unittest
from uuid import uuid4

import psycopg
import redis
from fastapi.testclient import TestClient

from backend import LedgerPay, create_app, sign_payload


class LedgerPayBackendTests(unittest.TestCase):
    def setUp(self):
        self.database_url = os.environ["LEDGERPAY_DATABASE_URL"]
        self.redis_url = os.environ["LEDGERPAY_REDIS_URL"]
        self.namespace = f"ledgerpay_{uuid4().hex}"
        with psycopg.connect(self.database_url, autocommit=True) as db:
            db.execute(f"CREATE SCHEMA {self.namespace}")
        self.backend = LedgerPay(self.database_url, self.redis_url, self.namespace)
        self.backend.seed()
        self.client = TestClient(create_app(self.backend))

    def tearDown(self):
        self.backend.queue.delete(self.backend.queue_key)
        with psycopg.connect(self.database_url, autocommit=True) as db:
            db.execute(f"DROP SCHEMA {self.namespace} CASCADE")

    def send(self, event_id="evt-42", event_type="activation", version=1):
        payload = {"event_id": event_id, "subscription_id": "sub-1",
                   "type": event_type, "version": version}
        raw, signature = sign_payload(payload)
        return self.client.post("/webhooks/billing", content=raw,
                                headers={"X-LedgerPay-Signature": signature})

    def test_signature_and_single_signed_ingress(self):
        denied = self.client.post("/webhooks/billing", content=b"{}",
                                  headers={"X-LedgerPay-Signature": "bad"})
        self.assertEqual(denied.status_code, 401)
        self.assertEqual(self.backend.event_count(), 0)
        self.assertEqual(self.send().status_code, 202)
        self.assertEqual(self.send().status_code, 200)
        self.assertEqual(self.backend.event_count(), 1)
        self.assertEqual(self.backend.queue.llen(self.backend.queue_key), 1)

    def test_two_deliveries_cross_barrier_and_duplicate_seeded_grant(self):
        self.assertEqual(self.send().status_code, 202)
        self.backend.duplicate_delivery("evt-42")
        barrier = threading.Barrier(2)
        outcomes = self.backend.consume_pair("seeded", barrier)
        self.assertEqual([item["event_id"] for item in outcomes], ["evt-42", "evt-42"])
        self.assertEqual(self.backend.state(), {"grant_count": 2, "active": True, "version": 1})
        self.assertEqual(self.backend.receipt_count(), 1)

    def test_protected_transaction_applies_once(self):
        self.assertEqual(self.send().status_code, 202)
        self.backend.duplicate_delivery("evt-42")
        outcomes = self.backend.consume_pair("protected", threading.Barrier(2))
        self.assertEqual(sorted(item["applied"] for item in outcomes), [False, True])
        self.assertEqual(self.backend.state(), {"grant_count": 1, "active": True, "version": 1})
        self.assertEqual(self.backend.receipt_count(), 1)

    def test_newer_cancellation_rejects_old_activation(self):
        self.assertEqual(self.send("evt-cancel", "cancellation", 2).status_code, 202)
        self.assertEqual(self.backend.consume_one("seeded")["applied"], True)
        self.assertEqual(self.send("evt-old", "activation", 1).status_code, 202)
        self.assertEqual(self.backend.consume_one("seeded")["reason"], "stale_version")
        self.assertEqual(self.backend.state(), {"grant_count": 0, "active": False, "version": 2})

    def test_lost_ack_after_update_has_uncertain_outcome(self):
        self.assertEqual(self.send().status_code, 202)
        outcome = self.backend.consume_one("seeded", lose_ack_after_update=True)
        self.assertEqual(outcome["status"], "ack_lost")
        self.assertEqual(self.backend.probe_outcome("evt-42"),
                         {"event_id": "evt-42", "grant_count": 1, "receipt": False,
                          "status": "unknown_do_not_retry"})
        self.assertEqual(self.backend.queue.llen(self.backend.queue_key), 0)

    def test_missing_redis_is_environment_failure(self):
        missing = LedgerPay(self.database_url, "redis://127.0.0.1:1/0", self.namespace)
        client = TestClient(create_app(missing))
        raw, signature = sign_payload({"event_id": "evt-no-queue", "subscription_id": "sub-1",
                                       "type": "activation", "version": 1})
        response = client.post("/webhooks/billing", content=raw,
                               headers={"X-LedgerPay-Signature": signature})
        self.assertEqual(response.status_code, 503)
        self.assertEqual(response.json()["unavailable_resource"], "redis_queue")
        self.assertEqual(self.backend.state()["grant_count"], 0)
        retry = self.client.post("/webhooks/billing", content=raw,
                                 headers={"X-LedgerPay-Signature": signature})
        self.assertEqual(retry.status_code, 202)
        self.assertEqual(self.backend.queue.lrange(self.backend.queue_key, 0, -1),
                         ["evt-no-queue"])


if __name__ == "__main__":
    unittest.main()
