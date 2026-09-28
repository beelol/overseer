#!/usr/bin/env node
// AC-214: scripts/deploy onto a temporary production (HOME, VS Code profile and extensions folder
// all temporary; the notifier is not registered with macOS). Two commits, A and B, of this
// repository are built by the deploy itself in its own clone:
//   1. deploy A: installed, the daemon started, build A running and recorded;
//   2. deploy B with a run active and --no-wait: stops, says so, changes nothing;
//   3. deploy B while a run is active: waits without restarting; once the run ends it installs B
//      and restarts the daemon once;
//   4. roll back to A: data (tasks, runs, profiles) and logins intact; --status shows it.
// The owner's own daemon, VS Code and data are never involved. Run: node test/deploy/run.js
const assert = require('assert');
const crypto = require('crypto');
const fs = require('fs');
const net = require('net');
const path = require('path');
const cp = require('child_process');

const repo = path.resolve(__dirname, '../..');
require('../../scripts/git-fallback').ensureGit('deploy'); // AC-159
const DEPLOY = path.join(repo, 'scripts/deploy');
const realHome = require('os').homedir();
const tmp = fs.realpathSync(fs.mkdtempSync('/tmp/ovs-dpl-'));
const home = path.join(tmp, 'home');
const profile = path.join(tmp, 'code/profile');
const extensions = path.join(tmp, 'code/ext');
const cache = path.join(repo, 'target/deploy-test-cache'); // kept between runs: later builds are incremental
const results = [];
const delay = ms => new Promise(r => setTimeout(r, ms));

async function check(name, fn) {
  const t0 = Date.now();
  try { await fn(); results.push(true); console.log(`ok   ${name} (${Math.round((Date.now() - t0) / 1000)}s)`); } catch (e) { results.push(false); console.log(`FAIL ${name}\n     ${(e.stack || e.message).split('\n').slice(0, 8).join('\n     ')}`); }
}

const env = (() => {
  const e = { ...process.env, HOME: home, OVERSEER_DEPLOY_CACHE: cache, CARGO_HOME: process.env.CARGO_HOME || path.join(realHome, '.cargo'), RUSTUP_HOME: process.env.RUSTUP_HOME || path.join(realHome, '.rustup'), npm_config_cache: process.env.npm_config_cache || path.join(realHome, '.npm') };
  for (const k of Object.keys(e)) if (k.startsWith('OVERSEER_') && k !== 'OVERSEER_DEPLOY_CACHE' && k !== 'OVERSEER_CODE') delete e[k];
  return e;
})();
const target = ['--user-data-dir', profile, '--extensions-dir', extensions, '--skip-notifier-registration'];
function deploy(args) { return cp.spawnSync(process.execPath, [DEPLOY, ...args, ...target], { env, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, timeout: 30 * 60 * 1000 }); }
const ok = r => { if (r.status !== 0) throw new Error(`scripts/deploy exited ${r.status}:\n${r.stdout}\n${r.stderr.split('\n').slice(-25).join('\n')}`); return r; };

function call(socket, method, params = {}) {
  return new Promise((resolve, reject) => {
    const s = net.createConnection(socket); let buf = '';
    s.setEncoding('utf8');
    s.on('connect', () => s.write(JSON.stringify({ id: 1, method, params }) + '\n'));
    s.on('data', d => { buf += d; const k = buf.indexOf('\n'); if (k < 0) return; s.destroy(); const m = JSON.parse(buf.slice(0, k)); m.error ? reject(new Error(m.error.message)) : resolve(m.result); });
    s.on('error', reject);
  });
}
const git = (cwd, ...a) => cp.execFileSync('git', a, { cwd, encoding: 'utf8' }).trim();
const sha = f => crypto.createHash('sha256').update(fs.readFileSync(f)).digest('hex');
const daemonBin = () => { const d = fs.readdirSync(extensions).find(n => n.startsWith('beelol.overseer-')); return path.join(extensions, d, 'bin', `overseerd-${process.platform}-${process.arch}`); };
const installedBuild = () => (cp.execFileSync(daemonBin(), ['version'], { encoding: 'utf8' }).match(/build ([^)\s]+)/) || [])[1];
const history = () => JSON.parse(fs.readFileSync(path.join(home, 'Library/Application Support/Overseer/deploys/history.json'), 'utf8'));

