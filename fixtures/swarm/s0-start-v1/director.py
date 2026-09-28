"""Scripted S0 director, launched by the daemon's normal Swarm start.

It plans three audit jobs, commits a synthetic benefit estimate, and admits
each job through Swarm admission. It first offers `members` to the approved
Claude account target; an audit run refuses a native worker before any
booking, so the job goes to the fixture target. It launches three supervised
workers, records `dispatched`, and waits for the test's gate (a local file)
before coordinating: it routes D1 to `members`, accepts each result on its
evidence, and completes the run once every worker has exited. It writes each
step to a trace file. Its choices are scripted, not model reasoning.
"""

import json
import os
from pathlib import Path
import sys
import time

from protocol import call, wait_for


RUN = os.environ["OVERSEER_SWARM_RUN_ID"]
GENERATION = int(os.environ["OVERSEER_SWARM_GENERATION"])
OWNER = os.environ["OVERSEER_SWARM_DIRECTOR_TOKEN"]
TRACE, GATE, REPO = sys.argv[1], sys.argv[2], sys.argv[3]
FIXTURE = Path(__file__).resolve().parent
AUTH = {"run_id": RUN, "generation": GENERATION, "owner_token": OWNER}
JOBS = ("projects", "tasks", "members")


def trace(step, **fields):
    with open(TRACE, "a") as out:
        out.write(json.dumps({"step": step, **fields}) + "\n")


planned = call("swarm.plan", {**AUTH, "id": RUN, "revision": 0, "jobs": [
    {"id": "projects", "title": "Project route audit", "acceptance": "checked path evidence",
     "deps": [], "required_capabilities": ["audit"]},
    {"id": "tasks", "title": "Task mutation audit", "acceptance": "local reproduction",
     "deps": [], "required_capabilities": ["audit"]},
    {"id": "members", "title": "Membership role audit", "acceptance": "role matrix evidence",
     "deps": [], "required_capabilities": ["audit"]},
]})
trace("planned", revision=planned["revision"])

cost = {"elapsed_ms": 10, "usage_milli": {"points": 1}}
workers = [{"id": job, "elapsed_ms": 100, "usage_milli": {"points": 10}} for job in JOBS]
serial = {name: cost for name in ("planning", "context", "integration", "review", "retries")}
serial["workers"] = workers
parallel = {**serial, "context": {"elapsed_ms": 20, "usage_milli": {"points": 1}}}
benefit = call("swarm.benefit.commit", {**AUTH, "revision": 1, "estimate": {
    "independent": True, "max_workers": 3, "allocation_milli": {"points": 100000},
    "finishing_reserve_milli": {"points": 20000}, "serial": serial, "parallel": parallel}})
trace("benefit", decision=benefit["decision"])

now = int(time.time() * 1000)
window = lambda: {"id": "run", "unit": "points", "remaining_milli": 1000000,
    "protected_milli": 0, "reserved_milli": 0, "confidence": "exact", "expires_ms": now + 120000}
snapshot = {"version": 1, "observed_ms": now - 1000, "expires_ms": now + 120000,
    "targets": [
        {"id": "fixture-local", "harness": "generic", "account_id": "fixture-account",
         "pool_ids": ["fixture-pool"], "capabilities": ["audit"], "health": "up", "auth": "ok"},
        {"id": "system-claude", "harness": "claude", "profile_id": "system-claude",
         "model": "sonnet", "effort": "medium", "account_id": "claude-account",
         "pool_ids": ["claude-pool"], "capabilities": ["audit"], "health": "up", "auth": "ok"}],
    "pools": [{"id": "fixture-pool", "windows": [window()]},
              {"id": "claude-pool", "windows": [window()]}]}


def admit(job, target):
    return call("swarm.admit", {**AUTH, "revision": 1, "job_id": job, "target_id": target,
        "request_id": f"s0-{job}-{target}", "now_ms": now, "snapshot": snapshot,
        "required_capabilities": ["audit"], "estimate_milli": {"points": 100},
        "purpose": "worker"})


attempts = {}
for job in JOBS:
    if job == "members":
        offered = admit(job, "system-claude")
        trace("offered", job=job, target="system-claude", status=offered["status"],
              reason=offered.get("reason"))
        if offered["status"] == "admitted":
            raise RuntimeError("an audit run admitted a native worker")
    admission = admit(job, "fixture-local")
    if admission["status"] != "admitted":
        raise RuntimeError(f"admission of {job}: {admission}")
    attempts[job] = admission
    launched = call("swarm.worker.launch", {"run_id": RUN, "job_id": job,
        "attempt_id": admission["attempt_id"], "token": admission["token"], "repo": REPO,
        "program": sys.executable, "args": [str(FIXTURE / "worker.py"), GATE],
        "prompt": f"Audit {job} and report evidence", "title": f"S0 {job}"})
    if launched["status"] != "launched":
        raise RuntimeError(f"launch of {job}: {launched}")
    trace("launched", job=job, attempt=admission["attempt_id"], worker=launched["overseer_run_id"])
trace("dispatched", active=call("agents.limit.get", {})["active"])

wait_for(GATE, 120)
routed = False
accepted = {}
deadline = time.monotonic() + 60
while time.monotonic() < deadline:
    state = call("swarm.get", {"id": RUN})
    batch = call("swarm.director.claim_batch", {**AUTH, "revision": state["revision"],
        "now_ms": int(time.time() * 1000)})
    if batch["status"] == "claimed":
        for message in batch["messages"]:
            if message["message_id"] == "D1" and not routed:
                call("swarm.direct", {**AUTH, "revision": state["revision"], "job_id": "members",
                    "attempt_id": attempts["members"]["attempt_id"], "message_id": "D1-to-members",
                    "type": "advisory", "payload": {"discovery_id": "D1",
                        "focus": "Check whether role changes rely on the task lookup"}})
                routed = True
                trace("routed", message="D1", to="members")
            if message["type"] == "result" and message["job_id"] in JOBS:
                evidence = message["payload"]["artifact_ids"]
                decision = call("swarm.decide", {**AUTH, "revision": state["revision"],
                    "job_id": message["job_id"], "decision": "accept", "evidence": evidence})
                if decision["status"] != "accepted":
                    raise RuntimeError(f"decision on {message['job_id']}: {decision}")
                accepted[message["job_id"]] = evidence
                trace("accepted", job=message["job_id"], evidence=evidence)
        call("swarm.director.complete_batch", {**AUTH, "turn_id": batch["turn_id"],
            "token": batch["token"], "outcome": "progress"})
    if len(accepted) == len(JOBS) and state["registered_attempts"] == 0 and batch["status"] == "idle":
        completed = call("swarm.complete", {**AUTH, "revision": state["revision"],
            "request_id": "s0-complete",
            "summary": "Tenant isolation audit: one confirmed task-mutation defect; project and membership paths checked",
            "verification": "Three supervised fixture workers submitted evidence and exited",
            "checks": [{"job_id": job, "outcome": "passed", "evidence": accepted[job]} for job in JOBS]})
        trace("completed", status=completed["status"])
        break
    time.sleep(0.05)
else:
    raise RuntimeError(f"S0 director timed out: accepted={sorted(accepted)}")
