// Packaged-UI scenario for AC-209 (no paid tokens): two dev daemons, A and B, each with its own
// agent; `scripts/dev code --name a` opens an isolated VS Code pinned to A. It shows A's agent and
// not B's, reads "Overseer dev-a" in the status bar and "[dev-a]" in the title; A counts it as a
// client and B does not. When A stops (down --keep-clients) the window says A is not running and
// nothing connects to B or starts a daemon; when A comes back, the window reconnects on its own.
// The dev root is temporary; the owner's daemon, VS Code and data are never involved.
const fs = require('fs');
const net = require('net');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

const DEV = path.join(repoRoot, 'scripts/dev');

function call(socket, method, params = {}) {
  return new Promise((resolve, reject) => {
    const c = net.createConnection(socket); let buf = '';
    c.setEncoding('utf8');
    c.on('connect', () => c.write(JSON.stringify({ id: 1, method, params }) + '\n'));
    c.on('data', d => { buf += d; const k = buf.indexOf('\n'); if (k < 0) return; c.destroy(); const m = JSON.parse(buf.slice(0, k)); m.error ? reject(new Error(m.error.message)) : resolve(m.result); });
    c.on('error', reject);
  });
}

(async () => {
  const s = new Session('dev-instance');
  const root = path.join(s.root, 'dr');
  const env = { ...process.env, OVERSEER_DEV_ROOT: root };
  for (const k of Object.keys(env)) if (k.startsWith('OVERSEER_') && k !== 'OVERSEER_DEV_ROOT' && k !== 'OVERSEER_CODE') delete env[k];
  const dev = (...args) => {
    const r = cp.spawnSync(process.execPath, [DEV, ...args], { env, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
    s.note(`$ scripts/dev ${args.join(' ')} → exit ${r.status}`, (r.stdout + r.stderr).trim().split('\n').slice(-6).join(' | '));
    if (r.status !== 0) throw new Error(`scripts/dev ${args.join(' ')} failed: ${r.stderr}`);
    return r.stdout;
  };
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const a = JSON.parse(dev('up', '--name', 'a', '--json'));
    const b = JSON.parse(dev('up', '--name', 'b', '--no-build', '--json'));
    const repoA = makeRepo(path.join(s.root, 'shop-a'), { dirty: false });
    const repoB = makeRepo(path.join(s.root, 'shop-b'), { dirty: false });
    await call(a.socket, 'task.create', { repo: repoA, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo working on A; sleep 300'], prompt: '', title: 'Agent of A' });
    await call(b.socket, 'task.create', { repo: repoB, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo working on B; sleep 300'], prompt: '', title: 'Agent of B' });

    dev('code', '--name', 'a', repoA, '--vsix', latestVsix(), '--inspect');
    // The harness's helpers work on the instance's own profile (scripts/dev made it).
    s.profile = path.join(root, 'a/vscode/profile');
    s.extensions = path.join(root, 'a/vscode/extensions');
    const cdp = await s.connect();
    await s.openOverseerView();
    await cdp.waitFor(`[...document.querySelectorAll('.monaco-list-row .label-name')].some(e => e.textContent.trim() === 'Agent of A')`, 30000, "A's agent");
    await delay(1500);
    const rows = (await s.agentRows()).map(r => r.label);
    check("the pinned window lists A's agent and not B's", rows.includes('Agent of A') && !rows.includes('Agent of B'), rows);
    const statusText = () => cdp.evalWorkbench(`[...document.querySelectorAll('.statusbar-item')].map(e => e.textContent.trim()).find(t => /Overseer/.test(t)) || ''`);
    const title = await cdp.evalWorkbench('document.title');
    const bar = await statusText();
    check('the status bar reads "Overseer dev-a" and the title starts with [dev-a]', /Overseer dev-a/.test(bar) && title.startsWith('[dev-a]'), { bar, title });
    await s.screenshot('pinned-to-a');
    const clients = { a: (await call(a.socket, 'daemon.clients')).ui, b: (await call(b.socket, 'daemon.clients')).ui };
    check('A counts the window as a client; B does not', clients.a >= 1 && clients.b === 0, clients);
    const settings = JSON.parse(fs.readFileSync(path.join(s.profile, 'User/settings.json'), 'utf8'));
    check('the profile pins the window (overseer.daemonPath, daemonSocket, devInstance) and lives in the instance folder, not the owner\'s',
      settings['overseer.daemonSocket'] === a.socket && settings['overseer.devInstance'] === 'dev-a' && settings['overseer.daemonPath'] === path.join(root, 'a/bin/overseerd') && s.profile.startsWith(root), settings);

    // A stops; the window stays open and waits.
    dev('down', '--name', 'a', '--keep-clients');
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer dev-a not running/.test(e.textContent))`, 20000, 'the not-running status');
    await delay(4000);
    const toast = await cdp.evalWorkbench(`[...document.querySelectorAll('.notification-toast, .notifications-list-container .monaco-list-row')].map(e => e.textContent.trim()).join(' | ')`);
    await s.screenshot('a-not-running');
    const aUp = cp.spawnSync('pgrep', ['-f', `${path.join(root, 'a/bin/overseerd')} serve`], { encoding: 'utf8' }).stdout.trim();
    check('when A stops the window says so and names scripts/dev up, and no daemon is started for it',
      /Dev daemon dev-a is not running/.test(toast) && /scripts\/dev up --name a/.test(toast) && !aUp && !fs.existsSync(a.socket), { toast, status: await statusText(), started: aUp });
    check('nothing connects to B meanwhile', (await call(b.socket, 'daemon.clients')).ui === 0);

    // A comes back (the dev loop: rebuild, up): the open window reconnects by itself.
    dev('up', '--name', 'a', '--no-build');
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer dev-a \\d+ active/.test(e.textContent))`, 30000, 'reconnected');
    await s.screenshot('a-back');
    check('when A is up again the window reconnects on its own', (await call(a.socket, 'daemon.clients')).ui >= 1, await statusText());
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    try { await s.quit(); } catch {}
    try { dev('clean', '--all'); } catch (e) { s.note('clean failed: ' + e.message); }
    const left = cp.spawnSync('pgrep', ['-f', root], { encoding: 'utf8' }).stdout.trim();
    if (left) { s.note('left running after clean', left); result.error = result.error || 'processes left running'; }
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
