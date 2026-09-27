"""Report D1 early; submit the task result only after J4 applies the advisory."""

import os
from pathlib import Path
import sys
import time

from protocol import call


run = os.environ["OVERSEER_SWARM_RUN_ID"]
job = os.environ["OVERSEER_SWARM_JOB_ID"]
attempt = os.environ["OVERSEER_SWARM_ATTEMPT_ID"]
token = os.environ["OVERSEER_SWARM_TOKEN"]
revision = int(os.environ["OVERSEER_SWARM_REVISION"])
assert job == "j2"
call("swarm.report", {"run_id": run, "job_id": job, "attempt_id": attempt,
    "token": token, "revision": revision, "message_id": "D1", "type": "discovery",
    "payload": {"symbol": "TaskRepository.findById", "source_revision": "fixture-v1",
        "note": "lookup uses only task id; check callers before concluding"}})
marker = Path(sys.argv[1])
deadline = time.monotonic() + 15
while not marker.exists():
    if time.monotonic() >= deadline:
        raise RuntimeError("J4 never applied D1")
    time.sleep(0.05)
call("swarm.artifact.put", {"run_id": run, "job_id": job, "attempt_id": attempt,
    "token": token, "artifact_id": "proof-j2", "source_revision": revision,
    "kind": "finding", "content": "Task lookup and callers checked"})
call("swarm.report", {"run_id": run, "job_id": job, "attempt_id": attempt,
    "token": token, "revision": revision, "message_id": "J2-result", "type": "result",
    "payload": {"artifact_ids": ["proof-j2"]}})
