import subprocess, json
from pathlib import Path
p = Path(__file__).resolve().parent
rows = []
for mode in ['sandboxed', 'control']:
    cmd = ['/Applications/Xcode.app/Contents/Developer/usr/bin/python3', str(p / 'program/fork-probe.py')]
    if mode == 'sandboxed':
        cmd = ['/usr/bin/sandbox-exec', '-f', str(p / 'runner-no-fork.sb'), *cmd]
    r = subprocess.run(cmd, env={'PATH': '/usr/bin:/bin', 'HOME': str(p / 'allowed'), 'TMPDIR': str(p / 'allowed'), 'PYTHONDONTWRITEBYTECODE': '1'}, cwd=p / 'allowed', capture_output=True, text=True, timeout=8)
    rows.append({'mode': mode, 'exit': r.returncode, 'stdout': r.stdout, 'stderr': r.stderr})
(p / 'fork-denial-results.json').write_text(json.dumps(rows, indent=2) + '\n')
print(json.dumps(rows, indent=2))
for r in rows:
    assert r['exit'] == 0, r
    checks = json.loads(r['stdout'])
    expected = {'errno': 1} if r['mode'] == 'sandboxed' else {'child_exit': 0}
    assert checks == {'fork': expected, 'subprocess': expected}, checks
print('Fork and subprocess denied; positive controls passed')
