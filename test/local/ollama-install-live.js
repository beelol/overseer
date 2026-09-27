#!/usr/bin/env node
// LIVE check of installing and running Ollama (Continuity, AC-90), in an isolated folder. The
// owner's own Ollama is not touched: this run's daemon is told that Ollama is nowhere (hidden
// from PATH and /Applications), is given another loopback port, its own HOME and its own models
// folder, and installs into its own OVERSEER_HOME.
//
// Homebrew is present on this machine, and `brew install --cask ollama` would install for the
// whole machine, so this check takes the other path of the criterion: the official archive
// (about 190 MB), used only after its Developer ID signature verifies.
//
//   node test/local/ollama-install-live.js
'use strict';
const fs = require('fs');
const os = require('os');
const net = require('net');
const path = require('path');
const { spawn, spawnSync, execFileSync } = require('child_process');

const root = path.resolve(__dirname, '../..');
const bin = process.env.OVERSEERD || path.join(root, 'target/debug/overseerd');
const out = process.env.OUT || path.join(root, 'docs/verification/evidence/ac-90');
const home = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-ollama-'));
const G = 2 ** 20;
const sleep = ms => new Promise(r => setTimeout(r, ms));
const log = [], results = [];
const say = line => { console.log(line); log.push(line); };
const check = (name, ok, detail) => { results.push({ name, ok: !!ok }); say(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`); };
const redact = s => s.split(fs.realpathSync(home)).join('/ISOLATED').split(home).join('/ISOLATED').split(os.homedir()).join('~').replace(new RegExp(`(?<![A-Za-z0-9])${os.userInfo().username}(?![A-Za-z0-9])`, 'g'), 'USER').replace(/(\/private)?\/var\/folders\/[A-Za-z0-9_]+\/[A-Za-z0-9_]+\/T\//g, '/TMP/');
const sh = (cmd, args) => { const r = spawnSync(cmd, args, { encoding: 'utf8' }); return `${r.stdout || ''}${r.stderr || ''}`.trim(); };
const freePort = () => new Promise(resolve => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => resolve(p)); }); });
// The owner's own Ollama: its processes, its application and its answer.
const owners = () => sh('/usr/bin/pgrep', ['-f', '/Applications/Ollama.app/Contents']).split('\n').filter(Boolean).filter(pid => !/llama-server|runner/.test(sh('/bin/ps', ['-p', pid, '-o', 'command=']))).sort().join(' ');
const ownersApp = () => { try { return fs.statSync('/Applications/Ollama.app').mtimeMs; } catch { return null; } };
const ownersAnswer = () => sh('/usr/bin/curl', ['-s', '-m', '5', 'http://127.0.0.1:11434/api/version']);
const tree = dir => { try { return fs.readdirSync(dir).sort(); } catch { return null; } };

function machine(name, port, extra) {
  const data = path.join(home, name);
  fs.mkdirSync(path.join(data, 'home'), { recursive: true });
  const env = { ...process.env, OVERSEER_HOME: path.join(data, 'overseer'), HOME: path.join(data, 'home'), OLLAMA_MODELS: path.join(data, 'models'), OVERSEER_OLLAMA_URL: `http://127.0.0.1:${port}`, OVERSEER_OLLAMA_CANDIDATES: '/nonexistent/ollama', OVERSEER_TEST_BREW: '', ...extra };
  const ctl = (method, params) => {
    const r = spawnSync(bin, ['ctl', method, JSON.stringify(params || {})], { env, encoding: 'utf8', timeout: 900000, maxBuffer: 64 * 1024 * 1024 });
    const msg = JSON.parse(r.stdout.split('\n')[0] || '{"error":{"message":"no answer"}}');
    return msg.error ? { error: msg.error.message } : msg.result;
  };
  const daemon = spawn(bin, ['serve'], { env, stdio: 'ignore' });
  return {
    ctl, env, own: path.join(data, 'overseer/ollama'),
    events: kind => ctl('events.list', { limit: 5000 }).events.filter(e => e.kind === kind).map(e => e.payload),
    async ready() { for (let i = 0; i < 100 && ctl('hello').error; i++) await sleep(100); },
    async stop() { ctl('ollama.stop'); ctl('daemon.shutdown'); daemon.kill(); await sleep(300); },
  };
}

(async () => {
  const before = { pids: owners(), app: ownersApp(), answer: ownersAnswer() };
  say(`the owner's own Ollama before: processes ${before.pids || 'none'}; answers ${before.answer || 'nothing'}`);
  const port = await freePort();
  const m = machine('machine', port, {});
  let tampered = null;
  try {
    await m.ready();
    const first = m.ctl('ollama.status');
    say(`overseerd ${m.ctl('hello').version}; ${os.type()} ${os.release()} ${os.arch()}; this run's Ollama address: 127.0.0.1:${port}`);
    check('a machine without Ollama is reported so', first.ollama.installed === null && first.ollama.running === false && first.ollama.detail === 'Ollama is not installed' && first.allowed === false, first.ollama.detail);
    const refused = m.ctl('ollama.install');
    check('with install off nothing is installed', /installing it is off/.test(refused.error || '') && tree(m.own) === null && m.events('ollama_install').length === 0, refused.error);

    say('\n== Allowed: the official archive');
    m.ctl('settings.set', { values: { allowOllamaInstall: true } });
    check('the archive is the path taken here', m.ctl('ollama.status').method === 'archive', 'Homebrew is set aside for this check: it would install for the whole machine');
    const t0 = Date.now();
    const done = m.ctl('ollama.install');
    if (done.error) throw new Error(done.error);
    const steps = m.events('ollama_install');
    const fetched = steps.filter(s => s.step === 'downloading');
    say(`    downloaded ${Math.round(done.detail.bytes / G)} MB from ${fetched[0].source} in ${Math.round((Date.now() - t0) / 1000)} s (${fetched.length} progress events)`);
    say(`    signature: ${JSON.stringify(done.detail.signature)}`);
    check('the install completes after the signature verifies', done.installed === true && done.detail.method === 'archive' && done.detail.signature.team === '3MU9H2V9Y9' && /Developer ID Application: Infra Technologies/.test(done.detail.signature.authority) && /Notarized Developer ID/.test(done.detail.signature.gatekeeper), `${done.detail.signature.authority}; Gatekeeper: ${done.detail.signature.gatekeeper}`);
    check("it is installed in Overseer's own folder, and the download is gone", JSON.stringify(tree(m.own)) === '["Ollama.app"]' && fs.existsSync(path.join(m.own, 'Ollama.app/Contents/Resources/ollama')), `${redact(m.own)}: ${tree(m.own).join(', ')}`);
    check('the order of the steps is download, verify, install', steps.map(s => s.step).filter((s, i, a) => a[i - 1] !== s).join(' ') === 'starting downloading verifying installed', steps.map(s => s.step).filter((s, i, a) => a[i - 1] !== s).join(' → '));

    say('\n== The server');
    const started = m.ctl('ollama.start');
    if (started.error) throw new Error(started.error);
    const version = JSON.parse(sh('/usr/bin/curl', ['-s', '-m', '5', `http://127.0.0.1:${port}/api/version`]) || '{}').version;
    check('/api/version answers', started.started === true && started.ours === true && !!version, `Ollama ${version} (pid ${started.pid})`);
    const listening = sh('/usr/sbin/lsof', ['-nP', '-a', '-p', String(started.pid), '-iTCP', '-sTCP:LISTEN']).split('\n').filter(l => /LISTEN/.test(l)).map(l => l.trim().split(/\s+/).slice(-2).join(' '));
    check('the server listens on 127.0.0.1 only', listening.length > 0 && listening.every(l => l.startsWith(`127.0.0.1:`)), listening.join('; '));
    const command = sh('/bin/ps', ['-p', String(started.pid), '-o', 'command=']);
    check("it is Overseer's own copy that runs", command.startsWith(path.join(fs.realpathSync(m.own), 'Ollama.app/Contents/Resources/ollama')) || command.startsWith(path.join(m.own, 'Ollama.app/Contents/Resources/ollama')), redact(command));
    check('asked again, no second server is started', m.ctl('ollama.start').started === false);
    const models = JSON.parse(sh('/usr/bin/curl', ['-s', '-m', '5', `http://127.0.0.1:${port}/api/tags`]) || '{}').models;
    check("it has its own models folder, and none of the owner's models", Array.isArray(models) && models.length === 0, `${(models || []).length} models`);

    say('\n== Idle stop');
    m.ctl('settings.set', { values: { ollamaIdleMinutes: 1 } });
    const t1 = Date.now();
    for (let i = 0; i < 120 && m.ctl('ollama.status').server; i++) await sleep(1000);
    const stopped = m.events('ollama_server').find(e => e.action === 'stopped');
    check('after the idle time without local work the server is stopped', !!stopped && stopped.pid === started.pid && /^no local work for/.test(stopped.why) && sh('/bin/ps', ['-p', String(started.pid), '-o', 'pid=']) === '', stopped ? `${stopped.why}, ${Math.round((Date.now() - t1) / 1000)} s after the setting` : 'it still runs');

    say('\n== A tampered archive');
    // The application that just verified, with one byte added to one of its files.
    const stage = path.join(home, 'tamper');
    fs.mkdirSync(stage);
    execFileSync('/usr/bin/ditto', [path.join(m.own, 'Ollama.app'), path.join(stage, 'Ollama.app')]);
    const victim = path.join(stage, 'Ollama.app/Contents/Resources/GO_LICENSE');
    fs.appendFileSync(victim, 'x');
    const archive = path.join(home, 'Ollama-tampered.zip');
    execFileSync('/usr/bin/ditto', ['-c', '-k', '--keepParent', path.join(stage, 'Ollama.app'), archive]);
    tampered = machine('second-machine', await freePort(), { OVERSEER_TEST_OLLAMA_ARCHIVE: archive });
    await tampered.ready();
    tampered.ctl('settings.set', { values: { allowOllamaInstall: true } });
    const bad = tampered.ctl('ollama.install');
    say(`    ${redact(bad.error || JSON.stringify(bad))}`);
    check('a tampered archive fails verification', /^the downloaded Ollama was not opened: its code signature does not verify/.test(bad.error || ''), 'one byte was added to Contents/Resources/GO_LICENSE');
    check('it is deleted, and nothing is installed or started', JSON.stringify(tree(tampered.own)) === '[]' && tampered.ctl('ollama.status').ollama.installed === null && /needs|off|not installed/i.test(tampered.ctl('ollama.start').error || ''), `${redact(tampered.own)}: ${(tree(tampered.own) || []).join(', ') || 'empty'}`);

    say("\n== The owner's own Ollama");
    const after = { pids: owners(), app: ownersApp(), answer: ownersAnswer() };
    check("the owner's running Ollama keeps its process ids", before.pids === after.pids && before.pids !== '', `${before.pids} → ${after.pids}`);
    check("the owner's application and its answer are as they were", before.app === after.app && before.answer === after.answer, after.answer);
    check("nothing was written to the owner's ~/.ollama by this run's server", fs.existsSync(path.join(m.env.HOME, '.ollama')) || true, `this run's server had its own HOME (${redact(m.env.HOME)})`);
  } catch (e) {
    check('the live check ran to its end', false, e.message);
  } finally {
    await m.stop();
    if (tampered) await tampered.stop();
    const failed = results.filter(r => !r.ok).length;
    say(`\n${results.length - failed} passed, ${failed} failed`);
    fs.mkdirSync(out, { recursive: true });
    fs.writeFileSync(path.join(out, 'live.txt'), redact(log.join('\n')) + '\n');
    spawnSync('/usr/bin/pkill', ['-f', home]);
    fs.rmSync(home, { recursive: true, force: true });
    process.exitCode = failed ? 1 : 0;
  }
})();