(async () => {
  fs.mkdirSync(home, { recursive: true });
  // Two commits: A (this repository's HEAD) and B (A plus one change), in a source repository of their own.
  const src = path.join(tmp, 'src');
  fs.mkdirSync(src);
  git(src, 'init', '-q', '-b', 'main');
  git(src, 'fetch', '-q', repo, 'HEAD');
  git(src, 'checkout', '-q', '-b', 'main-a', 'FETCH_HEAD');
  git(src, 'config', 'user.name', 'Deploy Test'); git(src, 'config', 'user.email', 'deploy-test@example.invalid'); git(src, 'config', 'commit.gpgsign', 'false');
  const A = git(src, 'rev-parse', '--short=12', 'HEAD');
  fs.appendFileSync(path.join(src, 'README.md'), '\n<!-- deploy test: commit B -->\n');
  git(src, 'commit', '-q', '-am', 'deploy test: commit B');
  const B = git(src, 'rev-parse', '--short=12', 'HEAD');
  // Logins the deploy must never touch.
  const logins = [path.join(home, '.codex/auth.json'), path.join(home, '.claude/.credentials.json')];
  for (const f of logins) { fs.mkdirSync(path.dirname(f), { recursive: true }); fs.writeFileSync(f, JSON.stringify({ token: crypto.randomBytes(16).toString('hex') })); }
  const loginHashes = logins.map(sha);
  let socket, pidA;

  await check('deploy A: builds it in its own clone, installs it, starts the daemon, records it', async () => {
    const r = JSON.parse(ok(deploy(['--yes', '--json', '--repo', src, '--ref', A, '--no-fetch'])).stdout);
    assert.strictEqual(r.build, A); assert.strictEqual(r.restarted, false);
    socket = cp.execFileSync(daemonBin(), ['socket-path'], { env, encoding: 'utf8' }).trim();
    const hello = await call(socket, 'hello');
    assert.strictEqual(hello.build, A); assert.strictEqual(hello.instance, null);
    pidA = hello.pid;
    assert.strictEqual(installedBuild(), A);
    assert.deepStrictEqual(history().map(h => h.build), [A]);
    assert.ok(fs.existsSync(path.join(home, 'Library/Application Support/Overseer', history()[0].vsix)), 'the VSIX is kept');
  });

  const work = path.join(tmp, 'work');
  fs.mkdirSync(work); git(work, 'init', '-q', '-b', 'main'); git(work, 'config', 'user.name', 'T'); git(work, 'config', 'user.email', 't@example.invalid'); git(work, 'config', 'commit.gpgsign', 'false');
  fs.writeFileSync(path.join(work, 'a.txt'), 'a\n'); git(work, 'add', '.'); git(work, 'commit', '-q', '-m', 'base');
  let runId;

  await check('with a run active and --no-wait it stops, says how many, and changes nothing', async () => {
    runId = (await call(socket, 'task.create', { repo: work, harness: 'generic', program: '/bin/sh', args: ['-c', 'sleep 1200'], prompt: '', title: 'long run' })).run.id;
    for (let k = 0; k < 50 && !(await call(socket, 'run.active')).length; k++) await delay(100);
    const r = deploy(['--yes', '--repo', src, '--ref', B, '--no-fetch', '--no-wait']);
    assert.strictEqual(r.status, 3, r.stderr.slice(-800));
    assert.ok(/1 run is active on the installed Overseer; nothing was changed/.test(r.stderr), r.stderr.slice(-400));
    assert.strictEqual((await call(socket, 'hello')).pid, pidA, 'not restarted');
    assert.strictEqual(installedBuild(), A, 'not installed');
    assert.strictEqual(history().length, 1, 'not recorded');
  });

  await check('while a run is active it waits without restarting; when the run ends it installs B and restarts once', async () => {
    const child = cp.spawn(process.execPath, [DEPLOY, '--yes', '--json', '--repo', src, '--ref', B, '--no-fetch', '--wait', '20', '--poll', '1', ...target], { env });
    let out = '', err = '';
    child.stdout.on('data', d => { out += d; }); child.stderr.on('data', d => { err += d; });
    const exited = new Promise(r => child.on('close', r));
    const pids = new Set();
    const watch = setInterval(() => call(socket, 'hello').then(h => pids.add(h.pid), () => {}), 150);
    for (let k = 0; k < 20 * 60 * 10 && !/waiting for quiet/.test(err); k++) { if (child.exitCode !== null) break; await delay(100); }
    assert.ok(/1 run is active; waiting for quiet/.test(err), err.slice(-600));
    await delay(3000);
    assert.strictEqual((await call(socket, 'hello')).pid, pidA, 'no restart while the run is active');
    assert.strictEqual(installedBuild(), A, 'nothing installed while waiting');
    await call(socket, 'run.interrupt', { run_id: runId });
    const code = await exited;
    clearInterval(watch);
    assert.strictEqual(code, 0, err.slice(-800));
    const r = JSON.parse(out);
    assert.strictEqual(r.build, B); assert.strictEqual(r.restarted, true); assert.strictEqual(r.previous_pid, pidA);
    const hello = await call(socket, 'hello');
    assert.strictEqual(hello.build, B);
    pids.add(hello.pid);
    assert.deepStrictEqual([...pids].sort(), [pidA, hello.pid].sort(), 'exactly one restart');
    assert.strictEqual(installedBuild(), B);
    assert.deepStrictEqual(history().map(h => h.build), [A, B]);
  });

  await check('--rollback brings A back; data and logins intact; --status shows it', async () => {
    const snap = async () => { const st = await call(socket, 'state', { include_hidden: true }); return JSON.stringify({ tasks: st.tasks.map(t => [t.id, t.title]), runs: st.runs.map(r => [r.id, r.status, r.exit_reason]), profiles: await call(socket, 'profile.list') }); };
    const data = path.join(home, 'Library/Application Support/Overseer');
    const files = d => fs.readdirSync(d).filter(n => !/-(wal|shm|journal)$/.test(n)).sort();
    const before = await snap(), filesBefore = files(data);
    const r = JSON.parse(ok(deploy(['--yes', '--json', '--rollback'])).stdout);
    assert.strictEqual(r.build, A); assert.strictEqual(r.rollback_of, B); assert.strictEqual(r.restarted, true);
    assert.strictEqual((await call(socket, 'hello')).build, A);
    assert.strictEqual(installedBuild(), A);
    assert.strictEqual(await snap(), before, 'tasks, runs and profiles unchanged');
    for (const f of filesBefore) assert.ok(fs.existsSync(path.join(data, f)), `${f} kept`);
    assert.deepStrictEqual(logins.map(sha), loginHashes, 'login files byte-identical');
    const st = JSON.parse(ok(deploy(['--status', '--json'])).stdout);
    assert.strictEqual(st.deployed.build, A); assert.strictEqual(st.deployed.rollback_of, B); assert.strictEqual(st.running.build, A); assert.strictEqual(st.deploys, 3);
  });

  await check('it refuses to run unattended without --yes', () => {
    const r = cp.spawnSync(process.execPath, [DEPLOY, '--rollback', ...target], { env, encoding: 'utf8', input: '' });
    assert.strictEqual(r.status, 2); assert.ok(/Run it yourself, or pass --yes when the owner asked/.test(r.stderr), r.stderr);
  });

  // Cleanup: the temporary production daemon and anything else from this test.
  try { await call(socket, 'daemon.stop_all'); } catch {}
  await delay(1500);
  const left = cp.spawnSync('pgrep', ['-f', tmp], { encoding: 'utf8' }).stdout.split('\n').filter(Boolean).map(Number);
  for (const p of left) { try { process.kill(p, 'SIGKILL'); } catch {} }
  await check('nothing is left running', () => assert.deepStrictEqual(left, []));
  fs.rmSync(tmp, { recursive: true, force: true });
  const passed = results.filter(Boolean).length;
  console.log(`${passed} of ${results.length} passed`);
  process.exit(passed === results.length ? 0 : 1);
})();
