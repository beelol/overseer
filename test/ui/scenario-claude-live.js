// LIVE paid scenario (tiny prompts, Claude Haiku) for Claude Code through the packaged UI:
// nested native subagents, a permission request answered in the run panel, the edit landing
// in the worktree, a follow-up with its own baseline, and an interrupt. SHARED_DAEMON=1 uses
// the user's real Overseer state so the runs also appear in their own VS Code window.
const fs = require('fs');
const path = require('path');
const os = require('os');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

const MODEL = process.env.CLAUDE_MODEL || 'haiku';
const PROMPT = "Use the Agent tool to launch one general-purpose subagent with this exact prompt: \"Use the Agent tool to launch one general-purpose subagent whose only job is to reply with exactly: hi. Then reply with exactly: child done.\" After it returns, use the Write tool to create hello.md containing exactly one line: Hello from Claude via Overseer. Then reply with exactly: done";

(async () => {
  const s = new Session('claude-live');
  const shared = !!process.env.SHARED_DAEMON;
  if (shared) s.home = path.join(os.homedir(), 'Library/Application Support/Overseer');
  const result = { checks: [], model: MODEL, sharedDaemon: shared };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const runState = id => s.ctl('state').runs.find(r => r.id === id);
  const waitFor = async (id, pred, secs = 240) => { for (let i = 0; i < secs * 2; i++) { const r = runState(id); if (pred(r)) return r; await delay(500); } return runState(id); };
  const allowPending = async (panel, label) => {
    await panel.waitFor(`!!document.querySelector('.perm button')`, 60000);
    const tool = await panel.eval(`document.querySelector('.perm div').textContent`);
    await s.screenshot('permission-' + label);
    // The card re-renders on every run update; tag and locate the button until it holds still.
    for (let attempt = 0; attempt < 10; attempt++) {
      try {
        await panel.eval(`(() => { const b = [...document.querySelectorAll('.perm button')].find(b => /Allow/.test(b.textContent)); b.id = 'allow-btn'; b.scrollIntoView({ block: 'center' }); })()`);
        const p = await s.webviewPoint(panel, '#allow-btn');
        await s.cdp.click(p.x, p.y);
        return tool;
      } catch { await delay(300); }
    }
    throw new Error('could not click Allow');
  };
  const sendFollowUp = async (panel, text) => {
    await panel.waitFor(`!document.getElementById('send').disabled`, 60000);
    const pr = await s.webviewPoint(panel, '#prompt');
    await s.cdp.click(pr.x, pr.y);
    await s.cdp.type(text);
    await delay(300);
    const sd = await s.webviewPoint(panel, '#send');
    await s.cdp.click(sd.x, sd.y);
  };
  try {
    const repo = makeRepo(path.join(s.root, 'claude-demo'), { dirty: false });
    s.settings();
    s.install(latestVsix());
    s.launch(repo, shared ? { OVERSEER_HOME: s.home } : {});
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer [0-9]+ active/.test(e.textContent))`, 60000, 'status bar');
    const status = s.ctl('profile.status', { id: 'system-claude' });
    check('Claude profile signed in with a claude.ai account', status.logged_in && status.method === 'claude.ai', { version: status.version, plan: status.identity && status.identity.plan, id: status.identity && status.identity.fingerprint });
    await cdp.command('Overseer: New Task');
    await cdp.pick('New task: repository');
    await cdp.pick('New task: harness', 'claude');
    await cdp.pick('New task: account profile', 'claude (existing login)');
    await cdp.pick('New task: workspace');
    await cdp.pick('Start the worktree from');
    await cdp.input('Model (optional)', MODEL);
    await cdp.input('Task prompt', PROMPT);
    await delay(3000);
    const root = s.ctl('state').runs.filter(r => r.harness === 'claude' && !r.parent_run_id).sort((a, b) => b.created_ms - a.created_ms)[0];
    const ws = s.ctl('state').workspaces.find(w => w.id === root.workspace_id);
    s.note('run', { run: root.id, workspace: ws.path, branch: ws.branch });
    const output = await cdp.webview(`!!document.getElementById('prompt') && document.getElementById('title')?.textContent.includes('Use the Agent tool')`, 60000);
    const w1 = await waitFor(root.id, r => r.status === 'waiting_for_user' || !['queued', 'starting', 'running'].includes(r.status));
    check('turn 1 reaches a permission request (not auto-approved)', w1.status === 'waiting_for_user', { status: w1.status, attention: w1.attention && w1.attention.tool, reason: w1.exit_reason });
    if (w1.status === 'waiting_for_user') result.perm1 = await allowPending(output, 'write');
    const d1 = await waitFor(root.id, r => r.status === 'waiting_for_user' || !['queued', 'starting', 'running'].includes(r.status));
    if (d1.status === 'waiting_for_user') { result.extraPermission = d1.attention && d1.attention.tool; await allowPending(output, 'extra'); }
    const done1 = await waitFor(root.id, r => !['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status));
    check('turn 1 completed', done1.status === 'completed', { status: done1.status, reason: done1.exit_reason, native: done1.native_id });
    const tree = s.ctl('state').runs.filter(r => r.task_id === root.task_id);
    const kids = tree.filter(r => r.parent_run_id === root.id);
    const grand = tree.filter(r => kids.some(k => k.id === r.parent_run_id));
    result.tree = tree.map(r => ({ id: r.id, parent: r.parent_run_id, native: r.native_id, status: r.status, title: r.title, confidence: r.relation_confidence }));
    check('native Claude child captured', kids.length >= 1 && kids.every(k => /^exact/.test(k.relation_confidence)), result.tree);
    check('native Claude grandchild captured', grand.length >= 1, grand.map(g => ({ title: g.title, status: g.status })));
    const hello = path.join(ws.path, 'hello.md');
    check('allowed Write landed in the worktree', fs.existsSync(hello) && /Hello from Claude via Overseer/.test(fs.readFileSync(hello, 'utf8')), fs.existsSync(hello) && fs.readFileSync(hello, 'utf8'));
    await s.screenshot('turn1-done');
    // Follow-up turn 2.
    await sendFollowUp(output, 'Use the Edit tool to append one final line to hello.md: Edited by a follow-up turn. Change nothing else, then reply exactly: done');
    const w2 = await waitFor(root.id, r => s.ctl('run.turns', { run_id: root.id }).length >= 2 && (r.status === 'waiting_for_user' || !['queued', 'starting', 'running'].includes(r.status)));
    if (w2.status === 'waiting_for_user') await allowPending(output, 'edit');
    const done2 = await waitFor(root.id, r => !['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status) && s.ctl('run.turns', { run_id: root.id }).length >= 2);
    check('follow-up turn completed', done2.status === 'completed' && /Edited by a follow-up turn/.test(fs.readFileSync(hello, 'utf8')), { status: done2.status, reason: done2.exit_reason });
    const latest = s.ctl('comparison.options', { run_id: root.id }).options.find(o => o.mode === 'latest_run');
    const diff = s.ctl('workspace.diff', { workspace_id: ws.id, base: latest.base });
    check('latest-run comparison shows only turn-2 changes', diff.changes.length === 1 && diff.changes[0].path === 'hello.md' && diff.changes[0].status === 'M', diff.changes);
    // Turn 3: interrupt a running command.
    result.turn3Seq = Math.max(...s.ctl('events.list', { run_id: root.id, limit: 5000 }).events.map(e => e.seq));
    await sendFollowUp(output, 'Use the Bash tool to run exactly this command in the foreground: for i in 1 2 3 4 5 6 7 8 9 10 11 12; do echo tick $i; sleep 5; done. Then reply exactly: done');
    // Wait until the sleep is actually running (answer a permission request if Claude asks).
    for (let i = 0; i < 240; i++) {
      const r = runState(root.id);
      if (r.status === 'waiting_for_user') { await allowPending(output, 'bash'); continue; }
      const bash = s.ctl('events.list', { run_id: root.id, limit: 5000 }).events.some(e => e.kind === 'tool' && e.payload.name === 'Bash' && /tick/.test(e.payload.summary) && e.seq > (result.turn3Seq || 0));
      if (bash && r.status === 'running') break;
      await delay(500);
    }
    await delay(4000);
    await output.waitFor(`!document.getElementById('interrupt').disabled`, 20000);
    const stop = await s.webviewPoint(output, '#interrupt');
    await cdp.click(stop.x, stop.y);
    const done3 = await waitFor(root.id, r => !['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status), 60);
    check('interrupt from UI stops the Claude run', done3.status === 'interrupted', { status: done3.status, reason: done3.exit_reason });
    await delay(1500);
    await s.screenshot('interrupted');
    result.turns = s.ctl('run.turns', { run_id: root.id }).map(t => ({ n: t.n, status: t.status, snapshot: t.snapshot_id }));
    result.usage = s.ctl('events.list', { run_id: root.id, limit: 5000 }).events.filter(e => e.kind === 'usage').map(e => e.payload);
    result.run = root.id; result.workspace = ws.path;
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) await s.quit();
    if (!shared) s.stopDaemon();
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
