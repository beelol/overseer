#!/usr/bin/env python3
"""T-24 with a daemon that has no audio methods: the terminal bell still rings.

    python3 docs/verification/evidence/tui/t24-old-daemon.py OLD_OVERSEERD OVERSEER_TUI

OLD_OVERSEERD is an overseerd built from a revision without Audio Mode; OVERSEER_TUI is this
branch's binary. The daemon runs in its own temporary home with the SYNTHETIC Claude fixture
(permission mode). The TUI runs in a pseudo-terminal for six seconds; an agent starts waiting
while it is attached. Exit code 0 means every check passed."""
import json, os, pathlib, subprocess, sys, tempfile, threading, time

old, tui = (os.path.abspath(p) for p in sys.argv[1:3])
root = pathlib.Path(__file__).resolve().parents[4]
helper = root / "tui/tests/pty_run.py"
fixture = root / "fixtures/fake-harness/claude-fixture.js"
failed = []


def check(ok, text):
    print(("ok    " if ok else "FAIL  ") + text)
    if not ok:
        failed.append(text)


with tempfile.TemporaryDirectory() as tmp:
    home, repo = os.path.join(tmp, "home"), os.path.join(tmp, "repo")
    env = dict(os.environ, OVERSEER_HOME=home, OVERSEER_CLAUDE_PATH=str(fixture), CLAUDE_FIXTURE_MODE="permission",
               OVERSEER_HARNESS_ENV_PASSTHROUGH="CLAUDE_FIXTURE_MODE")

    def ctl(method, params=None):
        out = subprocess.run([old, "ctl", method, json.dumps(params or {})], env=env, capture_output=True, text=True).stdout
        return json.loads(out.splitlines()[0]) if out.strip() else {"error": {"message": "no reply"}}

    os.makedirs(repo)
    for args in (["init", "-q", "-b", "main"], ["config", "user.name", "Overseer Test"], ["config", "user.email", "overseer-test@example.invalid"], ["config", "commit.gpgsign", "false"]):
        subprocess.run(["git", *args], cwd=repo, check=True)
    pathlib.Path(repo, "README.md").write_text("# fixture\n")
    subprocess.run(["git", "add", "."], cwd=repo, check=True)
    subprocess.run(["git", "commit", "-q", "-m", "base"], cwd=repo, check=True)

    daemon = subprocess.Popen([old, "serve"], env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            if "result" in ctl("hello"):
                break
            time.sleep(0.1)
        answer = ctl("audio.get")
        check("error" in answer, f"the daemon has no audio.get: {answer.get('error', {}).get('message')!r}")
        quiet = ctl("task.create", {"repo": repo, "harness": "generic", "program": "/bin/sh", "args": ["-c", "echo quiet; while read l; do echo $l; done"], "prompt": "", "title": "Quiet agent", "workspace_mode": "worktree"})["result"]["run"]["id"]
        result = {}

        def session():
            result["out"] = subprocess.run(["python3", str(helper), "30", "120", "6.0", "q", "--", tui, "--daemon", old, "--home", home],
                                           env=dict(env, TERM="xterm-256color"), capture_output=True).stdout.decode(errors="replace")

        thread = threading.Thread(target=session)
        thread.start()
        time.sleep(2.5)
        waiting = ctl("task.create", {"repo": repo, "harness": "claude", "prompt": "write perm.txt", "title": "Asks permission"})["result"]["run"]["id"]
        for _ in range(200):
            status = next((r["status"] for r in ctl("state")["result"]["runs"] if r["id"] == waiting), "")
            if status == "waiting_for_user":
                break
            time.sleep(0.1)
        check(status == "waiting_for_user", "an agent starts waiting while the TUI is attached")
        thread.join()
        text = result["out"]
        titles, bells = text.count("\x1b]0;"), text.count("\x07")
        check("\x1b]0;Overseer · 1 needs you · 2 active\x07" in text, "the window title says 1 needs you · 2 active")
        check("Asks permission needs you" in text, "the notice names the agent")
        check(bells == titles + 1, f"one bell beyond the title's terminators ({bells} bells, {titles} titles)")
        for run in (quiet, waiting):
            ctl("run.interrupt", {"run_id": run})
    finally:
        ctl("daemon.stop_all")
        time.sleep(0.3)
        daemon.kill()
        daemon.wait()

print("\n" + ("PASS" if not failed else f"FAIL ({len(failed)})"))
sys.exit(1 if failed else 0)
