#!/usr/bin/env python3
"""Checks the bundled Reactor pack (daemon/assets/reactor). Run from the repository root:

    python3 docs/verification/evidence/audio-mode/check-pack.py [--approved DIR] [--write]

Every MP3 must decode, last under 0.5 s and match its manifest entry (size, SHA-256,
duration); the pack must hold 12 cues and 31,488 MP3 bytes; the repository must track no
other audio file. With --approved, each file is also compared byte for byte with the
owner-approved folder. With --write, the manifest's size, hash and duration fields and the
table in the pack's README are rewritten from the files first. Needs ffmpeg and ffprobe on
PATH. Exit code 0 means pass."""
import argparse, hashlib, json, pathlib, subprocess, sys

ROOT = pathlib.Path(__file__).resolve().parents[4]
PACK = ROOT / "daemon/assets/reactor"
KEYS = 12
TOTAL_BYTES = 31_488
MAX_SECONDS = 0.5
AUDIO_SUFFIXES = (".mp3", ".wav", ".aif", ".aiff", ".m4a", ".ogg", ".flac", ".caf", ".opus")

failures = []


def check(ok, text):
    print(("ok    " if ok else "FAIL  ") + text)
    if not ok:
        failures.append(text)


def decoded_seconds(path):
    """Length of the decoded audio: PCM bytes / (rate x channels x 2). A file that does
    not decode cleanly returns None."""
    probe = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries",
                            "stream=codec_name,sample_rate,channels", "-of", "json", str(path)],
                           capture_output=True, text=True)
    if probe.returncode != 0 or probe.stderr.strip():
        return None
    streams = json.loads(probe.stdout).get("streams", [])
    if len(streams) != 1 or streams[0].get("codec_name") != "mp3":
        return None
    rate, channels = int(streams[0]["sample_rate"]), int(streams[0]["channels"])
    pcm = subprocess.run(["ffmpeg", "-v", "error", "-xerror", "-i", str(path), "-f", "s16le", "-"],
                         capture_output=True)
    if pcm.returncode != 0 or pcm.stderr.strip() or not pcm.stdout:
        return None
    return len(pcm.stdout) / (rate * channels * 2)


def table(manifest):
    rows = ["| Key | Meaning | Plays by itself | Seconds | Bytes | SHA-256 |", "| --- | --- | --- | --- | --- | --- |"]
    for cue in manifest:
        rows.append(f"| `{cue['key']}` | {cue['meaning']} | {'yes' if cue['default_auto'] else 'no'} | {cue['duration']:.3f} | {cue['mp3_bytes']:,} | `{cue['sha256']}` |")
    rows.append(f"| 12 cues | | 3 | | {sum(cue['mp3_bytes'] for cue in manifest):,} | |")
    return "\n".join(rows)


def readme_table(text):
    start, end = "<!-- pack:start -->\n", "\n<!-- pack:end -->"
    return text[text.index(start) + len(start):text.index(end)], start, end


def main():
    args = argparse.ArgumentParser()
    args.add_argument("--approved", type=pathlib.Path)
    args.add_argument("--write", action="store_true")
    args = args.parse_args()

    manifest_path = PACK / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    files = sorted(PACK.glob("*.mp3"))
    facts = {}
    for f in files:
        data = f.read_bytes()
        facts[f.name] = dict(bytes=len(data), sha256=hashlib.sha256(data).hexdigest(), seconds=decoded_seconds(f))

    if args.write:
        for cue in manifest:
            fact = facts[cue["mp3"]]
            cue.pop("wav", None)
            cue["duration"] = round(fact["seconds"], 3)
            cue["mp3_bytes"] = fact["bytes"]
            cue["sha256"] = fact["sha256"]
        manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
        readme = (PACK / "README.md").read_text()
        current, start, end = readme_table(readme)
        (PACK / "README.md").write_text(readme.replace(start + current + end, start + table(manifest) + end))
        print(f"manifest and README table rewritten from {len(files)} files\n")

    print(f"{'cue':<28}{'bytes':>7}{'seconds':>9}  sha256")
    for name, fact in facts.items():
        seconds = "-" if fact["seconds"] is None else f"{fact['seconds']:.3f}"
        print(f"{name:<28}{fact['bytes']:>7}{seconds:>9}  {fact['sha256']}")
    total = sum(fact["bytes"] for fact in facts.values())
    print(f"{'total':<28}{total:>7}\n")

    check(len(files) == KEYS, f"{len(files)} MP3 files in the pack (expected {KEYS})")
    check(total == TOTAL_BYTES, f"{total:,} MP3 bytes in total (expected {TOTAL_BYTES:,})")
    check(all(fact["seconds"] is not None for fact in facts.values()), "every MP3 decodes without an error")
    longest = max((fact["seconds"] or 99 for fact in facts.values()), default=99)
    check(longest < MAX_SECONDS, f"longest cue {longest:.3f} s (limit {MAX_SECONDS} s)")

    check(sorted(cue["mp3"] for cue in manifest) == [f.name for f in files], "the manifest lists exactly the files in the pack")
    for cue in manifest:
        fact = facts.get(cue["mp3"])
        if not fact:
            continue
        same = (cue["key"] + ".mp3" == cue["mp3"] and cue.get("mp3_bytes") == fact["bytes"] and cue.get("sha256") == fact["sha256"]
                and fact["seconds"] is not None and abs(cue.get("duration", -1) - fact["seconds"]) < 0.0005)
        check(same, f"manifest entry {cue['key']} matches its file (size, SHA-256, duration)")
        check("wav" not in cue and "original" in cue.get("provenance", ""), f"manifest entry {cue['key']} names no WAV and states original synthesis")
    if all("sha256" in cue for cue in manifest):
        check(readme_table((PACK / "README.md").read_text())[0] == table(manifest), "the README table states the manifest's facts")
    auto = sorted(cue["key"] for cue in manifest if cue.get("default_auto"))
    check(auto == ["agent_complete", "agent_needs_attention", "agent_started"], f"cues that play by themselves: {', '.join(auto)}")

    tracked = subprocess.run(["git", "ls-files", "-z"], cwd=ROOT, capture_output=True, text=True, check=True).stdout.split("\0")
    audio = sorted(p for p in tracked if p.lower().endswith(AUDIO_SUFFIXES))
    expected = sorted(str(f.relative_to(ROOT)) for f in files)
    check(audio == expected, f"the repository tracks {len(audio)} audio files, all in the pack" if audio == expected
          else f"unexpected tracked audio files: {sorted(set(audio) ^ set(expected))}")
    commander = [p for p in tracked if "commander" in p.lower()]
    check(not commander, "no tracked path belongs to a Commander pack" if not commander else f"tracked Commander paths: {commander}")

    if args.approved:
        approved = sorted(args.approved.glob("*.mp3"))
        check([f.name for f in approved] == [f.name for f in files], f"the approved folder holds the same {len(approved)} file names")
        for f in approved:
            digest = hashlib.sha256(f.read_bytes()).hexdigest()
            fact = facts.get(f.name)
            check(bool(fact) and fact["sha256"] == digest, f"{f.name} is byte-identical to the approved file ({digest[:16]}…)")
    else:
        print("note  no --approved folder given: the comparison with the owner-approved pack was not run")

    print("\n" + ("PASS" if not failures else f"FAIL ({len(failures)})"))
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
