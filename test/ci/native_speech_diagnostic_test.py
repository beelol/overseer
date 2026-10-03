"""Synthetic boundary tests. No Cargo, native speech, UI, provider, or owner profiles."""
import importlib.util
import json
from pathlib import Path
import tempfile
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('native_diag', ROOT / 'scripts/ci-native-speech-diagnostic.py')
DIAG = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DIAG)


class Boundaries(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name) / 'repo'
        (self.repo / 'voice/src').mkdir(parents=True)
        for name in ['lib.rs', 'memspeech.rs', 'memspeech_diagnostic.rs', 'pcm.rs']:
            (self.repo / 'voice/src' / name).write_text('// synthetic source\n')
        self.exe = Path(self.temp.name) / 'overseer_listener-synthetic'
        self.exe.write_bytes(b'synthetic executable, never launched')
        self.dep = self.exe.with_suffix('.d')
        self.dep.write_text('voice/src/lib.rs voice/src/memspeech.rs voice/src/memspeech_diagnostic.rs\n')
        self.calls = []
        self.fresh = False
        self.source = str(self.repo / 'voice/src/lib.rs')
        self.inventory = f'{DIAG.TEST}: test\n1 test, 0 benchmarks\n'
        self.native_code = 0
        self.native_signal = None
        self.mutate_exe = False

    def capture(self, args, **_):
        self.calls.append(args)
        stdout = b''
        code, signal = 0, None
        if args == ['git', 'rev-parse', 'HEAD']:
            stdout = b'1111111111111111111111111111111111111111\n'
        elif args == ['rustc', '--version']:
            stdout = b'rustc synthetic\n'
        elif args[:2] == ['cargo', 'test']:
            stdout = (json.dumps({'reason':'compiler-artifact','target':{'kind':['lib'],
                'name':'overseer_listener','src_path':self.source},'profile':{'test':True},
                'fresh':self.fresh,'executable':str(self.exe)}) + '\n' +
                json.dumps({'reason':'build-finished','success':True}) + '\n').encode()
        elif '--list' in args:
            stdout = self.inventory.encode()
        elif args[0] == str(self.exe):
            code, signal = self.native_code, self.native_signal
            stdout = b'1 selected test\n'
            if self.mutate_exe:
                self.exe.write_bytes(b'changed synthetic executable')
        return {'code':code,'signal':signal,'timed_out':False,'output_limit':False,
            'reaped':True,'stdout':stdout,'stderr':b'','seconds':0.001}

    def run_diag(self, **overrides):
        inputs = dict(repo=self.repo, out=Path(self.temp.name)/'evidence', invoke_native=True,
            event='workflow_dispatch', selected=True,
            expected_commit='1111111111111111111111111111111111111111', capture=self.capture,
            environment={'ImageOS':'synthetic','ImageVersion':'synthetic'})
        inputs.update(overrides)
        return DIAG.run(**inputs)

    def test_default_and_non_dispatch_refuse_before_any_subprocess(self):
        for fields in [{'invoke_native':False},{'selected':False},{'event':'pull_request'}]:
            with self.subTest(fields=fields), self.assertRaises(DIAG.Refused):
                self.run_diag(**fields)
        self.assertEqual(self.calls, [])

    def test_cached_or_foreign_source_artifact_cannot_start_native(self):
        for fresh, source in [(True,self.source),(False,str(self.repo/'elsewhere/lib.rs'))]:
            self.fresh, self.source = fresh, source
            with self.subTest(fresh=fresh), self.assertRaises(DIAG.Refused):
                self.run_diag()
            self.assertFalse(any(args[0] == str(self.exe) for args in self.calls))
            self.calls.clear()
            # Each run needs its own private leaf, never reused/overwritten.
            if (Path(self.temp.name)/'evidence').exists():
                import shutil
                shutil.rmtree(Path(self.temp.name)/'evidence')

    def test_exact_zero_or_two_inventory_never_starts_native(self):
        for inventory in ['',f'{DIAG.TEST}: test\nother: test\n']:
            self.inventory = inventory
            with self.subTest(inventory=inventory), self.assertRaises(DIAG.Refused):
                self.run_diag()
            self.assertFalse(any(args[0] == str(self.exe) and '--list' not in args for args in self.calls))
            import shutil
            shutil.rmtree(Path(self.temp.name)/'evidence')
            self.calls.clear()

    def test_one_attempt_failure_stays_failed_without_retry(self):
        self.native_code = 101
        receipt = self.run_diag()
        self.assertEqual(receipt['native']['code'], 101)
        self.assertFalse(receipt['success'])
        self.assertEqual(sum(args[0] == str(self.exe) and '--list' not in args for args in self.calls), 1)
        self.assertTrue(all(DIAG.TEST in args for args in self.calls if args[0] == str(self.exe)))

    def test_success_preserves_private_logs_and_allowlisted_environment(self):
        receipt = self.run_diag(environment={'ImageOS':'synthetic','ImageVersion':'synthetic',
            'OWNER_SECRET':'must never appear'})
        self.assertTrue(receipt['success'])
        self.assertEqual(receipt['selected_count'], 1)
        out = Path(self.temp.name)/'evidence'
        self.assertEqual(out.stat().st_mode & 0o777, 0o700)
        for path in out.rglob('*'):
            if path.is_file():
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                self.assertNotIn('must never appear', path.read_text())
        self.assertEqual(receipt['runner_image'], {'ImageOS':'synthetic','ImageVersion':'synthetic'})

    def test_executable_change_after_one_attempt_is_not_a_pass(self):
        self.mutate_exe = True
        receipt = self.run_diag()
        self.assertFalse(receipt['success'])
        self.assertNotEqual(receipt['executable']['sha256_before'],receipt['executable']['sha256_after'])

    def test_existing_output_leaf_is_refused_without_touching_it(self):
        out=Path(self.temp.name)/'evidence';out.mkdir();(out/'owner.txt').write_text('untouched')
        with self.assertRaises(DIAG.Refused):self.run_diag()
        self.assertEqual((out/'owner.txt').read_text(),'untouched')
        self.assertEqual(self.calls,[])


class CaptureBounds(unittest.TestCase):
    # Tiny synthetic Python children only; no Cargo or native speech. Unrun checkpoint.
    def test_timeout_kills_and_reaps_owned_child(self):
        result=DIAG.capture([sys.executable,'-c','import time; time.sleep(10)'],
            cwd=ROOT,environment={},seconds=0.1,limit=128)
        self.assertTrue(result['timed_out'])
        self.assertTrue(result['reaped'])
        self.assertEqual(result['signal'],9)
        self.assertLess(result['seconds'],3)

    def test_output_cap_stops_and_preserves_only_bounded_bytes(self):
        result=DIAG.capture([sys.executable,'-c',"import sys; sys.stdout.write('synthetic'*1024); sys.stdout.flush()"],
            cwd=ROOT,environment={},seconds=2,limit=128)
        self.assertTrue(result['output_limit'])
        self.assertTrue(result['reaped'])
        self.assertEqual(len(result['stdout'])+len(result['stderr']),128)


if __name__ == '__main__':
    unittest.main()
