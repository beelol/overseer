// LIVE check for AC-62 (Codex usage matches Codex's own output), on the OWNER'S daemon through its
// API only: no VS Code, no install, no restart, and it refuses to run while any run is active. One
// tiny Codex app-server turn on ChatGPT A (gpt-5.6-luna, "Reply with exactly: ok"), one attempt.
// Codex reports limits in two independent places: the app-server streams
// `account/rateLimits/updated` to the client, and Codex writes `token_count` lines with
// `rate_limits` into the account's session log (which `account.usage` reads). The check compares
// Overseer's account usage with the raw notification from the run's own output, window by window.
// Only the rate-limit lines are written to the evidence (no other raw output, no credentials).
const fs = require('fs');
const os = require('os');
const path = require('path');
const cp = require('child_process');
const { makeRepo, delay, repoRoot } = require('./harness');

const CHATGPT_A = process.env.CHATGPT_A || 'p-f262c1bc4958';
const ACTIVE = ['queued', 'starting', 'running', 'waiting_for_user'];
const evidence = path.join(repoRoot, 'docs/verification/evidence/ui/codex-usage-live');

(async () => {
  fs.rmSync(evidence, { recursive: true, force: true }); fs.mkdirSync(evidence, { recursive: true });
  const log = []; const note = (m, d) => { const line = `[${new Date().toISOString()}] ${m}${d === undefined ? '' : ' ' + JSON.stringify(d)}`; log.push(line); console.log(line); };
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  // This build's daemon (the one the Gate K VSIX ships).
  const bin = path.join(repoRoot, 'extension/bin', `overseerd-${process.platform}-${process.arch}`);
  const env = { ...process.env }; delete env.OVERSEER_HOME;
  const ctl = (m, p = {}) => { const msg = JSON.parse(cp.execFileSync(bin, ['ctl', m, JSON.stringify(p)], { env, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).split('\n')[0]); if (msg.error) throw new Error(`${m}: ${msg.error.message}`); return msg.result; };
  let runId, started = false;
  try {
    // If the owner's daemon is not running (so nothing is active), start it for this check and stop it after.
    try { ctl('hello'); } catch {
      cp.spawn(bin, ['serve'], { env, detached: true, stdio: 'ignore' }).unref(); started = true;
      for (let i = 0; i < 40; i++) { await delay(250); try { ctl('hello'); break; } catch {} }
      note('owner daemon was not running; started this build for the check');
    }
    const before = ctl('state');
    const busy = before.runs.filter(r => ACTIVE.includes(r.status));
    note('owner daemon', { pid: before.daemon?.pid, runs: before.runs.length, active: busy.length });
    if (busy.length) throw new Error(`owner daemon has ${busy.length} active run(s); not starting anything`);
    const repo = makeRepo(fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-usage-')), { dirty: false });

    const t = ctl('task.create', { repo, harness: 'codex-app', profile_id: CHATGPT_A, model: 'gpt-5.6-luna', prompt: 'Reply with exactly: ok', title: 'Live usage check (AC-62)' });
    runId = t.run.id; note('started', { run: runId });
    let run;
    for (let i = 0; i < 240; i++) { run = ctl('state').runs.find(r => r.id === runId); if (!ACTIVE.includes(run.status)) break; await delay(500); }
    note('finished', { status: run.status });
    const reply = ctl('events.list', { run_id: runId, limit: 2000 }).events.filter(e => e.kind === 'output' && e.payload?.role === 'assistant').map(e => e.payload.text).join('');
    check('the tiny Codex turn completed', run.status === 'completed' && /ok/i.test(reply), { status: run.status, reply: reply.slice(0, 40) });

    // The harness's own report: the last account/rateLimits/updated notification in the raw output.
    const raw = ctl('run.raw_output', { run_id: runId, max_bytes: 4 * 1024 * 1024 }).lines || [];
    const notes = [];
    for (const l of raw) {
      const text = typeof l === 'string' ? l : (l.d ?? l.data ?? l.text ?? '');
      if (!text.includes('account/rateLimits/updated')) continue;
      try { const v = JSON.parse(text); if (v.method === 'account/rateLimits/updated') notes.push(v.params.rateLimits); } catch {}
    }
    fs.writeFileSync(path.join(evidence, 'rate-limit-notifications.json'), JSON.stringify(notes, null, 2));
    const last = notes[notes.length - 1];
    note('raw notifications', { count: notes.length, last });

    // Overseer's value for the account (from Codex's session log).
    const usage = ctl('account.usage', { id: CHATGPT_A });
    fs.writeFileSync(path.join(evidence, 'account-usage.json'), JSON.stringify(usage, null, 2));
    note('account.usage', usage);
    const label = mins => ({ 300: '5 hours', 10080: 'week', 1440: 'day' }[mins] || `${mins} min`);
    const pairs = last ? ['primary', 'secondary'].filter(k => last[k]).map(k => {
      const w = last[k]; const o = (usage.windows || []).find(x => x.label === label(w.windowDurationMins));
      return { window: label(w.windowDurationMins), codex: { usedPercent: w.usedPercent, resetsAt: w.resetsAt }, overseer: o && { used: o.used, resets_at_ms: o.resets_at_ms },
        match: !!o && Math.abs(o.used * 100 - w.usedPercent) < 1e-6 && o.resets_at_ms === w.resetsAt * 1000 };
    }) : [];
    check('Codex usage in Overseer matches Codex\'s own account/rateLimits/updated for the same account (used share and reset time per window)',
      usage.reported && pairs.length > 0 && pairs.every(p => p.match), { pairs, plan: { codex: last?.planType, overseer: usage.plan }, source: usage.source });
    check('the plan matches', !last?.planType || !usage.plan || last.planType === usage.plan, { codex: last?.planType, overseer: usage.plan });
  } catch (error) {
    note('ERROR ' + (error.stack || error.message)); result.error = error.message;
  } finally {
    try { if (runId) { const r = ctl('state').runs.find(x => x.id === runId); if (ACTIVE.includes(r?.status)) ctl('run.interrupt', { run_id: runId }); ctl('task.archive', { task_id: r.task_id, archived: true }); } } catch (e) { note('cleanup: ' + e.message); }
    if (started) { try { ctl('daemon.shutdown'); note('stopped the daemon again (it was not running before)'); } catch {} }
    fs.writeFileSync(path.join(evidence, 'scenario.log'), log.join('\n') + '\n');
    fs.writeFileSync(path.join(evidence, 'result.json'), JSON.stringify(result, null, 2));
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED');
    process.exit(failed ? 1 : 0);
  }
})();
