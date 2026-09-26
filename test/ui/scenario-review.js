// Packaged-UI review scenario (mock OpenCode + generic fixture runs, no paid tokens):
// two repositories with identical filenames, agent switching, attribution of user vs agent
// edits, live refresh timing (incl. a missed watcher event), current-checkout editing with
// native undo/redo, draft conflicts across reload, and unsafe/unsupported file states.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, startMock, openCodeConfig, latestVsix, delay, git } = require('./harness');

(async () => {
  const s = new Session('review');
  const result = { checks: [], timings: {} };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const mock = startMock(s.root, { MOCK_STEP_DELAY_MS: '2000' });
  const outside = path.join(s.root, 'outside.txt');
  fs.writeFileSync(outside, 'outside the workspace\n');
  try {
    const repoA = makeRepo(path.join(s.root, 'repoA'), { dirty: false });
    const repoB = makeRepo(path.join(s.root, 'repoB'), { dirty: false });
    s.settings({ 'files.watcherExclude': { '**/unwatched/**': true } });
    s.install(latestVsix());
    s.launch(repoA);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const profile = s.ctl('profile.create', { name: 'OpenCode mock', harness: 'opencode' });
    fs.mkdirSync(path.join(profile.home, 'config/opencode'), { recursive: true });
    fs.writeFileSync(path.join(profile.home, 'config/opencode/opencode.json'), openCodeConfig(await mock.port()));
    const taskA = s.ctl('task.create', { repo: repoA, harness: 'opencode', profile_id: profile.id, model: 'mock/mock-coder', prompt: 'sequence 10', title: 'A seq' });
    const taskB = s.ctl('task.create', { repo: repoB, harness: 'opencode', profile_id: profile.id, model: 'mock/mock-coder', prompt: 'sequence 3', title: 'B seq' });
    const wsA = taskA.workspace.path, wsB = taskB.workspace.path;
    s.note('workspaces', { wsA, wsB });

    // Gate K: agents are selected in the Overseer side bar.
    const selectRun = title => s.selectAgent(title, { settle: 1500 });
    const reviewFor = async ws => cdp.webview(`document.getElementById('workspace-note')?.textContent.includes(${JSON.stringify(ws)})`, 30000);

    // AC-25/30: select A, turn Follow on; then B does not inherit Follow and shows B's worktree.
    // Both already have changes, so selecting them is "an existing run" (a first edit while its chat
    // is shown would bring the review forward in follow mode, AC-73/74).
    for (const t of [taskA, taskB]) for (let i = 0; i < 60 && !(s.ctl('workspace.changes', { workspace_id: t.workspace.id }).files > 0); i++) await delay(250);
    await selectRun('A seq');
    let reviewA = await reviewFor(wsA);
    check('selecting run A opens A worktree review (outside the open folder)', await reviewA.eval(`document.getElementById('workspace-note').textContent`), wsA);
    check('selecting an existing run does not turn Follow on', !(await reviewA.eval(`(document.getElementById('follow').dataset.state !== 'off')`)), await reviewA.eval(`document.getElementById('follow').dataset.state`));
    const box = await s.webviewPoint(reviewA, '#follow');
    await cdp.click(box.x, box.y);
    await reviewA.waitFor(`document.getElementById('follow-state').textContent.startsWith('Following')`, 20000);
    // AC-29: a user/unrelated change (not reported by the harness) must not be followed or attributed.
    fs.writeFileSync(path.join(wsA, 'user-note.txt'), 'written by the user, not the agent\n');
    const seen = [];
    for (let i = 0; i < 24; i++) { const t = await reviewA.eval(`document.getElementById('follow-state').textContent`); if (seen[seen.length - 1] !== t) seen.push(t); await delay(250); }
    check('follow ignores unattributed user file', seen.length > 1 && !seen.some(t => t.includes('user-note')), seen);
    await reviewA.waitFor(`[...document.querySelectorAll('.file-path')].some(e => e.textContent === 'user-note.txt')`, 6000);
    check('user file still listed in the live review', true, 'user-note.txt listed');
    const topA = await reviewA.eval(`document.getElementById('diffs').scrollTop`);
    await s.screenshot('run-a-following');

    await selectRun('B seq');
    const reviewB = await reviewFor(wsB);
    check('selecting run B shows B worktree', true, await reviewB.eval(`document.getElementById('workspace-note').textContent`));
    check('agent switch does not inherit Follow', !(await reviewB.eval(`(document.getElementById('follow').dataset.state !== 'off')`)));
    const topB1 = await reviewB.eval(`document.getElementById('diffs').scrollTop`);
    await delay(5000);
    const topB2 = await reviewB.eval(`document.getElementById('diffs').scrollTop`);
    check('switched-to review does not jump during live edits', topB1 === topB2, { topB1, topB2 });
    await s.screenshot('run-b-selected');
    // Wait for both runs to finish their edits.
    for (const t of [taskA, taskB]) {
      for (let i = 0; i < 90; i++) { const r = s.ctl('state').runs.find(x => x.id === t.run.id); if (!['queued', 'starting', 'running'].includes(r.status)) break; await delay(1000); }
    }
    // AC-25: identical relative filenames; an edit in B's review lands only in B.
    const aBefore = fs.readFileSync(path.join(wsA, 'a.txt'), 'utf8');
    await reviewB.waitFor(`[...document.querySelectorAll('.diff-file')].some(e => e.querySelector('.file-path')?.textContent === 'a.txt' && e.dataset.loadState === 'rendered')`, 20000);
    const editLine = async (frame, file, text, pattern) => {
      await frame.waitFor(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === ${JSON.stringify(file)}); if (!e) return false; e.scrollIntoView(); const l = [...e.querySelectorAll('.editor.modified .view-lines .view-line')].filter(l => !l.closest('.view-zones')).find(l => ${pattern}.test(l.textContent)); if (!l) return false; l.scrollIntoView({ block: 'center' }); l.id = 'edit-target'; e.querySelector('.save-file').id = 'save-target'; return true; })()`, 15000);
      await delay(500);
      const p = await s.webviewPoint(frame, '#edit-target');
      await cdp.click(p.x - 20, p.y);
      await cdp.key('End');
      await cdp.type(text);
      await delay(800);
      s.note('edit diagnostics', await frame.eval(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === ${JSON.stringify(file)}); return { target: document.getElementById('edit-target')?.textContent, active: document.activeElement.className, save: e.querySelector('.save-file').disabled, status: e.querySelector('.edit-status').textContent, notice: document.getElementById('notice').textContent, typed: [...e.querySelectorAll('.view-line')].some(l => l.textContent.includes(${JSON.stringify(text.trim())})) }; })()`));
      await s.screenshot('edit-' + file.replace(/\W/g, ''));
    };
    await editLine(reviewB, 'a.txt', ' EDIT-IN-B', '/agent.edit/');
    const saveB = await s.webviewPoint(reviewB, '#save-target');
    await cdp.click(saveB.x, saveB.y);
    await delay(1500);
    check('review edit wrote only B worktree', fs.readFileSync(path.join(wsB, 'a.txt'), 'utf8').includes('EDIT-IN-B') && fs.readFileSync(path.join(wsA, 'a.txt'), 'utf8') === aBefore && !fs.readFileSync(path.join(repoB, 'a.txt'), 'utf8').includes('EDIT-IN-B'), 'A unchanged, source repoB unchanged');

    // AC-31: refresh timing without manual refresh.
    const listed = (frame, pathName, present = true) => frame.eval(`[...document.querySelectorAll('.diff-file .file-path')].some(e => e.textContent === ${JSON.stringify(pathName)}) === ${present}`);
    const timeUntil = async (label, act, frame, pred, limit = 8000) => {
      const t0 = Date.now(); await act();
      while (Date.now() - t0 < limit) { if (await pred()) { result.timings[label] = Date.now() - t0; s.note('timing', { label, ms: result.timings[label] }); return result.timings[label]; } await delay(50); }
      result.timings[label] = null; s.note('timing', { label, ms: null }); return null;
    };
    await reviewB.eval(`[...document.querySelectorAll('#tree .file')].find(b => b.textContent.includes('a.txt')).id = 'sel-a'`);
    const selA = await s.webviewPoint(reviewB, '#sel-a');
    await cdp.click(selA.x, selA.y);
    await delay(500);
    const selectedBefore = await reviewB.eval(`document.querySelector('#tree .file.active')?.textContent`);
    const tWrite = await timeUntil('write new file', () => fs.writeFileSync(path.join(wsB, 'x1.txt'), 'new\n'), reviewB, () => listed(reviewB, 'x1.txt'));
    const revBefore = await reviewB.eval(`[...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'b.txt')?.dataset.revision`);
    const tAtomic = await timeUntil('atomic replace', () => { fs.writeFileSync(path.join(wsB, '.b.tmp'), 'atomically replaced\n'); fs.renameSync(path.join(wsB, '.b.tmp'), path.join(wsB, 'b.txt')); }, reviewB,
      async () => (await reviewB.eval(`[...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'b.txt')?.dataset.revision`)) !== revBefore && await reviewB.eval(`[...document.querySelectorAll('.view-line')].some(l => /atomically/.test(l.textContent))`));
    // Staging shows in the review's Staged scope (AC-75 replaced the Workspace Dirty view).
    const setScope = async (frame, v) => { await frame.eval(`(() => { const e = document.getElementById('scope'); e.value = ${JSON.stringify(v)}; e.dispatchEvent(new Event('change')); return true; })()`); await frame.waitFor(`document.body.dataset.scope === ${JSON.stringify(v)}`, 10000); };
    await setScope(reviewB, 'staged'); await delay(800);
    const tStage = await timeUntil('staging (review Staged scope)', () => git(wsB, 'add', 'x1.txt'), reviewB, () => listed(reviewB, 'x1.txt'));
    await setScope(reviewB, 'all'); await reviewB.waitFor(`[...document.querySelectorAll('.diff-file .file-path')].some(e => e.textContent === 'x1.txt') && document.querySelectorAll('.diff-file').length > 1`, 10000);
    const tRename = await timeUntil('rename', () => git(wsB, 'mv', 'x1.txt', 'x2.txt'), reviewB, async () => await listed(reviewB, 'x2.txt') && await listed(reviewB, 'x1.txt', false));
    const tDelete = await timeUntil('delete', () => fs.rmSync(path.join(wsB, 'c.txt')), reviewB, () => reviewB.eval(`[...document.querySelectorAll('.diff-file')].some(e => e.querySelector('.file-path').textContent === 'c.txt' && /D/.test(e.querySelector('.status').textContent))`));
    const tBranch = await timeUntil('branch change', () => { git(wsB, 'switch', '-q', '-c', 'review-branch'); fs.writeFileSync(path.join(wsB, 'after-switch.txt'), 'x\n'); }, reviewB, () => listed(reviewB, 'after-switch.txt'));
    const tMissed = await timeUntil('missed watcher event (excluded folder)', () => { fs.mkdirSync(path.join(wsB, 'unwatched'), { recursive: true }); fs.writeFileSync(path.join(wsB, 'unwatched/y.txt'), 'y\n'); }, reviewB, () => listed(reviewB, 'unwatched/y.txt'), 12000);
    check('ordinary changes refresh within 2 s', [tWrite, tAtomic, tRename, tDelete, tBranch].every(t => t !== null && t <= 2000) && tStage !== null && tStage <= 2000, result.timings);
    check('missed watcher event reconciled within 5 s', tMissed !== null && tMissed <= 5000, tMissed);
    const selectedAfter = await reviewB.eval(`document.querySelector('#tree .file.active')?.textContent`);
    check('navigator selection preserved across live refreshes', selectedBefore && selectedBefore === selectedAfter, { selectedBefore, selectedAfter });
    await s.screenshot('refresh');

    // AC-34: unsafe/unsupported files keep truthful states.
    fs.symlinkSync(outside, path.join(wsB, 'escape.txt'));
    fs.writeFileSync(path.join(wsB, 'latin1.txt'), Buffer.from([0x63, 0x61, 0x66, 0xe9, 0x0a]));
    fs.writeFileSync(path.join(wsB, 'big.txt'), 'y'.repeat(3 * 1024 * 1024));
    await reviewB.waitFor(`['escape.txt','latin1.txt','big.txt'].every(p => [...document.querySelectorAll('.diff-file .file-path')].some(e => e.textContent === p))`, 10000);
    const stateOf = async name => {
      await reviewB.eval(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === ${JSON.stringify(name)}); e.scrollIntoView(); })()`);
      await delay(1500);
      return reviewB.eval(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === ${JSON.stringify(name)}); return { load: e.dataset.loadState, problem: e.querySelector('.file-problem')?.textContent || '', card: e.querySelector('.large-diff')?.textContent || '', save: e.querySelector('.save-file').disabled, readOnly: !!e.querySelector('.editor.modified .monaco-editor.read-only, .monaco-editor.readonly') }; })()`);
    };
    const esc = await stateOf('escape.txt'), latin = await stateOf('latin1.txt'), big = await stateOf('big.txt');
    s.note('file states', { esc, latin, big });
    check('escaping symlink is not editable in the review', esc.save && (esc.readOnly || esc.problem || esc.load === 'rendered'), esc);
    check('invalid UTF-8 shows a truthful problem', /Binary|decode|encoding|native/i.test(latin.problem), latin);
    check('oversized file shows the preview limit', /2 MiB|limit/i.test(big.problem + big.card), big);
    // Try to type into the escaping symlink row: it must stay read-only and never write outside.
    await reviewB.eval(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'escape.txt'); const l = [...e.querySelectorAll('.editor.modified .view-lines .view-line')].pop(); l.scrollIntoView({ block: 'center' }); l.id = 'sym-line'; })()`);
    const sp = await s.webviewPoint(reviewB, '#sym-line');
    await cdp.click(sp.x, sp.y); await cdp.key('End'); await cdp.type('HACK'); await delay(800);
    const symAfter = await reviewB.eval(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'escape.txt'); return { typed: [...e.querySelectorAll('.view-line')].some(l => /HACK/.test(l.textContent)), save: e.querySelector('.save-file').disabled }; })()`);
    check('symlink escaping the workspace cannot be edited in the review', !symAfter.typed && symAfter.save, symAfter);
    check('symlink target outside workspace untouched', fs.readFileSync(outside, 'utf8') === 'outside the workspace\n');
    // Merge conflict: marked Conflicted in the review's file list.
    fs.writeFileSync(path.join(wsB, 'conflict.txt'), 'base\n');
    git(wsB, 'add', '-A'); git(wsB, 'commit', '-q', '-m', 'scenario checkpoint');
    git(wsB, 'branch', 'other');
    fs.writeFileSync(path.join(wsB, 'conflict.txt'), 'ours\n'); git(wsB, 'commit', '-q', '-m', 'ours', 'conflict.txt');
    git(wsB, 'switch', '-q', 'other'); fs.writeFileSync(path.join(wsB, 'conflict.txt'), 'theirs\n'); git(wsB, 'commit', '-q', '-m', 'theirs', 'conflict.txt');
    git(wsB, 'switch', '-q', 'review-branch');
    s.note('merge', cp.spawnSync('git', ['merge', 'other'], { cwd: wsB, encoding: 'utf8' }).stdout.trim());
    const conflictShown = await reviewB.waitFor(`[...document.querySelectorAll('#tree .file.conflicted')].some(b => /conflict\\.txt/.test(b.getAttribute('aria-label') || '') && /conflicted/.test(b.getAttribute('aria-label')))`, 8000).then(() => true, () => false);
    const conflictListed = await reviewB.waitFor(`[...document.querySelectorAll('.diff-file .file-path')].some(e => e.textContent === 'conflict.txt')`, 8000).catch(() => false);
    check('merge conflict shown as Conflicted and listed in the review', conflictShown && conflictListed, { conflictShown, conflictListed });
    await s.screenshot('conflict');
    // Rename during an unsaved review edit: the draft is not silently lost.
    await reviewB.waitFor(`[...document.querySelectorAll('.diff-file')].some(e => e.querySelector('.file-path')?.textContent === 'a.txt' && e.dataset.loadState === 'rendered')`, 20000);
    await editLine(reviewB, 'a.txt', ' DRAFT-BEFORE-RENAME', '/EDIT.IN.B/');
    git(wsB, 'mv', '-f', 'a.txt', 'a-renamed.txt');
    await delay(4000);
    const docs = await cdp.evalWorkbench(`[...document.querySelectorAll('.tab')].map(t => t.getAttribute('aria-label')).join(' | ')`);
    const draftSomewhere = await cdp.evalWorkbench(`[...document.querySelectorAll('.tab')].some(t => /a\.txt|Untitled/.test(t.getAttribute('aria-label') || '') && t.classList.contains('dirty'))`);
    check('draft survives a rename during edit (kept as a dirty buffer)', draftSomewhere, docs);
    await s.screenshot('rename-during-edit');
    await s.screenshot('unsupported-files');

    // AC-32/33: current checkout — review edit + save, native undo/redo, draft vs external write, reload recovery.
    const cur = s.ctl('task.create', { repo: repoA, harness: 'generic', workspace_mode: 'current', program: '/bin/sh', args: ['-c', 'echo "L2: current run edit" > run.txt'], prompt: '', title: 'C current' });
    for (let i = 0; i < 20; i++) { if (s.ctl('state').runs.find(x => x.id === cur.run.id).status === 'completed') break; await delay(500); }
    fs.writeFileSync(path.join(repoA, 'a.txt'), fs.readFileSync(path.join(repoA, 'a.txt'), 'utf8').replace('L5: original', 'L5: user change before review'));
    await selectRun('C current');
    const reviewC = await reviewFor(repoA);
    check('current checkout review is the repository itself', true, await reviewC.eval(`document.getElementById('workspace-note').textContent`));
    await reviewC.waitFor(`[...document.querySelectorAll('.diff-file')].some(e => e.querySelector('.file-path')?.textContent === 'a.txt' && e.dataset.loadState === 'rendered')`, 20000);
    const fbox = await s.webviewPoint(reviewC, '#follow');
    await cdp.click(fbox.x, fbox.y);
    await delay(700);
    const note = await reviewC.eval(`document.getElementById('follow-state').textContent`);
    check('filesystem-only harness shows the Follow limitation', /Filesystem evidence only/.test(note), note);
    await cdp.click(fbox.x, fbox.y);
    await delay(300);
    await editLine(reviewC, 'a.txt', ' SAVED-FROM-REVIEW', '/user.change/');
    const saveC = await s.webviewPoint(reviewC, '#save-target');
    await cdp.click(saveC.x, saveC.y);
    await delay(1500);
    check('current-checkout review edit saved to the checkout path', fs.readFileSync(path.join(repoA, 'a.txt'), 'utf8').includes('SAVED-FROM-REVIEW'));
    // Native diff editor: type, undo, redo.
    await reviewC.eval(`[...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'a.txt').querySelector('.open-native').id = 'native-a'`);
    const nat = await s.webviewPoint(reviewC, '#native-a');
    await cdp.click(nat.x, nat.y);
    await cdp.waitFor(`!!document.querySelector('.monaco-diff-editor .editor.modified')`, 10000, 'native diff');
    await delay(800);
    const modLine = await cdp.evalWorkbench(`(() => { const l = [...document.querySelectorAll('.monaco-diff-editor .editor.modified .view-lines .view-line')].find(l => /SAVED-FROM-REVIEW/.test(l.textContent)); const b = l.getBoundingClientRect(); return { x: b.left + 20, y: b.top + b.height / 2 }; })()`);
    await cdp.click(modLine.x, modLine.y);
    await cdp.key('End');
    await cdp.type(' NATIVE-DRAFT');
    await delay(300);
    const textNow = () => cdp.evalWorkbench(`[...document.querySelectorAll('.monaco-diff-editor .editor.modified .view-lines .view-line')].map(l => l.textContent).join('\\n')`);
    const typed = /NATIVE.DRAFT/.test(await textNow());
    await cdp.key('z', { meta: true }); await delay(300);
    const undone = !/NATIVE.DRAFT/.test(await textNow());
    await cdp.key('z', { meta: true, shift: true }); await delay(300);
    const redone = /NATIVE.DRAFT/.test(await textNow());
    check('native editor undo/redo on the selected file', typed && undone && redone, { typed, undone, redone });
    await s.screenshot('native-draft');
    // External agent write to the same line while the draft is unsaved.
    const disk = fs.readFileSync(path.join(repoA, 'a.txt'), 'utf8');
    fs.writeFileSync(path.join(repoA, 'a.txt'), disk.replace(/L5: [^\n]*/, 'L5: AGENT-EXTERNAL-WRITE'));
    await delay(2500);
    const draftKept = /NATIVE.DRAFT/.test(await textNow());
    const diskHasAgent = fs.readFileSync(path.join(repoA, 'a.txt'), 'utf8').includes('AGENT-EXTERNAL-WRITE');
    check('same-line external write does not overwrite the unsaved draft; both versions exist', draftKept && diskHasAgent, { draftKept, diskHasAgent });
    await cdp.evalWorkbench('0');
    const draftListed = await reviewC.waitFor(`[...document.querySelectorAll('#tree .file')].some(b => /^a\\.txt, .*unsaved/.test(b.getAttribute('aria-label') || '') && !!b.querySelector('.marker.codicon-circle-filled'))`, 8000).then(() => true, () => false);
    await s.screenshot('unsaved-marker');
    check('unsaved draft marked in the review file list (AC-75 replaced Workspace Dirty)', draftListed);
    // Reload the window; the draft must survive.
    await cdp.command('Developer: Reload Window');
    await delay(6000);
    const cdp2 = await s.connect();
    s.cdp = cdp2;
    await cdp2.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status after reload');
    const recovered = await cdp2.waitFor(`[...document.querySelectorAll('.tab')].some(t => /a\\.txt/.test(t.getAttribute('aria-label') || '') && t.classList.contains('dirty'))`, 20000).catch(() => false);
    check('pending draft recovered after reload', recovered);
    await s.screenshot('after-reload');
    const tab = await cdp2.waitFor(`(() => { const t = [...document.querySelectorAll('.tab')].find(t => /Review.*C current/.test(t.getAttribute('aria-label') || t.textContent)); if (!t) return null; const b = t.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`, 20000).catch(() => undefined);
    if (tab) await cdp2.click(tab.x, tab.y);
    const reviewAfter = tab && await cdp2.webview(`document.getElementById('workspace-note')?.textContent.includes(${JSON.stringify(repoA)}) && document.querySelectorAll('.diff-file').length > 0`, 30000).catch(() => undefined);
    check('review restored after reload', !!reviewAfter, tab ? 'tab restored' : 'no tab');
    const chatRestored = await cdp2.webview(`document.getElementById('title')?.textContent === 'C current'`, 20000).then(() => true, () => false);
    const markerRestored = !!reviewAfter && await reviewAfter.waitFor(`[...document.querySelectorAll('#tree .file')].some(b => /^a\\.txt, .*unsaved/.test(b.getAttribute('aria-label') || ''))`, 10000).then(() => true, () => false);
    check('selected agent and the unsaved marker restored after reload', chatRestored && markerRestored, { chatRestored, markerRestored });
    await s.screenshot('review-after-reload');
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
