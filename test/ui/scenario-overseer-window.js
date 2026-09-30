// Packaged-UI scenario for AC-264 (the one Overseer layout, way B), fixture agents only: the Claude
// fixture as Overseer, generic agents, the simulated voice. No paid turn.
//   - The first launch offers the layout once (answered Not Now; it never comes back).
//   - A cluttered window (Explorer, the terminal running a command, two groups of files, one with
//     unsaved words) becomes the layout in one step (the status bar's Workspace button): Overseer
//     asks once in its own words (save the file; the terminal command stops), saves, and the window
//     reopens as the Overseer window: the agents list, the review in the middle, Overseer on the right,
//     no tab rows, no breadcrumbs. VS Code's own save question never shows.
//   - A second VS Code window keeps its tab strip, and no user setting changes.
//   - Picking an agent turns the right panel into its chat; back (the arrow, then ⌥⌘U both ways)
//     returns to Overseer's conversation with its history.
//   - Voice Mode takes over the same panel. Follow shows the agent's files in the middle; no file tree
//     covers the right panel. The side bar is the agents and Accounts (no Search section).
//   - The button again restores the cluttered layout exactly (groups and their shares, tabs in
//     order, active tabs, Explorer, the terminal panel), and the terminal command's fate is measured.
//   - Screenshots at 1920×1080 and 1440×900 in the three Overseer themes.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, git, repoRoot } = require('./harness');
const L = require('./overseer-window-helpers');

