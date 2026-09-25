// LIVE paid scenario (3 tiny turns) for Codex permission requests through the app-server
// transport: Allow, Deny, and Interrupt while waiting — answered in the packaged VS Code UI.
// DRY_RUN=1 uses the synthetic app-server fixture instead of Codex.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

const PROMPT = 'Run exactly this shell command in the workspace: touch approved.txt. If the command is declined, do not retry anything and reply exactly: declined. Otherwise reply exactly: done';

(async () => {
  const dry = !!process.env.DRY_RUN;
  const s = new Session(dry ? 'codex-approval-dry-run' : 'codex-approval-live');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const env = dry ? { OVERSEER_CODEX_PATH: path.join(repoRoot, 'fixtures/fake-harness/codex-app-fixture.js') } : {};
  const runState = id => s.ctl('state').runs.find(r => r.id === id);
  const waitFor = async (id, pred, secs = 240) => { for (let i = 0; i < secs * 2; i++) { const r = runState(id); if (pred(r)) return r; await delay(500); } return runState(id); };
  try {
    const repo = makeRepo(path.join(s.root, 'repo'), { dirty: false });
    s.settings();
    s.install(latestVsix());
    s.launch(repo, env);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    // Task 1 entirely through the UI.
    await cdp.command('Overseer: New Task');
    await cdp.pick('New task: repository');
    await cdp.pick('New task: harness', 'codex-app');
    await cdp.pick('New task: account for', 'codex (existing login)');
    if (dry) {
      const b = await cdp.waitFor(`(() => { const b = [...document.querySelectorAll('.notification-toast .monaco-button')].find(b => b.textContent.includes('Launch anyway')); if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, 10000);
      await cdp.click(b.x, b.y);
    }
    await cdp.pick('New task: workspace');
    await cdp.pick('Start the worktree from');
    await cdp.input('Model (optional)', dry ? '' : 'gpt-5.6-luna');
    await cdp.pick('Codex approval policy', 'untrusted');
    await cdp.input('Task prompt', PROMPT);
    const r1 = s.ctl('state').runs.find(r => r.harness === 'codex-app');
    const w1 = await waitFor(r1.id, r => r.status === 'waiting_for_user' || !['queued', 'starting', 'running'].includes(r.status));
    check('codex-app run waits for permission (not auto-approved)', w1.status === 'waiting_for_user', { status: w1.status, attention: w1.attention && w1.attention.tool, reason: w1.exit_reason });
    const output = await cdp.webview(`!!document.querySelector('.perm button')`, 30000);
    await s.screenshot('permission-request');
    await output.eval(`(() => { const b = [...document.querySelectorAll('.perm button')].find(b => /Allow/.test(b.textContent)); b.id = 'allow-btn'; b.scrollIntoView({ block: 'center' }); })()`);
    const allow = await s.webviewPoint(output, '#allow-btn');
    await cdp.click(allow.x, allow.y);
    const answered = await waitFor(r1.id, r => r.status !== 'waiting_for_user', 20);
    check('Allow click reached the run', answered.status !== 'waiting_for_user', { status: answered.status });
    const d1 = await waitFor(r1.id, r => !['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status));
    const ws1 = s.ctl('state').workspaces.find(w => w.id === r1.workspace_id).path;
    check('Allow runs the command', d1.status === 'completed' && fs.existsSync(path.join(ws1, 'approved.txt')), { status: d1.status, reason: d1.exit_reason });
    await s.screenshot('allowed');
    // Tasks 2 and 3: created through the daemon, answered in the UI.
    for (const mode of ['deny', 'interrupt']) {
      const t = s.ctl('task.create', { repo, harness: 'codex-app', profile_id: 'system-codex', model: dry ? undefined : 'gpt-5.6-luna', prompt: PROMPT, title: `approval ${mode}`, approval_policy: 'untrusted' });
      const w = await waitFor(t.run.id, r => r.status === 'waiting_for_user' || !['queued', 'starting', 'running'].includes(r.status));
      check(`${mode}: waiting for permission`, w.status === 'waiting_for_user', { status: w.status });
      await s.openOverseerView();
      const row = await cdp.waitFor(`(() => { const rows = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top); const i = rows.findIndex(r => r.textContent.includes(${JSON.stringify(`approval ${mode}`)})); const r = rows[i + 1]; if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 20000);
      await cdp.click(row.x, row.y);
      const panel = await cdp.webview(`document.getElementById('title')?.textContent.includes(${JSON.stringify(`approval ${mode}`)}) && !!document.querySelector('.perm button')`, 30000);
      if (mode === 'deny') {
        await panel.eval(`(() => { const b = [...document.querySelectorAll('.perm button')].find(b => /Deny/.test(b.textContent)); b.id = 'deny-btn'; b.scrollIntoView({ block: 'center' }); })()`);
        const p = await s.webviewPoint(panel, '#deny-btn');
        await cdp.click(p.x, p.y);
      } else {
        const p = await s.webviewPoint(panel, '#interrupt');
        await cdp.click(p.x, p.y);
      }
      const d = await waitFor(t.run.id, r => !['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status));
      const ws = s.ctl('state').workspaces.find(x => x.id === t.run.workspace_id).path;
      if (mode === 'deny') check('Deny: command not run, harness told', d.status === 'completed' && !fs.existsSync(path.join(ws, 'approved.txt')), { status: d.status, reason: d.exit_reason, output: s.ctl('events.list', { run_id: t.run.id }).events.filter(e => e.kind === 'output').map(e => e.payload.text).slice(-2) });
      else check('Interrupt while waiting for permission', d.status === 'interrupted' && !fs.existsSync(path.join(ws, 'approved.txt')), { status: d.status, reason: d.exit_reason });
      await s.screenshot(mode);
    }
    const ids = s.ctl('profile.status', { id: 'system-codex' });
    result.identity = { plan: ids.identity && ids.identity.plan, account: ids.identity && ids.identity.account_fingerprint, version: ids.version };
    result.usage = s.ctl('state').runs.filter(r => r.harness === 'codex-app').map(r => ({ run: r.id, status: r.status, usage: s.ctl('events.list', { run_id: r.id }).events.filter(e => e.kind === 'usage').map(e => e.payload).slice(-1)[0] }));
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
