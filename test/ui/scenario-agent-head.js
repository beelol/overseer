// Packaged-UI scenario for AC-233 (clicking an agent puts you in its head), AC-257 (following an
// agent sits beside Overseer's conversation) and AC-264 (Follow happens inside the review in the
// middle, the owner 2026-09-30), Claude fixture only (no paid turns). The fixture agent edits a.txt
// and b.txt one step at a time (each step waits for this scenario's go file).
//   AC-257: the agent is opened from its card in Overseer's conversation: its review opens beside the
//   conversation, which keeps its tab. ⌥⌘U goes to the conversation leaving the review as it was; the
//   conversation's "Back to" chip returns to it; the same from an agent opened in the side bar, whose
//   chat the Overseer tab shows again on the way back.
//   AC-233, AC-264: in Follow the review's middle shows the file the agent is in (a.txt, then b.txt),
//   live, at the changed line, its changes marked ("was:" on changed lines, a marker for removed
//   lines); screenshots in the three themes and at 1920×1080 and 1440×900 (Dark and Light). The list
//   beside it is All files (the whole worktree, the agent's file selected). A file picked there
//   shows in the same place with no VS Code editor; Follow goes back to the agent when it moves to
//   another file, and "Follow the agent" goes back at once. The switch shows Diffs only (the diffs,
//   the list Changed) and back, the review staying on screen. The window's folder and window count
//   never change.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot, git } = require('./harness');

const THEMES = ['Overseer Dark', 'Overseer Light', 'Overseer'];

