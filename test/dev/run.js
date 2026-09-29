#!/usr/bin/env node
// Gate T (AC-206, AC-207, AC-208, AC-209's TUI part, AC-211): dev daemons through scripts/dev,
// with a temporary HOME (a temporary "production") and a temporary dev root. The owner's own
// daemon, data, VS Code and logins are never involved.
//   Phase A: two instances side by side; everything scripts/dev does; the default data folder is
//            never created; clean --all leaves nothing.
//   Phase B: a standard daemon runs under the temporary HOME; dev daemons beside it, busy, then
//            cleaned; the standard daemon keeps its pid, socket, state, clients and data, and
//            nothing of it mentions them.
// Run: node test/dev/run.js   (scripts/test-all runs it)
const assert = require('assert');
const fs = require('fs');
const net = require('net');
const os = require('os');
const path = require('path');
const cp = require('child_process');

const repo = path.resolve(__dirname, '../..');
require('../../scripts/git-fallback').ensureGit('dev daemons'); // AC-159
const DEV = path.join(repo, 'scripts/dev');
const realHome = os.homedir();
const tmp = fs.realpathSync(fs.mkdtempSync('/tmp/ovs-dvt-'));
require('../processes').markFolder(tmp); // scripts/test-all's check for leftovers
const results = [];
let current = null;

async function check(name, fn) {
  current = name;
  const t0 = Date.now();
  try { await fn(); results.push({ name, ok: true }); console.log(`ok   ${name} (${Math.round((Date.now() - t0) / 1000)}s)`); } catch (e) { results.push({ name, ok: false }); console.log(`FAIL ${name}\n     ${(e.stack || e.message).split('\n').slice(0, 6).join('\n     ')}`); }
}

/** Runs scripts/dev with the temporary HOME and dev root (cargo and rustup keep their real homes). */
function dev(home, root, args, { json = false, ok = true } = {}) {
  const env = { ...process.env, HOME: home, OVERSEER_DEV_ROOT: root, CARGO_HOME: process.env.CARGO_HOME || path.join(realHome, '.cargo'), RUSTUP_HOME: process.env.RUSTUP_HOME || path.join(realHome, '.rustup') };
  for (const k of Object.keys(env)) if (k.startsWith('OVERSEER_') && k !== 'OVERSEER_DEV_ROOT') delete env[k];
  const r = cp.spawnSync(process.execPath, [DEV, ...args, ...(json ? ['--json'] : [])], { env, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, timeout: 15 * 60 * 1000 });
  if (ok && r.status !== 0) throw new Error(`scripts/dev ${args.join(' ')} exited ${r.status}:\n${r.stdout}\n${r.stderr}`);
  return json && r.status === 0 ? JSON.parse(r.stdout) : r;
}

function call(socket, method, params = {}) {
  return new Promise((resolve, reject) => {
    const s = net.createConnection(socket); let buf = '';
    s.setEncoding('utf8');
    s.on('connect', () => s.write(JSON.stringify({ id: 1, method, params }) + '\n'));
    s.on('data', d => { buf += d; const k = buf.indexOf('\n'); if (k < 0) return; s.destroy(); const m = JSON.parse(buf.slice(0, k)); m.error ? reject(new Error(m.error.message)) : resolve(m.result); });
    s.on('error', reject);
  });
}

function makeRepo(dir) {
  fs.mkdirSync(dir, { recursive: true });
  const git = (...a) => cp.execFileSync('git', a, { cwd: dir, stdio: 'ignore' });
  git('init', '-q', '-b', 'main'); git('config', 'user.name', 'Dev Test'); git('config', 'user.email', 'dev-test@example.invalid'); git('config', 'commit.gpgsign', 'false');
  fs.writeFileSync(path.join(dir, 'a.txt'), 'a\n'); git('add', '.'); git('commit', '-q', '-m', 'base');
  return dir;
}

const pgrep = text => cp.spawnSync('pgrep', ['-f', text], { encoding: 'utf8' }).stdout.split('\n').filter(Boolean).map(Number).filter(p => p !== process.pid);
const envOf = pid => cp.spawnSync('ps', ['-E', '-ww', '-o', 'command=', '-p', String(pid)], { encoding: 'utf8' }).stdout;
const delay = ms => new Promise(r => setTimeout(r, ms));

