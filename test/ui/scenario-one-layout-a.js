// AC-264 phase 1, way A (fixture agents only): the Overseer panel is a view in VS Code's secondary
// side bar, widened with VS Code's resize command; the review stays in the editor area with its one
// tab row. Two windows of one profile, as in scenario-one-layout-b.js. Measured: how wide the panel
// gets and stays, the review's tab row and breadcrumbs, the other window's tab strip and the user
// settings (unchanged), home on the right, an agent picked (its review in the middle and its chat on
// the right, with back), at 1920×1080 and 1440×900; then running it again.
const fs = require('fs');
const path = require('path');
const { Session, latestVsix, delay } = require('./harness');
const L = require('./one-layout-common');

const COMMAND = 'Overseer: One Layout, Way A: Overseer in the Right Side Bar';

(async () => {
  const s = new Session('one-layout/a');
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
    const main = await L.attach(s, t => /ws-repo/.test(t)); conns.push(main);
    s.cdp = main;
    await main.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const second = await L.secondWindow(s, main, other); conns.push(second);
    const settingsBefore = L.norm(fs.readFileSync(settingsFile, 'utf8'));
    const { live } = L.agents(s, repo);
    await delay(2500);

    await main.command('View: Show Explorer'); await delay(500);
    for (const f of ['a.txt', 'c.txt']) await L.openFile(main, f);
    await main.command('View: Split Editor Right'); await delay(800);
    await L.openFile(main, 'd.txt');
    await main.command('View: Toggle Terminal'); await delay(1500);
    await L.openFile(second, 'README.md');
    for (const c of [main, second]) await L.size(c, 1920, 1080);
    const before = await L.layout(main), secondBefore = await L.layout(second);
    check('the start: two groups of files, the terminal and Explorer; the other window has its tab strip', before.groups.length === 2 && before.panel && secondBefore.tabStrips === 1, { before, secondBefore });
    await L.shot(s, main, 'before-1920x1080');

    const t0 = Date.now();
    await main.command(COMMAND);
    await main.waitFor(`(() => { const a = document.querySelector('.part.auxiliarybar'); return !!a && a.offsetWidth > 0; })()`, 20000, 'secondary side bar');
    let home = await L.overseerView(main);
    for (let i = 0; i < 60 && home.mode !== 'composer'; i++) { await delay(250); home = await L.overseerView(main); }
    await delay(2500);
    measure('command to the arranged layout (ms)', Date.now() - t0);
    const inA = await L.layout(main);
    home = await L.overseerView(main);
    measure('way A: layout', inA);
    check('home: the agents list on the left, the review in the middle, Overseer\'s conversation in the right side bar; the terminal is closed',
      inA.sidebar && /Overseer/i.test(inA.sidebarTitle) && inA.auxiliary && inA.groups.length === 1 && !inA.panel && home.mode === 'composer', { inA, home });
    measure('the right panel\'s width (px) and share of the window', { width: inA.auxiliaryWidth, share: Math.round(inA.auxiliaryWidth / inA.window * 100) / 100, review: inA.groups[0]?.width });
    measure('widening the panel (the extension\'s log)', L.extensionLog(s));
    measure('tab rows over the review, breadcrumbs', { tabStrips: inA.tabStrips, breadcrumbs: inA.breadcrumbs });
    await L.settled(main, 'live.txt');
    await L.shot(s, main, 'home-1920x1080');
    const secondShot = await L.shot(s, second, 'other-window-1920x1080');
    const secondDuring = await L.layout(second);
    check('the other window keeps its tab strip and its tabs', secondDuring.tabStrips === 1 && JSON.stringify(secondDuring.groups.map(g => g.tabs)) === JSON.stringify(secondBefore.groups.map(g => g.tabs)), secondDuring);
    check('no user setting changed', L.norm(fs.readFileSync(settingsFile, 'utf8')) === settingsBefore);
    L.sideBySide(s, path.join(s.evidence, fs.readdirSync(s.evidence).filter(f => /home-1920x1080/.test(f))[0]), secondShot, 'home-beside-other-window');

    await s.selectAgent('Finished edit', { settle: 2500 });
    let view = await L.overseerView(main);
    for (let i = 0; i < 20 && view.title !== 'Finished edit'; i++) { await delay(250); view = await L.overseerView(main); }
    let picked = await L.layout(main);
    for (let i = 0; i < 20 && !/Finished edit/.test(picked.groups[0]?.active || ''); i++) { await delay(250); picked = await L.layout(main); }
    check('an agent picked: its review in the middle (one tab row), its chat in the right panel with a back control',
      picked.auxiliary && picked.groups.length === 1 && /Finished edit/.test(picked.groups[0].active) && view.mode === 'chat' && view.title === 'Finished edit' && view.back, { picked, view });
    await L.settled(main, 'notes.md');
    const agentShot = await L.shot(s, main, 'agent-1920x1080');
    L.sideBySide(s, agentShot, secondShot, 'agent-beside-other-window');
    await L.size(main, 1440, 900); await delay(1000);
    await L.shot(s, main, 'agent-1440x900');
    measure('1440x900 layout', await L.layout(main));
    await L.size(main, 1920, 1080);

    const frame = await main.webview(`!!document.querySelector('#back-to-overseer') && !document.querySelector('#back-to-overseer').hidden && document.body.dataset.mode === 'chat'`, 10000);
    await frame.eval(`document.querySelector('#back-to-overseer').click()`);
    await delay(1200);
    const back = await L.overseerView(main);
    check('back returns the right panel to Overseer\'s conversation', back.mode === 'composer', back);

    // Does the panel keep its width? Close and reopen the secondary side bar by hand (⌥⌘B).
    await main.command('View: Toggle Secondary Side Bar Visibility'); await delay(800);
    await main.command('View: Toggle Secondary Side Bar Visibility'); await delay(1500);
    measure('panel width after the owner hides and shows the secondary side bar', (await L.layout(main)).auxiliaryWidth);

    await main.command(COMMAND); await delay(3000);
    const after = await L.layout(main);
    measure('after running it again', after);
    await L.shot(s, main, 'after-1920x1080');
    check('running it again closes the panel and puts the owner\'s tabs back', !after.auxiliary && after.groups.length === 2 && JSON.stringify(after.groups.map(g => g.tabs.filter(t => /\.(txt|md)$/.test(t)))) === JSON.stringify(before.groups.map(g => g.tabs.filter(t => /\.(txt|md)$/.test(t)))), { before: before.groups, after: after.groups });
    check('the other window and the user settings are unchanged throughout', (await L.layout(second)).tabStrips === 1 && L.norm(fs.readFileSync(settingsFile, 'utf8')) === settingsBefore);
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
