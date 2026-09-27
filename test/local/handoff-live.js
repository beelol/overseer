#!/usr/bin/env node
// LIVE checks of keeping the work going (Continuity, AC-84 and AC-91) on this machine, with a
// real `overseerd` in its own OVERSEER_HOME.
//
//   local      AC-91. Codex is a fixture that fails on the network, and the system's answer is a
//              fixture that says "no network" (the machine's own network stays on). The handoff
//              goes to the REAL OpenCode and a REAL local model through Ollama: the daemon's own
//              pick, inside the memory budget, through the guard. No account, no paid tokens.
//   failover   AC-84. The REAL Codex with its hosts blocked for its own process only (every
//              connection goes to a loopback port nothing listens on), and the REAL Claude Code
//              with the existing login and its smallest model. The probe answers are a fixture
//              that says OpenAI cannot be reached, because this machine can reach it. Two tiny
//              Claude turns; Codex never reaches its provider.
//
// A model this run did not load is never unloaded. Nothing of the user's is written.
//
//   node test/local/handoff-live.js [local] [failover]
'use strict';
const fs = require('fs');
const os = require('os');
const net = require('net');
const path = require('path');
const { spawn, spawnSync, execFileSync } = require('child_process');

const root = path.resolve(__dirname, '../..');
const bin = process.env.OVERSEERD || path.join(root, 'target/debug/overseerd');
const home = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-handoff-'));
const G = 2 ** 30;
const gib = b => Math.round((b / G) * 10) / 10;
const sleep = ms => new Promise(r => setTimeout(r, ms));
const redact = s => s.split(fs.realpathSync(home)).join('/OVERSEER_HOME').split(home).join('/OVERSEER_HOME').split(os.homedir()).join('~').replace(new RegExp(`(?<![A-Za-z0-9])${os.userInfo().username}(?![A-Za-z0-9])`, 'g'), 'USER').replace(/(\/private)?\/var\/folders\/[A-Za-z0-9_]+\/[A-Za-z0-9_]+\/T\//g, '/TMP/').replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[a-z]{2,}/g, 'EMAIL');

function session(name, extra) {
  const env = { ...process.env, OVERSEER_HOME: home, ...extra };
  const results = [], log = [], transcript = [];
  const say = line => { console.log(line); log.push(line); };
  const check = (what, ok, detail) => { results.push({ name: what, ok: !!ok }); say(`${ok ? 'PASS' : 'FAIL'}  ${what}${detail ? '  — ' + detail : ''}`); };
  const ctl = (method, params) => {
    const r = spawnSync(bin, ['ctl', method, JSON.stringify(params || {})], { env, encoding: 'utf8', timeout: 900000, maxBuffer: 64 * 1024 * 1024 });
    const msg = JSON.parse(r.stdout.split('\n')[0] || '{"error":{"message":"no answer"}}');
    return msg.error ? { error: msg.error.message } : msg.result;
  };
  const state = () => ctl('state');
  const run = id => state().runs.find(r => r.id === id);
  const events = id => ctl('events.list', { run_id: id, limit: 5000 }).events;
  const said = (id, role) => events(id).filter(e => e.kind === 'output' && e.payload.role === role).map(e => e.payload.text);
  const keep = (scenario, id) => { for (const e of events(id)) transcript.push({ scenario, run: id, seq: e.seq, ts: e.ts, kind: e.kind, source: e.source, payload: e.payload }); };
  async function until(what, pred, limit = 120000) {
    const started = Date.now();
    for (;;) {
      const v = pred();
      if (v) return v;
      if (Date.now() - started > limit) throw new Error(`${what} did not happen within ${limit / 1000} s`);
      await sleep(200);
    }
  }
  /// Follows a run to its end; permission requests are allowed and written down.
  async function follow(id, limit = 600000) {
    const asked = [];
    const started = Date.now();
    for (;;) {
      const r = run(id);
      if (r.status === 'waiting_for_user' && r.attention && !asked.some(a => a.request_id === r.attention.request_id)) {
        asked.push(r.attention);
        say(`    asked: ${r.attention.tool} → Allow`);
        ctl('run.permission', { run_id: id, request_id: r.attention.request_id, allow: true });
      } else if (!['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status)) return { run: r, asked };
      if (Date.now() - started > limit) { ctl('run.interrupt', { run_id: id }); throw new Error(`run ${id} did not end within ${limit / 1000} s (status ${r.status})`); }
      await sleep(250);
    }
  }
  let daemon = null;
  return {
    env, say, check, ctl, state, run, events, said, keep, until, follow, results,
    /// What every run is doing, for a check that did not run to its end.
    diagnose() {
      for (const r of state().runs || []) {
        say(`    run ${r.id} (${r.harness}): ${r.status} — ${r.exit_reason || ''}`);
        for (const e of events(r.id).filter(e => ['retry', 'stall', 'handoff', 'local_load', 'error'].includes(e.kind)).slice(-4)) say(`      ${e.kind}: ${JSON.stringify(e.payload).slice(0, 400)}`);
      }
    },
    async start() {
      daemon = spawn(bin, ['serve'], { env, stdio: 'ignore' });
      for (let i = 0; i < 100 && ctl('hello').error; i++) await sleep(100);
    },
    async stop(out) {
      for (const r of (state().runs || []).filter(r => ['queued', 'starting', 'running', 'waiting_for_user', 'waiting_for_connection', 'waiting_for_memory'].includes(r.status))) ctl('run.interrupt', { run_id: r.id });
      const failed = results.filter(r => !r.ok).length;
      say(`\n${results.length - failed} passed, ${failed} failed`);
      fs.mkdirSync(out, { recursive: true });
      fs.writeFileSync(path.join(out, 'live.txt'), redact(log.join('\n')) + '\n');
      fs.writeFileSync(path.join(out, 'live-events.jsonl'), transcript.map(t => redact(JSON.stringify(t))).join('\n') + '\n');
      ctl('daemon.shutdown');
      if (daemon) daemon.kill();
      await sleep(300);
      return failed;
    },
  };
}

function repo(name) {
  const dir = path.join(home, name);
  fs.mkdirSync(dir, { recursive: true });
  const git = (...a) => execFileSync('git', a, { cwd: dir, encoding: 'utf8' });
  git('init', '-q', '-b', 'main'); git('config', 'user.name', 'T'); git('config', 'user.email', 't@example.invalid'); git('config', 'commit.gpgsign', 'false');
  fs.writeFileSync(path.join(dir, 'README.md'), '# fixture\n'); git('add', '.'); git('commit', '-q', '-m', 'base');
  return dir;
}
const write = (file, v) => { fs.writeFileSync(file + '.tmp', JSON.stringify(v)); fs.renameSync(file + '.tmp', file); };
const freePort = () => new Promise(resolve => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => resolve(p)); }); });

