"""Owner-local fixture protocol helper; no provider account is used.

A call that never reached the daemon (the daemon restarting) is retried for a
bounded time. A daemon error is raised at once. Every call the fixture makes
while the daemon may restart is replay-safe (keyed by a request, message or
artifact id), and the scripts wait on local files, not the daemon, at the
point the test restarts it.
"""

import json
import os
import subprocess
import time


def call(method, params, retry_s=30):
    deadline = time.monotonic() + retry_s
    while True:
        done = subprocess.run(
            [os.environ["OVERSEER_BIN"], "ctl", method, json.dumps(params)],
            text=True, capture_output=True,
        )
        if done.returncode == 0 and done.stdout.strip():
            reply = json.loads(done.stdout)
            if "error" in reply:
                raise RuntimeError(f"{method}: {reply['error']['message']}")
            return reply["result"]
        if time.monotonic() >= deadline:
            raise RuntimeError(f"{method}: daemon unreachable: {done.stderr.strip()}")
        time.sleep(0.2)


def wait_for(path, seconds):
    deadline = time.monotonic() + seconds
    while not os.path.exists(path):
        if time.monotonic() >= deadline:
            raise RuntimeError(f"timed out waiting for {path}")
        time.sleep(0.05)
