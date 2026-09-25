// LIVE scenario for AC-44 (tiny Codex gpt-5.6-luna prompts) in a disposable repository:
// a clean merge back, a conflicting merge back resolved by the same Codex session and
// reviewed before completing, a dirty target checkout refused and left untouched, the action
// disabled with an explanation while a run is active, and no automatic merge ever.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, git } = require('./harness');

const MODEL = process.env.CODEX_MODEL || 'gpt-5.6-luna';

(async () => {
  const s = new Session('merge');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const ACTIVE = ['queued', 'starting', 'running', 'waiting_for_user'];
  const fingerprint = dir => JSON.stringify({ status: git(dir, 'status', '--porcelain=v1'), head: git(dir, 'rev-parse', 'HEAD'), index: git(dir, 'diff', '--cached'), readme: fs.readFileSync(path.join(dir, 'README.md'), 'utf8') });
  try {
    const repo = makeRepo(path.join(s.root, 'merge-demo'), { dirty: false });
    s.settings({ 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const runState = id => s.ctl('state').runs.find(r => r.id === id);
    const waitDone = async (id, secs = 300) => { for (let i = 0; i < secs * 2 && ACTIVE.includes(runState(id).status); i++) await delay(500); return runState(id); };
    const codex = (title, prompt) => s.ctl('task.create', { repo, harness: 'codex', profile_id: 'system-codex', model: MODEL, title, prompt });
    const selectRun = async title => {
      await s.openOverseerView();
      const pt = await cdp.waitFor(`(() => { const rows = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top);
        const i = rows.findIndex(r => r.textContent.includes(${JSON.stringify(title)})); const r = rows[i + 1]; if (!r || !/codex|generic/.test(r.textContent)) return null; const b = r.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 30000, 'run row ' + title);
      await cdp.click(pt.x, pt.y);
      await delay(1500);
    };
    const panel = runId => cdp.webview(`document.body.dataset.runId === ${JSON.stringify(runId)} && !!document.getElementById('merge')`, 30000);
    const clickMerge = async runId => {
      const p = await panel(runId);
      await p.waitFor(`!document.getElementById('merge').disabled`, 20000);
      const pt = await s.webviewPoint(p, '#merge');
      await cdp.click(pt.x, pt.y);
      await delay(800);
    };
    const dialog = async (button, timeout = 30000) => {
      const d = await cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d) return null; const b = [...d.querySelectorAll('.monaco-button')].find(b => b.textContent.trim() === ${JSON.stringify(button)}); if (!b) return null; const r = b.getBoundingClientRect(); return { text: d.innerText, x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, timeout, 'dialog ' + button);
      await s.screenshot('dialog-' + button.replace(/\W+/g, '-').toLowerCase());
      await cdp.click(d.x, d.y);
      await delay(1000);
      return d.text;
    };
    const toast = pattern => cdp.waitFor(`[...document.querySelectorAll('.notification-toast')].map(t => t.innerText).find(t => ${pattern}.test(t)) || null`, 60000).catch(() => null);
    const clearToasts = async () => { await cdp.command('Notifications: Clear All Notifications'); await delay(400); };

    // Two live runs; the target branch moves on under the second one.
    const clean = codex('merge clean', 'Append exactly one new line "feature from agent" to the end of b.txt. Change nothing else. Then reply exactly: done');
    const conflict = codex('merge conflict', 'In a.txt change the line "L5: original" to "L5: from agent". Change nothing else. Then reply exactly: done');
    const main0 = git(repo, 'rev-parse', 'main');
    for (const t of [clean, conflict]) check(`live run "${t.run.title}" completed`, (await waitDone(t.run.id)).status === 'completed', runState(t.run.id).status);
    await delay(1000);
    check('no automatic merge: main unchanged after the runs finished', git(repo, 'rev-parse', 'main') === main0, git(repo, 'log', '--oneline', '-3'));
    fs.writeFileSync(path.join(repo, 'a.txt'), fs.readFileSync(path.join(repo, 'a.txt'), 'utf8').replace('L5: original', 'L5: from main'));
    git(repo, 'commit', '-qam', 'main changes L5');

    // --- Clean merge back through the run panel.
    await selectRun('merge clean');
    await clickMerge(clean.run.id);
    const prepText = await dialog('Prepare Merge Back');
    const completeText = await dialog('Complete Merge Back', 60000);
    const reviewLanding = await cdp.webview(`document.getElementById('base-label')?.textContent.includes('Merge-base with main') && document.getElementById('workspace-note').textContent.includes(${JSON.stringify(clean.workspace.path)})`, 20000).then(() => true, () => false);
    await toast('/Merged overseer\\/merge-clean into main/');
    check('clean merge back: explained, prepared, reviewed (merge-base with main), then merged on confirmation',
      /Commit 1 uncommitted worktree file/.test(prepText) && /exactly what lands on main/.test(completeText) && reviewLanding && /feature from agent/.test(fs.readFileSync(path.join(repo, 'b.txt'), 'utf8')) && /Overseer merge back/.test(git(repo, 'log', '-1', '--format=%s')) && git(repo, 'status', '--porcelain') === '',
      { prepText, completeText, reviewLanding, log: git(repo, 'log', '--oneline', '-4') });
    await s.screenshot('clean-merged');
    await clearToasts();

    // --- Conflicting merge back: the same Codex session resolves it, then it is reviewed.
    await selectRun('merge conflict');
    const turnsBefore = s.ctl('run.turns', { run_id: conflict.run.id }).length;
    await clickMerge(conflict.run.id);
    await dialog('Prepare Merge Back');
    const conflictToast = await toast('/conflicts in a\\.txt/');
    const main1 = git(repo, 'rev-parse', 'main');
    for (let i = 0; i < 20 && s.ctl('run.turns', { run_id: conflict.run.id }).length === turnsBefore; i++) await delay(500);
    const resolved = await waitDone(conflict.run.id);
    const turns = s.ctl('run.turns', { run_id: conflict.run.id });
    check('conflicts are handed to the same run/session as a follow-up and main is untouched meanwhile',
      /Sent to codex as a follow-up/.test(conflictToast || '') && turns.length === turnsBefore + 1 && resolved.status === 'completed' && git(repo, 'rev-parse', 'main') === main1,
      { conflictToast, turns: turns.map(t => ({ n: t.n, status: t.status, prompt: t.prompt.slice(0, 80) })), native: resolved.native_id });
    await clearToasts();
    await clickMerge(conflict.run.id);
    const landingText = await dialog('Complete Merge Back', 60000);
    const l5 = fs.readFileSync(path.join(repo, 'a.txt'), 'utf8').split('\n')[4];
    const markers = /^(<<<<<<<|=======|>>>>>>>)/m.test(fs.readFileSync(path.join(repo, 'a.txt'), 'utf8'));
    check('resolved merge reviewed then completed: no conflict markers land on main', /a\.txt/.test(landingText) && !markers && /Overseer merge back/.test(git(repo, 'log', '-1', '--format=%s')), { l5, markers, landingText: landingText.slice(0, 300) });
    await s.screenshot('conflict-merged');
    await clearToasts();

    // --- Dirty target checkout: refused with an explanation and left untouched.
    const dirty = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'printf "generic line\\n" >> c.txt'], prompt: '', title: 'merge dirty target' });
    await waitDone(dirty.run.id, 30);
    fs.appendFileSync(path.join(repo, 'README.md'), 'user work in progress\n');
    const before = fingerprint(repo);
    await selectRun('merge dirty target');
    await clickMerge(dirty.run.id);
    const dirtyPrep = await dialog('Prepare Merge Back');
    const blocked = await toast('/ready but blocked/');
    check('dirty target checkout: refused with an explanation, checkout untouched', /uncommitted changes/.test(dirtyPrep) && /never disturbs/.test(blocked || '') && fingerprint(repo) === before, { blocked, dirtyPrep: dirtyPrep.slice(0, 300) });
    await clearToasts();
    fs.writeFileSync(path.join(repo, 'README.md'), fs.readFileSync(path.join(repo, 'README.md'), 'utf8').replace('user work in progress\n', ''));

    // --- Active run: the action is disabled with an explanation.
    const busy = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'sleep 60'], prompt: '', title: 'merge busy' });
    await selectRun('merge busy');
    const bp = await panel(busy.run.id);
    const state = await bp.waitFor(`(() => { const b = document.getElementById('merge'); return b.disabled && { disabled: b.disabled, title: b.title }; })()`, 20000);
    check('while the run is active Merge back is disabled with an explanation', state.disabled && /Wait for the run to finish/.test(state.title), state);
    s.ctl('run.interrupt', { run_id: busy.run.id });
    await s.screenshot('busy-disabled');
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { for (const r of s.ctl('state').runs.filter(r => !r.parent_run_id && ACTIVE.includes(r.status))) s.ctl('run.interrupt', { run_id: r.id }); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