// ------------------------------------------------------------------ AC-91

async function local() {
  const netFile = path.join(home, 'net.json'), control = path.join(home, 'harness.json'), harnessLog = path.join(home, 'harness.log');
  write(netFile, { system: 'connected', baseline: { by_name: true, by_ip: true }, providers: { openai: true, anthropic: true } });
  write(control, { codex: 'network' });
  const s = session('local', {
    OVERSEER_TEST_NET: netFile,
    OVERSEER_CODEX_PATH: path.join(root, 'fixtures/fake-harness/continuity-harness.js'),
    OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CONTINUITY_FIXTURE,CONTINUITY_LOG',
    CONTINUITY_FIXTURE: control, CONTINUITY_LOG: harnessLog,
  });
  const { say, check, ctl } = s;
  const mine = [];
  await s.start();
  try {
    const status = ctl('continuity.status');
    const opencode = ctl('harness.list').find(h => h.harness === 'opencode-serve');
    say(`overseerd ${ctl('hello').version}; OpenCode ${opencode.version} (real); Ollama ${status.ollama.version} (real); Codex: fixture that fails on the network; system answer: fixture`);
    say(`memory before: ${gib(status.memory.available)} GiB available of ${gib(status.memory.total)} GiB, pressure ${status.memory.pressure}, level ${status.memory.level}%; budget ${gib(status.budget.budget)} GiB`);
    const loadedBefore = ctl('local.inventory').loaded.map(l => l.tag);
    say(`loaded in Ollama before: ${loadedBefore.join(', ') || 'nothing'}`);
    const pick = ctl('local.pick').pick.chosen;
    say(`the pick (AC-86): ${pick.tag} at a ${pick.context / 1024}k context, ${gib(pick.bytes)} GiB`);

    write(netFile, { system: 'none' });
    await s.until('offline', () => ctl('connection.status').status.state === 'offline', 10000);
    say(`connection: ${ctl('connection.status').status.state} (${ctl('connection.status').status.reason})`);
    const dir = repo('repo-local');
    const prompt = 'Use the write tool to create a file named handoff.txt in the current directory containing exactly: hello from the local model. Then reply with the single word done.';
    const c = ctl('task.create', { repo: dir, harness: 'codex', title: 'live handoff', prompt });
    if (c.error || c.launch_error) throw new Error(c.error || c.launch_error);
    const first = c.run.id;
    const next = await s.until('the handoff', () => (ctl('continuity.handoffs').handoffs.find(h => h.predecessor === first) || {}).successor, 180000);
    for (const e of s.events(next).filter(e => e.kind === 'local_load' && e.payload.run_tag && !e.payload.error)) mine.push(e.payload.run_tag);
    const failedAt = s.events(first).find(e => e.kind === 'status' && e.payload.status === 'waiting_for_connection').ts;
    const startedAt = s.events(next).find(e => e.kind === 'turn_started' || e.kind === 'status').ts;
    check('the local run started within 30 s of the failed turn', startedAt - failedAt < 30000, `${startedAt - failedAt} ms after the turn failed (the model's load included)`);
    const model = s.events(next).find(e => e.kind === 'local_model').payload;
    say(`    model: ${model.model} at a ${model.context / 1024}k context, ${gib(model.bytes)} GiB of a ${gib(model.budget.budget)} GiB budget${model.already_loaded ? ' (a copy that was already loaded is shared; nothing was loaded)' : ', loaded under the watchdog'}`);
    check('the successor runs the pick, inside the budget', model.base === pick.tag && model.bytes <= model.budget.budget && model.auto === true, `${model.base}; ${gib(model.bytes)} ≤ ${gib(model.budget.budget)} GiB`);
    const f = await s.follow(next);
    s.keep('local', first); s.keep('local', next);
    const ws = s.state().workspaces.find(w => w.id === c.workspace.id);
    const file = path.join(ws.path, 'handoff.txt');
    const before = s.run(first);
    check('the successor is a real OpenCode run on the local model, in the same task and worktree', f.run.harness === 'opencode-serve' && f.run.profile_id === 'local-ollama' && f.run.task_id === before.task_id && f.run.workspace_id === before.workspace_id, `${f.run.harness} ${f.run.harness_version}, ${f.run.model}`);
    check('it completes the write check in that worktree', f.run.status === 'completed' && fs.existsSync(file) && /hello from the local model/.test(fs.readFileSync(file, 'utf8')), `${f.run.status}; handoff.txt: ${fs.existsSync(file) ? JSON.stringify(fs.readFileSync(file, 'utf8')) : 'missing'}; asked: ${f.asked.map(a => a.tool).join(', ') || 'nothing'}`);
    check('the predecessor reads handed off, not failed', before.status === 'handed_off' && before.exit_reason === `handed off to ${next} (offline)` && !s.events(first).some(e => e.kind === 'status' && e.payload.status === 'failed'), before.exit_reason);
    const told = s.said(first, 'system').pop(), opened = s.said(next, 'system')[0];
    check('both chats say what happened', told === `Transitioning to **${pick.tag}** (local, Ollama) because you've disconnected. Work continues in the same worktree.` && /^Continued from "live handoff" after the connection was lost at \d\d:\d\d\.$/.test(opened), `"${told}" / "${opened}"`);
    const h = s.events(next).find(e => e.kind === 'handoff').payload;
    check('the record links predecessor and successor with the reason', h.predecessor === first && h.successor === next && h.reason === 'offline' && h.connection.state === 'offline' && f.run.relation_source === `handoff from ${first} (offline)`, f.run.relation_source);
    const ended = Math.max(...s.events(first).filter(e => e.source !== 'daemon' || e.kind === 'status').map(e => e.ts).filter(t => t <= startedAt));
    const starts = fs.readFileSync(harnessLog, 'utf8').trim().split('\n').length;
    check('one writer throughout: Codex had ended before OpenCode was started, and was not started again', starts === 1 && ended <= startedAt && ws.owner_run_id !== first, `Codex started ${starts} time; worktree owner afterwards: ${s.state().workspaces.find(w => w.id === c.workspace.id).owner_run_id || 'nobody'}`);
    const usage = s.events(next).filter(e => e.kind === 'usage').pop();
    check('the local turn cost nothing', usage && (usage.payload.cost_usd || 0) === 0, usage ? JSON.stringify(usage.payload).slice(0, 160) : 'no usage reported');
    const mem = ctl('local.inventory').memory;
    say(`memory after: ${gib(mem.available)} GiB available, pressure ${mem.pressure}, level ${mem.level}%`);
    check('memory pressure stayed normal', mem.pressure === 'normal', `level ${mem.level}%`);
  } catch (e) {
    check('the live check ran to its end', false, e.message);
    s.diagnose();
  } finally {
    for (const tag of [...new Set(mine)]) {
      ctl('local.unload', { tag });
      for (let i = 0; i < 40 && ctl('local.inventory').loaded.some(l => l.tag === tag); i++) await sleep(250);
      check(`${tag}, which this run loaded, is unloaded again`, !ctl('local.inventory').loaded.some(l => l.tag === tag));
    }
    if (!mine.length) say('this run loaded nothing, so it unloads nothing');
  }
  return s.stop(process.env.OUT || path.join(root, 'docs/verification/evidence/ac-91'));
}

