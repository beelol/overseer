#!/usr/bin/env node
// Evaluation of Codex's own local mode (`codex exec --oss --local-provider ollama`) as a second
// local harness (Continuity, AC-87), on this machine. The REAL Codex and a REAL Ollama; no
// account and no paid tokens: Codex is given an empty CODEX_HOME of its own, so the owner's
// login is neither read nor written.
//
// Codex would load a model itself, outside Overseer's guard. So every model is first loaded by
// the daemon (`local.load`: the guard on fresh memory, then the watchdog), under a tag that sets
// its context, and Codex is pointed at that loaded tag. The order of work is the catalogue's:
// one model at a time, smallest first, memory before and after, unloaded after its check, and
// the run stops if the system reports memory pressure.
//
//   node test/local/codex-oss-eval.js [tag ...]
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, spawnSync, execFileSync } = require('child_process');

const root = path.resolve(__dirname, '../..');
const bin = process.env.OVERSEERD || path.join(root, 'target/debug/overseerd');
const out = process.env.OUT || path.join(root, 'docs/verification/evidence/ac-87');
const home = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-codex-oss-'));
const env = { ...process.env, OVERSEER_HOME: path.join(home, 'overseer') };
const ATTEMPTS = 3;
const G = 2 ** 30;
const gib = b => Math.round((b / G) * 10) / 10;
const sleep = ms => new Promise(r => setTimeout(r, ms));
const log = [];
const say = line => { console.log(line); log.push(line); };
const redact = s => s.split(fs.realpathSync(home)).join('/ISOLATED').split(home).join('/ISOLATED').split(os.homedir()).join('~').replace(new RegExp(`(?<![A-Za-z0-9])${os.userInfo().username}(?![A-Za-z0-9])`, 'g'), 'USER').replace(/(\/private)?\/var\/folders\/[A-Za-z0-9_]+\/[A-Za-z0-9_]+\/T\//g, '/TMP/');
function ctl(method, params) {
  const r = spawnSync(bin, ['ctl', method, JSON.stringify(params || {})], { env, encoding: 'utf8', timeout: 900000, maxBuffer: 64 * 1024 * 1024 });
  const msg = JSON.parse(r.stdout.split('\n')[0] || '{"error":{"message":"no answer"}}');
  return msg.error ? { error: msg.error.message } : msg.result;
}
const memory = () => { const m = ctl('local.inventory'); return { available: m.memory.available, pressure: m.memory.pressure, level: m.memory.level, loaded: m.loaded.map(l => ({ tag: l.tag, gib: gib(l.size), context: l.context })) }; };
const mem = m => `${gib(m.available)} GiB available, pressure ${m.pressure}, level ${m.level}%, loaded: ${m.loaded.map(l => `${l.tag} (${l.gib} GiB at ${Math.round((l.context || 0) / 1024)}k)`).join(', ') || 'nothing'}`;
function repo(name) {
  const dir = path.join(home, name);
  fs.mkdirSync(dir, { recursive: true });
  const git = (...a) => execFileSync('git', a, { cwd: dir, encoding: 'utf8' });
  git('init', '-q', '-b', 'main'); git('config', 'user.name', 'T'); git('config', 'user.email', 't@example.invalid'); git('config', 'commit.gpgsign', 'false');
  fs.writeFileSync(path.join(dir, 'README.md'), '# fixture\n'); git('add', '.'); git('commit', '-q', '-m', 'base');
  return dir;
}

/// One Codex turn on the loaded tag. Memory is sampled while it runs; pressure stops it.
function codex(program, tag, dir, prompt, samples) {
  return new Promise(resolve => {
    const codexHome = path.join(home, 'codex-home');
    fs.mkdirSync(codexHome, { recursive: true });
    const clean = { HOME: path.join(home, 'codex-user'), PATH: process.env.PATH, TMPDIR: process.env.TMPDIR || '/tmp', CODEX_HOME: codexHome, LANG: 'en_US.UTF-8' };
    fs.mkdirSync(clean.HOME, { recursive: true });
    const args = ['exec', '--json', '--oss', '--local-provider', 'ollama', '-m', tag, '-s', 'workspace-write', '--skip-git-repo-check', '-C', dir, '--', prompt];
    const child = spawn(program, args, { env: clean, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '', stderr = '', pressure = null;
    child.stdout.on('data', d => { stdout += d; });
    child.stderr.on('data', d => { stderr += d; });
    const watch = setInterval(() => {
      const m = memory();
      samples.push({ available: m.available, pressure: m.pressure, level: m.level, loaded: m.loaded.map(l => l.tag) });
      if (m.pressure !== 'normal') { pressure = m.pressure; child.kill('SIGINT'); }
    }, 2000);
    const limit = setTimeout(() => child.kill('SIGINT'), 300000);
    child.on('close', code => { clearInterval(watch); clearTimeout(limit); resolve({ code, stdout, stderr, pressure, args }); });
  });
}

(async () => {
  const daemon = spawn(bin, ['serve'], { env, stdio: 'ignore' });
  const results = [];
  const mine = new Set();
  let stopped = null;
  try {
    for (let i = 0; i < 100 && ctl('hello').error; i++) await sleep(100);
    const program = ctl('harness.list').find(h => h.harness === 'codex');
    const status = ctl('continuity.status');
    const inventory = ctl('local.inventory');
    const wanted = process.argv.slice(2);
    const models = ctl('local.catalogue').models.filter(e => !wanted.length || wanted.includes(e.tag)).map(e => ({ tag: e.tag, tier: e.tier, max_context: e.max_context, installed: inventory.models.find(m => m.tag === e.tag) })).filter(m => m.installed).sort((a, b) => a.installed.size - b.installed.size);
    say(`overseerd ${ctl('hello').version}; Codex ${program.version} with --oss --local-provider ollama (no account: its own empty CODEX_HOME); Ollama ${status.ollama.version}; ${os.type()} ${os.release()} ${os.arch()}`);
    say(`memory: ${gib(status.memory.total)} GiB installed; budget now ${gib(status.budget.budget)} GiB`);
    say(`order (smallest first): ${models.map(m => `${m.tag} ${gib(m.installed.size)} GiB`).join(', ')}`);
    for (const model of models) {
      say(`\n== ${model.tag}`);
      const result = { tag: model.tag, harness: 'codex --oss', harness_version: program.version, ollama: status.ollama.version, attempts: [] };
      results.push(result);
      const before = memory();
      result.memory_before = before;
      say(`    memory before: ${mem(before)}`);
      if (before.pressure !== 'normal') { stopped = `memory pressure is ${before.pressure} before ${model.tag}`; break; }
      if (before.loaded.some(l => mine.has(l.tag))) { stopped = `a model of an earlier check is still loaded before ${model.tag}`; break; }
      // The longest context of the usual steps that the guard allows, loaded by the daemon.
      let loaded = null;
      for (const context of [65536, 32768, 16384].filter(c => c <= model.max_context)) {
        const r = ctl('local.load', { tag: model.tag, context });
        if (!r.error) { loaded = { context, run_tag: r.detail.run_tag, bytes: (r.detail.measured || {}).size || r.detail.approved.bytes, budget: r.detail.approved.budget.budget }; break; }
        result.refused = r.error;
      }
      if (!loaded) { result.status = 'not run'; result.note = result.refused; say(`    the guard refused it: ${result.refused}`); continue; }
      mine.add(loaded.run_tag);
      Object.assign(result, loaded);
      say(`    the guard allowed ${loaded.run_tag} at a ${loaded.context / 1024}k context: ${gib(loaded.bytes)} GiB of a ${gib(loaded.budget)} GiB budget; loaded by the daemon under the watchdog`);
      const samples = [];
      for (let n = 1; n <= ATTEMPTS; n++) {
        const dir = repo(`repo-${model.tag.replace(/[^a-z0-9]+/gi, '-')}-${n}`);
        const text = `verified ${n}`;
        const started = Date.now();
        const r = await codex(program.program, loaded.run_tag, dir, `Create a file named check-${n}.txt in the current directory containing exactly: ${text}. Then reply with the single word done.`, samples);
        const file = path.join(dir, `check-${n}.txt`);
        const written = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : null;
        const lines = r.stdout.split('\n').filter(Boolean).map(l => { try { return JSON.parse(l); } catch { return null; } }).filter(Boolean);
        const items = lines.filter(l => l.type === 'item.completed').map(l => l.item);
        const attempt = { n, exit: r.code, seconds: Math.round((Date.now() - started) / 100) / 10, file: written, items: items.map(i => i.type), reply: (items.filter(i => i.type === 'agent_message').pop() || {}).text || null, errors: [...lines.filter(l => l.type === 'error' || l.type === 'turn.failed').map(l => String(l.message || (l.error || {}).message)), ...items.filter(i => i.type === 'error').map(i => String(i.message || i.text || JSON.stringify(i)))].map(e => e.slice(0, 240)), stderr: r.stderr.trim().split('\n').slice(-2).join(' | ').slice(0, 300) };
        attempt.ok = r.code === 0 && written !== null && written.trim() === text;
        result.attempts.push(attempt);
        if (n === 1) result.command = `codex ${r.args.slice(0, -1).join(' ')} <prompt>`;
        say(`    attempt ${n}: ${attempt.ok ? 'PASS' : 'FAIL'}  exit ${r.code} in ${attempt.seconds} s; file: ${written === null ? 'missing' : JSON.stringify(written)}; items: ${attempt.items.join(', ') || 'none'}; reply: ${JSON.stringify((attempt.reply || '').slice(0, 80))}${attempt.errors.length ? '; ' + attempt.errors[0] : ''}${!lines.length && attempt.stderr ? '; ' + attempt.stderr : ''}`);
        if (r.pressure) { stopped = `memory pressure became ${r.pressure} during ${model.tag}`; break; }
      }
      const during = memory();
      result.loaded_during = during.loaded;
      say(`    loaded while Codex worked: ${during.loaded.map(l => `${l.tag} (${l.gib} GiB at ${Math.round((l.context || 0) / 1024)}k)`).join(', ') || 'nothing'}`);
      // Everything Codex or the daemon loaded for this model goes.
      for (const l of during.loaded.filter(l => l.tag === loaded.run_tag || l.tag === model.tag || !before.loaded.some(b => b.tag === l.tag))) {
        ctl('local.unload', { tag: l.tag });
        for (let i = 0; i < 80 && memory().loaded.some(x => x.tag === l.tag); i++) await sleep(250);
      }
      await sleep(1500);
      const after = memory();
      result.memory_after = after;
      result.lowest_available = samples.length ? Math.min(...samples.map(s => s.available)) : null;
      result.lowest_level = samples.length ? Math.min(...samples.map(s => s.level)) : null;
      result.pressure_seen = [...new Set(samples.map(s => s.pressure))];
      result.largest_loaded = Math.max(0, ...[during, ...[]].flatMap(m => m.loaded.map(l => l.gib)));
      const passed = result.attempts.filter(a => a.ok).length;
      result.status = passed === ATTEMPTS ? 'passed' : 'failed';
      result.note = `${passed} of ${ATTEMPTS} write checks${passed === ATTEMPTS ? '' : ': ' + [...new Set(result.attempts.filter(a => !a.ok).map(a => a.errors[0] || (a.file === null ? 'no file was written' : 'the file has other content')))].join('; ')}`;
      say(`    memory while it worked: lowest ${gib(result.lowest_available || 0)} GiB available, lowest level ${result.lowest_level}%, pressure ${result.pressure_seen.join(', ') || 'not sampled'}`);
      say(`    memory after: ${mem(after)}`);
      say(`    RESULT ${model.tag}: ${result.status} (${result.note})`);
      if (after.loaded.some(l => !before.loaded.some(b => b.tag === l.tag))) { stopped = `${model.tag} could not be unloaded`; break; }
      if (stopped) break;
    }
    if (stopped) say(`\nSTOPPED: ${stopped}`);
  } catch (e) {
    say(`\nSTOPPED: ${e.message}`);
    stopped = e.message;
  } finally {
    for (const tag of mine) if (memory().loaded.some(l => l.tag === tag)) ctl('local.unload', { tag });
    say(`\nat the end: ${mem(memory())}`);
    say(`summary: ${results.map(r => `${r.tag} ${r.status}`).join('; ')}`);
    fs.mkdirSync(out, { recursive: true });
    const name = process.argv.slice(2).length ? `codex-oss-${process.argv.slice(2).join('+').replace(/[^a-z0-9.+]+/gi, '-')}` : 'codex-oss';
    fs.writeFileSync(path.join(out, `${name}.txt`), redact(log.join('\n')) + '\n');
    fs.writeFileSync(path.join(out, `${name}.json`), redact(JSON.stringify({ on: new Date().toISOString().slice(0, 10), stopped, results }, null, 2)) + '\n');
    ctl('daemon.shutdown');
    daemon.kill();
    spawnSync('/usr/bin/pkill', ['-f', home]);
    fs.rmSync(home, { recursive: true, force: true });
    process.exitCode = stopped ? 1 : 0;
  }
})();
