// Packaged-UI scenario for AC-250 (one command opens the whole Overseer layout), fixture agents
// only. A cluttered window: the Explorer side bar, the terminal panel and two editor groups with six
// tabs of five files. The status bar's Workspace button arranges the whole view in one step:
// Overseer's conversation, the working agent's review (following it) and its chat, in three columns
// sized for the screen; the terminal panel and the owner's tabs are gone. The same button (now
// "Close Workspace") puts back the original layout exactly: the parts, the groups and their sizes,
// each group's tabs in order and its active tab. At 1440×900 and at 1920×1080; the palette command
// and its second run do the same. No setting changes.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, git } = require('./harness');

(async () => {
  const s = new Session('workspace');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'ws-repo'), { dirty: false });
    for (const f of ['d.txt', 'e.txt']) fs.writeFileSync(path.join(repo, f), `${f}\n`);
    git(repo, 'add', '.'); git(repo, 'commit', '-q', '-m', 'more files');
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'workbench.editor.enablePreview': false, 'workbench.editor.enablePreviewFromQuickOpen': false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.install(latestVsix());
    s.launch(repo, {});
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const norm = text => { const o = JSON.parse(text); if (o['extensions.autoUpdate'] === false) o['extensions.autoUpdate'] = 'off'; return JSON.stringify(o); };
    const settingsBefore = norm(fs.readFileSync(settingsFile, 'utf8'));
    // A working agent that keeps editing (Follow has something to follow) and a finished one.
    s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', "sed -i '' 's/^L3: original$/L3: done/' b.txt"], prompt: '', title: 'Finished edit' });
    const live = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'i=0; while [ $i -lt 400 ]; do i=$((i+1)); echo "step $i" >> live.txt; sleep 1; done'], prompt: '', title: 'Live edits' });
    await delay(2500);

    const layout = () => cdp.evalWorkbench(`(() => {
      const vis = sel => { const e = document.querySelector(sel); return !!e && e.offsetWidth > 0 && e.offsetHeight > 0 && getComputedStyle(e).display !== 'none' && !e.classList.contains('hidden'); };
      const groups = [...document.querySelectorAll('.part.editor .editor-group-container')].filter(g => g.offsetParent);
      const total = groups.reduce((n, g) => n + g.getBoundingClientRect().width, 0);
      return { sidebar: vis('.part.sidebar'), panel: vis('.part.panel'), auxiliary: vis('.part.auxiliarybar'), sidebarTitle: document.querySelector('.part.sidebar .title-label')?.textContent.trim() || '',
        groups: groups.map(g => ({ width: Math.round(g.getBoundingClientRect().width), share: Math.round(g.getBoundingClientRect().width / total * 100) / 100,
          tabs: [...g.querySelectorAll('.tab')].map(t => (t.getAttribute('aria-label') || '').split(',')[0]), active: (g.querySelector('.tab.active')?.getAttribute('aria-label') || '').split(',')[0] })) };
    })()`);
    const openFile = async name => {
      await cdp.focusWorkbench();
      await cdp.key('p', { meta: true });
      await cdp.waitFor(`(() => { const i = document.querySelector('.quick-input-widget input'); return !!i && i === document.activeElement; })()`, 5000, 'quick open');
      await cdp.type(name);
      await cdp.waitFor(`[...document.querySelectorAll('.quick-input-widget .monaco-list-row')].some(r => (r.getAttribute('aria-label') || '').startsWith(${JSON.stringify(name)}))`, 8000, 'quick open ' + name);
      await cdp.key('Enter'); await delay(600);
    };
    const button = () => cdp.evalWorkbench(`(() => { const e = [...document.querySelectorAll('.statusbar-item')].find(e => /^(Workspace|Close Workspace)$/.test(e.textContent.trim())); if (!e) return null; const b = e.getBoundingClientRect(); return { text: e.textContent.trim(), x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`);
    const home = async () => (await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.view-composer')`, 15000)).eval(`({ mode: document.body.dataset.mode, composer: !document.querySelector('.view-composer').hidden })`);
    const reviewText = async () => { const f = await cdp.webview(`!!document.getElementById('diffs') && document.getElementById('diffs').innerText.includes('live.txt')`, 20000).catch(() => null); return f ? f.eval(`document.getElementById('diffs').innerText`) : ''; };
    const same = (a, b) => a.sidebar === b.sidebar && a.panel === b.panel && a.auxiliary === b.auxiliary && a.groups.length === b.groups.length
      && a.groups.every((g, i) => JSON.stringify(g.tabs) === JSON.stringify(b.groups[i].tabs) && g.active === b.groups[i].active && Math.abs(g.share - b.groups[i].share) <= 0.02);

    // The clutter: Explorer, the terminal, two groups of files.
    await cdp.command('View: Show Explorer'); await delay(600);
    for (const f of ['a.txt', 'b.txt', 'c.txt']) await openFile(f);
    await cdp.command('View: Split Editor Right'); await delay(800);
    for (const f of ['d.txt', 'e.txt']) await openFile(f);
    await openFile('README.md');
    await cdp.command('View: Toggle Terminal'); await delay(1500);

    for (const [width, height] of [[1440, 900], [1920, 1080]]) {
      const size = `${width}x${height}`;
      await cdp.call('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 0, mobile: false }, cdp.workbench); await delay(1500);
      const before = await layout();
      s.note(`${size}: layout before`, before);
      check(`${size}: the cluttered start: side bar, terminal panel and two editor groups with the owner's tabs`, before.sidebar && before.panel && before.groups.length === 2 && before.groups.flatMap(g => g.tabs).length >= 5, before);
      await s.screenshot(`cluttered-${size}`);

      // One click on the status bar's Workspace button.
      const b = await cdp.waitFor(`(() => { const e = [...document.querySelectorAll('.statusbar-item')].find(e => e.textContent.trim() === 'Workspace'); return !!e; })()`, 10000, 'Workspace button').then(button);
      check(`${size}: the status bar has a Workspace button`, b && b.text === 'Workspace', b);
      const t0 = Date.now();
      await cdp.click(b.x, b.y);
      let during;
      for (let i = 0; i < 100; i++) {
        during = await layout();
        if (during.groups.length === 3 && /^Review/.test(during.groups[1].active) && during.groups[2].active === 'Live edits') break;
        await delay(200);
      }
      const took = Date.now() - t0;
      await delay(1500);
      during = await layout();
      const view = await home();
      const text1 = await reviewText();
      await s.screenshot(`workspace-${size}`);
      const ownerTabs = during.groups.flatMap(g => g.tabs).filter(t => /\.(txt|md)$/.test(t));
      check(`${size}: one click gives the three-part layout: Overseer's conversation, the agent's review, the agent's chat`,
        during.groups.length === 3 && during.groups[0].active === 'Overseer' && /^Review: Live edits/.test(during.groups[1].active) && during.groups[2].active === 'Live edits' && view.mode === 'composer' && view.composer,
        { took, groups: during.groups, view });
      check(`${size}: the terminal panel and the owner's tabs are gone; each column is at least 360 px`, !during.panel && ownerTabs.length === 0 && during.groups.every(g => g.width >= 360), { panel: during.panel, ownerTabs, widths: during.groups.map(g => g.width) });
      check(`${size}: sized for the screen: ${width < 1600 ? 'the side bar gives way at this width' : 'the side bar stays (Overseer\'s agents)'}; the review is the widest column`,
        (width < 1600 ? !during.sidebar : during.sidebar && /Overseer/i.test(during.sidebarTitle)) && during.groups[1].width > during.groups[0].width && during.groups[1].width > during.groups[2].width, { sidebar: during.sidebar, title: during.sidebarTitle, widths: during.groups.map(g => g.width) });
      await delay(3000);
      const text2 = await reviewText();
      const steps = t => (t.match(/step\s\d+/g) || []).length;
      s.note('review text (start)', { first: text1.slice(0, 300), later: text2.slice(0, 300) });
      check(`${size}: the review follows the working agent (its file grows while the workspace is open)`, steps(text2) > steps(text1) && steps(text1) > 0, { before: steps(text1), after: steps(text2) });
      const b2 = await button();
      check(`${size}: the button now reads Close Workspace`, b2 && b2.text === 'Close Workspace', b2);

      // The same button puts everything back.
      await cdp.click(b2.x, b2.y);
      let after;
      for (let i = 0; i < 60; i++) { after = await layout(); if (same(before, after)) break; await delay(250); }
      await s.screenshot(`restored-${size}`);
      check(`${size}: closing restores the original layout exactly (parts, groups and sizes, each group's tabs in order and its active tab)`, same(before, after), { before, after });
    }

    // The palette command toggles too, and a second run closes it.
    await cdp.command('Overseer: Open Workspace'); await delay(4000);
    const viaPalette = await layout();
    await cdp.command('Overseer: Open Workspace'); await delay(3500);
    const back = await layout();
    check('Overseer: Open Workspace from the palette opens it and running it again closes it', viaPalette.groups.length === 3 && back.groups.length === 2 && back.panel, { viaPalette: viaPalette.groups.map(g => g.active), back: back.groups.map(g => g.active) });
    check('no setting changed (apart from VS Code\'s own migration)', norm(fs.readFileSync(settingsFile, 'utf8')) === settingsBefore);
    s.ctl('run.interrupt', { run_id: live.run.id });
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