(async () => {
  const s = new Session('overseer-window');
  const result = { checks: [], measures: {} };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const measure = (name, value) => { result.measures[name] = value; s.note(`MEASURE ${name}`, value); };
  const conns = [];
  const modeFile = path.join(s.root, 'claude-mode');
  const tick = path.join(s.root, 'tick');
  const settingsFile = path.join(s.profile, 'User/settings.json');
  let live;
  try {
    const repo = makeRepo(path.join(s.root, 'ws-repo'), { dirty: false });
    fs.writeFileSync(path.join(repo, 'd.txt'), 'd.txt\n');
    git(repo, 'add', '.'); git(repo, 'commit', '-q', '-m', 'more files');
    const other = makeRepo(path.join(s.root, 'notes-repo'), { dirty: false });
    // The offer is on (the harness turns it off for every other scenario); VS Code's in-window folder picker for the second window.
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'workbench.editor.enablePreview': false, 'workbench.editor.enablePreviewFromQuickOpen': false, 'files.simpleDialog.enable': true,
      'overseer.layout.offerOnStartup': true, 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'overseer');
    // A window VS Code reopens on another folder or workspace forgets the launch's --extensions-dir
    // (the owner's windows use the default folder); VS Code's own variable keeps the test's.
    s.launch(repo, { VSCODE_EXTENSIONS: s.extensions,
      OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE',
      OVERSEER_VOICE_SIMULATE: '1', OVERSEER_LISTENER_TEST_VOICE: '1', OVERSEER_LISTENER_TEST_MIC_USERS: path.join(s.root, 'mic-users') });
    await s.connect();
    let main = await L.attach(s, t => /ws-repo/.test(t)); conns.push(main);
    s.cdp = main;
    const ready = c => c.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    await ready(main);
    await L.size(main, 1920, 1080);

    // ---------- The first launch offers the layout, once.
    const toast = c => c.evalWorkbench(`(() => { const t = [...document.querySelectorAll('.notifications-toasts .notification-list-item')].find(n => /Set up the Overseer layout/.test(n.textContent)); if (!t) return null;
      return { text: t.querySelector('.notification-list-item-message')?.textContent || '', buttons: [...t.querySelectorAll('.monaco-button')].map(b => b.textContent.trim()).filter(Boolean) }; })()`);
    const offer = await main.waitFor(`[...document.querySelectorAll('.notifications-toasts .notification-list-item')].some(n => /Set up the Overseer layout/.test(n.textContent))`, 20000, 'the layout offer').then(() => toast(main)).catch(() => null);
    await s.screenshot('offer');
    check('the first launch offers the Overseer layout, with one click to set it up', offer && offer.buttons.includes('Set Up'), offer);
    await main.evalWorkbench(`[...document.querySelectorAll('.notifications-toasts .monaco-button')].find(b => b.textContent.trim() === 'Not Now')?.click()`);
    await delay(800);

    // ---------- Agents, and the clutter.
    const settingsBefore = L.norm(fs.readFileSync(settingsFile, 'utf8'));
    s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', "sed -i '' 's/^L3: original$/L3: done/; s/^L8: original$/L8: checked/' b.txt; printf 'notes\\n- b.txt: lines 3 and 8 updated\\n' > notes.md; echo 'Updated b.txt (lines 3 and 8) and wrote notes.md.'"], prompt: '', title: 'Finished edit' });
    live = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'i=0; while [ $i -lt 900 ]; do i=$((i+1)); echo "step $i" >> live.txt; sleep 1; done'], prompt: '', title: 'Live edits' });
    await delay(2500);
    await main.command('View: Show Explorer'); await delay(500);
    for (const f of ['a.txt', 'c.txt']) await L.openFile(main, f);
    await main.command('View: Split Editor Right'); await delay(800);
    await L.openFile(main, 'd.txt');
    await main.type('unsaved words from the owner\n'); await delay(300);
    // The terminal runs a command that keeps writing (to see whether it survives the reopen).
    await main.command('Terminal: Create New Terminal'); await delay(2500);
    await main.type(`i=0; while true; do i=$((i+1)); echo $i > ${tick}; sleep 0.5; done`); await main.key('Enter');
    await delay(2000);
    const ticking = async () => { const a = Number(fs.readFileSync(tick, 'utf8')); await delay(1500); return Number(fs.readFileSync(tick, 'utf8')) > a; };
    check('the terminal command is running', fs.existsSync(tick) && await ticking());
    // The editor keeps the keyboard, as the owner left it.
    await main.command('View: Focus Active Editor Group'); await delay(300);
    const second = await L.secondWindow(s, main, other); conns.push(second);
    await L.openFile(second, 'README.md');
    for (const c of [main, second]) await L.size(c, 1920, 1080);
    const before = await L.layout(main), secondBefore = await L.layout(second);
    check('the cluttered start: Explorer, the terminal, two groups of files (one with unsaved words); the other window has its tab strip',
      before.groups.length === 2 && before.panel && before.sidebarTitle === 'Explorer' && secondBefore.tabStrips === 1, { before, secondBefore });
    await s.screenshot('cluttered-1920x1080');
    const recentBefore = await L.recent(main);

    // ---------- One step: the status bar's Workspace button.
    const button = c => c.evalWorkbench(`(() => { const e = [...document.querySelectorAll('.statusbar-item')].find(e => /^(Workspace|Close Workspace)$/.test(e.textContent.trim())); if (!e) return null; const b = e.getBoundingClientRect(); return { text: e.textContent.trim(), x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`);
    const dialog = c => c.evalWorkbench(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d || !d.offsetParent) return null; return { text: d.querySelector('.dialog-message-text')?.textContent || '', detail: d.querySelector('.dialog-message-detail')?.textContent || '', buttons: [...d.querySelectorAll('.monaco-button')].map(b => b.textContent.trim()).filter(Boolean) }; })()`).catch(() => null);
    const b1 = await button(main);
    check('the status bar has the Workspace button', b1?.text === 'Workspace', b1);
    const t0 = Date.now();
    await main.click(b1.x, b1.y);
    let ask = null;
    for (let i = 0; i < 40 && !ask; i++) { ask = await dialog(main); if (!ask) await delay(150); }
    await s.screenshot('asks-first');
    check('Overseer asks once, in its words: save the file, and the terminal command stops', ask && /^Save 1 file and open the Overseer layout\?$/.test(ask.text) && /command running in the terminal stops/.test(ask.detail) && ask.buttons.includes('Save All'), ask);
    await main.evalWorkbench(`[...document.querySelectorAll('.monaco-dialog-box .monaco-button')].find(b => b.textContent.trim() === 'Save All')?.click()`);
    // VS Code's own question must not follow before the window reloads.
    let vsQuestion = null;
    for (let i = 0; i < 20; i++) { const d = await dialog(main); if (d && /Do you want to save/.test(d.text)) { vsQuestion = d; break; } await delay(100); }
    check('VS Code\'s own save question never shows; the unsaved words are saved', !vsQuestion && /unsaved words from the owner/.test(fs.readFileSync(path.join(repo, 'd.txt'), 'utf8')), vsQuestion);
    await delay(1500);
    try { main.close(); } catch {}
    main = await L.reopened(s, true); conns.push(main);
    s.cdp = main;
    await ready(main);
    await L.size(main, 1920, 1080);
    await main.waitFor(`[...document.querySelectorAll('.part.editor .editor-group-container')].filter(g => g.offsetParent).length === 2`, 30000, 'the review and the Overseer panel');
    let home = await L.overseerView(main);
    for (let i = 0; i < 40 && home.mode !== 'composer'; i++) { await delay(250); home = await L.overseerView(main); }
    measure('one step: click to the arranged layout (ms)', Date.now() - t0);
    await delay(1000);
    const inB = await L.layout(main);
    check('the layout: the agents list on the left, the working agent\'s review wide in the middle, Overseer\'s conversation on the right; no tab rows, no breadcrumbs, no panel',
      inB.sidebar && /Overseer/i.test(inB.sidebarTitle) && inB.groups.length === 2 && inB.groups[0].width > inB.groups[1].width && inB.tabStrips === 0 && inB.breadcrumbs === 0 && !inB.panel && !inB.auxiliary && home.mode === 'composer', { inB, home });
    measure('the window title', inB.title);
    const terminalInB = await ticking().catch(() => false);
    measure('the terminal command keeps running after the reopen', terminalInB);
    check('what Overseer said about the terminal command is what happened (it stopped)', !terminalInB);
    const secondDuring = await L.layout(second);
    check('the other window keeps its tab strip and its tabs', secondDuring.tabStrips === 1 && JSON.stringify(secondDuring.groups.map(g => g.tabs)) === JSON.stringify(secondBefore.groups.map(g => g.tabs)), secondDuring);
    check('no user setting changed', L.norm(fs.readFileSync(settingsFile, 'utf8')) === settingsBefore);
    const layoutsDir = path.join(s.profile, 'User/globalStorage/beelol.overseer/layouts');
    const files = fs.existsSync(layoutsDir) ? fs.readdirSync(layoutsDir).flatMap(d => fs.readdirSync(path.join(layoutsDir, d)).map(f => path.join(layoutsDir, d, f))) : [];
    check('the workspace file is in Overseer\'s own storage, not in the repository, and holds the look', files.length === 1 && !fs.readdirSync(repo).some(f => f.endsWith('.code-workspace')) && JSON.parse(fs.readFileSync(files[0], 'utf8')).settings['workbench.editor.showTabs'] === 'none', files.map(f => path.relative(s.profile, f)));
    const recentIn = await L.recent(main);
    check('VS Code\'s recent list never shows Overseer\'s file', !recentIn.concat(recentBefore).some(r => /code-workspace|\(Workspace\)/.test(r)), { recentBefore, recentIn });

    // ---------- Home, then Overseer's conversation gets a message (its history).
    const shots = async (label, theme) => {
      await L.size(main, 1920, 1080); await delay(600);
      const big = await s.screenshot(`${label}-${theme}-1920x1080`);
      await L.size(main, 1440, 900); await delay(900);
      await s.screenshot(`${label}-${theme}-1440x900`);
      await L.size(main, 1920, 1080); await delay(600);
      return big;
    };
    await L.settled(main, 'live.txt');
    await shots('home', 'dark');
    const view = await main.webview(`!!document.querySelector('#task') && document.body.dataset.mode === 'composer'`, 15000);
    const p = await s.webviewPoint(view, '#task'); await main.click(p.x, p.y); await delay(250);
    await main.type('What is everyone doing?'); await delay(200); await main.key('Enter');
    await view.waitFor(`[...document.querySelectorAll('#home-conv .home-msg.from-owner')].some(m => /What is everyone doing/.test(m.textContent))`, 20000).catch(() => {});
    await delay(2500);

    // ---------- Picking an agent: the same right panel is its chat.
    await s.selectAgent('Finished edit', { settle: 2500 });
    let chat = await L.overseerView(main);
    for (let i = 0; i < 20 && chat.title !== 'Finished edit'; i++) { await delay(250); chat = await L.overseerView(main); }
    const picked = await L.layout(main);
    const review = await main.webview(`!!document.getElementById('diffs') && document.getElementById('diffs').innerText.includes('notes.md')`, 20000).then(f => f.eval(`document.body.dataset.runId || ''`)).catch(() => null);
    check('picking an agent: its review wide in the middle, its chat in the same right panel with a back control; still no tab rows',
      picked.groups.length === 2 && picked.groups[0].width > picked.groups[1].width && picked.tabStrips === 0 && chat.mode === 'chat' && chat.title === 'Finished edit' && chat.back && review !== null, { picked, chat });
    await L.settled(main, 'notes.md');
    const agentShot = await shots('agent', 'dark');
    const secondShot = await L.shot(s, second, 'other-window-1920x1080');
    L.sideBySide(s, agentShot, secondShot, 'agent-beside-other-window');

    // Back (the arrow), then ⌥⌘U both ways.
    const frame = await main.webview(`!!document.querySelector('#back-to-overseer') && !document.querySelector('#back-to-overseer').hidden && document.body.dataset.mode === 'chat'`, 10000);
    await frame.eval(`document.querySelector('#back-to-overseer').click()`);
    await delay(1200);
    const back = await L.overseerView(main);
    check('back returns the right panel to Overseer\'s conversation, with its history; the review stays in the middle',
      back.mode === 'composer' && back.said.some(t => /What is everyone doing/.test(t)) && (await L.layout(main)).groups.length === 2, back);
    await main.focusWorkbench(); await main.key('u', { meta: true, alt: true }); await delay(1500);
    const toAgent = await L.overseerView(main);
    await main.focusWorkbench(); await main.key('u', { meta: true, alt: true }); await delay(1500);
    const toHome = await L.overseerView(main);
    check('⌥⌘U turns the panel to the agent\'s chat and back to Overseer', toAgent.mode === 'chat' && toAgent.title === 'Finished edit' && toHome.mode === 'composer' && toHome.said.some(t => /What is everyone doing/.test(t)), { toAgent, toHome });

    // ---------- Voice Mode takes over the same panel.
    await s.selectAgent('Finished edit', { settle: 2000 });
    await main.command('Overseer: Voice Mode: Turn On or Off');
    const voiceView = await main.webview(`!!window.__voice && !!document.getElementById('voice-canvas') && document.getElementById('voice-stage').checkVisibility()`, 30000).catch(() => null);
    await voiceView?.waitFor(`document.getElementById('voice-state').dataset.state === 'listening'`, 30000).catch(() => {});
    await delay(1500);
    const voiced = await L.layout(main);
    const voiceState = await voiceView?.eval(`({ state: document.getElementById('voice-state').dataset.state, width: innerWidth, mode: document.body.dataset.mode })`).catch(() => null);
    check('Voice Mode takes over the same right panel; the review stays in the middle',
      voiceState?.state === 'listening' && voiced.groups.length === 2 && voiced.tabStrips === 0 && Math.abs(voiceState.width - voiced.groups[1].width) <= 4, { voiced, voiceState });
    await shots('voice', 'dark');
    await main.command('Overseer: Voice Mode: Turn On or Off'); await delay(1500);

    // Follow: the agent's real files in the middle (inline changes); the right panel stays Overseer's and no file tree covers it.
    await s.selectAgent('Finished edit', { settle: 2000 });
    await main.command('Overseer: Follow the Agent in Its Files'); await delay(3000);
    const follow = await L.layout(main);
    const right = await L.overseerView(main);
    const trees = await main.evalWorkbench(`[...document.querySelectorAll('.pane-header')].filter(h => h.offsetParent).map(h => h.querySelector('.title')?.textContent.trim() || '')`);
    check('Follow in the Overseer window: the agent\'s files in the middle, Overseer\'s panel on the right, and no separate file tree anywhere',
      follow.groups.length === 2 && follow.tabStrips === 0 && follow.breadcrumbs === 0 && Math.abs(right.width - follow.groups[1].width) <= 4 && !trees.some(t => /^(Worktree|Files)/i.test(t)), { follow, right, trees });
    await shots('follow', 'dark');
    await main.command('Overseer: Diffs Only'); await delay(2000);

    // ---------- The button again: the cluttered layout, exactly.
    const b2 = await button(main);
    check('the button now reads Close Workspace', b2?.text === 'Close Workspace', b2);
    const t1 = Date.now();
    await main.click(b2.x, b2.y);
    await delay(1500);
    const askBack = await dialog(main);
    measure('a question on the way back (nothing unsaved, nothing running)', askBack || 'none');
    try { main.close(); } catch {}
    main = await L.reopened(s, false); conns.push(main);
    s.cdp = main;
    await ready(main);
    await L.size(main, 1920, 1080);
    let after;
    for (let i = 0; i < 40; i++) { after = await L.layout(main); if (after.groups.length === before.groups.length && after.panel) break; await delay(250); }
    await delay(1500); after = await L.layout(main);
    measure('back to the owner\'s window (ms)', Date.now() - t1);
    await s.screenshot('restored-1920x1080');
    const same = (a, b) => a.sidebar === b.sidebar && a.sidebarTitle === b.sidebarTitle && a.panel === b.panel && a.auxiliary === b.auxiliary && a.groups.length === b.groups.length
      && a.groups.every((g, i) => JSON.stringify(g.tabs) === JSON.stringify(b.groups[i].tabs) && g.active === b.groups[i].active && Math.abs(g.share - b.groups[i].share) <= 0.02);
    check('running it again restores the cluttered layout exactly (Explorer, the terminal panel, the groups and their shares, each group\'s tabs in order and its active tab)', same(before, after), { before, after });
    const terminals = await main.evalWorkbench(`[...document.querySelectorAll('.terminal-tabs-entry, .single-terminal-tab, .terminal-tab')].length`).catch(() => null);
    measure('terminal after coming back: tabs shown, the old command running', { terminals, running: await ticking().catch(() => false) });
    await delay(4000);
    check('the offer does not come back', !(await toast(main)));
    check('the other window and the user settings are unchanged throughout', (await L.layout(second)).tabStrips === 1 && L.norm(fs.readFileSync(settingsFile, 'utf8')) === settingsBefore);

    // ---------- The three themes (the theme is the owner's own setting, changed here by the test).
    const b3 = await button(main);
    await main.click(b3.x, b3.y);
    for (let i = 0; i < 20; i++) { const d = await dialog(main); if (d) { measure('the question the second time', d); await main.evalWorkbench(`[...document.querySelectorAll('.monaco-dialog-box .monaco-button')].find(b => /^(Open|Save All)$/.test(b.textContent.trim()))?.click()`); break; } await delay(150); }
    await delay(1500);
    try { main.close(); } catch {}
    main = await L.reopened(s, true); conns.push(main);
    s.cdp = main;
    await ready(main);
    await main.waitFor(`[...document.querySelectorAll('.part.editor .editor-group-container')].filter(g => g.offsetParent).length === 2`, 30000, 'the layout again');
    for (const [theme, tag] of [['Overseer Light', 'light'], ['Overseer', 'overseer']]) {
      const bg = `getComputedStyle(document.querySelector('.part.activitybar')).backgroundColor`;
      const was = await main.evalWorkbench(bg);
      const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = theme; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2));
      await main.waitFor(`${bg} !== ${JSON.stringify(was)}`, 20000, 'theme ' + theme).catch(() => {});
      await delay(1500);
      await main.command('Overseer: Back to Overseer\'s Conversation'); await delay(1200);
      await L.settled(main, 'live.txt');
      await shots('home', tag);
      await s.selectAgent('Finished edit', { settle: 2500 });
      await L.settled(main, 'notes.md');
      await shots('agent', tag);
    }
    const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = 'Overseer Dark'; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2));
    measure('the extension\'s log', L.extensionLog(s, /overseer window|focus mode/));
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { if (live) s.ctl('run.interrupt', { run_id: live.run.id }); } catch {}
    for (const c of conns) { try { c.close(); } catch {} }
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
