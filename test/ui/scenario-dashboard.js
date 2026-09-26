// Packaged-UI scenario for AC-57 (dashboard mode), fixture runs only. In a window WITHOUT a folder,
// with the side bar, the panel and two editor groups open: Overseer: Open Dashboard hides the side
// bar, panel and secondary side bar for this window, the dashboard fills the editor area and works
// (agents from repositories not open anywhere); it survives a window reload (AC-49); Exit Dashboard
// restores the previous layout exactly (parts and editor groups); no settings file changes; the
// optional open-on-startup setting opens it after a reload.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

(async () => {
  const s = new Session('dashboard');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'dash-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    // No folder: an empty window.
    s.launch('', {});
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const t = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', "sed -i '' 's/^L2: original$/L2: dashboard/' a.txt; echo done"], prompt: '', title: 'Dashboard demo' });
    for (let i = 0; i < 30 && s.ctl('state').runs.find(r => r.id === t.run.id).status !== 'completed'; i++) await delay(300);
    const settingsFile = path.join(s.profile, 'User/settings.json');
    // VS Code migrates some of its own settings on start (extensions.autoUpdate false → "off"); compare after that.
    const norm = text => { const o = JSON.parse(text); if (o['extensions.autoUpdate'] === false) o['extensions.autoUpdate'] = 'off'; return JSON.stringify(o); };
    const settingsBefore = fs.readFileSync(settingsFile, 'utf8');

    // A typical working layout: Explorer side bar, the panel (terminal) and two editor groups.
    await cdp.command('View: Show Explorer'); await delay(600);
    await cdp.command('View: Toggle Terminal'); await delay(1200);
    await cdp.command('File: New Untitled Text File'); await delay(600);
    await cdp.command('View: Split Editor Right'); await delay(600);
    const layout = () => cdp.evalWorkbench(`(() => {
      const vis = sel => { const e = document.querySelector(sel); return !!e && e.offsetWidth > 0 && e.offsetHeight > 0 && getComputedStyle(e).display !== 'none' && !e.classList.contains('hidden'); };
      const groups = [...document.querySelectorAll('.editor-group-container')].map(g => { const r = g.getBoundingClientRect(); return { w: Math.round(r.width), h: Math.round(r.height) }; });
      return { sidebar: vis('.part.sidebar'), panel: vis('.part.panel'), auxiliary: vis('.part.auxiliarybar'), groups: groups.length, sizes: groups };
    })()`);
    const before = await layout();
    s.note('layout before', before);
    await s.screenshot('before');
    check('starting layout: side bar and panel open, two editor groups', before.sidebar && before.panel && before.groups === 2, before);

    await cdp.command('Overseer: Open Dashboard');
    const dash = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail-list .row')`, 30000);
    await delay(1500);
    const during = await layout();
    const agents = await dash.eval(`[...document.querySelectorAll('.rail-list .row[data-run] .title')].map(e => e.textContent)`);
    await s.screenshot('dashboard');
    check('dashboard mode hides side bar, panel and secondary side bar; the dashboard works in a window without a folder',
      !during.sidebar && !during.panel && !during.auxiliary && agents.includes('Dashboard demo') && await dash.eval(`document.body.dataset.dashboard === '1'`), { during, agents });

    // Reload: the dashboard is restored and still in dashboard mode.
    await cdp.command('Developer: Reload Window');
    await delay(6000);
    cdp = await s.connect(); s.cdp = cdp;
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent)) || !!document.querySelector('iframe.webview')`, 60000, 'after reload');
    const dash2 = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail-list .row')`, 30000).catch(() => null);
    await delay(1500);
    const afterReload = await layout();
    check('the dashboard survives a window reload (still in dashboard mode)', !!dash2 && !afterReload.sidebar && !afterReload.panel && await dash2.eval(`document.body.dataset.dashboard === '1'`), afterReload);

    // Exit restores the previous layout exactly.
    await cdp.command('Overseer: Exit Dashboard');
    await delay(2500);
    const after = await layout();
    await s.screenshot('after-exit');
    check('Exit Dashboard restores the previous layout (side bar, panel, secondary side bar and editor groups)',
      after.sidebar === before.sidebar && after.panel === before.panel && after.auxiliary === before.auxiliary && after.groups === before.groups, { before, after });
    check('no setting changed (user settings identical apart from VS Code\'s own migration)', norm(fs.readFileSync(settingsFile, 'utf8')) === norm(settingsBefore), { before: norm(settingsBefore), after: norm(fs.readFileSync(settingsFile, 'utf8')) });

    // Optional: open the dashboard when VS Code starts.
    const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['overseer.dashboard.openOnStartup'] = true;
    fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2));
    await delay(800);
    await cdp.command('Developer: Reload Window');
    await delay(6000);
    cdp = await s.connect(); s.cdp = cdp;
    const dash3 = await cdp.webview(`document.body.dataset.ready === '1' && document.body.dataset.dashboard === '1'`, 40000).catch(() => null);
    check('with overseer.dashboard.openOnStartup the dashboard opens when VS Code starts', !!dash3);
    await s.screenshot('open-on-startup');
    await cdp.command('Overseer: Exit Dashboard');
    await delay(1500);
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
