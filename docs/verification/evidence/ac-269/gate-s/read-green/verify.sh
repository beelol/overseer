#!/bin/bash
# Prepared only. Execute exclusively in a coordinator-granted Cargo slot.
set -euo pipefail
cd /private/tmp/overseer-mods-gate-s-20261003
[ "$(git rev-parse HEAD)" = e78fb216fad8f09df8497fb389094d6ed877a2c8 ]
[ -z "$(git status --porcelain)" ]
export CARGO_TARGET_DIR=/private/tmp/overseer-closeout-verify-target
export CARGO_BUILD_JOBS=1
export RUST_TEST_THREADS=1
cargo build -p overseerd --bin overseerd --message-format=json > /private/tmp/overseer-mods-gate-s-green-build.jsonl 2> /private/tmp/overseer-mods-gate-s-green-build.log
cargo test -p overseerd --test mods_gate_s --test mods_bindings --test mods_delivery --test overseer --no-run --message-format=json > /private/tmp/overseer-mods-gate-s-green-no-run.jsonl 2> /private/tmp/overseer-mods-gate-s-green-no-run.log
python3 - <<'PY'
from pathlib import Path
import hashlib, json, shlex, subprocess
root=Path.cwd()
target=Path('/private/tmp/overseer-closeout-verify-target/debug')
source_commit=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
manifest=target/'overseerd.d'
body=manifest.read_text()
for source in ['mods/read.rs','overseer/mod.rs','overseer/session.rs']:
    assert str(root/'daemon/src'/source) in body, f'wrong standalone source: {source}'
needed={'mods_gate_s','mods_bindings','mods_delivery','overseer'}
executables={}
for line in Path('/private/tmp/overseer-mods-gate-s-green-no-run.jsonl').read_text().splitlines():
    artifact=json.loads(line)
    if artifact.get('reason')!='compiler-artifact' or not artifact.get('executable'):
        continue
    name=artifact['target']['name']
    if name not in needed or 'test' not in artifact['target']['kind']:
        continue
    assert artifact['target']['src_path']==str(root/'daemon/tests'/f'{name}.rs'), 'wrong test source'
    exe=Path(artifact['executable'])
    deps=exe.with_suffix('.d').read_text()
    assert f'daemon/tests/{name}.rs' in deps
    assert f'CARGO_MANIFEST_DIR={root}/daemon' in deps
    assert f'CARGO_BIN_EXE_overseerd={target}/overseerd' in deps
    executables[name]=str(exe)
assert set(executables)==needed, 'missing expected executable'
def sha(path):
    digest=hashlib.sha256()
    with path.open('rb') as file:
        for block in iter(lambda:file.read(8*1024*1024),b''):
            digest.update(block)
    return digest.hexdigest()
paths=list(root.joinpath('daemon/src/mods').glob('*.rs'))+[root/'daemon/src/overseer/mod.rs',root/'daemon/src/overseer/session.rs',manifest,target/'overseerd']
for name,exe in executables.items():
    paths.extend([root/'daemon/tests'/f'{name}.rs',Path(exe),Path(exe).with_suffix('.d')])
proof={'commit':source_commit,'source_root':str(root),'jobs':1,'test_threads':1,'executables':executables,'sha256':{str(path):sha(path) for path in paths}}
Path('/private/tmp/overseer-mods-gate-s-green-artifacts.json').write_text(json.dumps(proof,indent=2)+'\n')
# Shell-safe generated paths contain only target directory + Cargo identifiers.
Path('/private/tmp/overseer-mods-gate-s-green-executables.sh').write_text('\n'.join(f'{name.upper()}_EXE={shlex.quote(path)}' for name,path in sorted(executables.items()))+'\n')
print('Fresh standalone/test source identities and post-no-run hashes verified.')
PY
source /private/tmp/overseer-mods-gate-s-green-executables.sh
"$MODS_GATE_S_EXE" --nocapture 2>&1 | tee /private/tmp/overseer-mods-gate-s-read-green.log
"$MODS_BINDINGS_EXE" --nocapture 2>&1 | tee /private/tmp/overseer-mods-gate-s-bindings-green.log
"$MODS_DELIVERY_EXE" --nocapture 2>&1 | tee /private/tmp/overseer-mods-gate-s-delivery-green.log
"$OVERSEER_EXE" ac180_ --nocapture 2>&1 | tee /private/tmp/overseer-mods-gate-s-native-incumbent-green.log
python3 - <<'PY'
from pathlib import Path
import json, re
checks={'read':11,'bindings':14,'delivery':21,'native-incumbent':2}
results={}
for label,expected in checks.items():
    path=Path(f'/private/tmp/overseer-mods-gate-s-{label}-green.log')
    if label=='read':
        path=Path('/private/tmp/overseer-mods-gate-s-read-green.log')
    body=path.read_text()
    summaries=re.findall(r'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out',body)
    assert len(summaries)==1, f'{label}: missing or duplicate summary'
    result,passed,failed,ignored,measured,filtered=summaries[0]
    names=re.findall(r'^test ([\w:]+) \.\.\.',body,re.M)
    assert result=='ok' and int(passed)==expected and int(failed)==0 and int(ignored)==0 and int(measured)==0, f'{label}: unexpected actual summary {summaries}'
    assert len(names)==expected and len(set(names))==expected, f'{label}: actual named cases differ from expected count'
    results[label]={'passed':int(passed),'failed':int(failed),'filtered':int(filtered),'actual_names':names}
Path('/private/tmp/overseer-mods-gate-s-focused-counts.json').write_text(json.dumps(results,indent=2)+'\n')
print('Actual counts verified:11 read,14 bindings,21 delivery,2 native-incumbent.')
PY
