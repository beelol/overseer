const cp = require('child_process'); const fs = require('fs'); const path = require('path'); const os = require('os');
const BIN = path.join(os.homedir(), '.vscode/extensions/beelol.overseer-0.1.0/bin/overseerd-darwin-arm64');
const HOME = path.join(os.homedir(), 'Library/Application Support/Overseer');
const ctl = (m, p = {}) => { const o = JSON.parse(cp.execFileSync(BIN, ['ctl', m, JSON.stringify(p)], { env: { ...process.env, OVERSEER_HOME: HOME }, encoding: 'utf8' }).split('\n')[0]); if (o.error) throw new Error(o.error.message); return o.result; };
const sleep = ms => new Promise(r => setTimeout(r, ms));
(async () => {
  const repo = fs.mkdtempSync('/tmp/ovs-ac12-');
  const git = (...a) => cp.execFileSync('git', a, { cwd: repo, encoding: 'utf8' }).trim();
  git('init', '-q', '-b', 'main'); git('config', 'user.name', 'Overseer Test'); git('config', 'user.email', 'overseer-test@example.invalid'); git('config', 'commit.gpgsign', 'false');
  fs.writeFileSync(path.join(repo, 'README.md'), '# AC-12 disposable repo\n'); git('add', '.'); git('commit', '-qm', 'base');
  const A = 'p-f262c1bc4958', B = 'p-52fb6421edd2';
  const ids = {}; for (const p of [A, B]) { const s = ctl('profile.status', { id: p }); ids[p] = { logged_in: s.logged_in, method: s.method, plan: s.identity?.plan, account: (s.identity?.account_fingerprint || '').slice(0, 8), user: (s.identity?.user_fingerprint || '').slice(0, 8), api_key: s.identity?.has_api_key }; }
  const mk = (p, file, text) => ctl('task.create', { repo, harness: 'codex', profile_id: p, model: 'gpt-5.6-luna', title: `AC-12 ${file}`, prompt: `Create ${file} containing exactly one line: ${text}. Before replying, run the shell command: sleep 20. Then reply exactly: done` });
  const t0 = Date.now();
  const ta = mk(A, 'from-a.txt', 'written with ChatGPT account A'); const tb = mk(B, 'from-b.txt', 'written with ChatGPT account B');
  const runs = [ta.run.id, tb.run.id];
  const active = new Set(['queued', 'starting', 'running', 'waiting_for_user']);
  const seen = {}; let overlapMs = 0;
  while (true) {
    const st = ctl('state').runs.filter(r => runs.includes(r.id));
    const running = st.filter(r => r.status === 'running').length;
    if (running === 2) overlapMs += 1000;
    for (const r of st) seen[r.id] = { status: r.status, reason: r.exit_reason, created_ms: r.created_ms, ended_ms: r.ended_ms, native: r.native_id };
    if (st.every(r => !active.has(r.status)) || Date.now() - t0 > 360000) break;
    await sleep(1000);
  }
  const ev = id => ctl('events.list', { run_id: id, limit: 5000 }).events;
  const span = id => { const e = ev(id).filter(x => ['status', 'tool', 'output', 'turn_done', 'file_activity'].includes(x.kind)); const running = e.find(x => x.kind === 'status' && x.payload.status === 'running'); const done = e.find(x => x.kind === 'turn_done'); return { running_ts: running?.ts, done_ts: done?.ts, tools: e.filter(x => x.kind === 'tool').map(x => x.payload.summary.slice(0, 60)) }; };
  const sa = span(ta.run.id), sb = span(tb.run.id);
  const overlap = Math.min(sa.done_ts, sb.done_ts) - Math.max(sa.running_ts, sb.running_ts);
  const files = { a: fs.existsSync(path.join(ta.workspace.path, 'from-a.txt')) && fs.readFileSync(path.join(ta.workspace.path, 'from-a.txt'), 'utf8').trim(), b: fs.existsSync(path.join(tb.workspace.path, 'from-b.txt')) && fs.readFileSync(path.join(tb.workspace.path, 'from-b.txt'), 'utf8').trim(),
    aHasB: fs.existsSync(path.join(ta.workspace.path, 'from-b.txt')), bHasA: fs.existsSync(path.join(tb.workspace.path, 'from-a.txt')) };
  const out = { repo, identities: ids, runs: { A: { run: ta.run.id, workspace: ta.workspace.path, branch: ta.workspace.branch, ...seen[ta.run.id], ...sa }, B: { run: tb.run.id, workspace: tb.workspace.path, branch: tb.workspace.branch, ...seen[tb.run.id], ...sb } }, overlap_ms: overlap, both_running_polls_ms: overlapMs, files };
  console.log(JSON.stringify(out, null, 2));
})().catch(e => { console.error(e); process.exit(1); });
