"""Scripted category director: plan, launch, mediate D1, review, synthesize."""

import os
from pathlib import Path
import sys
import time

from protocol import call


RUN = os.environ["OVERSEER_SWARM_RUN_ID"]
GENERATION = int(os.environ["OVERSEER_SWARM_GENERATION"])
OWNER = os.environ["OVERSEER_SWARM_DIRECTOR_TOKEN"]
REPO = sys.argv[1]
MARKER = sys.argv[2]
FIXTURE = Path(__file__).resolve().parent
AUTH = {"run_id": RUN, "generation": GENERATION, "owner_token": OWNER}

planned = call("swarm.plan", {
    "id": RUN, "generation": GENERATION, "revision": 0, "owner_token": OWNER,
    "jobs": [
        {"id": "j2", "title": "Task lookup audit", "acceptance": "task lookup evidence", "deps": []},
        {"id": "j4", "title": "Attachment route audit", "acceptance": "scoped attachment evidence", "deps": []},
    ],
})
assert planned["revision"] == 1

workers = [{"id": job, "elapsed_ms": 100, "usage_milli": {"points": 10}}
           for job in ("j2", "j4")]
serial = {name: {"elapsed_ms": 10, "usage_milli": {"points": 1}}
          for name in ("planning", "context", "integration", "review", "retries")}
serial["workers"] = workers
parallel = {**serial, "context": {"elapsed_ms": 20, "usage_milli": {"points": 1}}}
benefit = call("swarm.benefit.commit", {**AUTH, "revision": 1,
    "estimate": {"independent": True, "max_workers": 2,
        "allocation_milli": {"points": 100000},
        "finishing_reserve_milli": {"points": 20000},
        "serial": serial, "parallel": parallel}})
assert benefit["decision"] == "parallel"

now = int(time.time() * 1000)
snapshot = {"version": 1, "observed_ms": now - 1000, "expires_ms": now + 60000,
    "targets": [{"id": "fixture-local", "account_id": "fixture-account",
        "pool_ids": ["fixture-pool"], "capabilities": ["audit"],
        "health": "up", "auth": "ok"}],
    "pools": [{"id": "fixture-pool", "windows": [{"id": "run", "unit": "points",
        "remaining_milli": 1000000, "protected_milli": 0, "reserved_milli": 0,
        "confidence": "exact", "expires_ms": now + 60000}]}]}
attempts = {}
for job in ("j2", "j4"):
    admission = call("swarm.admit", {**AUTH, "revision": 1, "job_id": job,
        "target_id": "fixture-local", "request_id": f"director-loop-{job}",
        "now_ms": now, "snapshot": snapshot, "required_capabilities": ["audit"],
        "estimate_milli": {"points": 100}, "purpose": "worker"})
    assert admission["status"] == "admitted", admission
    attempts[job] = admission
    launched = call("swarm.worker.launch", {"run_id": RUN, "job_id": job,
        "attempt_id": admission["attempt_id"], "token": admission["token"],
        "repo": REPO, "program": "/usr/bin/python3",
        "args": [str(FIXTURE / f"{job}.py"), MARKER],
        "prompt": f"Audit {job} and report evidence", "title": f"Fixture {job}"})
    assert launched["status"] == "launched", launched

routed = False
accepted = set()
deadline = time.monotonic() + 25
while time.monotonic() < deadline:
    state = call("swarm.get", {"id": RUN})
    batch = call("swarm.director.claim_batch", {**AUTH,
        "revision": state["revision"], "now_ms": int(time.time() * 1000)})
    if batch["status"] == "claimed":
        for message in batch["messages"]:
            if message["message_id"] == "D1" and not routed:
                call("swarm.direct", {**AUTH, "revision": state["revision"],
                    "job_id": "j4", "attempt_id": attempts["j4"]["attempt_id"],
                    "message_id": "D1-to-J4", "type": "advisory",
                    "payload": {"discovery_id": "D1", "focus": "Check the attachment boundary"}})
                routed = True
            if message["type"] == "result" and message["job_id"] in ("j2", "j4"):
                evidence = message["payload"]["artifact_ids"]
                decision = call("swarm.decide", {**AUTH, "revision": state["revision"],
                    "job_id": message["job_id"], "decision": "accept", "evidence": evidence})
                assert decision["status"] == "accepted", decision
                accepted.add(message["job_id"])
        call("swarm.director.complete_batch", {**AUTH,
            "turn_id": batch["turn_id"], "token": batch["token"], "outcome": "progress"})
    if accepted == {"j2", "j4"} and state["registered_attempts"] == 0 and batch["status"] == "idle":
        completed = call("swarm.complete", {**AUTH, "revision": state["revision"],
            "request_id": "director-loop-complete",
            "summary": "Task and attachment routes audited with the shared lookup discovery coordinated",
            "verification": "Both supervised fixture workers submitted evidence and exited",
            "checks": [{"job_id": job, "outcome": "passed", "evidence": [f"proof-{job}"]}
                       for job in ("j2", "j4")]})
        assert completed["status"] == "completed", completed
        break
    time.sleep(0.05)
else:
    raise RuntimeError(f"director loop timed out: {state}")
