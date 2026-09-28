// LIVE, PAID: the offline session of AC-97, with the network SIMULATED and everything else real.
// The packaged extension in an isolated VS Code (its own profile, extensions and OVERSEER_HOME);
// the REAL Codex (gpt-5.6-luna, low effort) with every connection of its process sent through the
// simulated network of daemon/examples/netsim.rs; the REAL OpenCode and a REAL local model through
// the owner's Ollama, chosen and loaded by the daemon inside its memory budget. Only the operating
// system's own answer is replaced (OVERSEER_TEST_SYSTEM_NET), because producing it needs the
// Wi-Fi itself; the daemon's probes are real.
//
// "Wi-Fi goes off" during a real Codex turn: the system says no network and the simulated link
// refuses every connection and ends the ones open. The run moves to the local model and finishes
// the task in the same worktree. "Wi-Fi comes back": the system says connected, the link tunnels
// again, and Switch back continues in Codex. Screenshots of both announcements, the run tree and
// the review at each step, for the owner's confirmation. Two tiny Codex turns.
//
//   node test/ui/scenario-offline-session.js      (build first: node extension/scripts/package.js)
const fs = require('fs');
const os = require('os');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

const writeWhole = (file, text) => { fs.writeFileSync(file + '.tmp', text); fs.renameSync(file + '.tmp', file); };
const clock = () => new Date().toISOString().slice(11, 19);
const PROMPT = 'Create the file notes/offline.md containing exactly three short lines that describe this repository. Then reply with the single word done.';

/** Asks a throwaway daemon which Codex it would run, so the wrapper wraps that one. */
async function codexProgram() {
  const bin = path.join(repoRoot, 'target/debug/overseerd');
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-probe-'));
  const env = { ...process.env, OVERSEER_HOME: home, OVERSEER_CONTINUITY_PROBES: 'off' };
  const d = cp.spawn(bin, ['serve'], { env, stdio: 'ignore' });
  const ctl = (m, p) => JSON.parse(cp.spawnSync(bin, ['ctl', m, JSON.stringify(p || {})], { env, encoding: 'utf8' }).stdout.split('\n')[0] || '{}');
  try {
    for (let i = 0; i < 100 && !ctl('hello').result; i++) await delay(100);
    return ctl('harness.list').result.find(h => h.harness === 'codex');
  } finally { ctl('daemon.shutdown'); d.kill(); fs.rmSync(home, { recursive: true, force: true }); }
}

/** The simulated network, as its own process, following a control file. */
async function startNetsim(dir) {
  const bin = path.join(repoRoot, 'target/debug/examples/netsim');
  if (!fs.existsSync(bin)) cp.execFileSync('cargo', ['build', '-q', '-p', 'overseerd', '--example', 'netsim'], { cwd: repoRoot, stdio: 'inherit' });
  const control = path.join(dir, 'netsim.ctl');
  writeWhole(control, 'online');
  const child = cp.spawn(bin, [control], { stdio: ['ignore', 'pipe', 'inherit'] });
  const lines = [];
  let buf = '';
  child.stdout.on('data', d => { buf += d; let i; while ((i = buf.indexOf('\n')) >= 0) { lines.push(buf.slice(0, i)); buf = buf.slice(i + 1); } });
  for (let i = 0; i < 100 && !lines.length; i++) await delay(50);
  const port = Number((/listening 127\.0\.0\.1:(\d+)/.exec(lines[0] || '') || [])[1]);
  if (!port) throw new Error('the simulated network did not start');
  return { port, lines, set: mode => writeWhole(control, mode), stop: () => child.kill() };
}

function memoryLevel() {
  const level = Number(cp.execFileSync('sysctl', ['-n', 'kern.memorystatus_level'], { encoding: 'utf8' }).trim());
  return level;
}

async function ollamaLoaded() {
  try { const r = await fetch('http://127.0.0.1:11434/api/ps'); return ((await r.json()).models || []).map(m => ({ name: m.name, gib: Math.round(m.size / 2 ** 29) / 2 })); } catch { return null; }
}

