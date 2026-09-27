"""Behavioral checks for the Auto Mode acceptance audit."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
SCRIPT = HERE / "check_ledger.py"
RFC = ROOT / "docs/rfcs/auto-mode.md"
LEDGER = HERE / "README.md"


def audit(rfc=RFC, ledger=LEDGER, *extra):
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--rfc", str(rfc), "--ledger", str(ledger), *extra],
        capture_output=True, text=True, cwd=ROOT, check=False,
    )


class LedgerAuditTests(unittest.TestCase):
    def test_current_rfc_tracks_all_core_criteria_and_keeps_release_files_separate(self):
        result = audit(RFC, LEDGER, "--base", "origin/main")
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report["core_total"], 40)
        self.assertEqual(report["conditional_deferred"], 1)
        self.assertTrue(report["release_files_unchanged"])

    def test_missing_rfc_criterion_is_a_failing_audit(self):
        with tempfile.TemporaryDirectory() as directory:
            rfc = Path(directory) / "rfc.md"
            text = RFC.read_text()
            rfc.write_text("\n".join(line for line in text.splitlines()
                if "**AUTO-AC-36 —" not in line) + "\n")
            result = audit(rfc)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing RFC criteria: 36", result.stderr)

    def test_checked_rfc_item_requires_verified_ledger_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            rfc = Path(directory) / "rfc.md"
            text = RFC.read_text().replace("- [ ] **AUTO-AC-21", "- [x] **AUTO-AC-21")
            rfc.write_text(text)
            result = audit(rfc)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("checkbox/status mismatch: 21", result.stderr)

    def test_duplicate_ledger_row_is_a_failing_audit(self):
        with tempfile.TemporaryDirectory() as directory:
            ledger = Path(directory) / "ledger.md"
            text = LEDGER.read_text()
            row = next(line for line in text.splitlines() if line.startswith("| AUTO-AC-16 —"))
            ledger.write_text(text + "\n" + row + "\n")
            result = audit(RFC, ledger)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("duplicate ledger criteria: 16", result.stderr)


if __name__ == "__main__":
    unittest.main()
