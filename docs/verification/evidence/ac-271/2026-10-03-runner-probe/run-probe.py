import subprocess,json,socket,sys
from pathlib import Path
p=Path(__file__).resolve().parent
results=[]
for mode in ['sandboxed','control']:
 unix=p/'forbidden/fake-daemon.sock'; unix.unlink(missing_ok=True)
 tcp=socket.socket();tcp.bind(('127.0.0.1',0));tcp.listen(2)
 u=socket.socket(socket.AF_UNIX);u.bind(str(unix));u.listen(2)
 try:
  cmd=['/Applications/Xcode.app/Contents/Developer/usr/bin/python3',str(p/'program/descendant.py'),str(tcp.getsockname()[1])]
  if mode=='sandboxed':cmd=['/usr/bin/sandbox-exec','-f',str(p/'runner-v6.sb'),*cmd]
  r=subprocess.run(cmd,env={'PATH':'/usr/bin:/bin','HOME':str(p/'allowed'),'TMPDIR':str(p/'allowed'),'PYTHONDONTWRITEBYTECODE':'1'},cwd=p/'allowed',capture_output=True,text=True,timeout=15)
  v={'mode':mode,'exit':r.returncode,'stdout':r.stdout,'stderr':r.stderr,'listeners':{}}
  for name,s in [('loopback',tcp),('unix',u)]:
   s.settimeout(.2)
   try:c,_=s.accept();c.close();v['listeners'][name]='connection accepted'
   except TimeoutError:v['listeners'][name]='no connection'
  results.append(v)
 finally:
  tcp.close();u.close();unix.unlink(missing_ok=True)
  for f in [p/'forbidden/injected',p/'program/injected']: f.unlink(missing_ok=True)
(p/'descendant-results.json').write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps(results,indent=2))
for row in results:
 assert row['exit']==0 and not row['stderr'],row
 child=json.loads(row['stdout']);assert child['child_exit']==0 and not child['child_stderr'],child
 checks=json.loads(child['child_stdout']);assert checks['scratch_write']=='allowed',checks
 for key,value in checks.items():
  if key=='scratch_write':continue
  assert value==({'denied_errno':1} if row['mode']=='sandboxed' else 'allowed'),(key,value)
 expected='no connection' if row['mode']=='sandboxed' else 'connection accepted'
 assert set(row['listeners'].values())=={expected},row
print('All descendant and positive-control assertions passed')
