// LIVE paid scenario (tiny prompts): Codex through the owner's existing ChatGPT login,
// driven from the packaged VS Code UI. Dogfoods Overseer on its own repository in an
// isolated worktree: native sub-agent, edit, review edit, follow-up, VS Code closure
// while a run continues, and interrupt. Run deliberately; it spends real tokens.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, snapshotTree, latestVsix, delay, repoRoot } = require('./harness');

const MODEL = process.env.CODEX_MODEL || 'gpt-5.6-luna';

async function waitRun(s, runId, pred, timeout = 300000) {
  const end = Date.now() + timeout; let run;
  while (Date.now() < end) { run = s.ctl('state').runs.find(r => r.id === runId); if (pred(run)) return run; await delay(1500); }
  throw new Error(`run ${runId} did not reach expected state (last ${run?.status})`);
}

async function sendFollowUp(s, output, text) {
  await output.waitFor(`!document.getElementById('send').disabled`, 60000);
  const prompt = await s.webviewPoint(output, '#prompt');
  await s.cdp.click(prompt.x, prompt.y);
  await s.cdp.type(text);
  const send = await s.webviewPoint(output, '#send');
  await s.cdp.click(send.x, send.y);
}

(async () => {
  const dry = !!process.env.DRY_RUN;
  const s = new Session(dry ? 'codex-dry-run' : 'codex-live');
  const dryEnv = dry ? { OVERSEER_CODEX_PATH: path.join(repoRoot, 'fixtures/fake-harness/replay.js'), OVERSEER_HARNESS_ENV_PASSTHROUGH: 'REPLAY_FILE,REPLAY_WRITE,REPLAY_DELAY_MS',
    REPLAY_FILE: path.join(repoRoot, 'fixtures/transcripts/codex-0.155-exec-subagent-live.jsonl'), REPLAY_WRITE: 'docs/dogfood/hello.md=Hello from an Overseer dogfood run', REPLAY_DELAY_MS: '3000' } : {};
  const result = { checks: [], model: MODEL };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const repo = repoRoot;
  const porcelain = () => cp.execFileSync('git', ['status', '--porcelain'], { cwd: repo, encoding: 'utf8' }).split('\n').filter(l => l && !l.includes('docs/verification/evidence/')).join('\n');
  const before = porcelain();
  try {
    s.settings();
    s.install(latestVsix());
    s.launch(repo, dryEnv);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'Overseer status bar');
    const status = s.ctl('profile.status', { id: 'system-codex' });
    check('codex profile signed in with ChatGPT account', status.logged_in && status.method === 'chatgpt-account', { version: status.version, method: status.method, plan: status.identity?.plan, account: status.identity?.account_fingerprint, api_key_present: status.identity?.has_api_key });

    await cdp.command('Overseer: Start Task with Quick Picks');
    await cdp.pick('New task: repository');
    await cdp.pick('New task: harness', 'codex');
    await cdp.pick('New task: account for', 'codex (existing login)');
    if (dry) {
      const b = await cdp.waitFor(`(() => { const b = [...document.querySelectorAll('.notification-toast .monaco-button')].find(b => b.textContent.includes('Launch anyway')); if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, 10000);
      await cdp.click(b.x, b.y);
    }
    await cdp.pick('New task: workspace');
    await cdp.pick('Start the worktree from');
    await cdp.input('Model (optional)', MODEL);
    await cdp.input('Task prompt', 'Spawn exactly one sub-agent whose only task is to reply with the word hi, and wait for it. Then create the file docs/dogfood/hello.md containing exactly one line: Hello from an Overseer dogfood run. Change nothing else and do not run tests.');
    const review = await cdp.webview('!!document.getElementById("diffs") && !!document.getElementById("follow")', 60000);
    const output = await cdp.webview('!!document.getElementById("prompt") && !!document.getElementById("log")', 30000);
    const root = s.ctl('state').runs.find(r => !r.parent_run_id && r.harness === 'codex');
    const ws = s.ctl('state').workspaces.find(w => w.id === root.workspace_id);
    s.note('run', { run: root.id, workspace: ws.path, branch: ws.branch });
    await delay(8000);
    await s.screenshot('codex-running');
    const done1 = await waitRun(s, root.id, r => !['queued', 'starting', 'running'].includes(r.status));
    check('turn 1 completed', done1.status === 'completed', { status: done1.status, reason: done1.exit_reason, native: done1.native_id });
    const state = s.ctl('state');
    const kids = state.runs.filter(r => r.parent_run_id === root.id);
    check('native Codex child captured', kids.length >= 1 && kids.every(k => k.relation_confidence.startsWith('exact')), kids.map(k => ({ id: k.id, native: k.native_id, status: k.status, title: k.title, source: k.relation_source })));
    const hello = path.join(ws.path, 'docs/dogfood/hello.md');
    check('codex edit landed in the worktree', fs.existsSync(hello) && /Hello from an Overseer dogfood run/.test(fs.readFileSync(hello, 'utf8')), fs.existsSync(hello) && fs.readFileSync(hello, 'utf8'));
    const followText = await review.eval(`document.getElementById('follow-state').textContent`);
    check('follow revealed the agent edit', /hello\.md/.test(followText) || /Following/.test(followText), followText);
    await review.waitFor(`[...document.querySelectorAll('.diff-file')].some(e => e.querySelector('.file-path')?.textContent === 'docs/dogfood/hello.md' && e.dataset.loadState === 'rendered')`, 30000);
    await s.screenshot('turn1-review');

    // Edit from the review, then save to the worktree.
    const lineSel = `[...[...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'docs/dogfood/hello.md').querySelectorAll('.editor.modified .view-lines .view-line')].find(l => /Hello/.test(l.textContent))`;
    const inner = await review.eval(`(() => { const l = ${lineSel}; l.scrollIntoView({ block: 'center' }); const r = l.getBoundingClientRect(); return { x: r.left + 20, y: r.top + r.height / 2 }; })()`);
    const anchor = await s.webviewPoint(review, '#diffs');
    const anchorInner = await review.eval(`(() => { const r = document.getElementById('diffs').getBoundingClientRect(); return { x: r.left + Math.min(r.width / 2, 40), y: r.top + Math.min(r.height / 2, 12) }; })()`);
    await cdp.click(anchor.x - anchorInner.x + inner.x, anchor.y - anchorInner.y + inner.y);
    await cdp.key('End');
    await cdp.key('Enter');
    await cdp.type('Reviewed and edited in the Overseer review.');
    await delay(1000);
    s.note('review edit state', await review.eval(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'docs/dogfood/hello.md'); return { save: e.querySelector('.save-file').disabled, status: e.querySelector('.edit-status').textContent, active: document.activeElement.className, lines: [...e.querySelectorAll('.editor.modified .view-lines .view-line')].map(l => l.textContent) }; })()`));
    await s.screenshot('review-edit');
    await review.eval(`[...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'docs/dogfood/hello.md').querySelector('.save-file').id = 'save-hello'`);
    const save = await s.webviewPoint(review, '#save-hello');
    await cdp.click(save.x, save.y);
    await delay(1500);
    check('review edit saved in the worktree', /Reviewed and edited in the Overseer review/.test(fs.readFileSync(hello, 'utf8')), fs.readFileSync(hello, 'utf8'));

    // Follow-up turn 2 goes only to this run and gets its own baseline.
    await sendFollowUp(s, output, 'Append one final line to docs/dogfood/hello.md: Edited by a follow-up turn. Change nothing else.');
    await waitRun(s, root.id, r => r.status === 'running' || r.status === 'starting', 30000);
    const done2 = await waitRun(s, root.id, r => !['queued', 'starting', 'running'].includes(r.status));
    check('follow-up turn completed', done2.status === 'completed', { status: done2.status, reason: done2.exit_reason });
    const opts = s.ctl('comparison.options', { run_id: root.id });
    const latest = opts.options.find(o => o.mode === 'latest_run');
    const diff = s.ctl('workspace.diff', { workspace_id: ws.id, base: latest.base });
    check('latest-run comparison shows only turn-2 changes', diff.changes.length === 1 && diff.changes[0].path === 'docs/dogfood/hello.md' && diff.changes[0].status === 'M', { base: latest.base, detail: latest.detail, changes: diff.changes });
    const taskStart = opts.options.find(o => o.mode === 'task_start');
    const diffS = s.ctl('workspace.diff', { workspace_id: ws.id, base: taskStart.base });
    check('task-start comparison shows the file as added', diffS.changes.some(c => c.path === 'docs/dogfood/hello.md' && c.status === 'A'), diffS.changes);
    await delay(3000);
    await s.screenshot('turn2-review');

    // Checks on the result, preserved in the worktree.
    const diffCheck = cp.spawnSync('git', ['diff', '--check', 'HEAD'], { cwd: ws.path, encoding: 'utf8' });
    check('git diff --check passes on the dogfood worktree', diffCheck.status === 0, diffCheck.stdout || 'clean');
    fs.writeFileSync(path.join(s.evidence, 'dogfood-hello.md'), fs.readFileSync(hello, 'utf8'));

    // Turn 3 keeps running while VS Code is closed, then is visible after reopening.
    await sendFollowUp(s, output, 'Run the shell command `sleep 25` and then reply with exactly: done');
    await waitRun(s, root.id, r => r.status === 'running', 60000);
    await delay(4000);
    await s.screenshot('turn3-before-close');
    await s.quit();
    const whileClosed = s.ctl('state').runs.find(r => r.id === root.id);
    check('run still active after VS Code closed', whileClosed.status === 'running', { status: whileClosed.status });
    const done3 = await waitRun(s, root.id, r => !['queued', 'starting', 'running'].includes(r.status), 180000);
    check('run completed while VS Code was closed', done3.status === 'completed', { status: done3.status, reason: done3.exit_reason });
    s.launch(repo, dryEnv);
    const cdp2 = await s.connect();
    await cdp2.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'Overseer status bar after reopen');
    const icon = await cdp2.waitFor(`(() => { const a = [...document.querySelectorAll('.activitybar .action-item a, .activitybar .action-label')].find(a => /^Overseer/.test(a.getAttribute('aria-label') || '')); if (!a) return null; const b = a.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`, 20000, 'Overseer activity icon');
    await cdp2.click(icon.x, icon.y);
    await delay(1500);
    const row = await cdp2.waitFor(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].find(r => /codex/.test(r.getAttribute('aria-label') || r.textContent) && /completed/.test(r.textContent)); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2, text: r.textContent }; })()`, 20000, 'codex row');
    check('reopened UI shows the run completed from daemon state', /completed/.test(row.text), row.text);
    await cdp2.click(row.x, row.y);
    const output2 = await cdp2.webview('!!document.getElementById("prompt") && !!document.getElementById("log")', 30000);
    const log = await output2.waitFor(`(() => { const t = document.getElementById('log').innerText; return /done/.test(t) && /sleep 25/.test(t) ? t : null; })()`, 20000);
    check('reopened output shows the turn finished while closed', /turn completed/.test(log) || /■ turn completed/.test(log), log.slice(-600));
    await s.screenshot('reopened');

    // Turn 4: interrupt from the UI.
    await sendFollowUp(s, output2, 'Run the shell command `sleep 90` and then reply with exactly: done');
    await waitRun(s, root.id, r => r.status === 'running', 60000);
    await delay(6000);
    await output2.waitFor(`!document.getElementById('interrupt').disabled`, 20000);
    const stop = await s.webviewPoint(output2, '#interrupt');
    await cdp2.click(stop.x, stop.y);
    const done4 = await waitRun(s, root.id, r => !['queued', 'starting', 'running'].includes(r.status), 60000);
    check('interrupt from UI stops the codex run', done4.status === 'interrupted', { status: done4.status, reason: done4.exit_reason });
    await delay(1500);
    await s.screenshot('interrupted');
    const turns = s.ctl('run.turns', { run_id: root.id });
    result.turns = turns.map(t => ({ n: t.n, status: t.status, snapshot: t.snapshot_id }));
    const usage = s.ctl('events.list', { run_id: root.id, limit: 5000 }).events.filter(e => e.kind === 'usage').map(e => e.payload);
    result.usage = usage;
    result.run = root.id; result.workspace = ws.path; result.branch = ws.branch;
    const after = porcelain();
    check('source checkout status unchanged by the run', after === before, { before, after });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message));
    result.error = error.message;
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