function listing(dir) {
  const out = [];
  const walk = rel => { for (const n of fs.readdirSync(path.join(dir, rel))) { const r = path.join(rel, n); if (/-(wal|shm|journal)$/.test(n)) continue; out.push(r); if (fs.statSync(path.join(dir, r)).isDirectory()) walk(r); } };
  walk('');
  return out.sort();
}
const mentions = (dir, text) => listing(dir).filter(r => { const f = path.join(dir, r); return fs.statSync(f).isFile() && fs.readFileSync(f).includes(Buffer.from(text)); });

async function snapshot(socket) {
  const hello = await call(socket, 'hello');
  const st = await call(socket, 'state', { include_hidden: true });
  return JSON.stringify({ pid: hello.pid, socket: hello.socket, data: hello.data_dir, instance: hello.instance, tasks: st.tasks, runs: st.runs, profiles: await call(socket, 'profile.list'), clients: await call(socket, 'daemon.clients') });
}

(async () => {
  // ---------------------------------------------------------------- phase A
  const homeA = path.join(tmp, 'ha'); fs.mkdirSync(homeA);
  const rootA = path.join(tmp, 'ra');
  const { COMMANDS, FLAGS, HELP } = require(DEV);
  let a, b;

  await check('AC-211: --help names every command and flag; unknown ones are refused', () => {
    const out = dev(homeA, rootA, ['--help']).stdout;
    assert.strictEqual(out.split('\n').length, HELP.split('\n').length, 'the same help text');
    assert.ok(out.includes(`Dev root: ${rootA}`), 'the help names the dev root in use');
    for (const c of Object.keys(COMMANDS)) assert.ok(new RegExp(`scripts/dev (${c}\\b|help \\| --help)`).test(out), `help names ${c}`);
    for (const f of Object.keys(FLAGS)) assert.ok(out.includes(f), `help names ${f}`);
    for (const topic of ['OVERSEER_DEV_ROOT', '--owner-logins', 'AGENTS.md', 'clean --name NAME (or --all)', 'Never touch the installed (production) daemon']) assert.ok(out.includes(topic), `help covers ${topic}`);
    assert.strictEqual(dev(homeA, rootA, ['up', '--name', 'a', '--bogus'], { ok: false }).status, 2);
    assert.strictEqual(dev(homeA, rootA, ['up', '--name', 'A'], { ok: false }).status, 1);
  });

  await check("AC-211: AGENTS.md's section says when, the command, cleanup and never production", () => {
    const text = fs.readFileSync(path.join(repo, 'AGENTS.md'), 'utf8');
    const sec = text.slice(text.indexOf('## Running a dev Overseer'), text.indexOf('\n## ', text.indexOf('## Running a dev Overseer') + 5));
    for (const s of ['scripts/dev up --name', 'scripts/dev clean --name', 'Never touch the production daemon', 'never deploy', 'scripts/dev --help']) assert.ok(sec.includes(s), `AGENTS.md section has ${s}`);
  });

  await check('AC-206: two instances start side by side from the checkout, each with its own everything', () => {
    a = dev(homeA, rootA, ['up', '--name', 'a'], { json: true });
    b = dev(homeA, rootA, ['up', '--name', 'b', '--no-build'], { json: true });
    for (const [x, name] of [[a, 'a'], [b, 'b']]) {
      assert.strictEqual(x.running, true); assert.strictEqual(x.instance, `dev-${name}`);
      assert.strictEqual(x.repo, fs.realpathSync(repo)); assert.ok(x.commit, 'commit recorded');
      assert.ok(fs.existsSync(path.join(rootA, name, 'bin/overseer-dev-instance')), 'binaries marked dev');
      assert.strictEqual(x.logins, 'fixture');
    }
    assert.notStrictEqual(a.socket, b.socket); assert.notStrictEqual(a.home, b.home);
    assert.notStrictEqual(a.port, b.port); assert.notStrictEqual(a.pid, b.pid);
    assert.ok(a.port >= 47900 && b.port >= 47900, 'never the standard gateway port 47810');
    const again = dev(homeA, rootA, ['up', '--name', 'a', '--no-build'], { json: true });
    assert.strictEqual(again.already_running, true); assert.strictEqual(again.pid, a.pid);
  });

  await check("AC-206: each instance's state holds only its own agent; list and status show both", async () => {
    for (const [x, name] of [[a, 'a'], [b, 'b']]) {
      const r = makeRepo(path.join(tmp, `repo-${name}`));
      dev(homeA, rootA, ['ctl', '--name', name, 'task.create', JSON.stringify({ repo: r, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo hi; sleep 20'], prompt: '', title: `agent of ${name}` })]);
    }
    const ta = JSON.parse(dev(homeA, rootA, ['ctl', '--name', 'a', 'state']).stdout).tasks.map(t => t.title);
    const tb = (await call(b.socket, 'state')).tasks.map(t => t.title);
    assert.deepStrictEqual(ta, ['agent of a']); assert.deepStrictEqual(tb, ['agent of b']);
    const list = dev(homeA, rootA, ['list'], { json: true });
    assert.deepStrictEqual(list.map(s => [s.instance, s.running]), [['dev-a', true], ['dev-b', true]]);
    assert.strictEqual(dev(homeA, rootA, ['status', '--name', 'b'], { json: true }).pid, b.pid);
    assert.ok(dev(homeA, rootA, ['status', '--name', 'a']).stdout.includes('Next:'));
    const env = dev(homeA, rootA, ['env', '--name', 'a']).stdout;
    assert.ok(env.includes(`export OVERSEER_SOCKET='${a.socket}'`) && env.includes("export OVERSEER_INSTANCE='dev-a'"));
  });

  await check("AC-207: a dev daemon's environment: fixture harnesses, no owner logins, no Ollama, its own gateway", () => {
    const e = envOf(a.pid);
    for (const s of ['OVERSEER_INSTANCE=dev-a', `OVERSEER_CLAUDE_PATH=${path.join(rootA, 'a/bin/claude-fixture.js')}`, 'OVERSEER_CODEX_PATH=/nonexistent/', `OVERSEER_TEST_SYSTEM_HOME=${path.join(rootA, 'a/system')}`, 'OVERSEER_OLLAMA_URL=http://127.0.0.1:9', `OVERSEER_GATEWAY_PORT=${a.port}`, 'OVERSEER_GATEWAY_MDNS=off', 'OVERSEER_NOTIFY_COMMAND=']) assert.ok(e.includes(s), `environment has ${s}`);
  });

  await check("AC-240: with an unfocused VS Code window, a fixture permission, finish and failure each write one entry to the instance's notifications.log with the agent's title and its click URL; a focused window writes none", async () => {
    const log = path.join(rootA, 'a/notifications.log');
    const entries = title => (fs.existsSync(log) ? fs.readFileSync(log, 'utf8') : '').split('\n').filter(l => l.split('\t')[1] === title);
    // A VS Code window as the extension speaks: hello, then whether it has the OS focus.
    const window = focused => new Promise((resolve, reject) => {
      const c = net.createConnection(a.socket); let buf = '', n = 0;
      c.setEncoding('utf8');
      c.on('connect', () => c.write(JSON.stringify({ id: 1, method: 'hello', params: { client: 'vscode' } }) + '\n' + JSON.stringify({ id: 2, method: 'ui.window', params: { focused } }) + '\n'));
      c.on('data', d => { buf += d; n = buf.split('\n').length - 1; if (n >= 2) resolve(c); });
      c.on('error', reject);
    });
    const r = makeRepo(path.join(tmp, 'repo-notices'));
    const mode = path.join(rootA, 'a/fixture-mode');
    const wait = async (id, re) => { for (let i = 0; i < 100; i++) { const run = (await call(a.socket, 'state')).runs.find(x => x.id === id); if (re.test(run?.status || '')) return run.status; await delay(200); } throw new Error(`${id} never reached ${re}`); };
    const focusedWindow = await window(true);
    fs.writeFileSync(mode, 'echo');
    const quiet = (await call(a.socket, 'task.create', { repo: r, harness: 'claude', prompt: 'hi', title: 'Finished while focused' })).run.id;
    await wait(quiet, /completed/); await delay(800);
    assert.deepStrictEqual(entries('Finished while focused'), [], 'a focused window writes none');
    focusedWindow.destroy();
    const background = await window(false);
    fs.writeFileSync(mode, 'permission');
    const asks = (await call(a.socket, 'task.create', { repo: r, harness: 'claude', prompt: 'write perm.txt', title: 'Write the changelog' })).run.id;
    await wait(asks, /waiting_for_user/);
    fs.writeFileSync(mode, 'echo');
    const done = (await call(a.socket, 'task.create', { repo: r, harness: 'claude', prompt: 'hi', title: 'Summarise the notes' })).run.id;
    await wait(done, /completed/);
    const failed = (await call(a.socket, 'task.create', { repo: r, harness: 'generic', program: '/bin/sh', args: ['-c', 'exit 2'], prompt: '', title: 'Broken build' })).run.id;
    await wait(failed, /failed/);
    for (let i = 0; i < 30 && !entries('Broken build').length; i++) await delay(200);
    await delay(800);
    for (const [title, id, what] of [['Write the changelog', asks, 'Needs your permission to use Write'], ['Summarise the notes', done, 'Finished'], ['Broken build', failed, 'Stopped with an error']]) {
      const got = entries(title);
      assert.strictEqual(got.length, 1, `one entry for ${title}: ${JSON.stringify(got)}`);
      const [, , body, url] = got[0].split('\t');
      assert.ok(body.startsWith(what), `${title}: ${body}`);
      assert.strictEqual(url, `vscode://beelol.overseer/open-agent?run=${id}`);
    }
    background.destroy();
    await call(a.socket, 'run.interrupt', { run_id: asks });
  });

  await check('AC-209: tui --dry-run resolves the instance; a stopped instance is refused', () => {
    const t = JSON.parse(dev(homeA, rootA, ['tui', '--name', 'a', '--dry-run']).stdout);
    assert.deepStrictEqual(t.command, [path.join(rootA, 'a/bin/overseer-tui'), '--daemon', path.join(rootA, 'a/bin/overseerd')]);
    assert.strictEqual(t.env.OVERSEER_SOCKET, a.socket);
    dev(homeA, rootA, ['down', '--name', 'b']);
    const r = dev(homeA, rootA, ['tui', '--name', 'b', '--dry-run'], { ok: false });
    assert.strictEqual(r.status, 1); assert.ok(r.stderr.includes('dev-b is not running'), r.stderr);
    assert.strictEqual(dev(homeA, rootA, ['status', '--name', 'b'], { json: true }).running, false);
    assert.ok(!fs.existsSync(b.socket), 'socket removed');
    assert.deepStrictEqual(pgrep(path.join(rootA, 'b') + '/'), [], "nothing runs from b's folder");
    assert.strictEqual(dev(homeA, rootA, ['status', '--name', 'a'], { json: true }).running, true, 'a is untouched');
  });

  await check('AC-206, AC-211: clean --all leaves no process, socket or folder; the default data folder was never created', () => {
    dev(homeA, rootA, ['clean', '--all']);
    assert.ok(!fs.existsSync(rootA), 'dev root removed');
    assert.deepStrictEqual(pgrep(rootA), []);
    for (const p of ['Library/Application Support/Overseer', 'Library/LaunchAgents', 'Library/Application Support/Code', '.vscode', '.overseer-dev']) assert.ok(!fs.existsSync(path.join(homeA, p)), `${p} not created under HOME`);
  });

  // ---------------------------------------------------------------- phase B
  const homeB = path.join(tmp, 'hb'); fs.mkdirSync(homeB);
  const rootB = path.join(tmp, 'rb');
  const bin = path.join(repo, 'target/debug/overseerd');
  const stdEnv = { ...process.env, HOME: homeB, OVERSEER_CONTINUITY_PROBES: 'off', OVERSEER_CODEX_PATH: '/nonexistent/x', OVERSEER_CLAUDE_PATH: '/nonexistent/x', OVERSEER_OPENCODE_PATH: '/nonexistent/x' };
  for (const k of ['OVERSEER_HOME', 'OVERSEER_SOCKET', 'OVERSEER_INSTANCE']) delete stdEnv[k];
  let std, stdSocket, stdData, before, filesBefore;

  await check('a standard daemon runs under the temporary HOME (the "production" of this test)', async () => {
    stdSocket = cp.execFileSync(bin, ['socket-path'], { env: stdEnv, encoding: 'utf8' }).trim();
    std = cp.spawn(bin, ['serve'], { env: stdEnv, stdio: 'ignore' });
    for (let k = 0; k < 150; k++) { try { await call(stdSocket, 'hello'); break; } catch { await delay(100); } }
    const hello = await call(stdSocket, 'hello');
    stdData = hello.data_dir;
    assert.strictEqual(hello.instance, null);
    if (process.platform === 'darwin') assert.strictEqual(stdData, path.join(homeB, 'Library/Application Support/Overseer'));
    before = await snapshot(stdSocket); filesBefore = listing(stdData);
  });

  await check('AC-207, AC-208: dev daemons beside it, busy, then cleaned, leave the standard daemon as it was', async () => {
    const c = dev(homeB, rootB, ['up', '--name', 'c', '--no-build'], { json: true });
    const d = dev(homeB, rootB, ['up', '--name', 'd', '--no-build'], { json: true });
    for (const [x, name] of [[c, 'c'], [d, 'd']]) {
      assert.notStrictEqual(x.socket, stdSocket); assert.notStrictEqual(x.home, stdData);
      const r = makeRepo(path.join(tmp, `repo-${name}`));
      await call(x.socket, 'task.create', { repo: r, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo busy; sleep 20'], prompt: '', title: `busy ${name}` });
    }
    await delay(800);
    assert.strictEqual(await snapshot(stdSocket), before, 'while dev daemons run');
    dev(homeB, rootB, ['down', '--name', 'd']);
    dev(homeB, rootB, ['clean', '--all']);
    assert.strictEqual(await snapshot(stdSocket), before, 'after they are cleaned');
    assert.deepStrictEqual(listing(stdData), filesBefore, "the standard data folder's files");
    for (const trace of [rootB, 'dev-c', 'dev-d']) assert.deepStrictEqual(mentions(stdData, trace), [], `nothing in the standard data folder mentions ${trace}`);
    assert.ok(!fs.existsSync(path.join(homeB, 'Library/LaunchAgents')), 'no launch agent');
    for (const p of ['Library/Application Support/Code', '.vscode']) assert.ok(!fs.existsSync(path.join(homeB, p)), `no ${p}`);
  });

  await check('AC-208: the daemon and TUI never read the dev root', () => {
    const grep = cp.spawnSync('grep', ['-rlE', 'OVERSEER_DEV_ROOT|\\.overseer-dev', path.join(repo, 'daemon/src'), path.join(repo, 'tui/src')], { encoding: 'utf8' });
    assert.strictEqual(grep.stdout.trim(), '', grep.stdout);
  });

  // ---------------------------------------------------------------- cleanup
  try { await call(stdSocket, 'daemon.shutdown'); } catch {}
  for (let k = 0; k < 50 && std && std.exitCode === null; k++) await delay(100);
  try { std?.kill('SIGKILL'); } catch {}
  for (const root of [rootA, rootB]) if (fs.existsSync(root)) dev(root === rootA ? homeA : homeB, root, ['clean', '--all'], { ok: false });
  const left = pgrep(tmp);
  for (const p of left) { try { process.kill(p, 'SIGKILL'); } catch {} }
  await check('AC-211: nothing is left running', () => assert.deepStrictEqual(left, []));
  fs.rmSync(tmp, { recursive: true, force: true });
  const passed = results.filter(r => r.ok).length;
  console.log(`${passed} of ${results.length} passed`);
  process.exit(passed === results.length ? 0 : 1);
})();
