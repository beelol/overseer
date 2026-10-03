import json, os, subprocess
results = {}
try:
    pid = os.fork()
except OSError as e:
    results['fork'] = {'errno': e.errno}
else:
    if pid == 0: os._exit(0)
    results['fork'] = {'child_exit': os.waitpid(pid, 0)[1]}
try:
    r = subprocess.run(['/usr/bin/true'], timeout=2)
    results['subprocess'] = {'child_exit': r.returncode}
except OSError as e:
    results['subprocess'] = {'errno': e.errno}
print(json.dumps(results, sort_keys=True))
