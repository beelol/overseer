"""A replacement director reviews an already submitted self-attempt."""

import os
import sys

from protocol import call


run = os.environ["OVERSEER_SWARM_RUN_ID"]
generation = int(os.environ["OVERSEER_SWARM_GENERATION"])
owner_token = os.environ["OVERSEER_SWARM_DIRECTOR_TOKEN"]
attempt = sys.argv[1]
auth = {"run_id": run, "generation": generation, "owner_token": owner_token}

decision = call("swarm.decide", {**auth, "revision": 1, "job_id": "inspect",
    "decision": "accept", "evidence": ["serial-proof"]})
assert decision["status"] == "accepted", decision
finished = call("swarm.attempt.confirm_exit", {**auth, "revision": 1,
    "job_id": "inspect", "attempt_id": attempt})
assert finished["status"] == "finished", finished
