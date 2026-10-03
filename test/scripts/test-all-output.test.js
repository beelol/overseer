'use strict';
// Exercise the real test-all stage function at its child-process boundary.
// Controlled captured output avoids launching Cargo/UI or taking the machine lock.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const vm = require('node:vm');
const { test } = require('node:test');
const scriptPath = path.resolve(__dirname, '../../scripts/test-all');
const helperPath = path.resolve(__dirname, '../../scripts/test-output.js');

function stage(t, result, opts = {}, prepare = () => {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'test-all-output-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  prepare(root);
  const script = fs.readFileSync(scriptPath, 'utf8');
  const start = script.indexOf('function run(');
  const end = script.indexOf('\n/**', start);
  assert(start >= 0 && end > start, 'actual stage function must be available');
  // Baseline has neither helper nor evidence writer. Its actual run() still
  // executes, so missing files/failure details are assertion failures, not setup.
  const output = fs.existsSync(helperPath) ? require(helperPath) : undefined;
  const evidence = output ? output.createEvidence(root, 'ta-synthetic') : undefined;
  const lines = [];
  const context = { root, env: {}, results: [], output, evidence,
    Date, Buffer, console: { log: (...args) => lines.push(args.join(' ')) },
    process: { stdout: { write: text => lines.push(text) } },
    cp: { spawnSync: () => result } };
  vm.runInNewContext(`${script.slice(start, end)}\nrun('Synthetic Rust', 'synthetic-only', [], options);`,
    { ...context, options: opts }, { timeout: 1000 });
  const base = path.join(root, '.test-all-logs', 'ta-synthetic');
  const dirs = fs.existsSync(base) ? fs.readdirSync(base).map(name => path.join(base, name)) : [];
  return { root, lines: lines.join('\n'), dirs, result: context.results[0] };
}

const successes = Array.from({ length: 20 }, (_, i) => `test credential_error_case_${i} ... ok`).join('\n');
const failing = `${successes}\ntest actual_delayed_origin_boundary ... FAILED\n\nfailures:\n\n---- actual_delayed_origin_boundary stdout ----\nthread 'actual_delayed_origin_boundary' panicked at tests/native.rs:90:5:\nassertion failed: no replacement-session proposal\n  left: 1\n right: 0\n\nfailures:\n    actual_delayed_origin_boundary\n\ntest result: FAILED. 20 passed; 1 failed; 0 ignored\n`;

test('failing test and panic context survive many successful error-named cases', t => {
  const run = stage(t, { status: 101, signal: null, stdout: failing, stderr: '' });
  assert.equal(run.result.ok, false);
  assert.match(run.lines, /test actual_delayed_origin_boundary \.\.\. FAILED/);
  assert.match(run.lines, /panicked at tests\/native\.rs:90:5/);
  assert.match(run.lines, /assertion failed: no replacement-session proposal/);
  assert.doesNotMatch(run.lines, /credential_error_case_\d+ \.\.\. ok/);
});

test('failed stage preserves separate complete raw streams in a recoverable directory', t => {
  const stdout = `${failing}after-summary witness\n`;
  const stderr = 'warning: synthetic compiler warning\n';
  const run = stage(t, { status: 101, signal: null, stdout, stderr });
  assert.equal(run.dirs.length, 1, 'exact stage evidence directory');
  const dir = run.dirs[0];
  assert.equal(fs.readFileSync(path.join(dir, 'stdout.log'), 'utf8'), stdout);
  assert.equal(fs.readFileSync(path.join(dir, 'stderr.log'), 'utf8'), stderr);
  assert(run.lines.includes(dir), 'failure prints actual recoverable log location');
  for (const file of ['stdout.log', 'stderr.log', 'result.json']) {
    assert.equal(fs.statSync(path.join(dir, file)).mode & 0o777, 0o600);
  }
  assert.equal(fs.statSync(dir).mode & 0o777, 0o700);
  assert.equal(fs.statSync(path.dirname(dir)).mode & 0o777, 0o700);
});

test('spawn refusal is actionable even without stdout or stderr', t => {
  const error = Object.assign(new Error('synthetic executable unavailable'), { code: 'ENOENT' });
  const run = stage(t, { status: null, signal: null, error, stdout: '', stderr: '' });
  assert.equal(run.result.ok, false);
  assert.match(run.lines, /ENOENT/);
  assert.equal(run.dirs.length, 1);
  const receipt = JSON.parse(fs.readFileSync(path.join(run.dirs[0], 'result.json'), 'utf8'));
  assert.equal(receipt.status, null);
  assert.equal(receipt.spawn_error, 'ENOENT');
  assert.equal('env' in receipt, false);
  assert.equal('args' in receipt, false);
});

test('buffer termination records partial streams and signal instead of implying complete evidence', t => {
  const error = Object.assign(new Error('synthetic capture exceeded maxBuffer'), { code: 'ENOBUFS' });
  const run = stage(t, { status: null, signal: 'SIGTERM', error, stdout: 'partial stdout\n', stderr: 'partial stderr\n' });
  assert.equal(run.result.ok, false);
  assert.match(run.lines, /ENOBUFS/);
  assert.match(run.lines, /SIGTERM/);
  assert.equal(run.dirs.length, 1);
  const dir = run.dirs[0];
  assert.equal(fs.readFileSync(path.join(dir, 'stdout.log'), 'utf8'), 'partial stdout\n');
  assert.equal(fs.readFileSync(path.join(dir, 'stderr.log'), 'utf8'), 'partial stderr\n');
  const receipt = JSON.parse(fs.readFileSync(path.join(dir, 'result.json'), 'utf8'));
  assert.equal(receipt.signal, 'SIGTERM');
  assert.equal(receipt.spawn_error, 'ENOBUFS');
  assert.equal(receipt.capture_complete, false);
});


test('raw evidence preserves binary bytes rather than UTF8 replacement characters', t => {
  const run = stage(t, { status: 1, signal: null, stdout: Buffer.from([255, 0, 65]), stderr: Buffer.from([254, 10]) });
  assert.equal(run.dirs.length, 1);
  assert.deepEqual(fs.readFileSync(path.join(run.dirs[0], 'stdout.log')), Buffer.from([255, 0, 65]));
  assert.deepEqual(fs.readFileSync(path.join(run.dirs[0], 'stderr.log')), Buffer.from([254, 10]));
});

test('unsafe evidence destination fails stage without writing outside its private directory', t => {
  const run = stage(t, { status: 0, signal: null, stdout: 'test result: ok\n', stderr: '' }, {}, root => {
    const other = path.join(root, 'outside');
    fs.mkdirSync(other);
    fs.symlinkSync(other, path.join(root, '.test-all-logs'));
  });
  assert.equal(run.result.ok, false, 'lost or unsafe evidence cannot silently pass');
  assert.match(run.lines, /raw evidence unavailable: EACCES/);
  assert.deepEqual(fs.readdirSync(path.join(run.root, 'outside')), []);
});

test('parallel capture enforces one aggregate byte ceiling and keeps each stream separate', t => {
  const output = require(helperPath);
  const captured = output.capture(5);
  captured.add('stdout', Buffer.from([255, 0, 65, 66]));
  captured.add('stderr', Buffer.from([67, 68, 69]));
  captured.add('stdout', Buffer.from([70, 71]));
  const result = captured.result();
  assert.deepEqual(result.stdout, Buffer.from([255, 0, 65, 66]));
  assert.deepEqual(result.stderr, Buffer.from([67]));
  assert.equal(result.captureTruncated, true);
});
