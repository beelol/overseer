"""Disposable LedgerPay audit backend. All accounts and events are local fixtures."""

import hashlib
import hmac
import json
import re
import threading

import psycopg
import redis
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import JSONResponse


SIGNING_SECRET = b"ledgerpay-fixture-only-secret"


def sign_payload(payload):
    raw = json.dumps(payload, separators=(",", ":"), sort_keys=True).encode()
    return raw, hmac.new(SIGNING_SECRET, raw, hashlib.sha256).hexdigest()


class LedgerPay:
    def __init__(self, database_url, redis_url, namespace):
        if not re.fullmatch(r"[a-z][a-z0-9_]*", namespace):
            raise ValueError("invalid PostgreSQL namespace")
        self.database_url = database_url
        self.namespace = namespace
        self.queue = redis.Redis.from_url(redis_url, decode_responses=True,
                                          socket_connect_timeout=1, socket_timeout=1)
        self.queue_key = f"ledgerpay:{namespace}:deliveries"

    def connect(self):
        return psycopg.connect(self.database_url, options=f"-c search_path={self.namespace}")

    def seed(self):
        with self.connect() as db:
            db.execute("""CREATE TABLE provider_events (
                event_id text PRIMARY KEY, subscription_id text NOT NULL,
                event_type text NOT NULL, event_version integer NOT NULL,
                payload jsonb NOT NULL, queued boolean NOT NULL DEFAULT false)""")
            db.execute("""CREATE TABLE subscriptions (
                id text PRIMARY KEY, grant_count integer NOT NULL,
                active boolean NOT NULL, version integer NOT NULL)""")
            db.execute("""CREATE TABLE processed_events (
                event_id text PRIMARY KEY)""")
            db.execute("INSERT INTO subscriptions VALUES ('sub-1',0,false,0)")

    def event_count(self):
        with self.connect() as db:
            return db.execute("SELECT count(*) FROM provider_events").fetchone()[0]

    def receipt_count(self):
        with self.connect() as db:
            return db.execute("SELECT count(*) FROM processed_events").fetchone()[0]

    def state(self):
        with self.connect() as db:
            row = db.execute("SELECT grant_count,active,version FROM subscriptions WHERE id='sub-1'").fetchone()
            return {"grant_count": row[0], "active": row[1], "version": row[2]}

    def duplicate_delivery(self, event_id):
        self.queue.rpush(self.queue_key, event_id)

    def consume_one(self, mode, barrier=None, lose_ack_after_update=False):
        event_id = self.queue.lpop(self.queue_key)
        if event_id is None:
            raise ValueError("queue empty")
        with self.connect() as db:
            row = db.execute("SELECT subscription_id,event_type,event_version FROM provider_events WHERE event_id=%s",
                             (event_id,)).fetchone()
        if row is None:
            raise ValueError("unknown queued event")
        subscription_id, event_type, version = row
        if mode == "protected":
            if barrier is not None:
                barrier.wait(timeout=10)
            with self.connect() as db:
                inserted = db.execute("INSERT INTO processed_events VALUES (%s) ON CONFLICT DO NOTHING RETURNING event_id",
                                      (event_id,)).fetchone()
                if inserted is None:
                    return {"event_id": event_id, "applied": False, "reason": "duplicate"}
                current = db.execute("SELECT version FROM subscriptions WHERE id=%s FOR UPDATE",
                                     (subscription_id,)).fetchone()[0]
                if version <= current:
                    return {"event_id": event_id, "applied": False, "reason": "stale_version"}
                self._update(db, subscription_id, event_type, version)
            return {"event_id": event_id, "applied": True}
        if mode != "seeded":
            raise ValueError("unknown fixture variant")
        with self.connect() as db:
            receipt = db.execute("SELECT 1 FROM processed_events WHERE event_id=%s", (event_id,)).fetchone()
            current = db.execute("SELECT version FROM subscriptions WHERE id=%s",
                                 (subscription_id,)).fetchone()[0]
        if receipt is not None:
            return {"event_id": event_id, "applied": False, "reason": "duplicate"}
        # Version guards exclude older events; equal-version redelivery still relies
        # on the receipt, which is the seeded gap under audit.
        if version < current:
            return {"event_id": event_id, "applied": False, "reason": "stale_version"}
        if barrier is not None:
            barrier.wait(timeout=10)
        # Seeded defect: update commits before the unique receipt is inserted.
        # Two deliveries which both passed the prior checks may each increment.
        with self.connect() as db:
            self._update(db, subscription_id, event_type, version)
        if lose_ack_after_update:
            return {"event_id": event_id, "status": "ack_lost", "applied": "unknown"}
        with self.connect() as db:
            db.execute("INSERT INTO processed_events VALUES (%s) ON CONFLICT DO NOTHING", (event_id,))
        return {"event_id": event_id, "applied": True}

    def _update(self, db, subscription_id, event_type, version):
        if event_type == "activation":
            db.execute("UPDATE subscriptions SET grant_count=grant_count+1, active=true, version=%s WHERE id=%s",
                       (version, subscription_id))
        elif event_type == "cancellation":
            db.execute("UPDATE subscriptions SET active=false, version=%s WHERE id=%s",
                       (version, subscription_id))
        else:
            raise ValueError("unknown event type")

    def consume_pair(self, mode, barrier):
        results = [None, None]
        failures = []

        def consume(index):
            try:
                results[index] = self.consume_one(mode, barrier)
            except Exception as error:
                failures.append(error)

        threads = [threading.Thread(target=consume, args=(index,)) for index in range(2)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join(timeout=15)
        if any(thread.is_alive() for thread in threads):
            raise TimeoutError("delivery barrier did not release")
        if failures:
            raise failures[0]
        return results

    def probe_outcome(self, event_id):
        with self.connect() as db:
            receipt = db.execute("SELECT 1 FROM processed_events WHERE event_id=%s", (event_id,)).fetchone()
        count = self.state()["grant_count"]
        return {"event_id": event_id, "grant_count": count, "receipt": receipt is not None,
                "status": "applied" if receipt else "unknown_do_not_retry"}


def create_app(backend):
    app = FastAPI()

    @app.post("/webhooks/billing")
    async def billing(request: Request):
        raw = await request.body()
        supplied = request.headers.get("X-LedgerPay-Signature", "")
        expected = hmac.new(SIGNING_SECRET, raw, hashlib.sha256).hexdigest()
        if not hmac.compare_digest(supplied, expected):
            raise HTTPException(401, "invalid signature")
        try:
            event = json.loads(raw)
            event_id = event["event_id"]
            subscription_id = event["subscription_id"]
            event_type = event["type"]
            version = event["version"]
            if (not isinstance(event_id, str) or not isinstance(subscription_id, str)
                    or event_type not in ("activation", "cancellation")
                    or type(version) is not int or version < 1):
                raise ValueError("invalid event")
        except (ValueError, KeyError, TypeError):
            raise HTTPException(422, "invalid event")
        with backend.connect() as db:
            db.execute("""INSERT INTO provider_events
                (event_id,subscription_id,event_type,event_version,payload)
                VALUES (%s,%s,%s,%s,%s) ON CONFLICT DO NOTHING""",
                (event_id, subscription_id, event_type, version, json.dumps(event)))
            stored_payload, queued = db.execute(
                "SELECT payload,queued FROM provider_events WHERE event_id=%s FOR UPDATE",
                (event_id,)).fetchone()
            if stored_payload != event:
                raise HTTPException(409, "event ID reused with different payload")
            if queued:
                return JSONResponse({"event_id": event_id, "duplicate": True}, status_code=200)
            try:
                backend.queue.rpush(backend.queue_key, event_id)
            except redis.RedisError:
                return JSONResponse({"event_id": event_id, "unavailable_resource": "redis_queue"},
                                    status_code=503)
            db.execute("UPDATE provider_events SET queued=true WHERE event_id=%s", (event_id,))
        return JSONResponse({"event_id": event_id, "queued": True}, status_code=202)

    return app
