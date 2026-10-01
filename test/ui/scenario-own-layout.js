// Packaged-UI scenario for AC-244 (opening an agent leaves your layout alone), fixture agents only.
// The owner has two editor groups of their own files and the secondary side bar open. Selecting an
// agent with no changes, then one with changes (review and chat), then home (New Agent) keeps both
// groups with their tabs and the secondary side bar: Overseer opens beside them. No user setting
// changes. (The Overseer layout, which takes the whole window, is scenario-overseer-window.js.)
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('own-layout');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const fx = name => path.join(repoRoot, 'fixtures/fake-harness', name);
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'own-repo'), { dirty: false });
    const wsFile = path.join(s.root, 'own.code-workspace');
    fs.writeFileSync(wsFile, JSON.stringify({ folders: [{ path: repo }], settings: {} }, null, 2));
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'workbench.editor.enablePreview': false, 'workbench.editor.enablePreviewFromQuickOpen': false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.install(latestVsix());
    s.launch(wsFile, { OVERSEER_CLAUDE_PATH: fx('claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode', CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
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

    check('no user setting changed at any point', norm(fs.readFileSync(settingsFile, 'utf8')) === settingsBefore);
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
