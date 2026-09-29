"""Local evidence-producing worker for the serial/parallel evaluation fixture."""

import json
import os
import subprocess
import sys
import time


def call(method, params):
    output = subprocess.check_output(
        [os.environ["OVERSEER_BIN"], "ctl", method, json.dumps(params)], text=True
    )
    reply = json.loads(output)
    if "error" in reply:
        raise RuntimeError(f"{method}: {reply['error']['message']}")
    return reply["result"]


run = os.environ["OVERSEER_SWARM_RUN_ID"]
job = os.environ["OVERSEER_SWARM_JOB_ID"]
attempt = os.environ["OVERSEER_SWARM_ATTEMPT_ID"]
token = os.environ["OVERSEER_SWARM_TOKEN"]
revision = int(os.environ["OVERSEER_SWARM_REVISION"])
delay_ms = int(sys.argv[1])
assert 1 <= delay_ms <= 10000

time.sleep(delay_ms / 1000)
artifact = f"evaluation-proof-{job}"
call("swarm.artifact.put", {
    "run_id": run, "job_id": job, "attempt_id": attempt, "token": token,
    "artifact_id": artifact, "source_revision": revision,
    "kind": "reproduction", "content": f"completed fixture job {job}",
})
call("swarm.report", {
    "run_id": run, "job_id": job, "attempt_id": attempt, "token": token,
    "revision": revision, "message_id": f"evaluation-result-{job}",
    "type": "result", "payload": {
        "artifact_ids": [artifact],
        "fixture_work_units_milli": 100,
    },
})
