// Packaged-UI scenario: create an OpenCode (mock model) task from the real VS Code UI,
// watch output, Follow agent edits across/within files, pause by scrolling, resume,
// turn Follow off, edit the worktree from the review, send a follow-up, interrupt.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, snapshotTree, startMock, openCodeConfig, latestVsix, delay, git } = require('./harness');

(async () => {
  const s = new Session(process.env.SCENARIO_NAME || 'main');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const mock = startMock(s.root, { MOCK_STEP_DELAY_MS: '2500' });
  try {
    const repo = makeRepo(path.join(s.root, 'repo'));
    const before = snapshotTree(repo);
    s.settings();
    s.install(latestVsix());
    s.launch(repo);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'Overseer status bar');
    await s.screenshot('activated');

    // Account profile for OpenCode through the UI; its config points at the local mock.
    await cdp.command('Overseer: Add Account Profile');
    await cdp.pick('Harness for the new account profile', 'opencode');
    await cdp.input('Profile name', 'OpenCode mock');
    await delay(800);
    await cdp.key('Escape');
    const profile = s.ctl('profile.list').find(p => p.name === 'OpenCode mock');
    check('profile created from UI', profile, profile && { id: profile.id, harness: profile.harness });
    fs.mkdirSync(path.join(profile.home, 'config/opencode'), { recursive: true });
    fs.writeFileSync(path.join(profile.home, 'config/opencode/opencode.json'), openCodeConfig(await mock.port()));

    // New task through the command palette and quick picks.
    await cdp.command('Overseer: New Task');
    await cdp.pick('New task: repository');
    await cdp.pick('New task: harness', 'opencode');
    await cdp.pick('New task: account profile', 'OpenCode mock');
    // Not signed in (the mock needs no credentials): choose "Launch anyway" in the notification.
    await cdp.waitFor(`[...document.querySelectorAll('.notification-toast .monaco-button')].some(b => b.textContent.includes('Launch anyway'))`, 10000, 'launch anyway button');
    const btn = await cdp.evalWorkbench(`(() => { const b = [...document.querySelectorAll('.notification-toast .monaco-button')].find(b => b.textContent.includes('Launch anyway')); const r = b.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
    await cdp.click(btn.x, btn.y);
    await cdp.pick('New task: workspace');
    await cdp.pick('Start the worktree from');
    await cdp.input('Model (optional)', 'mock/mock-coder');
    await cdp.input('Task prompt', 'sequence 8');
    await s.screenshot('task-launched');

    const review = await cdp.webview('!!document.getElementById("diffs") && !!document.getElementById("follow")', 60000);
    const output = await cdp.webview('!!document.getElementById("prompt") && !!document.getElementById("log")', 30000);
    const state1 = s.ctl('state');
    const run = state1.runs.find(r => !r.parent_run_id && r.harness === 'opencode');
    const ws = state1.workspaces.find(w => w.id === run.workspace_id);
    check('run created in isolated worktree', ws.kind === 'worktree' && ws.path !== repo, { run: run.id, workspace: ws.path, branch: ws.branch });
    check('follow on for launched run', await review.waitFor(`document.getElementById('follow').checked`, 15000));

    // Follow across and within files: record reveal targets as the agent edits.
    const reveals = [];
    const end = Date.now() + 40000;
    while (Date.now() < end && reveals.length < 4) {
      const text = await review.eval(`document.getElementById('follow-state').textContent`);
      if (text.startsWith('Following:') && reveals[reveals.length - 1] !== text) { reveals.push(text); s.note('follow', text); if (reveals.length === 2) await s.screenshot('following'); }
      await delay(250);
    }
    const files = new Set(reveals.map(r => r.match(/Following: (\S+?):/)?.[1]));
    check('follow navigated across files', files.has('a.txt') && files.has('b.txt'), reveals);
    const aLines = reveals.filter(r => r.includes('a.txt:')).map(r => Number(r.match(/:(\d+)/)[1]));
    check('follow revealed distant lines', reveals.length >= 3 && new Set(reveals.map(r => r.match(/:(\d+)/)?.[1])).size >= 3, { aLines });
    const outText = await output.eval(`document.getElementById('conv').innerText`);
    check('output panel streams events', /edit/.test(outText) && /a\.txt|b\.txt/.test(outText), outText.slice(0, 400));

    // Manual scroll pauses Follow; position is then left alone until Resume.
    const point = await s.webviewPoint(review, '#diffs');
    await cdp.wheel(point.x, point.y + 100, 400);
    await delay(500);
    const paused = await review.eval(`({ state: document.getElementById('follow-state').textContent, resume: !document.getElementById('resume').hidden, top: document.getElementById('diffs').scrollTop })`);
    check('scroll pauses follow with visible Resume', paused.resume && /paused/i.test(paused.state), paused);
    await s.screenshot('paused');
    await delay(6000);
    const stillPaused = await review.eval(`({ state: document.getElementById('follow-state').textContent, top: document.getElementById('diffs').scrollTop })`);
    check('paused follow does not move the view', Math.abs(stillPaused.top - paused.top) < 2 && /paused/i.test(stillPaused.state), stillPaused);
    const resume = await s.webviewPoint(review, '#resume');
    await cdp.click(resume.x, resume.y);
    await delay(800);
    const resumed = await review.eval(`({ state: document.getElementById('follow-state').textContent, resume: !document.getElementById('resume').hidden })`);
    check('resume restarts follow', !resumed.resume && /Following/.test(resumed.state), resumed);

    // Selecting another file in the navigator also pauses Follow.
    await review.eval(`[...document.querySelectorAll('#tree .file')].find(b => b.textContent.includes('b.txt')).id = 'nav-b'`);
    const navB = await s.webviewPoint(review, '#nav-b');
    await cdp.click(navB.x, navB.y);
    await delay(500);
    const pausedBySelect = await review.eval(`({ state: document.getElementById('follow-state').textContent, resume: !document.getElementById('resume').hidden })`);
    check('selecting another file pauses follow', pausedBySelect.resume && /paused/i.test(pausedBySelect.state), pausedBySelect);
    const resume2 = await s.webviewPoint(review, '#resume');
    await cdp.click(resume2.x, resume2.y);
    await delay(500);
    const baseTitle = await review.eval(`document.getElementById('base').title`);
    check('base icon identifies snapshot and provenance', /Latest run/.test(baseTitle) && /Base: [0-9a-f]{40}/.test(baseTitle) && /snapshot s-/.test(baseTitle), baseTitle);

    // Follow off preserves position during further edits.
    const box = await s.webviewPoint(review, '#follow');
    await cdp.click(box.x, box.y);
    await delay(500);
    const offTop = await review.eval(`document.getElementById('diffs').scrollTop`);
    await delay(6000);
    const offAfter = await review.eval(`({ top: document.getElementById('diffs').scrollTop, checked: document.getElementById('follow').checked })`);
    check('follow off preserves position', !offAfter.checked && Math.abs(offAfter.top - offTop) < 2, { offTop, ...offAfter });

    // Wait for the run to finish its 8 edits.
    for (let i = 0; i < 60; i++) { const r = s.ctl('state').runs.find(x => x.id === run.id); if (!['queued', 'starting', 'running'].includes(r.status)) break; await delay(1000); }
    const done = s.ctl('state').runs.find(x => x.id === run.id);
    check('run completed', done.status === 'completed', { status: done.status, reason: done.exit_reason });
    await s.screenshot('completed');

    // Edit the worktree from the review (a.txt working side) and save.
    await review.waitFor(`[...document.querySelectorAll('.diff-file')].some(e => e.querySelector('.file-path')?.textContent === 'a.txt' && e.dataset.loadState === 'rendered')`, 20000);
    const lineSel = `[...[...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'a.txt').querySelectorAll('.editor.modified .view-lines .view-line')].find(l => /agent.edit/.test(l.textContent))`;
    const linePoint = await review.eval(`(() => { const l = ${lineSel}; l.scrollIntoView({ block: 'center' }); const r = l.getBoundingClientRect(); return { x: r.left + Math.min(60, r.width / 2), y: r.top + r.height / 2 }; })()`);
    // Convert inner coordinates to page coordinates using the #diffs anchor.
    const diffsPoint = await s.webviewPoint(review, '#diffs');
    const diffsInner = await review.eval(`(() => { const r = document.getElementById('diffs').getBoundingClientRect(); return { x: r.left + Math.min(r.width / 2, 40), y: r.top + Math.min(r.height / 2, 12) }; })()`);
    const abs = { x: diffsPoint.x - diffsInner.x + linePoint.x, y: diffsPoint.y - diffsInner.y + linePoint.y };
    await cdp.click(abs.x, abs.y);
    await cdp.type('USER EDIT FROM REVIEW ');
    await delay(1200);
    const save = await review.eval(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'a.txt'); return { disabled: e.querySelector('.save-file').disabled, status: e.querySelector('.edit-status').textContent }; })()`);
    s.note('save button', save);
    await s.screenshot('edited-in-review');
    await review.eval(`[...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'a.txt').querySelector('.save-file').id = 'save-a'`);
    const savePoint = await s.webviewPoint(review, '#save-a');
    await cdp.click(savePoint.x, savePoint.y);
    await delay(1500);
    const worktreeA = fs.readFileSync(path.join(ws.path, 'a.txt'), 'utf8');
    check('review edit saved to the selected worktree', worktreeA.includes('USER EDIT FROM REVIEW'), worktreeA.split('\n').find(l => l.includes('USER EDIT')));
    check('source checkout untouched', JSON.stringify(snapshotTree(repo)) === JSON.stringify(before), { status: snapshotTree(repo).status });

    // Follow-up and interrupt from the output panel.
    const prompt = await s.webviewPoint(output, '#prompt');
    await cdp.click(prompt.x, prompt.y);
    await cdp.type('slow please');
    const send = await s.webviewPoint(output, '#send');
    await cdp.click(send.x, send.y);
    for (let i = 0; i < 30; i++) { const r = s.ctl('state').runs.find(x => x.id === run.id); if (r.status === 'running') break; await delay(500); }
    await delay(3000);
    await s.screenshot('follow-up-running');
    await output.waitFor(`!document.getElementById('interrupt').disabled`, 10000);
    const stop = await s.webviewPoint(output, '#interrupt');
    await cdp.click(stop.x, stop.y);
    for (let i = 0; i < 30; i++) { const r = s.ctl('state').runs.find(x => x.id === run.id); if (r.status === 'interrupted') break; await delay(500); }
    const interrupted = s.ctl('state').runs.find(x => x.id === run.id);
    check('interrupt from UI', interrupted.status === 'interrupted', { status: interrupted.status, reason: interrupted.exit_reason });
    const turns = s.ctl('run.turns', { run_id: run.id });
    check('follow-up created a new turn with its own snapshot', turns.length === 2 && turns[0].snapshot_id !== turns[1].snapshot_id, turns.map(t => ({ n: t.n, status: t.status, snapshot: t.snapshot_id })));
    await delay(1000);
    await s.screenshot('interrupted');
    result.run = run.id; result.workspace = ws.path;

    // Native child selected in the tree: controls are disabled with an explanation.
    const deleg = s.ctl('task.create', { repo, harness: 'opencode', profile_id: profile.id, model: 'mock/mock-coder', prompt: 'please delegate twice', title: 'delegation' });
    for (let i = 0; i < 40; i++) { if (s.ctl('state').runs.filter(r => r.task_id === deleg.task.id).length >= 3 && s.ctl('state').runs.find(r => r.id === deleg.run.id).status === 'completed') break; await delay(500); }
    const icon = await cdp.waitFor(`(() => { const a = [...document.querySelectorAll('.activitybar .action-item a, .activitybar .action-label')].find(a => /^Overseer/.test(a.getAttribute('aria-label') || '')); if (!a) return null; const b = a.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`, 20000);
    await cdp.click(icon.x, icon.y);
    const childRow = await cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent && /grandchild hi/.test(r.textContent)); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 80, y: b.top + b.height / 2, text: r.textContent }; })()`, 20000, 'grandchild row');
    check('three-level native tree visible in the UI', /native child/.test(childRow.text), childRow.text);
    await cdp.click(childRow.x, childRow.y);
    const childOut = await cdp.webview(`document.getElementById('title')?.textContent.includes('grandchild hi')`, 20000);
    const ctl = await childOut.eval(`({ interrupt: document.getElementById('interrupt').disabled, why: document.getElementById('interrupt-why').textContent, send: document.getElementById('send').disabled, sendWhy: document.getElementById('send-why').textContent, ws: document.getElementById('ws').textContent })`);
    check('child controls disabled with explanation', ctl.interrupt && ctl.send && /parent/.test(ctl.why) && /top-level/.test(ctl.sendWhy) && /shared with parent/.test(ctl.ws), ctl);
    await s.screenshot('native-child-selected');

    // Daemon crash while the UI is open: UI shows disconnected, restarts the daemon and reconnects.
    const hello = s.ctl('hello');
    const tasksBefore = s.ctl('state').tasks.map(t => t.id).sort();
    process.kill(hello.pid, 'SIGKILL');
    const sawDisconnect = await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /disconnected/.test(e.textContent))`, 5000).catch(() => false);
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer [0-9]+ active/.test(e.textContent))`, 20000, 'reconnected');
    const hello2 = s.ctl('hello');
    const tasksAfter = s.ctl('state').tasks.map(t => t.id).sort();
    check('UI survives daemon crash: disconnected state, daemon restarted, same tasks', sawDisconnect && hello2.pid !== hello.pid && JSON.stringify(tasksBefore) === JSON.stringify(tasksAfter), { sawDisconnect, oldPid: hello.pid, newPid: hello2.pid, tasks: tasksAfter.length });
    await s.screenshot('reconnected');
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message));
    result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); mock.child.kill(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
