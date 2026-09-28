import unittest

from probe import probe


class LedgerPayJobProbes(unittest.TestCase):
    def test_job_evidence_and_variants(self):
        evidence = {job: probe(job)["evidence"] for job in
                    ("k1", "k2", "k3", "k4", "k3-protected", "k2-missing-redis")}
        self.assertEqual(evidence["k1"]["invalidSignatureStatus"], 401)
        self.assertEqual(evidence["k1"]["validSignatureStatus"], 202)
        self.assertEqual(evidence["k2"]["queuedEventIds"], ["evt-42", "evt-42"])
        self.assertEqual(evidence["k2"]["outcomeProbe"]["status"], "unknown_do_not_retry")
        self.assertEqual(evidence["k3"]["finalState"]["grant_count"], 2)
        self.assertEqual(evidence["k3-protected"]["finalState"]["grant_count"], 1)
        self.assertEqual(evidence["k4"]["staleActivation"]["reason"], "stale_version")
        self.assertEqual(evidence["k2-missing-redis"]["webhookStatus"], 503)


if __name__ == "__main__":
    unittest.main()
