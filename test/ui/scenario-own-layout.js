// Packaged-UI scenario for AC-244 (opening an agent leaves your layout alone), fixture agents only.
// The owner has two editor groups of their own files and the secondary side bar open. Selecting an
// agent with no changes, then one with changes (review and chat), then home (New Agent) keeps both
// groups with their tabs and the secondary side bar: Overseer opens beside them. Then the dashboard
// in its own window (Open Dashboard in New Window) hides the tab strips there only: the first
// window's tab strips stay, and user settings never change; leaving the dashboard removes the
// settings from that window's workspace file.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');
const { Cdp } = require('./cdp');

(async () => {
  const s = new Session('own-layout');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const fx = name => path.join(repoRoot, 'fixtures/fake-harness', name);
  const modeFile = path.join(s.root, 'claude-mode');
  let cdp2;
  try {
    const repo = makeRepo(path.join(s.root, 'own-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'workbench.editor.enablePreview': false, 'workbench.editor.enablePreviewFromQuickOpen': false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: fx('claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode', CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const norm = text => { const o = JSON.parse(text); if (o['extensions.autoUpdate'] === false) o['extensions.autoUpdate'] = 'off'; return JSON.stringify(o); };
    const settingsBefore = norm(fs.readFileSync(settingsFile, 'utf8'));
    fs.writeFileSync(modeFile, 'echo');
    const quiet = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', prompt: 'Say hello.', title: 'Say hello' });
    const edits = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', "sed -i '' 's/^L2: original$/L2: edited/' a.txt"], prompt: '', title: 'Edit a file' });
    for (let i = 0; i < 40 && !['completed', 'waiting_for_user'].includes(s.ctl('state').runs.find(r => r.id === quiet.run.id).status); i++) await delay(300);

    const layout = (c = cdp) => c.evalWorkbench(`(() => {
      const vis = sel => { const e = document.querySelector(sel); return !!e && e.offsetWidth > 0 && e.offsetHeight > 0 && getComputedStyle(e).display !== 'none' && !e.classList.contains('hidden'); };
      const groups = [...document.querySelectorAll('.part.editor .editor-group-container')].filter(g => g.offsetParent);
      const shown = e => !!e && e.offsetWidth > 0 && e.offsetHeight > 0 && getComputedStyle(e).display !== 'none';
      return { auxiliary: vis('.part.auxiliarybar'), panel: vis('.part.panel'), tabStrips: groups.filter(g => shown(g.querySelector('.tabs-and-actions-container, .tabs-container'))).length,
        groups: groups.map(g => ({ tabs: [...g.querySelectorAll('.tab')].map(t => (t.getAttribute('aria-label') || '').split(',')[0]), active: (g.querySelector('.tab.active')?.getAttribute('aria-label') || '').split(',')[0] })) };
    })()`);
    const openFile = async name => {
      await cdp.focusWorkbench();
      await cdp.key('p', { meta: true });
      await cdp.waitFor(`(() => { const i = document.querySelector('.quick-input-widget input'); return !!i && i === document.activeElement; })()`, 5000, 'quick open');
      await cdp.type(name);
      await cdp.waitFor(`[...document.querySelectorAll('.quick-input-widget .monaco-list-row')].some(r => (r.getAttribute('aria-label') || '').startsWith(${JSON.stringify(name)}))`, 8000, 'quick open ' + name);
      await cdp.key('Enter'); await delay(600);
    };
    // The owner's own arrangement: two groups of files and the secondary side bar.
    await openFile('a.txt'); await openFile('b.txt');
    await cdp.command('View: Split Editor Right'); await delay(800);
    await openFile('c.txt');
    await cdp.command('View: Toggle Secondary Side Bar Visibility'); await delay(1200);
    const start = await layout();
    const owner = start.groups.map(g => JSON.stringify(g.tabs));
    check('the owner\'s layout: two editor groups of their files and the secondary side bar open', start.groups.length === 2 && start.auxiliary, start);
    await s.screenshot('owner-layout');
    const kept = l => owner.every(tabs => l.groups.some(g => JSON.stringify(g.tabs.filter(t => /\.txt$|\.md$/.test(t))) === tabs && g.tabs.every(t => /\.txt$|\.md$/.test(t)))) && l.auxiliary;

    await s.selectRun(quiet.run.id);
    const l1 = await layout();
    await s.screenshot('agent-chat-beside');
    check('selecting an agent keeps both of the owner\'s groups (their tabs, nothing of Overseer\'s in them) and the secondary side bar; its chat opens beside them',
      kept(l1) && l1.groups.length === 3 && l1.groups[2].active === 'Overseer', l1);

    await s.selectRun(edits.run.id);
    await cdp.webview(`!!document.getElementById('diffs') && document.querySelectorAll('.diff-file').length > 0`, 20000).catch(() => null);
    await delay(1000);
    const l2 = await layout();
    await s.screenshot('review-and-chat-beside');
    check('an agent with changes: its review and chat open beside the owner\'s groups, which stay as they were',
      kept(l2) && l2.groups.length === 4 && /^Review: Edit a file/.test(l2.groups[2].active) && l2.groups[3].active === 'Overseer', l2);

    await cdp.command('Overseer: New Agent'); await delay(1500);
    const l3 = await layout();
    check('opening home keeps them too (the review closes; the owner\'s groups stay)', kept(l3) && l3.groups.length === 3, l3);
    await s.screenshot('home-beside');

    // The dashboard in its own window: its settings stay in that window (its workspace file).
    const { targetInfos: before } = await cdp.call('Target.getTargets');
    const known = new Set(before.filter(t => t.type === 'page').map(t => t.targetId));
    await cdp.command('Overseer: Open Dashboard in New Window');
    let page;
    for (let i = 0; i < 80 && !page; i++) {
      const { targetInfos } = await cdp.call('Target.getTargets');
      page = targetInfos.find(t => t.type === 'page' && t.url.includes('workbench') && !known.has(t.targetId));
      if (!page) await delay(250);
    }
    if (!page) throw new Error('the dashboard window did not open');
    cdp2 = await Cdp.connect(s.profile).catch(() => null);
    const { sessionId } = await cdp2.call('Target.attachToTarget', { targetId: page.targetId, flatten: true });
    cdp2.workbench = sessionId;
    await cdp2.call('Runtime.enable', {}, sessionId); await cdp2.call('Page.enable', {}, sessionId);
    await cdp2.call('Emulation.setFocusEmulationEnabled', { enabled: true }, sessionId).catch(() => {});
    await cdp2.waitFor(`!!document.querySelector('.monaco-workbench .part.activitybar')`, 60000, 'dashboard window');
    await cdp2.waitFor(`[...document.querySelectorAll('.part.editor .editor-group-container')].some(g => g.offsetParent && g.querySelector('iframe'))`, 60000, 'dashboard in the new window');
    await delay(3000);
    const wsFile = path.join(s.profile, 'User/globalStorage/beelol.overseer/Overseer.code-workspace');
    const wsSettings = () => { try { return JSON.parse(fs.readFileSync(wsFile, 'utf8')).settings || {}; } catch { return null; } };
    const inDash = { first: await layout(), second: await layout(cdp2), user: norm(fs.readFileSync(settingsFile, 'utf8')) === settingsBefore, workspace: wsSettings() };
    await cdp2.screenshot(path.join(s.evidence, `${String(++s.shot).padStart(2, '0')}-dashboard-window.png`));
    await s.screenshot('first-window-while-dashboard-open');
    check('Open Dashboard in New Window: the dashboard window hides its tab strips (its own workspace settings); the first window keeps its tab strips and user settings do not change',
      inDash.second.tabStrips === 0 && inDash.first.tabStrips === inDash.first.groups.length && inDash.first.tabStrips > 0 && inDash.user && inDash.workspace && inDash.workspace['workbench.editor.showTabs'] === 'none',
      { firstWindowTabStrips: `${inDash.first.tabStrips} of ${inDash.first.groups.length}`, secondWindowTabStrips: inDash.second.tabStrips, userSettingsUnchanged: inDash.user, workspaceSettings: inDash.workspace });
    // Leaving the dashboard there.
    await cdp2.focusWorkbench();
    await cdp2.key('p', { meta: true, shift: true });
    await cdp2.waitFor(`(() => { const i = document.querySelector('.quick-input-widget input'); return !!i && i === document.activeElement; })()`, 5000, 'palette in the dashboard window');
    await cdp2.type('Overseer: Exit Dashboard');
    await delay(800); await cdp2.key('Enter'); await delay(3000);
    const left = { second: await layout(cdp2), first: await layout(), workspace: wsSettings(), user: norm(fs.readFileSync(settingsFile, 'utf8')) === settingsBefore };
    check('leaving it: the settings leave that window\'s workspace file; the first window and user settings are unchanged throughout',
      left.workspace && !('workbench.editor.showTabs' in left.workspace) && left.first.tabStrips === left.first.groups.length && left.user, left);
    await s.quiet.main(`(() => { const ws = require('electron').BrowserWindow.getAllWindows().filter(w => /Overseer \\(Workspace\\)|Overseer.code-workspace|Dashboard/i.test(w.getTitle())); ws.forEach(w => w.close()); return ws.map(w => w.getTitle()); })()`).then(t => s.note('closed the dashboard window', t));
    await delay(1500);
    check('no user setting changed at any point', norm(fs.readFileSync(settingsFile, 'utf8')) === settingsBefore);
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { cdp2?.close(); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
