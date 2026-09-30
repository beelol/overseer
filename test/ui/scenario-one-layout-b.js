// AC-264 phase 1, way B (fixture agents only): the window reopens on an Overseer-owned workspace file
// (in the extension's global storage) for the owner's folder, whose own settings hide the tab rows
// and breadcrumbs. Two windows of one profile: the owner's repository (Explorer, the terminal, two
// groups of files, one with unsaved words) and another folder with a file open. Measured: what the
// reopen asks and how long it takes, the window title, VS Code's recent list, the other window's tab
// strip and the user settings (unchanged), home on the right, an agent picked (its review in the
// middle and its chat on the right, with back), at 1920×1080 and 1440×900; then running it again
// reopens the owner's folder, and what VS Code put back.
const fs = require('fs');
const path = require('path');
const { Session, latestVsix, delay } = require('./harness');
const L = require('./one-layout-common');

const COMMAND = 'Overseer: One Layout, Way B: Reopen as the Overseer Window';

(async () => {
  const s = new Session('one-layout/b');
  const result = { checks: [], measures: {} };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const measure = (name, value) => { result.measures[name] = value; s.note(`MEASURE ${name}`, value); };
  const conns = [];
  try {
    const { repo, other, settingsFile } = L.setup(s);
    s.install(latestVsix());
    // A window VS Code reopens on another folder or workspace forgets the launch's --extensions-dir
    // (the owner's windows use the default folder); VS Code's own variable keeps the test's.
    s.launch(repo, { VSCODE_EXTENSIONS: s.extensions });
    await s.connect();
    let main = await L.attach(s, t => /ws-repo/.test(t)); conns.push(main);
    s.cdp = main;
    await main.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const second = await L.secondWindow(s, main, other); conns.push(second);
    const settingsBefore = L.norm(fs.readFileSync(settingsFile, 'utf8'));
    const { live } = L.agents(s, repo);
    await delay(2500);

    // The owner's window: Explorer, two groups of files (d.txt with unsaved words), the terminal.
    await main.command('View: Show Explorer'); await delay(500);
    for (const f of ['a.txt', 'c.txt']) await L.openFile(main, f);
    await main.command('View: Split Editor Right'); await delay(800);
    await L.openFile(main, 'd.txt');
    await main.type('unsaved words from the owner\n'); await delay(300);
    await main.command('View: Toggle Terminal'); await delay(1500);
    // The other window: a file open, so it has a tab strip.
    await L.openFile(second, 'README.md');
    for (const c of [main, second]) await L.size(c, 1920, 1080);
    const before = await L.layout(main), secondBefore = await L.layout(second);
    s.note('before', { main: before, second: secondBefore });
    check('the start: the owner\'s window has two groups of files, the terminal and Explorer; the other window has its tab strip',
      before.groups.length === 2 && before.panel && before.sidebar && secondBefore.tabStrips === 1, { before, secondBefore });
    await L.shot(s, main, 'before-1920x1080');
    const recentBefore = await L.recent(main);
    measure('recent list before', recentBefore);
    const cfg = c => c.evalWorkbench(`(() => { try { const x = globalThis.vscode?.context?.configuration?.(); return x ? { dir: x['extensions-dir'], env: x.userEnv?.VSCODE_EXTENSIONS, keys: Object.keys(x).length } : 'no config'; } catch (e) { return e.message; } })()`).catch(e => e.message);
    measure('window config before', await cfg(main));
    const titlesBefore = await s.quiet?.main(`require('electron').BrowserWindow.getAllWindows().map(w => w.getTitle())`).catch(() => null);
    measure('window titles before (macOS title bar)', titlesBefore);

    // Way B: one command.
    const t0 = Date.now();
    await main.command(COMMAND);
    // What the reopen asks: VS Code's own question about unsaved words, if any.
    let prompt = null;
    for (let i = 0; i < 15 && !prompt; i++) {
      prompt = await main.evalWorkbench(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d || !d.offsetParent) return null; return { text: d.querySelector('.dialog-message-text')?.textContent || d.textContent, detail: d.querySelector('.dialog-message-detail')?.textContent || '', buttons: [...d.querySelectorAll('.monaco-button')].map(b => b.textContent.trim()).filter(Boolean) }; })()`).catch(() => null);
      if (!prompt) await delay(200);
    }
    measure('what VS Code asks when the window has unsaved words', prompt || 'nothing');
    if (prompt) {
      await L.shot(s, main, 'unsaved-prompt');
      // The owner keeps their words: Save.
      await main.evalWorkbench(`(() => { const b = [...document.querySelectorAll('.monaco-dialog-box .monaco-button')].find(b => /^Save$/.test(b.textContent.trim())); b?.click(); return !!b; })()`);
    }
    // The same window reloads on Overseer's workspace file.
    await delay(1500);
    try { main.close(); } catch {}
    main = await L.attach(s, t => !/notes-repo/.test(t) && /ws-repo/.test(t), 60000); conns.push(main);
    s.cdp = main;
    await delay(3000);
    measure('window config after the reopen', await cfg(main));
    await main.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar in the Overseer window');
    await main.waitFor(`[...document.querySelectorAll('.part.editor .editor-group-container')].filter(g => g.offsetParent).length === 2 && !document.querySelector('.part.editor .tabs-container')?.offsetParent`, 30000, 'review and Overseer view, no tab rows');
    let home = await L.overseerView(main);
    for (let i = 0; i < 40 && home.mode !== 'composer'; i++) { await delay(250); home = await L.overseerView(main); }
    measure('reopen: command to the arranged layout (ms)', Date.now() - t0);
    await L.size(main, 1920, 1080);
    await delay(1500);
    const inB = await L.layout(main);
    const titles = await s.quiet?.main(`require('electron').BrowserWindow.getAllWindows().map(w => w.getTitle())`).catch(() => null);
    measure('window titles in the Overseer window (macOS title bar)', titles);
    measure('the Overseer window: title and layout', inB);
    const layoutsDir = path.join(s.profile, 'User/globalStorage/beelol.overseer/layouts');
    const files = fs.existsSync(layoutsDir) ? fs.readdirSync(layoutsDir).flatMap(d => fs.readdirSync(path.join(layoutsDir, d)).map(f => path.join(layoutsDir, d, f))) : [];
    measure('Overseer\'s workspace file', files.map(f => ({ file: path.relative(s.profile, f), content: JSON.parse(fs.readFileSync(f, 'utf8')) })));
    measure('the extension\'s log', L.extensionLog(s));
    check('the workspace file is in the extension\'s global storage, not in the repository', files.length === 1 && !fs.readdirSync(repo).some(f => f.endsWith('.code-workspace')), files);
    check('home: the agents list on the left, the working agent\'s review in the middle, Overseer\'s conversation on the right; no tab rows, no breadcrumbs',
      inB.sidebar && /Overseer/i.test(inB.sidebarTitle) && inB.groups.length === 2 && inB.tabStrips === 0 && inB.breadcrumbs === 0 && !inB.panel && home.mode === 'composer', { inB, home });
    await L.settled(main, 'live.txt');
    await L.shot(s, main, 'home-1920x1080');
    const secondDuring = await L.layout(second);
    const secondShot = await L.shot(s, second, 'other-window-1920x1080');
    check('the other window keeps its tab strip and its tabs', secondDuring.tabStrips === 1 && JSON.stringify(secondDuring.groups.map(g => g.tabs)) === JSON.stringify(secondBefore.groups.map(g => g.tabs)), secondDuring);
    check('no user setting changed', L.norm(fs.readFileSync(settingsFile, 'utf8')) === settingsBefore);
    L.sideBySide(s, path.join(s.evidence, fs.readdirSync(s.evidence).filter(f => /home-1920x1080/.test(f))[0]), secondShot, 'home-beside-other-window');
    measure('recent list in the Overseer window', await L.recent(main));
    const saved = fs.readFileSync(path.join(repo, 'd.txt'), 'utf8');
    measure('d.txt on disk after the reopen (the unsaved words)', saved);

    // An agent picked: its review in the middle, its chat in the same right panel.
    await s.selectAgent('Finished edit', { settle: 2500 });
    let view = await L.overseerView(main);
    for (let i = 0; i < 20 && view.title !== 'Finished edit'; i++) { await delay(250); view = await L.overseerView(main); }
    const picked = await L.layout(main);
    const reviewTitle = await main.webview(`!!document.getElementById('diffs') && document.body.innerText.includes('b.txt')`, 20000).then(f => f.eval(`document.body.innerText.slice(0, 200)`)).catch(() => '');
    check('an agent picked: its review in the middle, its chat in the right panel with a back control; still no tab rows',
      picked.groups.length === 2 && picked.tabStrips === 0 && view.mode === 'chat' && view.title === 'Finished edit' && view.back && /b\.txt/.test(reviewTitle), { picked, view, review: reviewTitle.slice(0, 120) });
    await L.settled(main, 'notes.md');
    const agentShot = await L.shot(s, main, 'agent-1920x1080');
    L.sideBySide(s, agentShot, secondShot, 'agent-beside-other-window');
    await L.size(main, 1440, 900); await delay(800);
    await L.shot(s, main, 'agent-1440x900');
    measure('1440x900 layout', await L.layout(main));
    await L.size(main, 1920, 1080);

    // Back (the chat's back control), then ⌥⌘U to the agent and back again.
    const frame = await main.webview(`!!document.querySelector('#back-to-overseer') && !document.querySelector('#back-to-overseer').hidden && document.body.dataset.mode === 'chat'`, 10000);
    await frame.eval(`document.querySelector('#back-to-overseer').click()`);
    await delay(1200);
    const back = await L.overseerView(main);
    check('back returns the right panel to Overseer\'s conversation', back.mode === 'composer', back);
    await main.focusWorkbench(); await main.key('u', { meta: true, alt: true }); await delay(1500);
    const toAgent = await L.overseerView(main);
    await main.focusWorkbench(); await main.key('u', { meta: true, alt: true }); await delay(1500);
    const toHome = await L.overseerView(main);
    measure('⌥⌘U from home, then again', { first: toAgent, second: toHome });

    // Running it again: the owner's folder, as VS Code kept it.
    const t1 = Date.now();
    await main.command(COMMAND);
    await delay(1500);
    try { main.close(); } catch {}
    main = await L.attach(s, t => !/notes-repo/.test(t) && /ws-repo/.test(t), 60000); conns.push(main);
    s.cdp = main;
    await main.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar back in the folder window');
    await L.size(main, 1920, 1080);
    let after;
    for (let i = 0; i < 40; i++) { after = await L.layout(main); if (after.groups.flatMap(g => g.tabs).length >= 3) break; await delay(250); }
    await delay(1500); after = await L.layout(main);
    measure('back to the folder window (ms)', Date.now() - t1);
    measure('the folder window after coming back', after);
    await L.shot(s, main, 'back-in-folder-window-1920x1080');
    const ownerTabs = before.groups.map(g => g.tabs.filter(t => /\.(txt|md)$/.test(t)));
    check('running it again reopens the owner\'s folder with its groups, tabs, Explorer and terminal as VS Code kept them',
      after.groups.length === before.groups.length && JSON.stringify(after.groups.map(g => g.tabs.filter(t => /\.(txt|md)$/.test(t)))) === JSON.stringify(ownerTabs) && after.sidebar && after.panel, { before: before.groups, after: after.groups, panel: after.panel, sidebar: after.sidebarTitle });
    measure('recent list after coming back', await L.recent(main));
    measure('window titles after coming back', await s.quiet?.main(`require('electron').BrowserWindow.getAllWindows().map(w => w.getTitle())`).catch(() => null));
    const secondAfter = await L.layout(second);
    check('the other window and the user settings are unchanged throughout', secondAfter.tabStrips === 1 && L.norm(fs.readFileSync(settingsFile, 'utf8')) === settingsBefore, secondAfter);
    s.ctl('run.interrupt', { run_id: live.run.id });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    for (const c of conns) { try { c.close(); } catch {} }
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
