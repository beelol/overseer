import json
import subprocess
import sys
import unittest


def command(action, namespace=None):
    args = [sys.executable, "session.py", action]
    if namespace is not None:
        args.append(namespace)
    result = subprocess.run(args, capture_output=True, text=True, check=True)
    return json.loads(result.stdout)


class PersistentEffectSessionTests(unittest.TestCase):
    def test_separate_processes_can_probe_one_unreceipted_grant(self):
        initialized = command("init")
        namespace = initialized["namespace"]
        try:
            self.assertEqual(initialized["queued"], ["evt-42", "evt-42"])
            delivery = command("deliver", namespace)
            self.assertEqual(delivery["status"], "ack_lost")
            observed = command("outcome", namespace)
            self.assertEqual(observed["event_id"], "evt-42")
            self.assertEqual(observed["grant_count"], 1)
            self.assertEqual(observed["receipt"], False)
            self.assertEqual(observed["queued"], ["evt-42"])
            self.assertEqual(observed["status"], "unknown_do_not_retry")
        finally:
            command("cleanup", namespace)

    def test_blind_redelivery_would_double_apply(self):
        namespace = command("init")["namespace"]
        try:
            command("deliver", namespace)
            command("deliver", namespace)
            observed = command("outcome", namespace)
            self.assertEqual(observed["grant_count"], 2)
            self.assertEqual(observed["queued"], [])
        finally:
            command("cleanup", namespace)


if __name__ == "__main__":
    unittest.main()
