// Packaged-UI scenario for AC-232 (the review says what it shows) and AC-243 (merge from the agent,
// and it reads merged afterwards), with generic fixture agents in a disposable repository (no paid turns).
// 1. An agent commits a new file (data/features.js) and leaves an untracked .env, then a second
//    turn changes nothing (the owner's "0 files" case): its review counts both files, the new one
//    all added; Save says "Save your changes to the agent's copy"; each hunk offers Keep or Undo.
// 2. The repository has no remote: the chat offers Merge into main and Publish to GitHub, not
//    Open PR; Open PR from the palette offers the local merge in the window, never a dialog.
// 3. Merge from the chat's button: one confirmation listing the files, the .env apart; the
//    repository's pre-commit hook runs; afterwards the chat, its conversation, the side bar, the
//    grid and the review read "Merged into main (commit)", and the chat offers Clean up.
// 4. A second agent's merge stops on conflicts: the chat says so and offers Cancel merge; a GitHub
//    remote's Open PR is refused while the worktree is mid-merge; Cancel restores the worktree.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, git } = require('./harness');

(async () => {
  const s = new Session('review-merge');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const fingerprint = dir => JSON.stringify({ status: git(dir, 'status', '--porcelain=v1', '--untracked-files=all'), head: git(dir, 'rev-parse', 'HEAD'), index: git(dir, 'diff', '--cached'), a: fs.readFileSync(path.join(dir, 'a.txt'), 'utf8'), notes: fs.existsSync(path.join(dir, 'notes.txt')) && fs.readFileSync(path.join(dir, 'notes.txt'), 'utf8') });
  try {
    const repo = makeRepo(path.join(s.root, 'shop'), { dirty: false });
    // A pre-commit hook that logs each run (the worktrees share it).
    const hooks = path.join(s.root, 'hooks'), hookLog = path.join(s.root, 'hook.log');
    fs.mkdirSync(hooks, { recursive: true });
    fs.writeFileSync(path.join(hooks, 'pre-commit'), `#!/bin/sh\necho "pre-commit $(git rev-parse --abbrev-ref HEAD)" >> '${hookLog}'\nexit 0\n`, { mode: 0o755 });
    git(repo, 'config', 'core.hooksPath', hooks);
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const runState = id => s.ctl('state').runs.find(r => r.id === id);
    const waitDone = async id => { for (let i = 0; i < 80 && !['completed', 'failed', 'interrupted'].includes(runState(id).status); i++) await delay(250); return runState(id); };
    const agent = (title, script) => s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', script], prompt: '', title, workspace_mode: 'worktree' });
    const dialog = async (button, timeout = 30000) => {
      const d = await cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d) return null; const b = [...d.querySelectorAll('.monaco-button')].find(b => b.textContent.trim() === ${JSON.stringify(button)}); if (!b) return null; const r = b.getBoundingClientRect(); return { text: d.innerText, x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, timeout, 'dialog ' + button);
      await s.screenshot('dialog-' + button.replace(/\W+/g, '-').toLowerCase());
      await cdp.click(d.x, d.y);
      await delay(1000);
      return d.text;
    };
    const chatOf = runId => cdp.webview(`window.__overseer?.selected?.() === ${JSON.stringify(runId)} && !!document.getElementById('land')`, 30000);
    const click = async (frame, selector) => { const p = await s.webviewPoint(frame, selector); await cdp.click(p.x, p.y); await delay(600); };
    const hookRuns = () => (fs.existsSync(hookLog) ? fs.readFileSync(hookLog, 'utf8').trim().split('\n').filter(Boolean).length : 0);

    // ---- 1. The review of an agent that committed a new file, then had a turn that changed nothing.
    const features = agent('Add the features list', "mkdir -p data && printf 'export const features = [];\\nexport default features;\\n' > data/features.js && git add data && git -c user.name=Agent -c user.email=agent@example.invalid commit -qm 'add features' || true; printf 'TOKEN=fixture-not-a-secret\\n' > .env");
    await waitDone(features.run.id);
    s.ctl('run.follow_up', { run_id: features.run.id, prompt: '' });
    await delay(500); await waitDone(features.run.id);
    const ws = features.workspace.path;
    check('fixture: the agent committed data/features.js on its branch and left .env untracked', git(ws, 'log', '-1', '--format=%s') === 'add features' && git(ws, 'status', '--porcelain') === '?? .env', { log: git(ws, 'log', '--oneline', '-2'), status: git(ws, 'status', '--porcelain') });
    await s.selectAgent('Add the features list', { settle: 2000 });
    const chat = await chatOf(features.run.id);
    await click(chat, '#review');
    const review = await cdp.webview(`document.body.dataset.runId === ${JSON.stringify(features.run.id)} && !!document.getElementById('diffs')`, 60000);
    const shown = await review.waitFor(`(() => { if (document.body.dataset.checking !== 'false') return null; const rows = [...document.querySelectorAll('.diff-file')].map(e => ({ path: e.querySelector('.file-path').textContent, status: e.querySelector('.status').textContent, browsed: e.classList.contains('browsed'), added: e.dataset.additions, removed: e.dataset.deletions })); const f = rows.find(r => r.path === 'data/features.js'); if (!f || f.added === undefined) return null; return { total: document.getElementById('total').textContent, count: document.getElementById('total').dataset.count, base: document.getElementById('base-label').textContent, rows }; })()`, 30000).catch(() => null);
    await s.screenshot('review-committed-new-file');
    const row = shown && shown.rows.find(r => r.path === 'data/features.js');
    check('the review counts the committed new file (never "0 files") and shows it added, every line', shown && shown.count === '2' && /^2 files/.test(shown.total) && row.status === 'A' && !row.browsed && row.added === '2' && row.removed === '0' && shown.rows.some(r => r.path === '.env' && r.status === 'A'), shown);
    const words = await review.waitFor(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'data/features.js'); const s = e && e.querySelector('.save-file'); const hunk = e && e.querySelector('.hunk-actions'); if (!s || !hunk) return null; return { save: s.textContent, saveHidden: s.hidden, saveTitle: s.title, keep: hunk.querySelector('.hunk-accept').textContent, undo: hunk.querySelector('.hunk-reject').textContent, keepTitle: hunk.querySelector('.hunk-accept').title, undoTitle: hunk.querySelector('.hunk-reject').title }; })()`, 20000).catch(() => null);
    check('Save says what it does ("Save your changes to the agent\'s copy") and shows only once you have edits', words && words.save === "Save your changes to the agent's copy" && words.saveHidden && /does not keep or undo/.test(words.saveTitle), words);
    check("each hunk is a clear choice: Keep (the agent's change stays) or Undo (put back what was there)", words && words.keep === 'Keep' && words.undo === 'Undo' && /change stays/.test(words.keepTitle) && /put back/.test(words.undoTitle), words);

    // ---- 2. No remote: Merge into main and Publish to GitHub, never Open PR or a dialog.
    const land = () => chat.eval(`(() => { const b = document.getElementById('land'); return { hidden: b.hidden, state: b.dataset.state, text: document.getElementById('land-text')?.textContent, buttons: [...b.querySelectorAll('button')].map(x => x.id + ':' + x.textContent.trim()) }; })()`);
    const ready = await chat.waitFor(`document.getElementById('land').dataset.state === 'ready'`, 20000).then(land, () => land());
    await s.screenshot('chat-no-remote');
    check('no remote: the chat offers Merge into main and Publish to GitHub, not Open PR', ready.buttons.includes('merge-now:Merge into main') && ready.buttons.includes('publish:Publish to GitHub…') && !ready.buttons.some(b => b.startsWith('open-pr')), ready);
    const reviewLand = await review.waitFor(`(() => { const l = document.getElementById('land'); return l && !l.hidden && !document.getElementById('land-merge').hidden && { merge: document.getElementById('land-merge').textContent, pr: !document.getElementById('land-pr').hidden, publish: !document.getElementById('land-publish').hidden }; })()`, 20000).catch(() => null);
    check("the review has the same Merge button (and Publish, no Open PR)", reviewLand && reviewLand.merge === 'Merge into main' && !reviewLand.pr && reviewLand.publish, reviewLand);
    await cdp.command('Overseer: Open Pull Request…');
    const offered = await cdp.waitQuickTitle('no GitHub remote', 15000).then(() => cdp.quickInputState(), () => null);
    const alert = await cdp.evalWorkbench(`!!document.querySelector('.monaco-dialog-box')`);
    await s.screenshot('open-pr-no-remote');
    check('Open PR with no remote offers the local merge (or Publish to GitHub) in the window, never a dialog', offered && offered.rows.some(r => /Merge into main/.test(r)) && offered.rows.some(r => /Publish to GitHub/.test(r)) && !alert, { offered, alert });
    await cdp.key('Escape'); await delay(400);

    // ---- 3. Merge from the chat's button.
    const hooksBefore = hookRuns();
    await click(chat, '#merge-now');
    const confirm = await dialog('Merge into main');
    check('one confirmation lists the files that land, and the untracked .env apart', /data\/features\.js/.test(confirm) && /Not tracked by Git yet[\s\S]*\.env/.test(confirm) && /Git's hooks run/.test(confirm), confirm);
    const commit = await (async () => { for (let i = 0; i < 40; i++) { if (fs.existsSync(path.join(repo, 'data/features.js'))) return git(repo, 'rev-parse', 'main'); await delay(250); } return null; })();
    const short = (commit || '').slice(0, 7);
    check('merged: main has the agent\'s file, and the pre-commit hook ran for the commit of .env', !!commit && fs.existsSync(path.join(repo, '.env')) && hookRuns() > hooksBefore, { log: git(repo, 'log', '--oneline', '-3'), hooks: fs.existsSync(hookLog) && fs.readFileSync(hookLog, 'utf8') });
    const merged = `Merged into main (${short})`;
    const after = await chat.waitFor(`document.getElementById('land-text')?.textContent === ${JSON.stringify(merged)}`, 20000).then(land, () => land());
    const line = await chat.waitFor(`[...document.querySelectorAll('#conv .sys')].map(e => e.textContent.trim()).find(t => t === ${JSON.stringify(merged)}) || null`, 10000).catch(() => null);
    await s.screenshot('chat-merged');
    check('the chat reads "Merged into main (commit)" and offers Clean up', after.state === 'merged' && after.text === merged && after.buttons.includes('cleanup-now:Clean up'), after);
    check('its conversation says it once, with the commit (no raw "merge back" lines)', line === merged && !(await chat.eval(`[...document.querySelectorAll('#conv .sys')].some(e => /^merge back$/i.test(e.textContent.trim()))`)), line);
    const rows = await (async () => { for (let i = 0; i < 40; i++) { const r = (await s.agentRows()).find(r => r.label === 'Add the features list'); if (r && r.description.includes(merged)) return r; await delay(250); } return (await s.agentRows()).find(r => r.label === 'Add the features list'); })();
    check('the side bar reads "Merged into main (commit)"', rows && rows.description.includes(merged), rows);
    const reviewMerged = await review.waitFor(`document.getElementById('land-text')?.textContent === ${JSON.stringify(merged)} && !document.getElementById('land-cleanup').hidden && document.getElementById('land-merge').hidden`, 20000).then(() => true, () => false);
    check('the review reads "Merged into main (commit)" and offers Clean up, no Merge', reviewMerged, await review.eval(`document.getElementById('land')?.innerText`));
    // The grid: the finished agent pinned to it.
    const dash = await s.editorView();
    await dash.eval(`window.overseerApi.postMessage({ type: 'pin', runId: ${JSON.stringify(features.run.id)}, on: true })`);
    await cdp.command('Overseer: Toggle Agent Grid');
    const tile = await dash.waitFor(`(() => { const t = document.querySelector('.grid .tile[data-run=${JSON.stringify(features.run.id)}] .tile-landed'); return t && !t.hidden ? t.textContent : null; })()`, 20000).catch(() => null);
    await s.screenshot('grid-merged');
    check('the grid tile reads "Merged into main (commit)"', tile === merged, tile);
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(800);

    // ---- 4. A merge that stops on conflicts, cancelled from the chat.
    const conflict = agent('Change line five', "sed -i '' 's/^L5: original$/L5: from agent/' a.txt; printf 'agent notes\\n' > notes.txt");
    await waitDone(conflict.run.id);
    fs.writeFileSync(path.join(repo, 'a.txt'), fs.readFileSync(path.join(repo, 'a.txt'), 'utf8').replace('L5: original', 'L5: from main'));
    git(repo, 'commit', '-qam', 'main changes L5');
    const cws = conflict.workspace.path;
    const before = fingerprint(cws);
    await s.selectAgent('Change line five', { settle: 2000 });
    const chat2 = await chatOf(conflict.run.id);
    await chat2.waitFor(`document.getElementById('land').dataset.state === 'ready'`, 20000);
    await click(chat2, '#merge-now');
    await dialog('Merge into main');
    const stopped = await chat2.waitFor(`document.getElementById('land').dataset.state === 'conflicts'`, 20000).then(() => chat2.eval(`(() => { const b = document.getElementById('land'); return { text: document.getElementById('land-text').textContent, buttons: [...b.querySelectorAll('button')].map(x => x.id + ':' + x.textContent.trim()) }; })()`), () => null);
    await s.screenshot('chat-merge-stopped');
    check('a merge stopped on conflicts says so in the chat, with Finish merge and Cancel merge', stopped && stopped.text === 'Merge stopped: conflicts in a.txt' && stopped.buttons.includes('cancel-merge:Cancel merge') && stopped.buttons.includes('merge-now:Finish merge'), stopped);
    check('main is untouched while the merge is stopped', !/from agent/.test(fs.readFileSync(path.join(repo, 'a.txt'), 'utf8')), git(repo, 'log', '--oneline', '-2'));
    git(repo, 'remote', 'add', 'origin', 'https://github.com/test-owner/shop.git');
    const pr = s.ctl('workspace.pr_plan', { workspace_id: conflict.workspace.id });
    check('Open PR refuses the worktree in the middle of the merge', pr.ok === false && /unfinished|in progress/.test(pr.reason), pr);
    await click(chat2, '#cancel-merge');
    const back = await chat2.waitFor(`document.getElementById('land').dataset.state === 'ready'`, 20000).then(() => true, () => false);
    await s.screenshot('chat-merge-cancelled');
    const restored = fingerprint(cws);
    check('Cancel merge restores the worktree as it was before the merge (the work uncommitted again)', back && restored === before && !git(cws, 'status', '--porcelain').includes('UU'), { before: JSON.parse(before).status, after: JSON.parse(restored).status });
    const cancelLine = await chat2.eval(`[...document.querySelectorAll('#conv .sys')].map(e => e.textContent.trim()).filter(t => /^Merge (stopped|cancelled)/.test(t))`);
    check('the conversation says the merge stopped and was cancelled', cancelLine.some(t => t === 'Merge stopped: conflicts in a.txt') && cancelLine.some(t => /^Merge cancelled/.test(t)), cancelLine);
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { s.note('toasts', await s.cdp.evalWorkbench(`[...document.querySelectorAll('.notification-toast, .notifications-center .notification-list-item')].map(t => t.innerText)`)); } catch {}
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
