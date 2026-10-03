import subprocess,sys,json
from pathlib import Path
r=subprocess.run([sys.executable,str(Path(__file__).with_name('probe.py')),sys.argv[1]],capture_output=True,text=True,timeout=5)
print(json.dumps({'child_exit':r.returncode,'child_stdout':r.stdout,'child_stderr':r.stderr},sort_keys=True))
