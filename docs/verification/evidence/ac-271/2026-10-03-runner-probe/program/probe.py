import os,socket,json,sys
from pathlib import Path
root=Path(__file__).resolve().parents[1]
r={}
def connect(family,address):
 s=socket.socket(family,socket.SOCK_STREAM)
 try: s.settimeout(1);s.connect(address)
 finally: s.close()
for name,fn in [
 ('scratch_write',lambda:(root/'allowed'/'result').write_bytes(b'probe')),
 ('forbidden_read',lambda:(root/'forbidden'/'synthetic-credential').read_bytes()),
 ('forbidden_write',lambda:(root/'forbidden'/'injected').write_bytes(b'probe')),
 ('program_write',lambda:(root/'program'/'injected').write_bytes(b'probe')),
 ('symlink_escape',lambda:(root/'allowed'/'escape').read_bytes()),
 ('loopback_connect',lambda:connect(socket.AF_INET,('127.0.0.1',int(sys.argv[1])))),
 ('unix_connect',lambda:connect(socket.AF_UNIX,str(root/'forbidden'/'fake-daemon.sock')))]:
 try: fn();r[name]='allowed'
 except OSError as e:r[name]={'denied_errno':e.errno}
print(json.dumps(r,sort_keys=True))
