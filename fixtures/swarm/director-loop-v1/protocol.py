"""Owner-local fixture protocol helper; no provider account is used."""

import json
import os
import subprocess


def call(method, params):
    output = subprocess.check_output(
        [os.environ["OVERSEER_BIN"], "ctl", method, json.dumps(params)], text=True
    )
    reply = json.loads(output)
    if "error" in reply:
        raise RuntimeError(f"{method}: {reply['error']['message']}")
    return reply["result"]
