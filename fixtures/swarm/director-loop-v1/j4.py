"""Apply only the director's targeted D1 advisory, then submit evidence."""

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
assert job == "j4"
deadline = time.monotonic() + 15
while time.monotonic() < deadline:
    inbox = call("swarm.messages", {"run_id": run, "recipient": attempt, "token": token})
    advisory = next((message for message in inbox["messages"]
        if message["message_id"] == "D1-to-J4"), None)
    if advisory:
        assert advisory["payload"]["discovery_id"] == "D1"
        for phase in ("delivered", "applied"):
            call("swarm.ack", {"run_id": run, "recipient": attempt, "token": token,
                "message_id": "D1-to-J4", "phase": phase, "revision": revision})
        Path(sys.argv[1]).write_text("D1 applied")
        break
    time.sleep(0.05)
else:
    raise RuntimeError("J4 never received D1")
call("swarm.artifact.put", {"run_id": run, "job_id": job, "attempt_id": attempt,
    "token": token, "artifact_id": "proof-j4", "source_revision": revision,
    "kind": "finding", "content": "Attachment callers checked after D1"})
call("swarm.report", {"run_id": run, "job_id": job, "attempt_id": attempt,
    "token": token, "revision": revision, "message_id": "J4-result", "type": "result",
    "payload": {"artifact_ids": ["proof-j4"]}})
