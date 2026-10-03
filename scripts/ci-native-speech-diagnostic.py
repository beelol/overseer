#!/usr/bin/env python3
"""Explicit manual CI diagnostic only. Import and default invocation never start synthesis."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import selectors
import signal
import subprocess
import time

TEST = 'memspeech::diagnostic::one_native_attempt_has_complete_in_memory_speech'
SOURCE_FILES = ['voice/src/lib.rs','voice/src/memspeech.rs',
                'voice/src/memspeech_diagnostic.rs','voice/src/pcm.rs']


class Refused(RuntimeError):
    pass


def digest(path):
    value=hashlib.sha256()
    with open(path, 'rb') as stream:
        for block in iter(lambda:stream.read(65536), b''):
            value.update(block)
    return value.hexdigest()


def private_write(path, data):
    # No reuse or symlink following; restrictive mode applies at creation.
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        stream.write(data)


def capture(args, *, cwd, environment, seconds, limit):
    """Whole-command cutoff, combined stream cap, and owned group cleanup; no shell/retry."""
    start = time.monotonic()
    child = subprocess.Popen(args, cwd=cwd, env=environment, stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
    streams = {'stdout':bytearray(),'stderr':bytearray()}
    timed_out = output_limit = False
    reaped = False
    selector = selectors.DefaultSelector()
    try:
        for label, stream in [('stdout',child.stdout),('stderr',child.stderr)]:
            os.set_blocking(stream.fileno(), False)
            selector.register(stream, selectors.EVENT_READ, label)
        while selector.get_map() or child.poll() is None:
            left = seconds - (time.monotonic() - start)
            if left <= 0:
                timed_out = True
                break
            for key, _ in selector.select(min(left, 0.05)):
                data = os.read(key.fileobj.fileno(), 65536)
                if not data:
                    selector.unregister(key.fileobj)
                    continue
                available = max(0, limit - sum(len(b) for b in streams.values()))
                streams[key.data].extend(data[:available])
                if len(data) > available:
                    output_limit = True
                    break
            if output_limit:
                break
    finally:
        # Kill the group even when a leader exits while a child keeps its pipes open.
        # The process group is owned by this Popen launch (start_new_session), never discovered
        # from owner processes. Kill+wait also runs on signal/exception/unwind.
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        try:
            child.wait(timeout=2)
            reaped = True
        except subprocess.TimeoutExpired:
            pass
        selector.close()
        child.stdout.close()
        child.stderr.close()
    status = child.returncode
    return {'code':status if status is not None and status >= 0 else None,
            'signal':-status if status is not None and status < 0 else None,
            'timed_out':timed_out,'output_limit':output_limit,'reaped':reaped,
            'stdout':bytes(streams['stdout']),'stderr':bytes(streams['stderr']),
            'seconds':round(time.monotonic()-start,3)}


def run(*, repo, out, invoke_native, event, selected, expected_commit,
        capture=capture, environment=None):
    # Both an explicit controller switch and the actual workflow event/input are required.
    if not invoke_native or event != 'workflow_dispatch' or not selected:
        raise Refused('explicit_manual_opt_in_required')
    if not re.fullmatch('[0-9a-f]{40}', expected_commit):
        raise Refused('expected_commit_required')
    environment = dict(os.environ if environment is None else environment)
    repo, out = Path(repo).resolve(), Path(out)
    try:
        out.mkdir(mode=0o700)
    except FileExistsError:
        raise Refused('existing_evidence_leaf_refused') from None
    raw = out/'raw';raw.mkdir(mode=0o700)
    receipt = {'expected_source':expected_commit,'variant':'one_attempt_isolated_worker',
        'success':False,'phase':'source','stages':{},
        'runner_image':{k:environment[k] for k in ['ImageOS','ImageVersion'] if k in environment},
        'os':{'system':platform.system(),'architecture':platform.machine(),
              'version':platform.mac_ver()[0]},
        'limits':{'build_seconds':900,'inventory_seconds':10,'native_seconds':40,
                  'build_bytes':2*1024*1024,'native_bytes':128*1024},
        'qualification':'not the unchanged ordinary retry/concurrency test; no full or AC closure'}

    def step(name, args, seconds=10, limit=65536, publish=False):
        result = capture(args,cwd=repo,environment=environment,seconds=seconds,limit=limit)
        folder = out if publish else raw
        private_write(folder/f'{name}.stdout.log',result['stdout'])
        private_write(folder/f'{name}.stderr.log',result['stderr'])
        receipt['stages'][name] = {k:v for k,v in result.items() if k not in ['stdout','stderr']}
        return result

    def require_success(result, label):
        if result['code'] != 0 or result['signal'] is not None or result['timed_out'] \
                or result['output_limit'] or not result['reaped']:
            raise Refused(label)

    try:
        source = step('source',['git','rev-parse','HEAD'])
        require_success(source,'source_command_failed')
        receipt['source'] = source['stdout'].decode('ascii').strip()
        if receipt['source'] != expected_commit:
            raise Refused('checkout_source_mismatch')
        clean = step('source_clean',['git','status','--porcelain','--untracked-files=no'])
        require_success(clean,'source_clean_command_failed')
        if clean['stdout'].strip():
            raise Refused('modified_tracked_source')
        receipt['source_hashes'] = {name:digest(repo/name) for name in SOURCE_FILES}
        version = step('rust_version',['rustc','--version'])
        require_success(version,'rust_version_failed')
        receipt['rust_version'] = version['stdout'].decode('utf8').strip()
        if platform.system() == 'Darwin':
            build = step('os_build',['sw_vers','-buildVersion'])
            require_success(build,'os_build_failed')
            value = build['stdout'].decode('ascii').strip()
            receipt['os']['build'] = value if re.fullmatch('[A-Za-z0-9.]+',value) else None

        receipt['phase']='build'
        clean = step('package_clean',['cargo','clean','-p','overseer-listener'],seconds=120)
        require_success(clean,'package_invalidation_failed')
        build = step('build',['cargo','test','-p','overseer-listener','--lib','--no-run',
            '--message-format=json'],seconds=900,limit=2*1024*1024)
        require_success(build,'library_build_failed')
        artifacts=[];finished=False
        for line in build['stdout'].splitlines():
            try:value=json.loads(line)
            except (json.JSONDecodeError,UnicodeDecodeError):continue
            if value.get('reason')=='build-finished':finished=value.get('success') is True
            target=value.get('target',{})
            if value.get('reason')=='compiler-artifact' and target.get('kind')==['lib'] \
                    and target.get('name')=='overseer_listener' and value.get('profile',{}).get('test'):
                artifacts.append(value)
        if not finished or len(artifacts)!=1:
            raise Refused('exact_test_library_artifact_required')
        artifact=artifacts[0]
        if artifact.get('fresh') is not False or Path(artifact['target']['src_path']).resolve() \
                != repo/'voice/src/lib.rs':
            raise Refused('fresh_compiled_actual_source_required')
        executable=Path(artifact.get('executable') or '')
        if not executable.is_file() or executable.is_symlink():
            raise Refused('actual_executable_required')
        dep=executable.with_suffix('.d')
        if not dep.is_file() or 'voice/src/memspeech_diagnostic.rs' not in dep.read_text():
            raise Refused('diagnostic_dependency_source_required')
        private_write(out/'diagnostic-source.d',dep.read_bytes())
        receipt['compiler_artifact']={k:artifact.get(k) for k in ['target','profile','fresh','executable']}
        receipt['dependency_sha256']=digest(dep)
        receipt['executable']={'path':str(executable),'sha256_before':digest(executable)}

        receipt['phase']='inventory'
        listing=step('inventory',[str(executable),'--list','--ignored','--exact',TEST],publish=True)
        require_success(listing,'inventory_command_failed')
        names=[line.removesuffix(': test') for line in listing['stdout'].decode('utf8').splitlines()
               if line.endswith(': test')]
        receipt['selected_count']=len(names)
        if names != [TEST]:
            raise Refused('exact_one_diagnostic_required')
        if {name:digest(repo/name) for name in SOURCE_FILES} != receipt['source_hashes']:
            raise Refused('source_changed_before_native')
        receipt['phase']='native'
        native=step('native',[str(executable),'--exact',TEST,'--ignored','--nocapture',
            '--test-threads=1'],seconds=40,limit=128*1024,publish=True)
        receipt['native']=receipt['stages']['native']
        receipt['executable']['sha256_after']=digest(executable)
        unchanged=receipt['executable']['sha256_before']==receipt['executable']['sha256_after']
        same_source={name:digest(repo/name) for name in SOURCE_FILES}==receipt['source_hashes']
        receipt['success']=native['code']==0 and native['signal'] is None and not native['timed_out'] \
            and not native['output_limit'] and native['reaped'] and unchanged and same_source
        receipt['phase']='complete'
        return receipt
    except Refused as error:
        receipt['refusal']=str(error)
        raise
    except BaseException:
        # Do not dump arbitrary exception/environment strings into uploaded receipts.
        receipt['refusal']='controller_interrupted_or_setup_error'
        raise
    finally:
        private_write(out/'receipt.json',(json.dumps(receipt,indent=2)+'\n').encode())


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out',type=Path,required=True)
    parser.add_argument('--expected-commit',required=True)
    parser.add_argument('--invoke-native',action='store_true',default=False)
    args=parser.parse_args()
    def interrupted(signum,_):
        raise KeyboardInterrupt()
    signal.signal(signal.SIGTERM,interrupted)
    try:
        receipt=run(repo=Path(__file__).resolve().parents[1],out=args.out,
            invoke_native=args.invoke_native,event=os.environ.get('GITHUB_EVENT_NAME',''),
            selected=os.environ.get('NATIVE_SPEECH_DIAGNOSTIC','')=='true',
            expected_commit=args.expected_commit)
        return 0 if receipt['success'] else 1
    except Refused as error:
        print(f'native diagnostic refused: {error}')
        return 1
    except BaseException:
        print('native diagnostic controller failed; inspect bounded receipt')
        return 1


if __name__=='__main__':
    raise SystemExit(main())