// ------------------------------------------------------------------ AC-84

async function failover() {
  // The real Codex, found by a daemon that is told nothing about it.
  const probe = session('probe', {});
  await probe.start();
  const codex = probe.ctl('harness.list').find(h => h.harness === 'codex');
  const claude = probe.ctl('harness.list').find(h => h.harness === 'claude');
  await probe.stop(path.join(home, 'probe-out'));
  if (!codex.installed || !claude.installed) throw new Error('Codex and Claude Code must both be installed');
  const port = await freePort();
  const wrapper = path.join(home, 'codex-blocked.sh');
  fs.writeFileSync(wrapper, `#!/bin/sh\n# Codex with its hosts blocked, for this process only.\nexport HTTPS_PROXY=http://127.0.0.1:${port} HTTP_PROXY=http://127.0.0.1:${port} ALL_PROXY=http://127.0.0.1:${port} https_proxy=http://127.0.0.1:${port} http_proxy=http://127.0.0.1:${port} all_proxy=http://127.0.0.1:${port}\nexec "${codex.program}" "$@"\n`, { mode: 0o755 });
  const netFile = path.join(home, 'net-failover.json');
  write(netFile, { system: 'connected', baseline: { by_name: true, by_ip: true }, providers: { openai: true, anthropic: true } });
  const s = session('failover', { OVERSEER_TEST_NET: netFile, OVERSEER_CODEX_PATH: wrapper });
  const { say, check, ctl } = s;
  await s.start();
  try {
    say(`overseerd ${ctl('hello').version}; Codex ${codex.version} (real, every connection of its process sent to 127.0.0.1:${port}, where nothing listens); Claude Code ${claude.version} (real, existing login); probe answers: fixture`);
    const signedIn = ctl('profile.status', { id: 'system-claude' });
    if (!signedIn.logged_in) throw new Error(`Claude Code is not signed in: ${signedIn.detail || ''}`);
    const dir = repo('repo-failover');
    // The model last picked for Claude Code is its smallest: one tiny turn says so.
    say('\n== A first Claude Code turn, so that its last model is the smallest one');
    const seed = ctl('task.create', { repo: dir, harness: 'claude', model: 'haiku', title: 'seed', prompt: 'Reply with the single word ok.' });
    if (seed.error || seed.launch_error) throw new Error(seed.error || seed.launch_error);
    const seeded = await s.follow(seed.run.id, 180000);
    check('Claude Code works', seeded.run.status === 'completed', `${seeded.run.status}: ${s.said(seed.run.id, 'assistant').join(' ').slice(0, 80)}`);

    say('\n== Codex cannot reach its provider');
    write(netFile, { system: 'connected', baseline: { by_name: true, by_ip: true }, providers: { openai: 'connect', anthropic: true } });
    await s.until('degraded', () => ctl('connection.status').status.state === 'degraded', 10000);
    say(`connection: ${ctl('connection.status').status.state} (${ctl('connection.status').status.reason})`);
    const prompt = 'Create a file named failover.txt in the current directory containing exactly: hello from the other provider. Then reply with the single word done.';
    const c = ctl('task.create', { repo: dir, harness: 'codex', model: 'gpt-5.6-luna', effort: 'low', title: 'live failover', prompt });
    if (c.error || c.launch_error) throw new Error(c.error || c.launch_error);
    const first = c.run.id;
    const parked = await s.until('the failed turn', () => s.events(first).find(e => e.kind === 'status' && e.payload.status === 'waiting_for_connection'), 300000);
    say(`    Codex: ${parked.payload.reason.slice(0, 200)}`);
    const next = await s.until('the handoff', () => (ctl('continuity.handoffs').handoffs.find(h => h.predecessor === first) || {}).successor, 60000);
    const startedAt = s.events(next).find(e => e.kind === 'turn_started' || e.kind === 'status').ts;
    check('the successor started within 30 s of the failed turn', startedAt - parked.ts < 30000, `${startedAt - parked.ts} ms`);
    const f = await s.follow(next, 300000);
    s.keep('failover', first); s.keep('failover', next);
    const before = s.run(first);
    const ws = s.state().workspaces.find(w => w.id === c.workspace.id);
    const file = path.join(ws.path, 'failover.txt');
    check('Codex failed on the connection and never reached its provider', /^the connection to OpenAI failed: /.test(parked.payload.reason) && !s.events(first).some(e => e.kind === 'usage'), 'no usage was reported for the Codex turn');
    check('the successor is Claude Code on its smallest model, in the same task and worktree', f.run.harness === 'claude' && /haiku/.test(f.run.model || '') && f.run.task_id === before.task_id && f.run.workspace_id === before.workspace_id, `${f.run.harness} ${f.run.harness_version}, model ${f.run.model}, account ${f.run.profile_id}`);
    check('it finishes the task in that worktree', f.run.status === 'completed' && fs.existsSync(file) && /hello from the other provider/.test(fs.readFileSync(file, 'utf8')), `${f.run.status}; failover.txt: ${fs.existsSync(file) ? JSON.stringify(fs.readFileSync(file, 'utf8')) : 'missing'}; asked: ${f.asked.map(a => a.tool).join(', ') || 'nothing'}`);
    check('the predecessor reads handed off, not failed', before.status === 'handed_off' && before.exit_reason === `handed off to ${next} (provider_unreachable:openai)` && !s.events(first).some(e => e.kind === 'status' && e.payload.status === 'failed'), before.exit_reason);
    const told = s.said(first, 'system').pop();
    check('the announcement names the reason', /^OpenAI is unreachable; continuing with \*\*Claude Code\*\* \(account ".+"\) because it is the best working option\.$/.test(told), told);
    const changes = ctl('workspace.changes', { workspace_id: ws.id });
    check('the review shows the work in the same worktree', !changes.error ? (changes.names || []).includes('failover.txt') : execFileSync('git', ['status', '--porcelain'], { cwd: ws.path, encoding: 'utf8' }).includes('failover.txt'), ws.path.replace(home, '/OVERSEER_HOME'));
    check('nothing local was used while a provider worked', !s.events(next).some(e => e.kind === 'local_model' || e.kind === 'local_load'));
    const mode = (s.events(next).find(e => e.kind === 'handoff') || { payload: { target: {} } }).payload.target.mode;
    check('the permission mode was carried and not loosened', mode === 'acceptEdits', `Codex in its sandbox → Claude Code in ${mode}`);
  } catch (e) {
    check('the live check ran to its end', false, e.message);
    s.diagnose();
  }
  return s.stop(process.env.OUT || path.join(root, 'docs/verification/evidence/ac-84'));
}

(async () => {
  const want = process.argv.slice(2).length ? process.argv.slice(2) : ['local'];
  let failed = 0;
  try {
    if (want.includes('local')) failed += await local();
    if (want.includes('failover')) failed += await failover();
  } finally {
    spawnSync('/usr/bin/pkill', ['-f', home]);
    fs.rmSync(home, { recursive: true, force: true });
  }
  process.exitCode = failed ? 1 : 0;
})();
