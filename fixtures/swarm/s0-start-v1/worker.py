"""A scripted S0 audit worker: waits for the test's gate, then reports.

`tasks` reports discovery D1 before its result (a reproduction of a confirmed
defect); `members` applies the director's D1 advisory before its result;
`projects` reports a checked path. Evidence is fixture text, not a real audit.
"""

import os
import sys
import time

from protocol import call, wait_for


run = os.environ["OVERSEER_SWARM_RUN_ID"]
job = os.environ["OVERSEER_SWARM_JOB_ID"]
attempt = os.environ["OVERSEER_SWARM_ATTEMPT_ID"]
token = os.environ["OVERSEER_SWARM_TOKEN"]
revision = int(os.environ["OVERSEER_SWARM_REVISION"])
wait_for(sys.argv[1], 60)
ids = {"run_id": run, "job_id": job, "attempt_id": attempt, "token": token}

if job == "tasks":
    call("swarm.report", {**ids, "revision": revision, "message_id": "D1", "type": "discovery",
        "payload": {"symbol": "TaskRepository.findById", "source_revision": "fixture-s0",
            "note": "lookup uses only the task id; attachments may rely on it"}})
if job == "members":
    deadline = time.monotonic() + 30
    while True:
        inbox = call("swarm.messages", {"run_id": run, "recipient": attempt, "token": token})
        advisory = next((m for m in inbox["messages"] if m["message_id"] == "D1-to-members"), None)
        if advisory:
            assert advisory["payload"]["discovery_id"] == "D1", advisory
            for phase in ("delivered", "applied"):
                call("swarm.ack", {"run_id": run, "recipient": attempt, "token": token,
                    "message_id": "D1-to-members", "phase": phase, "revision": revision})
            break
        if time.monotonic() >= deadline:
            raise RuntimeError("members never received D1")
        time.sleep(0.05)

kind, outcome, content = {
    "projects": ("finding", "negative", "GET /projects/:id calls requireProjectMember; foreign request denied"),
    "tasks": ("reproduction", "confirmed_defect", "alice-test PATCH /tasks/task-b-7 returned 200 and changed Bob's row"),
    "members": ("finding", "negative", "member role change denied; checked after D1"),
}[job]
call("swarm.artifact.put", {**ids, "artifact_id": f"proof-{job}", "source_revision": revision,
    "kind": kind, "content": content})
call("swarm.report", {**ids, "revision": revision, "message_id": f"{job}-result", "type": "result",
    "payload": {"artifact_ids": [f"proof-{job}"], "audit_outcome": outcome}})