(async () => {
  const s = new Session('offline-session');
  const result = { checks: [], timeline: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const mark = (what, extra) => { result.timeline.push({ at: clock(), ms: Date.now(), what, ...(extra || {}) }); s.note(`== ${what}`, extra); };
  const system = path.join(s.root, 'system-answer');
  writeWhole(system, 'connected');
  let sim = null;
  const loadedBefore = await ollamaLoaded();
  const levelBefore = memoryLevel();
  result.memory = { level_before: levelBefore, loaded_before: loadedBefore };
  try {
    if (levelBefore < 50) throw new Error(`the system's free memory level is ${levelBefore}%; not starting a local model now`);
    const codex = await codexProgram();
    if (!codex || !codex.installed) throw new Error('Codex is not installed');
    sim = await startNetsim(s.root);
    const wrapper = path.join(s.root, 'codex-through-netsim.sh');
    const proxy = `http://127.0.0.1:${sim.port}`;
    fs.writeFileSync(wrapper, `#!/bin/sh\n# The real Codex, every connection of its process sent through the simulated network.\nexport HTTPS_PROXY=${proxy} HTTP_PROXY=${proxy} ALL_PROXY=${proxy} https_proxy=${proxy} http_proxy=${proxy} all_proxy=${proxy} NO_PROXY=127.0.0.1,localhost no_proxy=127.0.0.1,localhost\nexec "${codex.program}" "$@"\n`, { mode: 0o755 });
    result.versions = { codex: codex.version };
    mark(`Codex ${codex.version} through the simulated network on port ${sim.port}; memory level ${levelBefore}%`);

    const repo = makeRepo(path.join(s.root, 'offline-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(process.env.OFFLINE_VSIX || latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: wrapper, OVERSEER_TEST_SYSTEM_NET: system });
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const state = id => s.ctl('state').runs.find(r => r.id === id);
    const events = id => s.ctl('events.list', { run_id: id, limit: 5000 }).events;
    const waitFor = async (what, fn, ms) => { const t0 = Date.now(); for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t0 > ms) throw new Error(`${what} did not happen within ${Math.round(ms / 1000)} s`); await delay(250); } };
    const connectionItem = async () => (await cdp.evalWorkbench(`[...document.querySelectorAll('.statusbar-item')].filter(e => e.offsetParent).map(e => ({ text: e.textContent.trim(), aria: e.querySelector('[aria-label]')?.getAttribute('aria-label') || e.getAttribute('aria-label') || '' }))`)).find(i => /^Connection:/.test(i.aria));
    const notes = async frame => frame.eval(`[...document.querySelectorAll('#conv .cont-note')].map(n => n.textContent)`);
    /** Follows a run to its end, allowing what it asks (and writing it down). */
    const follow = async (id, ms) => {
      const asked = [];
      await waitFor(`run ${id} to end`, () => {
        const r = state(id);
        if (r?.status === 'waiting_for_user' && r.attention?.request_id && !asked.some(a => a.request_id === r.attention.request_id)) { asked.push(r.attention); s.ctl('run.permission', { run_id: id, request_id: r.attention.request_id, allow: true }); }
        return /completed|failed|interrupted|handed_off/.test(r?.status || '') ? r : null;
      }, ms);
      return { run: state(id), asked };
    };
    s.ctl('continuity.notice', { dismiss: true });
    const online = await waitFor('online', async () => { const c = await connectionItem(); return c && /Connection: Online/.test(c.aria) ? c : null; }, 60000);
    check('online at the start: the real probes answer and the status bar is quiet', !!online, online);

    // ---- A real Codex run ----
    const task = s.ctl('task.create', { repo, harness: 'codex', model: 'gpt-5.6-luna', effort: 'low', title: 'Offline session', prompt: PROMPT });
    if (task.error || task.launch_error) throw new Error(task.error || task.launch_error);
    const first = task.run.id;
    mark('Codex started', { run: first });
    await waitFor('the Codex turn to start', () => state(first)?.status === 'running' && events(first).some(e => ['session', 'turn_started'].includes(e.kind)), 60000);
    await s.selectRun(first, { settle: 1500 });
    await s.screenshot('running-online');

    // ---- Wi-Fi goes off, during the turn ----
    const doneBefore = events(first).some(e => e.kind === 'turn_done');
    const cutAt = Date.now();
    writeWhole(system, 'none');
    sim.set('refuse');
    mark('network off (simulated): the system says no network; the link refuses and ends its connections');
    check('the network went off while the Codex turn was running', state(first).status === 'running' && !doneBefore, { status: state(first).status });
    const offline = await waitFor('the status bar to say Offline', async () => { const c = await connectionItem(); return c && /Offline/.test(c.text) ? c : null; }, 15000);
    const offEvent = s.ctl('events.list', { limit: 5000 }).events.find(e => e.kind === 'connection' && e.ts >= cutAt && e.payload.status.state === 'offline');
    check('Overseer is offline within 10 s of the system\'s signal', offEvent && offEvent.ts - cutAt <= 10000 && offEvent.payload.status.reason === 'no network (system)', { ms: offEvent && offEvent.ts - cutAt, reason: offEvent?.payload.status.reason, bar: offline.text });
    await s.screenshot('offline-during-run');

    // ---- The run moves to the local model ----
    const successor = await waitFor('the handoff to a local model', () => s.ctl('continuity.handoffs').handoffs.find(h => h.predecessor === first)?.successor, 240000);
    mark('handed off', { successor, after_ms: Date.now() - cutAt });
    const why = events(first).filter(e => e.kind === 'stall' || (e.kind === 'status' && e.payload.status === 'waiting_for_connection')).map(e => e.payload.reason).filter(Boolean);
    const local = await follow(successor, 20 * 60000);
    mark('the local agent ended', { status: local.run.status });
    const pick = (events(successor).find(e => e.kind === 'local_model') || {}).payload || {};
    const ws = s.ctl('state').workspaces.find(w => w.id === local.run.workspace_id);
    const file = path.join(ws.path, 'notes/offline.md');
    const written = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : null;
    check('the run moved to a local model: the predecessor reads handed off, the successor is OpenCode on the daemon\'s pick in the same task and worktree', state(first).status === 'handed_off' && local.run.harness === 'opencode-serve' && local.run.task_id === state(first).task_id && local.run.workspace_id === state(first).workspace_id, { predecessor: state(first).status, exit: state(first).exit_reason, why, successor: `${local.run.harness} ${local.run.model}`, pick: { model: pick.model, context: pick.context, bytes: pick.bytes } });
    check('the local model finished the task in the same worktree', local.run.status === 'completed' && written && written.trim().split('\n').length >= 1, { status: local.run.status, written, asked: local.asked.map(a => a.tool) });
    // Both announcements and the tree. The task's row is now the agent that took over; the
    // handed-off Codex is folded under it as "Earlier: Codex".
    await s.selectRun(successor, { settle: 1500 });
    let dash = await s.editorView(`[...document.querySelectorAll('#conv .cont-note')].some(n => /^Continued from/.test(n.textContent))`);
    const opened = await notes(dash);
    if (!(await s.agentRows()).some(r => /^Earlier: Codex/.test(r.label || ''))) await s.clickAgentRow('Offline session', { twisty: true });
    const rows = await s.agentRows();
    check('the successor\'s chat says where it continues from, and the tree folds Codex under it', opened.some(t => /^Continued from "Offline session" after the connection was lost at \d\d:\d\d\./.test(t)) && rows.some(r => /^Earlier: Codex/.test(r.label || '')), { opened, rows: rows.map(r => `${r.label} · ${r.description || ''}`) });
    await s.screenshot('transition-successor-tree');
    await s.clickAgentRow('Earlier: Codex');
    dash = await s.editorView(`[...document.querySelectorAll('#conv .cont-note')].some(n => /Transitioning/.test(n.textContent))`);
    const told = await notes(dash);
    const status = await dash.eval(`document.getElementById('status')?.getAttribute('aria-label')`);
    check('the predecessor\'s chat announces the transition, and reads handed off, not failed', told.some(t => /^Transitioning to .+ \(local, Ollama\) because you've disconnected\. Work continues in the same worktree\.$/.test(t)) && status === 'Handed off', { told, status });
    await s.screenshot('transition-predecessor');
    await s.selectRun(successor, { settle: 1500 });
    const review1 = await cdp.webview(`!!document.getElementById('diffs')`, 30000).then(f => f.eval(`[...document.querySelectorAll('.diff-file, [data-path]')].map(e => e.getAttribute('data-path') || e.textContent.trim().slice(0, 60))`), () => null);
    check('the review shows the local agent\'s work', JSON.stringify(review1 || []).includes('offline.md'), review1);
    await s.screenshot('review-after-local');

    // ---- Wi-Fi comes back ----
    const backAt = Date.now();
    writeWhole(system, 'connected');
    sim.set('online');
    mark('network on (simulated): the system says connected; the link tunnels again');
    await waitFor('the status bar to say Online', async () => { const c = await connectionItem(); return c && /Connection: Online/.test(c.aria) ? c : null; }, 60000);
    const onEvent = s.ctl('events.list', { limit: 5000 }).events.find(e => e.kind === 'connection' && e.ts >= backAt && e.payload.status.state === 'online');
    mark('online again', { after_ms: onEvent && onEvent.ts - backAt });
    await s.selectRun(successor, { settle: 1500 });
    dash = await s.editorView(`!!document.querySelector('#conv [data-continuity-card="back"]')`);
    const back = await dash.eval(`(() => { const c = document.querySelector('#conv [data-continuity-card="back"]'); return c ? { title: c.querySelector('.cont-title')?.textContent, line: c.querySelector('.cont-line')?.textContent, actions: [...c.querySelectorAll('button')].map(b => b.textContent) } : null; })()`);
    check('back online, the local agent offers Switch back to Codex', back && back.title === 'Back online' && /^Switch back to Codex$/.test(back.actions[0] || ''), back);
    await s.screenshot('back-online-offer');
    { const at = await s.webviewPoint(dash, '[data-continuity="handoff:back"]'); await cdp.click(at.x, at.y); }
    mark('Switch back clicked');
    const backRun = await waitFor('the way back', () => s.ctl('continuity.handoffs').handoffs.find(h => h.predecessor === successor)?.successor, 60000);
    const resumed = await follow(backRun, 10 * 60000);
    mark('Codex ended again', { status: resumed.run.status });
    const firstRun = state(first);
    check('Switch back continued in Codex, in its own session and the same worktree, on the same small model', resumed.run.status === 'completed' && resumed.run.harness === 'codex' && resumed.run.workspace_id === firstRun.workspace_id && resumed.run.native_id === firstRun.native_id && /luna/.test(resumed.run.model || ''), { status: resumed.run.status, model: resumed.run.model, session: resumed.run.native_id === firstRun.native_id });
    await s.selectRun(backRun, { settle: 1500 });
    await s.screenshot('switched-back');
    const tunnels = sim.lines.filter(l => /CONNECT .*: tunnelled/.test(l));
    check('Codex\'s connections really went through the simulated link, before the cut and after it', tunnels.length >= 2 && sim.lines.some(l => /link Refuse/.test(l)), { tunnels: tunnels.length, log: sim.lines.slice(0, 40) });
    const review2 = await cdp.webview(`!!document.getElementById('diffs')`, 30000).then(f => f.eval(`[...document.querySelectorAll('.diff-file, [data-path]')].map(e => e.getAttribute('data-path') || e.textContent.trim().slice(0, 60))`), () => null);
    result.review_after = review2;
    await s.screenshot('review-after-switch-back');
    // What each run did, for the record.
    result.runs = [first, successor, backRun].map(id => { const r = state(id); return { id, harness: r.harness, model: r.model, status: r.status, exit: r.exit_reason, events: events(id).filter(e => ['status', 'handoff', 'local_model', 'local_load', 'stall', 'error', 'turn_done', 'usage'].includes(e.kind) || (e.kind === 'output' && e.payload.role !== 'user')).map(e => ({ at: new Date(e.ts).toISOString().slice(11, 19), kind: e.kind, payload: JSON.stringify(e.payload).slice(0, 300) })) }; });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    // A model this run's daemon loaded itself is unloaded; one it found loaded (another session's)
    // is left alone.
    try {
      const st = s.ctl('state');
      for (const r of st.runs.filter(r => r.harness === 'opencode-serve')) {
        const pick = (s.ctl('events.list', { run_id: r.id, limit: 5000 }).events.find(e => e.kind === 'local_model') || {}).payload;
        if (pick && pick.already_loaded === false) { s.ctl('local.unload', { tag: pick.model.replace(/^ollama\//, '') }); result.memory.unloaded = pick.model; }
        else if (pick) result.memory.shared = pick.model;
      }
    } catch (e) { result.memory.unload_error = e.message; }
    result.memory.level_after = memoryLevel();
    result.memory.loaded_after = await ollamaLoaded();
    if (sim) { result.netsim = sim.lines; sim.stop(); }
    const redact = t => t.split(os.homedir()).join('~').split(s.root).join('/SESSION').replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[a-z]{2,}/g, 'EMAIL');
    s.log = s.log.map(redact);
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), redact(JSON.stringify(result, null, 2)));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
