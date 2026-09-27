#!/usr/bin/env node
// Catalogue verification (Continuity, AC-87 and AC-140) on this machine: every model of the
// shipped catalogue that is installed goes through the REAL local harness (OpenCode's server
// through Overseer's bridge) and a REAL Ollama, with a real `overseerd` in its own OVERSEER_HOME.
// No account and no paid tokens.
//
// Memory safety is the daemon's own guard, which every run passes before anything is loaded, and
// this script's order of work:
//   - one model at a time, smallest first;
//   - memory is recorded before and after each model, and sampled while it works;
//   - the model is unloaded after its check, and the next one starts only when it is gone;
//   - a model the guard refuses is not run, and its reason is the result;
//   - a model this script did not load is never unloaded;
//   - the run stops at once if the system reports memory pressure.
//
// The check is the catalogue's: a one-turn task that must create a file with the write tool, in
// a disposable repository, three times. A model passes with three of three.
//
//   node test/local/catalogue-verify.js [tag ...]
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, spawnSync, execFileSync } = require('child_process');

const root = path.resolve(__dirname, '../..');
const bin = process.env.OVERSEERD || path.join(root, 'target/debug/overseerd');
const out = process.env.OUT || path.join(root, 'docs/verification/evidence/ac-87');
const home = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-catalogue-'));
const env = { ...process.env, OVERSEER_HOME: home };
const ATTEMPTS = 3;
const G = 2 ** 30;
const gib = b => Math.round((b / G) * 10) / 10;
const sleep = ms => new Promise(r => setTimeout(r, ms));
const log = [];
const say = line => { console.log(line); log.push(line); };
const redact = s => s.split(fs.realpathSync(home)).join('/OVERSEER_HOME').split(home).join('/OVERSEER_HOME').split(os.homedir()).join('~').replace(new RegExp(`(?<![A-Za-z0-9])${os.userInfo().username}(?![A-Za-z0-9])`, 'g'), 'USER').replace(/(\/private)?\/var\/folders\/[A-Za-z0-9_]+\/[A-Za-z0-9_]+\/T\//g, '/TMP/');
function ctl(method, params) {
  const r = spawnSync(bin, ['ctl', method, JSON.stringify(params || {})], { env, encoding: 'utf8', timeout: 900000, maxBuffer: 64 * 1024 * 1024 });
  const msg = JSON.parse(r.stdout.split('\n')[0] || '{"error":{"message":"no answer"}}');
  return msg.error ? { error: msg.error.message } : msg.result;
}
const state = () => ctl('state');
const run = id => state().runs.find(r => r.id === id);
const events = id => ctl('events.list', { run_id: id, limit: 5000 }).events;
const memory = () => { const m = ctl('local.inventory'); return { available: m.memory.available, pressure: m.memory.pressure, level: m.memory.level, loaded: m.loaded.map(l => ({ tag: l.tag, gib: gib(l.size), context: l.context })) }; };
const mem = m => `${gib(m.available)} GiB available, pressure ${m.pressure}, level ${m.level}%, loaded: ${m.loaded.map(l => `${l.tag} (${l.gib} GiB)`).join(', ') || 'nothing'}`;

function repo(name) {
  const dir = path.join(home, name);
  fs.mkdirSync(dir, { recursive: true });
  const git = (...a) => execFileSync('git', a, { cwd: dir, encoding: 'utf8' });
  git('init', '-q', '-b', 'main'); git('config', 'user.name', 'T'); git('config', 'user.email', 't@example.invalid'); git('config', 'commit.gpgsign', 'false');
  fs.writeFileSync(path.join(dir, 'README.md'), '# fixture\n'); git('add', '.'); git('commit', '-q', '-m', 'base');
  return dir;
}

(async () => {
  const daemon = spawn(bin, ['serve'], { env, stdio: 'ignore' });
  const results = [];
  const mine = new Set();
  let stopped = null;
  try {
    for (let i = 0; i < 100 && ctl('hello').error; i++) await sleep(100);
    const harness = ctl('harness.list').find(h => h.harness === 'opencode-serve');
    const status = ctl('continuity.status');
    const inventory = ctl('local.inventory');
    const catalogue = ctl('local.catalogue').models;
    const wanted = process.argv.slice(2);
    const installed = tag => inventory.models.find(m => m.tag === tag);
    const models = catalogue.filter(e => !wanted.length || wanted.includes(e.tag)).map(e => ({ tag: e.tag, tier: e.tier, size: (installed(e.tag) || {}).size || e.disk_bytes, installed: !!installed(e.tag), tools: ((installed(e.tag) || {}).capabilities || []).includes('tools') })).sort((a, b) => a.size - b.size);
    say(`overseerd ${ctl('hello').version}; OpenCode ${harness.version} through the opencode-serve harness; Ollama ${status.ollama.version}; ${os.type()} ${os.release()} ${os.arch()}`);
    say(`memory: ${gib(status.memory.total)} GiB installed; budget now ${gib(status.budget.budget)} GiB (${status.budget.ceiling_percent}% of total is ${gib(status.budget.ceiling_share)} GiB; headroom ${gib(status.budget.headroom)} GiB)`);
    say(`order (smallest first): ${models.map(m => `${m.tag} ${gib(m.size)} GiB${m.installed ? '' : ' (not installed)'}`).join(', ')}`);
    const others = memory().loaded;
    if (others.length) say(`loaded by someone else before the start, and left alone: ${others.map(l => l.tag).join(', ')}`);

    for (const model of models) {
      say(`\n== ${model.tag} (tier ${model.tier}, ${gib(model.size)} GiB on disk)`);
      const result = { tag: model.tag, tier: model.tier, disk_bytes: model.size, attempts: [], harness: 'opencode-serve', harness_version: harness.version, ollama: status.ollama.version };
      results.push(result);
      if (!model.installed) { result.status = 'not run'; result.note = 'not installed'; say('    not installed: not run'); continue; }
      if (!model.tools) { result.status = 'failed'; result.note = 'Ollama does not report tool calling for it'; say('    no tool calling: not run'); continue; }
      const before = memory();
      result.memory_before = before;
      say(`    memory before: ${mem(before)}`);
      if (before.pressure !== 'normal') { stopped = `memory pressure is ${before.pressure} before ${model.tag}`; break; }
      if (before.loaded.some(l => mine.has(l.tag))) { stopped = `a model of an earlier check is still loaded before ${model.tag}`; break; }
      const samples = [];
      let loadedTags = [];
      for (let n = 1; n <= ATTEMPTS; n++) {
        const dir = repo(`repo-${model.tag.replace(/[^a-z0-9]+/gi, '-')}-${n}`);
        const text = `verified ${n}`;
        const prompt = `Use the write tool to create a file named check-${n}.txt in the current directory containing exactly: ${text}. Then reply with the single word done.`;
        const started = Date.now();
        const c = ctl('task.create', { repo: dir, harness: 'opencode-serve', model: `ollama/${model.tag}`, permission_mode: 'auto', title: `${model.tag} check ${n}`, prompt });
        const attempt = { n };
        result.attempts.push(attempt);
        if (c.error || c.launch_error) {
          attempt.ok = false; attempt.refused = c.error || c.launch_error;
          say(`    attempt ${n}: not launched: ${attempt.refused}`);
          break;
        }
        const id = c.run.id;
        for (const e of events(id).filter(e => e.kind === 'local_load' && e.payload.run_tag && !e.payload.error)) { mine.add(e.payload.run_tag); loadedTags.push(e.payload.run_tag); }
        const chosen = events(id).find(e => e.kind === 'local_model');
        if (chosen) { result.run_tag = chosen.payload.model.replace('ollama/', ''); result.context = chosen.payload.context; result.loaded_bytes = chosen.payload.bytes; result.budget = chosen.payload.budget.budget; }
        if (n === 1 && chosen) say(`    the guard allowed ${result.run_tag} at a ${result.context / 1024}k context: ${gib(result.loaded_bytes)} GiB of a ${gib(result.budget)} GiB budget${chosen.payload.already_loaded ? ' (already loaded)' : ', loaded under the watchdog'}`);
        let r;
        for (;;) {
          r = run(id);
          const m = memory();
          samples.push({ at: Date.now(), available: m.available, pressure: m.pressure, level: m.level, loaded: m.loaded.map(l => l.tag) });
          if (m.pressure !== 'normal') { ctl('run.interrupt', { run_id: id }); stopped = `memory pressure became ${m.pressure} during ${model.tag}`; break; }
          if (!['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status)) break;
          if (Date.now() - started > 300000) { ctl('run.interrupt', { run_id: id }); attempt.timeout = true; await sleep(2000); r = run(id); break; }
          await sleep(500);
        }
        const ws = state().workspaces.find(w => w.id === c.workspace.id);
        const file = path.join(ws.path, `check-${n}.txt`);
        const written = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : null;
        const ev = events(id);
        attempt.status = r.status;
        attempt.reason = r.exit_reason;
        attempt.seconds = Math.round((Date.now() - started) / 100) / 10;
        attempt.file = written;
        attempt.tools = ev.filter(e => e.kind === 'tool').map(e => e.payload.summary || e.payload.name);
        attempt.replies = ev.filter(e => e.kind === 'output' && e.payload.role === 'assistant').map(e => e.payload.text.slice(0, 300));
        attempt.notes = ev.filter(e => e.kind === 'output' && e.payload.role === 'system').map(e => e.payload.text.slice(0, 300));
        attempt.errors = ev.filter(e => e.kind === 'error').map(e => `${e.payload.class}: ${String(e.payload.message).slice(0, 200)}`);
        const usage = ev.filter(e => e.kind === 'usage').pop();
        attempt.tokens = usage ? usage.payload.tokens : null;
        attempt.ok = r.status === 'completed' && written !== null && written.trim() === text;
        say(`    attempt ${n}: ${attempt.ok ? 'PASS' : 'FAIL'}  ${r.status} in ${attempt.seconds} s; file: ${written === null ? 'missing' : JSON.stringify(written)}; tools: ${attempt.tools.length}; reply: ${JSON.stringify((attempt.replies.pop() || '').slice(0, 80))}${attempt.errors.length ? '; ' + attempt.errors.join('; ') : ''}${attempt.timeout ? '; stopped after 300 s' : ''}`);
        if (stopped) break;
      }
      // Unload what this check loaded, and wait until it is gone.
      for (const tag of [...new Set(loadedTags)]) {
        ctl('local.unload', { tag });
        for (let i = 0; i < 80 && memory().loaded.some(l => l.tag === tag); i++) await sleep(250);
      }
      await sleep(1500);
      const after = memory();
      result.memory_after = after;
      result.unloaded = !after.loaded.some(l => mine.has(l.tag));
      result.lowest_available = samples.length ? Math.min(...samples.map(s => s.available)) : null;
      result.lowest_level = samples.length ? Math.min(...samples.map(s => s.level)) : null;
      result.pressure_seen = [...new Set(samples.map(s => s.pressure))];
      result.one_at_a_time = samples.every(s => s.loaded.filter(t => mine.has(t)).length <= 1);
      const passed = result.attempts.filter(a => a.ok).length;
      const refused = result.attempts.find(a => a.refused);
      result.status = refused ? 'not run' : passed === ATTEMPTS ? 'passed' : 'failed';
      result.note = refused ? refused.refused : `${passed} of ${ATTEMPTS} write checks${passed === ATTEMPTS ? '' : ': ' + [...new Set(result.attempts.filter(a => !a.ok).map(a => a.errors[0] || a.reason || (a.file === null ? 'no file was written' : 'the file has other content')))].join('; ')}`;
      say(`    memory while it worked: lowest ${gib(result.lowest_available || 0)} GiB available, lowest level ${result.lowest_level}%, pressure ${result.pressure_seen.join(', ') || 'not sampled'}; one model of this check loaded at a time: ${result.one_at_a_time}`);
      say(`    memory after: ${mem(after)}; unloaded: ${result.unloaded}`);
      say(`    RESULT ${model.tag}: ${result.status} (${result.note})`);
      if (!result.unloaded) { stopped = `${model.tag} could not be unloaded`; break; }
      if (stopped) break;
    }
    if (stopped) say(`\nSTOPPED: ${stopped}`);
  } catch (e) {
    say(`\nSTOPPED: ${e.message}`);
    stopped = e.message;
  } finally {
    for (const r of (state().runs || []).filter(r => ['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status))) ctl('run.interrupt', { run_id: r.id });
    for (const tag of mine) if (memory().loaded.some(l => l.tag === tag)) ctl('local.unload', { tag });
    const end = memory();
    say(`\nat the end: ${mem(end)}`);
    say(`tags of Overseer's own in Ollama (they share the weights and take no disk): ${ctl('local.inventory').models.map(m => m.tag).filter(t => t.startsWith('overseer/')).join(', ') || 'none'}`);
    say(`summary: ${results.map(r => `${r.tag} ${r.status}`).join('; ')}`);
    fs.mkdirSync(out, { recursive: true });
    const name = process.argv.slice(2).length ? `opencode-serve-${process.argv.slice(2).join('+').replace(/[^a-z0-9.+]+/gi, '-')}` : 'opencode-serve';
    fs.writeFileSync(path.join(out, `${name}.txt`), redact(log.join('\n')) + '\n');
    fs.writeFileSync(path.join(out, `${name}.json`), redact(JSON.stringify({ on: new Date().toISOString().slice(0, 10), stopped, results }, null, 2)) + '\n');
    ctl('daemon.shutdown');
    daemon.kill();
    spawnSync('/usr/bin/pkill', ['-f', home]);
    fs.rmSync(home, { recursive: true, force: true });
    process.exitCode = stopped ? 1 : 0;
  }
})();