(async () => {
  const s = new Session('agent-head');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const barrier = path.join(s.root, 'edit-barrier');
  fs.mkdirSync(barrier);
  const go = n => fs.writeFileSync(path.join(barrier, `go-${n}`), '');
  try {
    const repo = makeRepo(path.join(s.root, 'head-repo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    // The default (Follow) is what is tested: the harness's Diffs-only default for older scenarios is dropped.
    s.settings({ 'workbench.colorTheme': THEMES[0], 'overseer.agent.openIn': undefined, 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'editor');
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, FIXTURE_EDIT_BARRIER: barrier, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE,FIXTURE_EDIT_BARRIER' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const setTheme = async t => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = t; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(2500); };

    // What the window has open: its title, its Explorer root and how many windows there are.
    const windowState = async () => ({
      title: await cdp.evalWorkbench('document.title'),
      windows: (await cdp.call('Target.getTargets')).targetInfos.filter(t => t.type === 'page' && /workbench/.test(t.url)).length,
    });
    const window0 = await windowState();
    const repoBefore = { a: fs.readFileSync(path.join(repo, 'a.txt'), 'utf8'), b: fs.readFileSync(path.join(repo, 'b.txt'), 'utf8'), status: git(repo, 'status', '--porcelain') };

    // The editor groups: each one's active tab, and whether it is a VS Code text editor.
    const groups = () => cdp.evalWorkbench(`(() => [...document.querySelectorAll('.editor-group-container')].filter(g => g.offsetParent).map(g => {
      const tab = g.querySelector('.tab.active');
      const ed = [...g.querySelectorAll('.monaco-editor')].find(e => e.offsetParent);
      return { tab: tab?.getAttribute('aria-label') || '', title: (tab?.getAttribute('aria-label') || '').split(',')[0], active: g.classList.contains('active'), editor: !!ed };
    }))()`);
    const noEditors = gs => gs.every(g => !g.editor);
    const L = require('./overseer-window-helpers');
    // The review's Follow view: what it shows (the file, who put it there, the lines on screen, the marks) and its list.
    const reviewProbe = extra => `!!document.getElementById('follow-view') && document.body.dataset.runId === ${JSON.stringify(runId)}${extra ? ' && ' + extra : ''}`;
    const followOf = frame => frame.eval(`(() => { const nums = [...document.querySelectorAll('#follow-editor .line-numbers')].map(n => Number(n.textContent)).filter(Boolean);
      return { view: document.body.dataset.view, nav: document.body.dataset.nav, list: document.getElementById('list-title').textContent, path: document.body.dataset.followPath || '', source: document.body.dataset.followSource || '',
        state: document.body.dataset.followState || '', marks: JSON.parse(document.body.dataset.followMarks || '{}'), why: document.getElementById('follow-why').textContent, again: !document.getElementById('follow-again').hidden,
        was: [...document.querySelectorAll('#follow-editor .follow-was, #follow-editor .follow-removed')].map(e => e.textContent.replace(/\u00a0/g, ' ').trim()),
        first: nums.length ? Math.min(...nums) : 0, last: nums.length ? Math.max(...nums) : 0, text: [...document.querySelectorAll('#follow-editor .view-line')].map(l => l.textContent.replace(/\u00a0/g, ' ')).join('\\n').slice(0, 400),
        files: [...document.querySelectorAll('#tree .file')].map(f => f.dataset.path), active: document.querySelector('#tree .file.active')?.dataset.path || '',
        diffs: [...document.querySelectorAll('#diffs .diff-file .file-path')].filter(e => e.offsetParent).map(e => e.textContent) }; })()`);
    const waitFollow = async (frame, ok, ms = 30000) => { let f; for (let t = 0; t < ms; t += 250) { f = await followOf(frame).catch(() => null); if (f && ok(f)) return f; await delay(250); } return f; };

    // Overseer's conversation, with some history: a question and Overseer's answer.
    await cdp.command('Overseer: Open Overseer View'); await delay(2000);
    let home = await s.editorView(`!!document.querySelector('#home-conv')`);
    await home.eval(`window.overseerApi.postMessage({ type: 'overseerSend', text: 'what is everyone doing?' })`);
    await home.waitFor(`document.querySelectorAll('#home-conv .home-msg.from-overseer').length >= 1`, 45000).catch(() => {});
    // The agent, started by hand: its card appears in the conversation.
    const task = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title: 'Head agent',
      prompt: 'edit a.txt:40; edit b.txt:120-121; add a.txt:200; remove b.txt:250-252' });
    const runId = task.run.id;
    await home.waitFor(`!!document.querySelector('#home-conv .card[data-run=${JSON.stringify(runId)}]')`, 30000);
    const conversation = () => home.eval(`({ mode: window.__overseer.mode(), items: [...document.querySelectorAll('#home-conv .home-msg, #home-conv .card, #home-conv .proposal')].map(e => e.textContent.trim().slice(0, 80)),
      visible: !document.querySelector('.view-composer').hidden, back: (() => { const b = document.getElementById('home-back-agent'); return b && !b.hidden ? b.textContent : ''; })() })`);
    const conv0 = await conversation();
    s.note('conversation before', conv0);

    // ---- AC-257: opened from the conversation, the agent's review sits beside it (in Follow).
    const card = await s.webviewPoint(home, `#home-conv .card[data-run=${JSON.stringify(runId)}]`);
    await cdp.click(card.x, card.y);
    let review = await cdp.webview(reviewProbe(), 30000);
    await delay(1500);
    const conv1 = await conversation();
    const g1 = await groups();
    check('AC-257: opening the agent from its card in the conversation opens its review beside the conversation, which keeps its tab and its history',
      g1.length === 2 && g1.some(g => /^Overseer/.test(g.tab)) && g1.some(g => /^Review/.test(g.tab)) && conv1.mode === 'composer' && conv1.visible && JSON.stringify(conv1.items) === JSON.stringify(conv0.items), { groups: g1, conv1 });
    await s.openOverseerView();
    const panes = await cdp.evalWorkbench(`[...document.querySelectorAll('.part.sidebar .pane-header, .part.auxiliarybar .pane-header, .part.panel .pane-header')].filter(h => h.offsetParent).map(h => h.querySelector('.title')?.textContent.trim() || '')`);
    check('AC-264: no separate tree view of the agent\'s files anywhere (the side bar is the agents and Accounts)', !panes.some(t => /^(Worktree|Files)/i.test(t)), panes);
    await s.screenshot('opened-from-conversation');

    // ---- AC-233, AC-264: Follow shows the file the agent is in, in the review, at the line, marked.
    go(1);
    let f = await waitFollow(review, f => f.path === 'a.txt' && f.was.some(w => /^was: L40: original/.test(w)));
    check('AC-264: Follow shows the file the agent is editing (a.txt) in the review\'s middle, at the changed line, the line marked and saying what it was; no VS Code editor opens',
      f?.view === 'follow' && f.source === 'agent' && f.first <= 40 && f.last >= 40 && f.marks.changed >= 1 && f.was.some(w => /^was: L40: original/.test(w)) && noEditors(await groups()), f);
    check('AC-264: in Follow the list beside it is All files: the whole worktree, the agent\'s file selected',
      f?.nav === 'all' && f.list === 'All files' && ['a.txt', 'b.txt', 'c.txt', 'README.md'].every(n => f.files.includes(n)) && f.active === 'a.txt', f && { list: f.list, files: f.files, active: f.active });
    for (const [i, theme] of THEMES.entries()) {
      if (i) await setTheme(theme);
      await s.screenshot(`follow-${theme.toLowerCase().replace(/\s+/g, '-')}`);
    }
    // The owner's screen sizes, Dark and Light.
    for (const theme of THEMES.slice(0, 2)) {
      await setTheme(theme);
      for (const [w, h] of [[1920, 1080], [1440, 900]]) { await L.size(cdp, w, h); await delay(800); await s.screenshot(`follow-${theme.toLowerCase().replace(/\s+/g, '-')}-${w}x${h}`); }
    }
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench).catch(() => {}); await delay(1200);
    await setTheme(THEMES[0]);

    go(2);
    f = await waitFollow(review, f => f.path === 'b.txt' && f.was.filter(w => /^was: L12[01]: original/.test(w)).length === 2);
    check('AC-264: Follow moves with the agent to b.txt, live, at its two changed lines (120 and 121)',
      f?.path === 'b.txt' && f.first <= 120 && f.last >= 121 && f.was.filter(w => /^was: L12[01]: original/.test(w)).length === 2 && f.active === 'b.txt' && noEditors(await groups()), f);

    // ---- AC-264: a file picked in All files shows in the same place; no VS Code editor opens.
    await review.eval(`document.querySelector('#tree .file[data-path="c.txt"]').click()`);
    f = await waitFollow(review, f => f.path === 'c.txt' && f.state === 'shown' && /c base/.test(f.text), 15000);
    const gPick = await groups();
    check('AC-264: a file picked in All files (c.txt) shows in the review\'s middle, with "Follow the agent" to go back; the review stays and no VS Code editor opens',
      f?.path === 'c.txt' && f.source === 'user' && f.again && /c base/.test(f.text) && f.active === 'c.txt' && gPick.some(g => /^Review/.test(g.tab)) && noEditors(gPick), { f, groups: gPick });
    await s.screenshot('follow-picked-file');
    go(3);
    f = await waitFollow(review, f => f.path === 'a.txt' && f.source === 'agent' && f.marks.added >= 1 && f.first <= 200 && f.last >= 201);
    check('AC-264: when the agent moves to another file (it adds lines 200-201 to a.txt), Follow goes back to it',
      f?.path === 'a.txt' && f.source === 'agent' && !f.again && f.marks.added >= 1 && f.first <= 200 && f.last >= 201 && /added by the agent 1/.test(f.text), f);

    // ---- AC-233, AC-264: the switch shows Diffs only (the diffs, the list Changed) and back, the review staying.
    await review.eval(`document.querySelector('#view-mode .seg[data-view="diffs"]').click()`);
    f = await waitFollow(review, f => f.view === 'diffs' && f.diffs.includes('a.txt') && f.diffs.includes('b.txt'), 20000);
    const g3 = await groups();
    check('AC-233, AC-264: Diffs only shows the diffs in the same review, its list Changed (the changed files only); no VS Code editor opens',
      f?.view === 'diffs' && f.nav === 'changes' && f.list === 'Changed' && JSON.stringify([...f.files].sort()) === '["a.txt","b.txt"]' && g3.some(g => /^Review/.test(g.tab)) && noEditors(g3), { f, groups: g3 });
    await s.screenshot('diffs-only');
    await cdp.command('Overseer: Switch Between Follow and Diffs Only');
    f = await waitFollow(review, f => f.view === 'follow' && f.path === 'a.txt' && f.state === 'shown', 20000);
    check('AC-233, AC-264: and back to Follow, the agent\'s file (a.txt) again, the list All files, the review still on screen',
      f?.view === 'follow' && f.path === 'a.txt' && f.list === 'All files' && (await groups()).some(g => /^Review/.test(g.tab)), f);

    // "Follow the agent" goes back at once.
    await review.eval(`document.querySelector('#tree .file[data-path="README.md"]').click()`);
    await waitFollow(review, f => f.path === 'README.md' && f.again, 15000);
    await review.eval(`document.getElementById('follow-again').click()`);
    f = await waitFollow(review, f => f.path === 'a.txt' && f.source === 'agent', 15000);
    check('AC-264: "Follow the agent" goes from a picked file (README.md) back to the agent\'s file at once', f?.path === 'a.txt' && f.source === 'agent' && !f.again, f);

    go(4);
    f = await waitFollow(review, f => f.path === 'b.txt' && f.was.some(w => /^− 3 lines removed/.test(w)));
    check('AC-233: removed lines leave a marker where they were', f?.path === 'b.txt' && f.marks.removed >= 1 && f.was.some(w => /^− 3 lines removed/.test(w)), f);
    await s.screenshot('follow-removed-lines');

    // ---- AC-257: one action to the conversation and one back, the review as it was left.
    const left = await followOf(review);
    await cdp.key('u', { meta: true, alt: true }); await delay(1500);
    home = await s.editorView(`!!document.querySelector('#home-conv')`);
    const conv2 = await conversation();
    const g4 = await groups();
    const activeTab = g4.find(g => g.active)?.tab || '';
    review = await cdp.webview(reviewProbe(), 15000);
    const kept = await followOf(review);
    check('AC-257: ⌥⌘U goes to Overseer\'s conversation (its history intact, a "Back to" chip for the agent) and leaves the agent\'s review as it was',
      /^Overseer/.test(activeTab) && conv2.mode === 'composer' && conv2.visible && conv0.items.every(i => conv2.items.includes(i)) && /Head agent/.test(conv2.back)
      && g4.some(g => /^Review/.test(g.tab)) && kept.view === 'follow' && kept.path === left.path, { activeTab, conv2, groups: g4, kept: kept.path });
    await s.screenshot('back-in-conversation');
    const chip = await s.webviewPoint(home, '#home-back-agent');
    await cdp.click(chip.x, chip.y); await delay(1500);
    const active = (await groups()).find(g => g.active);
    review = await cdp.webview(reviewProbe(), 15000);
    const returned = await followOf(review);
    const conv3 = await conversation();
    check('AC-257: one click on the chip goes back to the agent: its review, in Follow on the same file, with the conversation intact beside it',
      /^Review/.test(active?.tab || '') && returned.view === 'follow' && returned.path === left.path && JSON.stringify(conv3.items) === JSON.stringify(conv2.items) && conv3.visible,
      { active, returned: returned.path, conv3: conv3.items.length });
    await s.screenshot('back-to-agent');

    // ---- AC-257 from the side bar: the Overseer tab shows the agent's chat; the way back brings it back too.
    await s.selectAgent('Head agent', { settle: 2500 });
    const chatMode = await home.eval(`window.__overseer.mode()`);
    await cdp.key('u', { meta: true, alt: true }); await delay(1500);
    const conv4 = await conversation();
    const g5 = await groups();
    await s.screenshot('sidebar-agent-to-conversation');
    await cdp.key('u', { meta: true, alt: true }); await delay(1500);
    const mode5 = await home.eval(`({ mode: window.__overseer.mode(), selected: window.__overseer.selected() })`);
    review = await cdp.webview(reviewProbe(), 15000);
    const returned2 = await followOf(review);
    check('AC-257: opened from the side bar (the Overseer tab shows its chat), ⌥⌘U shows the conversation beside the review, and ⌥⌘U again brings back its chat and its review in Follow',
      chatMode === 'chat' && conv4.mode === 'composer' && conv4.visible && g5.some(g => /^Review/.test(g.tab))
      && mode5.mode === 'chat' && mode5.selected === runId && returned2.view === 'follow' && returned2.path === left.path,
      { chatMode, conv4: conv4.mode, mode5, returned2: returned2.path });
    await s.screenshot('sidebar-back-to-agent');

    const window1 = await windowState();
    const repoAfter = { a: fs.readFileSync(path.join(repo, 'a.txt'), 'utf8'), b: fs.readFileSync(path.join(repo, 'b.txt'), 'utf8'), status: git(repo, 'status', '--porcelain') };
    const folderOf = t => String(t).split(' — ').pop();
    check('AC-233: the window\'s own folder is unchanged (same folder in its title, one window, the folder\'s files and status untouched)',
      folderOf(window1.title) === folderOf(window0.title) && folderOf(window0.title) === 'head-repo' && window1.windows === 1 && window0.windows === 1 && JSON.stringify(repoAfter) === JSON.stringify(repoBefore), { window0, window1 });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || !result.checks.length || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
