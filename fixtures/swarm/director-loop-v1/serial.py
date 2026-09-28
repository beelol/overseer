"""A one-slot director audits and submits one job without launching a worker."""

import os
from pathlib import Path
import sys
import time

from protocol import call


def signal_ready(path, attempt_id):
    ready = Path(path)
    pending = ready.with_name(ready.name + ".pending")
    pending.write_text(attempt_id)
    os.replace(pending, ready)


run = os.environ["OVERSEER_SWARM_RUN_ID"]
generation = int(os.environ["OVERSEER_SWARM_GENERATION"])
owner_token = os.environ["OVERSEER_SWARM_DIRECTOR_TOKEN"]
source = Path(sys.argv[1])
auth = {"run_id": run, "generation": generation, "owner_token": owner_token}

plan = call("swarm.plan", {**auth, "id": run, "revision": 0, "jobs": [
    {"id": "inspect", "title": "Inspect a.txt", "acceptance": "Record the source content", "deps": []},
]})
assert plan["revision"] == 1

cost = {"elapsed_ms": 10, "usage_milli": {"points": 1}}
estimate = {"independent": True, "max_workers": 1,
    "allocation_milli": {"points": 100000},
    "finishing_reserve_milli": {"points": 20000},
    "serial": {"planning": cost, "context": cost, "integration": cost,
        "review": cost, "retries": cost,
        "workers": [{"id": "inspect", **cost}]},
    "parallel": {"planning": cost, "context": cost, "integration": cost,
        "review": cost, "retries": cost,
        "workers": [{"id": "inspect", **cost}]}}
benefit = call("swarm.benefit.commit", {**auth, "revision": 1, "estimate": estimate})
assert benefit["decision"] == "serial", benefit

now = int(time.time() * 1000)
snapshot = {"version": 1, "observed_ms": now - 1000, "expires_ms": now + 60000,
    "targets": [{"id": "fixture-local", "harness": "generic",
        "account_id": "fixture-account", "pool_ids": ["fixture-pool"],
        "capabilities": ["audit"], "health": "up", "auth": "ok"}],
    "pools": [{"id": "fixture-pool", "windows": [{"id": "run", "unit": "points",
        "remaining_milli": 1000000, "protected_milli": 0, "reserved_milli": 0,
        "confidence": "exact", "expires_ms": now + 60000}]}]}
admission = call("swarm.admit", {**auth, "revision": 1, "job_id": "inspect",
    "target_id": "fixture-local", "request_id": "serial-inspect", "now_ms": now,
    "snapshot": snapshot, "required_capabilities": ["audit"],
    "estimate_milli": {"points": 100}, "purpose": "director_self"})
assert admission["status"] == "admitted", admission
assert call("agents.limit.get", {})["active"] == 1
if len(sys.argv) > 3 and sys.argv[3] == "after_effect_begin":
    effect = call("swarm.effect.begin", {"run_id": run, "job_id": "inspect",
        "attempt_id": admission["attempt_id"], "token": admission["token"],
        "effect_id": "inspect-effect", "operation_id": "fixture:inspect:effect",
        "revision": 1})
    assert effect["outcome"] == "unknown", effect
    signal_ready(sys.argv[2], admission["attempt_id"])
    time.sleep(30)
if len(sys.argv) > 3 and sys.argv[3] == "after_scope_narrowed":
    revised = call("swarm.revise", {**auth, "id": run, "expected_revision": 1,
        "reason": "Owner removed the inspect job", "jobs": []})
    assert revised["revision"] == 2, revised
    signal_ready(sys.argv[2], admission["attempt_id"])
    time.sleep(30)
if len(sys.argv) > 2 and (len(sys.argv) < 4 or sys.argv[3] == "after_admit"):
    signal_ready(sys.argv[2], admission["attempt_id"])
    time.sleep(30)

try:
    call("swarm.attempt.confirm_exit", {**auth, "revision": 1,
        "job_id": "inspect", "attempt_id": admission["attempt_id"]})
except RuntimeError as error:
    assert "director job has not been accepted or rejected" in str(error), error
else:
    raise AssertionError("unfinished director work released its attempt")

try:
    call("swarm.worker.launch", {"run_id": run, "job_id": "inspect",
        "attempt_id": admission["attempt_id"], "token": admission["token"],
        "repo": str(source), "harness": "generic", "program": "/bin/sleep",
        "args": ["1"], "prompt": "Do the director's job", "title": "Unwanted worker"})
except RuntimeError as error:
    assert "director-executed" in str(error), error
else:
    raise AssertionError("director-executed job spawned a worker")

content = (source / "a.txt").read_text()
assert content == "a\n"
call("swarm.artifact.put", {"run_id": run, "job_id": "inspect",
    "attempt_id": admission["attempt_id"], "token": admission["token"],
    "artifact_id": "serial-proof", "source_revision": 1,
    "kind": "finding", "content": "a.txt contains a"})
call("swarm.report", {"run_id": run, "job_id": "inspect",
    "attempt_id": admission["attempt_id"], "token": admission["token"],
    "message_id": "serial-result", "type": "result", "revision": 1,
    "payload": {"artifact_ids": ["serial-proof"], "audit_outcome": "negative"}})
if len(sys.argv) > 3 and sys.argv[3] == "after_report":
    signal_ready(sys.argv[2], admission["attempt_id"])
    time.sleep(30)
decision = call("swarm.decide", {**auth, "revision": 1, "job_id": "inspect",
    "decision": "accept", "evidence": ["serial-proof"]})
assert decision["status"] == "accepted", decision
call("swarm.attempt.confirm_exit", {**auth, "revision": 1,
    "job_id": "inspect", "attempt_id": admission["attempt_id"]})
deadline = time.monotonic() + 8
while True:
    batch = call("swarm.director.claim_batch", {**auth, "revision": 1,
        "now_ms": int(time.time() * 1000)})
    if batch["status"] == "claimed" or time.monotonic() >= deadline:
        break
    time.sleep(0.1)
assert batch["status"] == "claimed", batch
assert any(message["message_id"] == "serial-result" for message in batch["messages"])
call("swarm.director.complete_batch", {**auth, "turn_id": batch["turn_id"],
    "token": batch["token"], "outcome": "progress"})
completed = call("swarm.complete", {**auth, "revision": 1,
    "request_id": "serial-complete", "summary": "Inspected a.txt in the director",
    "verification": "a.txt was read from the fixture source",
    "checks": [{"job_id": "inspect", "outcome": "passed", "evidence": ["serial-proof"]}]})
assert completed["status"] == "completed", completed
