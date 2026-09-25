// Packaged-UI scenario for AC-42 (hunk Accept / Reject in the live review). Generic-harness
// runs make deterministic edits; LIVE=1 adds a tiny live Codex (gpt-5.6-luna) run whose edits
// span two files. Checks disk contents after Reject, no Git staging on Accept, native undo/redo
// in the VS Code editor, staged and untracked files, conflicts with a concurrent agent write,
// reviewed state across refresh and window reload (and clearing when the hunk changes), and
// both worktree and current-checkout workspaces.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, git } = require('./harness');

const EDIT = `sed -i '' -e 's/^L10: original$/L10: agent edit/' -e 's/^L100: original$/L100: agent edit/' -e 's/^L200: original$/L200: agent edit/' a.txt
sed -i '' -e 's/^L50: original$/L50: agent edit/' -e 's/^L250: original$/L250: agent edit/' b.txt
printf 'new one\\nnew two\\nnew three\\n' > new.txt
printf 'scratch\\n' > scratch.txt
echo done`;

(async () => {
  const s = new Session('hunks');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const line = (file, n) => fs.readFileSync(file, 'utf8').split('\n')[n - 1];
  try {
    const repo = makeRepo(path.join(s.root, 'hunk-demo'), { dirty: false });
    const repoC = makeRepo(path.join(s.root, 'current-demo'), { dirty: false });
    s.settings({ 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo);
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const sh = (r, mode, title, script) => s.ctl('task.create', { repo: r, workspace_mode: mode, harness: 'generic', program: '/bin/sh', args: ['-c', script], prompt: '', title });
    const wt = sh(repo, 'worktree', 'hunk worktree', EDIT);
    const cur = sh(repoC, 'current', 'hunk current', `sed -i '' -e 's/^L20: original$/L20: agent edit/' -e 's/^L30: original$/L30: agent edit/' a.txt; echo done`);
    const W = wt.workspace.path, C = cur.workspace.path;
    const runState = id => s.ctl('state').runs.find(r => r.id === id);
    for (const t of [wt, cur]) for (let i = 0; i < 40 && runState(t.run.id).status !== 'completed'; i++) await delay(300);
    git(W, 'add', 'b.txt'); // b.txt's agent edits are staged
    const indexBefore = git(W, 'diff', '--cached');

    await s.openOverseerView();
    const selectRun = async title => {
      const pt = await cdp.waitFor(`(() => { const rows = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top);
        const i = rows.findIndex(r => r.textContent.includes(${JSON.stringify(title)})); const r = rows[i + 1]; if (!r || !/generic|codex/.test(r.textContent)) return null; const b = r.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 20000, 'run row ' + title);
      await cdp.click(pt.x, pt.y);
      await delay(1500);
    };
    const reviewOf = ws => cdp.webview(`document.getElementById('workspace-note')?.textContent.includes(${JSON.stringify(ws)}) && !!document.querySelector('.hunk-actions')`, 30000);
    const fileState = (frame, file) => frame.eval(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === ${JSON.stringify(file)}); return e ? { hunks: Number(e.dataset.hunks || 0), reviewed: Number(e.dataset.reviewed || 0), load: e.dataset.loadState } : null; })()`);
    const waitFile = (frame, file, pred, ms = 15000) => frame.waitFor(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === ${JSON.stringify(file)}); if (!e) return ${pred.includes('missing') ? 'true' : 'false'}; const hunks = Number(e.dataset.hunks || 0), reviewed = Number(e.dataset.reviewed || 0); return e.dataset.loadState === 'rendered' && (${pred}); })()`, ms);
    // Clicks a hunk button: `which` is 'accept' or 'reject', hunk chosen by the text of its first modified line.
    const hunkButton = async (frame, file, text, which, before) => {
      const found = await frame.waitFor(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === ${JSON.stringify(file)}); if (!e || e.dataset.loadState !== 'rendered') return false;
        e.scrollIntoView({ block: 'start' });
        const lines = [...e.querySelectorAll('.editor.modified .view-line')];
        const target = lines.find(l => l.textContent.replace(/\u00a0/g, ' ').includes(${JSON.stringify(text)})); if (!target) return false;
        const top = target.getBoundingClientRect().top;
        const bars = [...e.querySelectorAll('.hunk-actions')].sort((a, b) => Math.abs(a.getBoundingClientRect().top - top) - Math.abs(b.getBoundingClientRect().top - top));
        const bar = bars[0]; if (!bar) return false; document.querySelectorAll('#hunk-btn').forEach(b => b.removeAttribute('id'));
        const b = bar.querySelector('.hunk-${which}'); b.id = 'hunk-btn'; document.getElementById('diffs').scrollTop += bar.getBoundingClientRect().top - 200; return true; })()`, 20000);
      await delay(300);
      const p = await s.webviewPoint(frame, '#hunk-btn');
      if (before) before(); // e.g. the agent writes right before the click reaches the host
      await cdp.click(p.x, p.y);
      await delay(300);
      return found;
    };

    // ---------------- worktree run
    await selectRun('hunk worktree');
    let r = await reviewOf(W);
    await waitFile(r, 'a.txt', 'hunks === 3'); await waitFile(r, 'b.txt', 'hunks === 2');
    check('review shows several hunks across two files', true, { a: await fileState(r, 'a.txt'), b: await fileState(r, 'b.txt') });
    await s.screenshot('hunks-before');
    // Reject the L100 hunk: only that region returns to the base.
    await hunkButton(r, 'a.txt', 'L100: agent edit', 'reject');
    await waitFile(r, 'a.txt', 'hunks === 2');
    for (let i = 0; i < 20 && line(path.join(W, 'a.txt'), 100) !== 'L100: original'; i++) await delay(250);
    check('Reject restores only that hunk on disk (other hunks and files untouched)', line(path.join(W, 'a.txt'), 100) === 'L100: original' && line(path.join(W, 'a.txt'), 10) === 'L10: agent edit' && line(path.join(W, 'a.txt'), 200) === 'L200: agent edit' && line(path.join(W, 'b.txt'), 50) === 'L50: agent edit',
      { l10: line(path.join(W, 'a.txt'), 10), l100: line(path.join(W, 'a.txt'), 100), l200: line(path.join(W, 'a.txt'), 200) });
    // Accept the L10 hunk: reviewed, nothing staged or written.
    const aBefore = fs.readFileSync(path.join(W, 'a.txt'), 'utf8');
    await hunkButton(r, 'a.txt', 'L10: agent edit', 'accept');
    await waitFile(r, 'a.txt', 'reviewed === 1');
    check('Accept marks the hunk reviewed without writing or staging', fs.readFileSync(path.join(W, 'a.txt'), 'utf8') === aBefore && git(W, 'diff', '--cached') === indexBefore, await fileState(r, 'a.txt'));
    await s.screenshot('accepted-and-rejected');
    // Native undo/redo in the VS Code editor for the Reject.
    await r.eval(`[...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'a.txt').querySelector('.file-path').id = 'open-a'`);
    const pa = await s.webviewPoint(r, '#open-a'); await cdp.click(pa.x, pa.y);
    await cdp.waitFor(`[...document.querySelectorAll('.tab.active')].some(t => /^a\\.txt/.test(t.getAttribute('aria-label') || ''))`, 15000);
    const editorPt = await cdp.waitFor(`(() => { const e = [...document.querySelectorAll('.editor-instance .monaco-editor .view-lines')].find(v => v.offsetParent && v.closest('.editor-group-container.active')); if (!e) return null; const b = e.getBoundingClientRect(); return { x: b.left + 80, y: b.top + 30 }; })()`, 10000);
    await cdp.click(editorPt.x, editorPt.y);
    await cdp.key('z', { meta: true });
    await delay(1500);
    const afterUndo = await waitFile(r, 'a.txt', 'hunks === 3', 10000).then(() => true, () => false);
    const dirtyUndo = await cdp.evalWorkbench(`[...document.querySelectorAll('.tab')].some(t => /^a\\.txt/.test(t.getAttribute('aria-label') || '') && t.classList.contains('dirty'))`);
    await cdp.key('z', { meta: true, shift: true });
    await delay(1500);
    const afterRedo = await waitFile(r, 'a.txt', 'hunks === 2', 10000).then(() => true, () => false);
    check('native undo brings the rejected hunk back (unsaved), redo removes it again; disk keeps the saved Reject', afterUndo && dirtyUndo && afterRedo && line(path.join(W, 'a.txt'), 100) === 'L100: original', { afterUndo, dirtyUndo, afterRedo });
    // After redo the buffer matches disk again; close the native tab (Cmd+W) and return to the review.
    await cdp.click(editorPt.x, editorPt.y); await cdp.key('w', { meta: true }); await delay(800);
    await selectRun('hunk worktree');
    r = await reviewOf(W);
    // Staged file: Reject changes the working tree only; the index keeps the staged edit.
    await hunkButton(r, 'b.txt', 'L50: agent edit', 'reject');
    await waitFile(r, 'b.txt', 'hunks === 1');
    for (let i = 0; i < 20 && line(path.join(W, 'b.txt'), 50) !== 'L50: original'; i++) await delay(250);
    check('staged file: Reject restores the working tree hunk and leaves the index unchanged', line(path.join(W, 'b.txt'), 50) === 'L50: original' && git(W, 'diff', '--cached') === indexBefore && /L50: agent edit/.test(git(W, 'diff', '--cached')), { l50: line(path.join(W, 'b.txt'), 50) });
    // Untracked files: accept one, reject the other (a new file has no base content, so it becomes empty).
    await hunkButton(r, 'new.txt', 'new one', 'accept');
    await waitFile(r, 'new.txt', 'reviewed === 1');
    await hunkButton(r, 'scratch.txt', 'scratch', 'reject');
    for (let i = 0; i < 20 && fs.readFileSync(path.join(W, 'scratch.txt'), 'utf8') !== ''; i++) await delay(250);
    check('untracked files: Accept marks reviewed; Reject restores the (empty) base content', (await fileState(r, 'new.txt')).reviewed === 1 && fs.readFileSync(path.join(W, 'scratch.txt'), 'utf8') === '', { scratch: JSON.stringify(fs.readFileSync(path.join(W, 'scratch.txt'), 'utf8')) });
    // Concurrent agent write during Reject: the agent changes the hunk after the review showed it
    // and before the click reaches VS Code. Detected as a conflict; the agent's text is kept.
    const agentWrite = text => () => fs.writeFileSync(path.join(W, 'a.txt'), fs.readFileSync(path.join(W, 'a.txt'), 'utf8').replace(/^L200: [^\n]*$/m, text));
    let conflict;
    for (let attempt = 0; attempt < 3 && !conflict; attempt++) {
      const shown = line(path.join(W, 'a.txt'), 200);
      await hunkButton(r, 'a.txt', shown, 'reject', agentWrite(`L200: agent edit v${attempt + 2}`)).catch(() => null);
      await delay(2500);
      const note = await r.eval(`document.getElementById('notice').textContent`);
      if (/conflict/.test(note)) conflict = { note, disk: line(path.join(W, 'a.txt'), 200) };
      else s.note('reject race lost (review refreshed first); retrying', { note, disk: line(path.join(W, 'a.txt'), 200) });
    }
    check('concurrent agent edit during Reject is a conflict and is not overwritten', conflict && /agent edit v\d/.test(conflict.disk), conflict);
    // Concurrent agent write during Accept.
    await r.eval(`document.getElementById('notice').textContent = ''`);
    let acceptConflict;
    for (let attempt = 0; attempt < 3 && !acceptConflict; attempt++) {
      const shown = line(path.join(W, 'a.txt'), 200);
      await hunkButton(r, 'a.txt', shown, 'accept', agentWrite(`L200: agent accept race ${attempt}`)).catch(() => null);
      await delay(2500);
      const note = await r.eval(`document.getElementById('notice').textContent`);
      if (/changed while you were accepting/.test(note)) acceptConflict = note;
    }
    const aState = await fileState(r, 'a.txt');
    check('concurrent agent edit during Accept is a conflict (not marked reviewed)', !!acceptConflict && aState.reviewed === 1, { acceptConflict, aState });
    // Reviewed state: survives Refresh; clears when the hunk changes again.
    await r.eval(`document.getElementById('refresh').click()`);
    await delay(2000);
    check('reviewed state survives a refresh', (await fileState(r, 'a.txt')).reviewed === 1 && (await fileState(r, 'new.txt')).reviewed === 1, { a: await fileState(r, 'a.txt'), n: await fileState(r, 'new.txt') });
    fs.writeFileSync(path.join(W, 'a.txt'), fs.readFileSync(path.join(W, 'a.txt'), 'utf8').replace('L10: agent edit', 'L10: agent edit again'));
    await waitFile(r, 'a.txt', 'reviewed === 0', 10000).catch(() => {});
    check('a reviewed hunk that changes again is no longer reviewed', (await fileState(r, 'a.txt')).reviewed === 0, await fileState(r, 'a.txt'));
    await s.screenshot('after-conflicts');
    // Reload keeps reviewed state.
    await cdp.command('Developer: Reload Window');
    await delay(6000);
    cdp = await s.connect(); s.cdp = cdp;
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'after reload');
    await delay(2000);
    const tab = await cdp.waitFor(`(() => { const t = [...document.querySelectorAll('.tab')].find(t => /Review: hunk worktree/.test(t.getAttribute('aria-label') || '')); if (!t) return null; const b = t.getBoundingClientRect(); return { x: b.left + 30, y: b.top + b.height / 2 }; })()`, 20000);
    await cdp.click(tab.x, tab.y);
    r = await reviewOf(W);
    await waitFile(r, 'new.txt', 'reviewed === 1', 20000).catch(() => {});
    check('reviewed state survives a window reload', (await fileState(r, 'new.txt'))?.reviewed === 1, await fileState(r, 'new.txt'));
    await s.screenshot('after-reload');

    // ---------------- current-checkout run
    await s.openOverseerView();
    await selectRun('hunk current');
    const rc = await reviewOf(C);
    await waitFile(rc, 'a.txt', 'hunks === 2');
    await hunkButton(rc, 'a.txt', 'L20: agent edit', 'reject');
    await waitFile(rc, 'a.txt', 'hunks === 1');
    await hunkButton(rc, 'a.txt', 'L30: agent edit', 'accept');
    await waitFile(rc, 'a.txt', 'reviewed === 1');
    for (let i = 0; i < 20 && line(path.join(C, 'a.txt'), 20) !== 'L20: original'; i++) await delay(250);
    check('current-checkout mode: Reject writes the checkout, Accept marks reviewed', line(path.join(C, 'a.txt'), 20) === 'L20: original' && line(path.join(C, 'a.txt'), 30) === 'L30: agent edit' && !git(C, 'diff', '--cached'), { l20: line(path.join(C, 'a.txt'), 20), l30: line(path.join(C, 'a.txt'), 30) });
    await s.screenshot('current-checkout');

    // ---------------- optional live run (tiny Codex prompt)
    if (process.env.LIVE) {
      const live = s.ctl('task.create', { repo, harness: 'codex', profile_id: 'system-codex', model: process.env.CODEX_MODEL || 'gpt-5.6-luna', title: 'hunk live',
        prompt: 'Edit files in place with apply_patch only. In a.txt change the line "L40: original" to "L40: codex edit" and the line "L140: original" to "L140: codex edit". In b.txt change the line "L90: original" to "L90: codex edit". Change nothing else. Then reply exactly: done' });
      for (let i = 0; i < 360 && ['queued', 'starting', 'running'].includes(runState(live.run.id).status); i++) await delay(500);
      const L = live.workspace.path;
      await s.openOverseerView();
      await selectRun('hunk live');
      const rl = await reviewOf(L);
      await waitFile(rl, 'a.txt', 'hunks === 2', 30000); await waitFile(rl, 'b.txt', 'hunks === 1', 30000);
      await hunkButton(rl, 'a.txt', 'L140: codex edit', 'reject');
      await waitFile(rl, 'a.txt', 'hunks === 1');
      await hunkButton(rl, 'b.txt', 'L90: codex edit', 'accept');
      await waitFile(rl, 'b.txt', 'reviewed === 1');
      for (let i = 0; i < 20 && line(path.join(L, 'a.txt'), 140) !== 'L140: original'; i++) await delay(250);
      check('LIVE Codex run: Reject one hunk, Accept another; disk matches', runState(live.run.id).status === 'completed' && line(path.join(L, 'a.txt'), 140) === 'L140: original' && line(path.join(L, 'a.txt'), 40) === 'L40: codex edit' && line(path.join(L, 'b.txt'), 90) === 'L90: codex edit',
        { status: runState(live.run.id).status, l40: line(path.join(L, 'a.txt'), 40), l140: line(path.join(L, 'a.txt'), 140), l90: line(path.join(L, 'b.txt'), 90) });
      await s.screenshot('live-codex');
    }
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
