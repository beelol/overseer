"""Audit Auto Mode's RFC checklist and independent verification ledger."""

import argparse
from collections import Counter
import json
from pathlib import Path
import re
import subprocess
import sys


RFC_ITEM = re.compile(r"^- \[([ x])\] \*\*AUTO-AC-(\d{2}) — ([^.]+)\.\*\*.*\*\*Verify:\*\* (.+)$", re.M)
LEDGER_ITEM = re.compile(r"^\| AUTO-AC-(\d{2}) — ([^|]+) \| ([^|]+) \| (.+) \|$", re.M)
REQUIRED_HEADINGS = (
    "Outcome", "Scope and coexistence", "Approaches considered",
    "What is known, and what still needs validation", "Observation contract",
    "Collecting evidence", "Measuring model and reasoning consumption",
    "Local-only telemetry from actual work", "Continuous task allocation",
    "Fallback and continuity", "Integration boundary and UI",
    "Acceptance criteria", "Verification loop and implementation sequencing",
    "Review decisions and feasibility gates",
)
ALLOWED_STATUSES = {
    "not started", "in progress", "implemented / unverified", "blocked",
    "verified", "deferred, conditional",
}
RELEASE_FILES = {
    "docs/overseer-rfc.md", "docs/verification/records.py",
    "docs/rfcs/account-governance.md", "docs/rfcs/claude-credentials.md",
}


def collect_duplicates(ids):
    return sorted(item for item, count in Counter(ids).items() if count != 1)


def changed_release_files(base):
    changed = set()
    # Compare against the latest main revision actually merged into this
    # branch. Main may advance while verification is running.
    ancestor = subprocess.run(["git", "merge-base", "HEAD", base],
        capture_output=True, text=True, check=False)
    if ancestor.returncode or not ancestor.stdout.strip():
        raise ValueError(f"cannot find merged main revision: {ancestor.stderr.strip()}")
    for args in (["git", "diff", "--name-only", ancestor.stdout.strip(), "HEAD"],
                 ["git", "diff", "--name-only"],
                 ["git", "diff", "--cached", "--name-only"]):
        result = subprocess.run(args, capture_output=True, text=True, check=False)
        if result.returncode:
            raise ValueError(f"cannot inspect release diff: {result.stderr.strip()}")
        changed.update(result.stdout.splitlines())
    return sorted(path for path in changed if path in RELEASE_FILES
        or re.fullmatch(r"docs/verification/AC-\d{2}\.md", path))


def audit(rfc_text, ledger_text, base=None):
    errors = []
    rfc = [(check, number, name.strip(), verify.strip())
           for check, number, name, verify in RFC_ITEM.findall(rfc_text)]
    ledger = [(number, name.strip(), status.strip(), evidence.strip())
              for number, name, status, evidence in LEDGER_ITEM.findall(ledger_text)]
    expected = {f"{number:02d}" for number in range(1, 42)}
    rfc_ids = {number for _, number, _, _ in rfc}
    ledger_ids = {number for number, _, _, _ in ledger}
    if missing := sorted(expected - rfc_ids):
        errors.append("missing RFC criteria: " + ", ".join(missing))
    if extra := sorted(rfc_ids - expected):
        errors.append("unexpected RFC criteria: " + ", ".join(extra))
    if duplicates := collect_duplicates(number for _, number, _, _ in rfc):
        errors.append("duplicate RFC criteria: " + ", ".join(duplicates))
    if missing := sorted(expected - ledger_ids):
        errors.append("missing ledger criteria: " + ", ".join(missing))
    if extra := sorted(ledger_ids - expected):
        errors.append("unexpected ledger criteria: " + ", ".join(extra))
    if duplicates := collect_duplicates(number for number, _, _, _ in ledger):
        errors.append("duplicate ledger criteria: " + ", ".join(duplicates))
    headings = set(re.findall(r"^## (.+)$", rfc_text, re.M))
    if missing := sorted(set(REQUIRED_HEADINGS) - headings):
        errors.append("missing original RFC sections: " + ", ".join(missing))
    by_id = {number: (name, status, evidence) for number, name, status, evidence in ledger}
    for checked, number, name, verify in rfc:
        if not verify:
            errors.append(f"empty Verify clause: {number}")
        if number not in by_id:
            continue
        ledger_name, status, evidence = by_id[number]
        if name != ledger_name:
            errors.append(f"criterion name mismatch: {number}")
        if status not in ALLOWED_STATUSES:
            errors.append(f"invalid ledger status: {number}")
        if (checked == "x") != (status == "verified"):
            errors.append(f"checkbox/status mismatch: {number}")
        if number != "30" and status == "deferred, conditional":
            errors.append(f"core criterion cannot be deferred: {number}")
        if len(evidence) < 30:
            errors.append(f"missing ledger evidence or next action: {number}")
    release_changes = changed_release_files(base) if base else []
    if release_changes:
        errors.append("release or account-governance files changed: " + ", ".join(release_changes))
    report = {
        "core_total": len(expected - {"30"}),
        "verified": sum(number != "30" and status == "verified" for number, _, status, _ in ledger),
        "conditional_deferred": sum(number == "30" and status == "deferred, conditional"
                                    for number, _, status, _ in ledger),
        "release_files_unchanged": not release_changes if base else None,
    }
    return report, errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    here = Path(__file__).resolve().parent
    parser.add_argument("--rfc", type=Path, default=here.parent.parent / "rfcs/auto-mode.md")
    parser.add_argument("--ledger", type=Path, default=here / "README.md")
    parser.add_argument("--base", help="Git baseline for release-file isolation")
    args = parser.parse_args()
    try:
        report, errors = audit(args.rfc.read_text(), args.ledger.read_text(), args.base)
    except (OSError, ValueError) as error:
        print(error, file=sys.stderr)
        return 2
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(json.dumps(report, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
