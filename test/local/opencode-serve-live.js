#!/usr/bin/env node
// LIVE check of the opencode-serve harness (AC-138) on this machine: a real `overseerd` with its
// own OVERSEER_HOME, the real OpenCode and a real local model through Ollama. No account and no
// paid tokens. The model is the daemon's own pick, inside the memory budget, loaded under the
// watchdog and unloaded at the end. The user's own OpenCode folders are hashed before and after.
//
//   node test/local/opencode-serve-live.js [plan manual deny acceptEdits auto interrupt followup]
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const { spawn, spawnSync, execFileSync } = require('child_process');

const root = path.resolve(__dirname, '../..');
const bin = process.env.OVERSEERD || path.join(root, 'target/debug/overseerd');
const out = process.env.OUT || path.join(root, 'docs/verification/evidence/ac-138');
const home = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-live-'));
const env = { ...process.env, OVERSEER_HOME: home };
const G = 2 ** 30;
const gib = b => Math.round((b / G) * 10) / 10;
const results = [];
const log = [];
const transcript = [];
const say = line => { console.log(line); log.push(line); };
const check = (name, ok, detail) => { results.push({ name, ok: !!ok, detail }); say(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`); };
function ctl(method, params) {
  const r = spawnSync(bin, ['ctl', method, JSON.stringify(params || {})], { env, encoding: 'utf8', timeout: 900000, maxBuffer: 64 * 1024 * 1024 });
  const msg = JSON.parse(r.stdout.split('\n')[0] || '{"error":{"message":"no answer"}}');
  return msg.error ? { error: msg.error.message } : msg.result;
}
const sleep = ms => new Promise(r => setTimeout(r, ms));
const redact = s => s.split(fs.realpathSync(home)).join('/OVERSEER_HOME').split(home).join('/OVERSEER_HOME').split(os.homedir()).join('~').replace(new RegExp(`(?<![A-Za-z0-9])${os.userInfo().username}(?![A-Za-z0-9])`, 'g'), 'USER');

// A hash of every file's path and content under the user's own OpenCode configuration
// (`~/.config/opencode`). Its data and state folders are not hashed: other OpenCode processes on
// the machine write logs and lock files there all the time. What this run's own server touches is
// shown instead by the files it holds open (`openFiles`).
function fingerprint() {
  const h = crypto.createHash('sha256');
  let files = 0;
  const walk = dir => {
    let entries = [];
    try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const e of entries.sort((a, b) => a.name.localeCompare(b.name))) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.isFile()) { files++; h.update(p); h.update(fs.readFileSync(p)); }
    }
  };
  walk(path.join(os.homedir(), '.config/opencode'));
  return { hash: h.digest('hex'), files };
}

// Every file the OpenCode servers started under this run's OVERSEER_HOME hold open.
function openFiles() {
  const procs = execFileSync('/bin/ps', ['-axo', 'pid=,command='], { encoding: 'utf8' }).split('\n').map(l => l.trim()).filter(l => /opencode serve/.test(l)).map(l => l.split(/\s+/)[0]);
  const mine = procs.filter(pid => {
    try { return execFileSync('/bin/ps', ['-Eww', '-p', pid, '-o', 'command='], { encoding: 'utf8' }).includes(home); } catch { return false; }
  });
  const files = [];
  for (const pid of mine) {
    const r = spawnSync('/usr/sbin/lsof', ['-Fn', '-p', pid], { encoding: 'utf8' });
    for (const l of (r.stdout || '').split('\n')) if (l.startsWith('n/')) files.push(l.slice(1));
  }
  return { servers: mine.length, files };
}

const state = () => ctl('state');
const run = id => state().runs.find(r => r.id === id);
const events = id => ctl('events.list', { run_id: id, limit: 5000 }).events;
const wsPath = created => state().workspaces.find(w => w.id === created.workspace.id).path;

/// Follows a run to its end, answering permission requests with `answer(request)` (true to allow).
async function follow(id, answer, limit = 300000) {
  const asked = [];
  const started = Date.now();
  for (;;) {
    const r = run(id);
    if (r.status === 'waiting_for_user' && r.attention && !asked.some(a => a.request_id === r.attention.request_id)) {
      const allow = answer(r.attention);
      asked.push({ ...r.attention, allow });
      say(`    asked: ${r.attention.tool} → ${allow ? 'Allow' : 'Deny'}`);
      ctl('run.permission', { run_id: id, request_id: r.attention.request_id, allow });
    } else if (!['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status)) return { run: r, asked };
    if (Date.now() - started > limit) { ctl('run.interrupt', { run_id: id }); throw new Error(`run ${id} did not end within ${limit / 1000} s (status ${r.status})`); }
    await sleep(250);
  }
}
function keep(name, id) {
  for (const e of events(id)) transcript.push({ scenario: name, seq: e.seq, kind: e.kind, source: e.source, payload: e.payload });
}
const tools = id => events(id).filter(e => e.kind === 'tool').map(e => `${e.payload.name} ${/\[(\w+)\]$/.exec(e.payload.summary)?.[1] || ''}`.trim());
const replies = id => events(id).filter(e => e.kind === 'output' && e.payload.role === 'assistant').map(e => e.payload.text);

(async () => {
  const want = process.argv.slice(2).length ? process.argv.slice(2) : ['plan', 'manual', 'deny', 'acceptEdits', 'auto', 'interrupt', 'followup'];
  const before = fingerprint();
  const repo = path.join(home, 'repo');
  fs.mkdirSync(repo);
  const git = (...a) => execFileSync('git', a, { cwd: repo, encoding: 'utf8' });
  git('init', '-q', '-b', 'main'); git('config', 'user.name', 'T'); git('config', 'user.email', 't@example.invalid'); git('config', 'commit.gpgsign', 'false');
  fs.writeFileSync(path.join(repo, 'README.md'), '# fixture\n'); git('add', '.'); git('commit', '-q', '-m', 'base');
  const daemon = spawn(bin, ['serve'], { env, stdio: 'ignore' });
  let loadedTag = null;
  try {
    for (let i = 0; i < 100 && ctl('hello').error; i++) await sleep(100);
    const harness = ctl('harness.list').find(h => h.harness === 'opencode-serve');
    const status = ctl('continuity.status');
    say(`overseerd ${ctl('hello').version}; OpenCode ${harness.version}; Ollama ${status.ollama.version}; memory ${gib(status.memory.available)} GiB available of ${gib(status.memory.total)} GiB, budget ${gib(status.budget.budget)} GiB`);
    check('the opencode-serve harness is installed and says what it supports', harness.installed && /plan, manual, acceptEdits, auto/.test(harness.capabilities.permission_mode));
    const start = (title, prompt, mode) => {
      const c = ctl('task.create', { repo, harness: 'opencode-serve', title, prompt, ...(mode ? { permission_mode: mode } : {}) });
      if (c.error || c.launch_error) throw new Error(`${title} was not launched: ${c.error || c.launch_error}`);
      // Whatever model the daemon loaded for this run is unloaded at the end.
      const m = events(c.run.id).find(e => e.kind === 'local_model');
      if (m) loadedTag = m.payload.model.replace('ollama/', '');
      return c;
    };

    if (want.includes('plan')) {
      say('\n== Plan only');
      const c = start('plan', 'Create a file named plan.txt in the current directory containing exactly: plan mode wrote this. If you cannot, reply with the single word blocked.', 'plan');
      const m = events(c.run.id).find(e => e.kind === 'local_model');
      say(`    model: ${m.payload.model} at a ${m.payload.context / 1024}k context, ${gib(m.payload.bytes)} GiB of a ${gib(m.payload.budget.budget)} GiB budget${m.payload.already_loaded ? ' (already loaded)' : ', loaded under the watchdog'}`);
      check('the model was picked by the daemon and passed the guard', /^ollama\//.test(m.payload.model) && m.payload.bytes <= m.payload.budget.budget, m.payload.model);
      const f = await follow(c.run.id, () => false);
      keep('plan', c.run.id);
      check('Plan only: the turn ends and no file is changed', f.run.status === 'completed' && !fs.existsSync(path.join(wsPath(c), 'plan.txt')) && git('status', '--porcelain').trim() === '', `${f.run.status}; tools: ${tools(c.run.id).join(', ') || 'none'}; asked ${f.asked.length}`);
      check('the run is a local one', run(c.run.id).profile_id === 'local-ollama' && run(c.run.id).model === m.payload.model);
    }
    if (want.includes('manual')) {
      say('\n== Ask first, allowed');
      const c = start('ask first', 'Use the write tool to create a file named asked.txt in the current directory containing exactly: hello from ask first. Then reply with the single word done.', 'manual');
      const file = path.join(wsPath(c), 'asked.txt');
      let early = false, open = null;
      const f = await follow(c.run.id, a => {
        if (/^edit/.test(a.tool)) early = fs.existsSync(file);
        // The server is waiting for the answer: a good moment to see what it holds open.
        if (!open) open = openFiles();
        return true;
      });
      keep('manual', c.run.id);
      const edit = f.asked.find(a => /^edit: asked\.txt/.test(a.tool));
      check('Ask first: the write asks before anything is written', edit && !early, edit && `${edit.tool}; request ${edit.request_id.slice(0, 8)}…`);
      check('Ask first: the request carries the path and the diff', edit && /asked\.txt$/.test(edit.input.path) && /\+hello from ask first/.test(edit.input.diff || ''));
      check('Ask first: Allow lets the write through', f.run.status === 'completed' && fs.existsSync(file) && fs.readFileSync(file, 'utf8').trim() === 'hello from ask first', `${f.run.status}; ${f.asked.length} request(s): ${f.asked.map(a => a.tool).join(', ')}`);
      check('Ask first: the file activity and the reply are in the conversation', events(c.run.id).some(e => e.kind === 'file_activity' && e.payload.paths.includes('asked.txt')) && replies(c.run.id).length > 0, replies(c.run.id).slice(-1)[0]);
      const own = ['.config/opencode', '.local/share/opencode', '.local/state/opencode', '.cache/opencode'].map(d => path.join(os.homedir(), d));
      const inOwn = open.files.filter(f => own.some(d => f.startsWith(d + '/')));
      const inProfile = open.files.filter(f => f.includes('/profiles/local-ollama/'));
      check("the run's OpenCode server holds files of Overseer's profile open, and none of the user's own OpenCode folders", open.servers === 1 && inProfile.length > 0 && inOwn.length === 0, `${open.servers} server, ${open.files.length} open files, ${inProfile.length} in the profile, ${inOwn.length} in the user's folders`);
      const u = events(c.run.id).filter(e => e.kind === 'usage').pop();
      check('usage is tokens with cost 0, marked local', u && u.payload.local === true && u.payload.cost === 0 && u.payload.tokens.total > 0, u && `${u.payload.tokens.total} tokens`);
    }
    if (want.includes('deny')) {
      say('\n== Ask first, denied');
      const c = start('ask first, denied', 'Use the write tool to create a file named denied.txt in the current directory containing exactly: should not exist. If the tool is rejected, do not try again; reply with the single word refused.', 'manual');
      const f = await follow(c.run.id, () => false);
      keep('deny', c.run.id);
      check('Ask first: Deny blocks the write', f.asked.length > 0 && !fs.existsSync(path.join(wsPath(c), 'denied.txt')), `${f.run.status}; ${f.asked.length} request(s) denied; reply: ${replies(c.run.id).slice(-1)[0]}`);
      check('Ask first: a denied write is not reported as file activity', !events(c.run.id).some(e => e.kind === 'file_activity'));
    }
    if (want.includes('acceptEdits')) {
      say('\n== Accept edits');
      const c = start('accept edits', 'First use the write tool to create a file named accepted.txt in the current directory containing exactly: hello from accept edits. Then use the bash tool to run exactly: ls. Then reply with the single word done.', 'acceptEdits');
      const file = path.join(wsPath(c), 'accepted.txt');
      let written = null;
      const f = await follow(c.run.id, a => { if (written === null) written = fs.existsSync(file); return true; });
      keep('acceptEdits', c.run.id);
      check('Accept edits: the edit is made without asking', fs.existsSync(file) && !f.asked.some(a => /^edit/.test(a.tool)), `requests: ${f.asked.map(a => a.tool).join(', ') || 'none'}`);
      check('Accept edits: a command asks first, after the edit was already made', f.asked.some(a => /^command: ls/.test(a.tool)) && written === true, `${f.run.status}; tools: ${tools(c.run.id).join(', ')}`);
    }
    if (want.includes('auto')) {
      say('\n== Auto');
      const c = start('auto', 'First use the write tool to create a file named auto.txt in the current directory containing exactly: hello from auto. Then use the bash tool to run exactly: ls. Then reply with the single word done.', 'auto');
      const f = await follow(c.run.id, () => false);
      keep('auto', c.run.id);
      const t = tools(c.run.id);
      check('Auto: the edit and the command both run without asking', f.run.status === 'completed' && f.asked.length === 0 && fs.existsSync(path.join(wsPath(c), 'auto.txt')) && t.some(x => /^bash completed/.test(x)), `${f.run.status}; tools: ${t.join(', ')}; asked ${f.asked.length}`);
    }
    if (want.includes('interrupt')) {
      say('\n== Interrupt');
      let c, t0;
      for (let attempt = 1; attempt <= 3; attempt++) {
        c = start(`interrupt ${attempt}`, 'Run exactly this shell command with the bash tool and nothing else: sleep 45. Then reply with the single word done.', 'auto');
        for (let i = 0; i < 240 && !events(c.run.id).some(e => e.kind === 'tool' && /sleep 45/.test(e.payload.summary)) && ['starting', 'running'].includes(run(c.run.id).status); i++) await sleep(250);
        if (events(c.run.id).some(e => e.kind === 'tool' && /sleep 45/.test(e.payload.summary))) break;
        say(`    attempt ${attempt}: the model did not call the tool (${run(c.run.id).status}: ${run(c.run.id).exit_reason || ''})`);
        await follow(c.run.id, () => false).catch(() => {});
      }
      t0 = Date.now();
      ctl('run.interrupt', { run_id: c.run.id });
      const f = await follow(c.run.id, () => false, 60000);
      keep('interrupt', c.run.id);
      check('Interrupt: the turn ends long before the 45 s command would, as interrupted', f.run.status === 'interrupted' && Date.now() - t0 < 15000, `${f.run.status} after ${Date.now() - t0} ms (${f.run.exit_reason})`);
      check('Interrupt: no OpenCode server is left running', !execFileSync('/bin/ps', ['-axo', 'command'], { encoding: 'utf8' }).split('\n').some(l => /opencode serve/.test(l) && l.includes('127.0.0.1')));
    }
    if (want.includes('followup')) {
      say('\n== Follow-up in the same session, with another mode');
      const c = start('follow-up', 'Use the write tool to create a file named first.txt in the current directory containing exactly: first turn. Then reply with the single word done.', 'auto');
      await follow(c.run.id, () => false);
      const session = run(c.run.id).native_id;
      const again = ctl('run.follow_up', { run_id: c.run.id, prompt: 'Which file did you create earlier in this conversation? Reply with only its name. Then use the write tool to create second.txt containing exactly: second turn.', permission_mode: 'manual' });
      if (again.error) check('a follow-up is accepted', false, again.error);
      const f = await follow(c.run.id, () => true);
      keep('followup', c.run.id);
      check('Follow-up: the same session continues with its history', run(c.run.id).native_id === session && replies(c.run.id).some(r => /first\.txt/.test(r)), `session ${String(session).slice(0, 12)}…; ${replies(c.run.id).slice(-2).join(' | ')}`);
      check('Follow-up: the new mode applies to the continued session', f.asked.some(a => /^edit: second\.txt/.test(a.tool)) && fs.existsSync(path.join(wsPath(c), 'second.txt')), `asked: ${f.asked.map(a => a.tool).join(', ') || 'nothing'}`);
    }

    const after = fingerprint();
    check("the user's own ~/.config/opencode is byte-identical before and after", before.hash === after.hash && before.files === after.files, `${after.files} files, sha256 ${after.hash.slice(0, 16)}…`);
    const logs = path.join(home, 'profiles/local-ollama/data/opencode/log');
    check("the servers' own logs and sessions are in Overseer's profile", fs.existsSync(logs) && fs.readdirSync(logs).length > 0 && fs.existsSync(path.join(home, 'profiles/local-ollama/data/opencode/opencode.db')), fs.existsSync(logs) ? `${fs.readdirSync(logs).length} log files` : 'no log folder');
    const profile = path.join(home, 'profiles/local-ollama/config/opencode/opencode.json');
    const cfg = JSON.parse(fs.readFileSync(profile, 'utf8'));
    check("Overseer's own profile names only the local provider and holds no rules", JSON.stringify(cfg.enabled_providers) === '["ollama"]' && !cfg.permission && /^ollama\//.test(cfg.small_model), `${Object.keys(cfg.provider.ollama.models).length} local models listed`);
    const mem = ctl('local.inventory').memory;
    check('memory pressure stayed normal', mem.pressure === 'normal', `level ${mem.level}%, available ${gib(mem.available)} GiB`);
  } catch (e) {
    check('the live check ran to its end', false, e.message);
  } finally {
    if (loadedTag) {
      ctl('local.unload', { tag: loadedTag });
      for (let i = 0; i < 40 && ctl('local.inventory').loaded.some(l => l.tag === loadedTag); i++) await sleep(250);
      check('the model is unloaded again', !ctl('local.inventory').loaded.some(l => l.tag === loadedTag));
    }
    for (const r of (state().runs || []).filter(r => ['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status))) ctl('run.interrupt', { run_id: r.id });
    const failed = results.filter(r => !r.ok).length;
    say(`\n${results.length - failed} passed, ${failed} failed`);
    fs.mkdirSync(out, { recursive: true });
    fs.writeFileSync(path.join(out, 'live.txt'), redact(log.join('\n')) + '\n');
    fs.writeFileSync(path.join(out, 'live-events.jsonl'), transcript.map(t => redact(JSON.stringify(t))).join('\n') + '\n');
    ctl('daemon.shutdown');
    daemon.kill();
    spawnSync('/usr/bin/pkill', ['-f', home]);
    fs.rmSync(home, { recursive: true, force: true });
    process.exitCode = failed ? 1 : 0;
  }
})();
