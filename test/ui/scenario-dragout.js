// Packaged-UI scenario for AC-71 (take an agent out), fixture runs only. An agent row is dragged
// from the side bar into the editor area with real HTML drag events: its chat opens there as an
// editor. The alternatives work too: Open to the Side (context menu) opens the chat beside, and
// Pin to Grid (row action) puts the agent on the grid.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

(async () => {
  const s = new Session('dragout');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'drag-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'window.dialogStyle': 'custom', 'window.menuStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const make = title => s.ctl('task.create', { repo, harness: 'generic', program: '/bin/echo', args: [`${title} says hello`], prompt: '', title });
    const a = make('Drag me'), b = make('Open me beside'), c = make('Pin me');
    for (let i = 0; i < 30 && s.ctl('state').runs.some(r => ['queued', 'starting', 'running'].includes(r.status)); i++) await delay(300);
    await s.openOverseerView(); await delay(1500);
    const rowPoint = title => cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === ${JSON.stringify(title)}).pop(); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 70, y: b.top + b.height / 2 }; })()`, 15000, title);
    const editorPoint = () => cdp.evalWorkbench(`(() => { const e = document.querySelector('.part.editor') .getBoundingClientRect(); return { x: e.left + e.width / 2, y: e.top + e.height / 2 }; })()`);

    // Drag "Drag me" into the editor area.
    const from = await rowPoint('Drag me');
    const data = await cdp.drag(from, await editorPoint());
    await delay(2500);
    const opened = await cdp.webview(`document.body.dataset.runId === ${JSON.stringify(a.run.id)} && /says hello/.test(document.body.innerText)`, 15000).then(() => true, () => false);
    const tabs = await cdp.evalWorkbench(`[...document.querySelectorAll('.tab')].map(t => t.getAttribute('aria-label'))`);
    await s.screenshot('dragged-into-editor');
    check('dragging an agent from the side bar into the editor area opens its chat there', !!data && opened && tabs.some(t => /^Drag me/.test(t || '')),
      { dragTypes: data?.items?.map(i => i.mimeType), opened, tabs });

    // Open to the Side from the context menu.
    const pt = await rowPoint('Open me beside');
    await cdp.click(pt.x, pt.y, { button: 'right' }); await delay(800);
    const item = await cdp.waitFor(`(() => { const a = [...document.querySelectorAll('.monaco-menu .action-item .action-label')].find(a => /^Open to the Side/.test(a.getAttribute('aria-label') || a.textContent.trim())); if (!a) return null; const r = a.getBoundingClientRect(); return { x: r.left + 20, y: r.top + r.height / 2 }; })()`, 5000, 'menu item');
    await cdp.click(item.x, item.y); await delay(2000);
    const beside = await cdp.webview(`document.body.dataset.runId === ${JSON.stringify(b.run.id)} && /says hello/.test(document.body.innerText)`, 15000).then(() => true, () => false);
    check('Open to the Side (context menu) opens the agent\'s chat beside', beside);

    // Pin to Grid from the row action.
    const pp = await rowPoint('Pin me'); await cdp.move(pp.x, pp.y); await delay(500);
    const pin = await cdp.evalWorkbench(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === 'Pin me').pop(); const a = [...r.querySelectorAll('.actions .action-label')].find(a => /^Pin to Grid/.test(a.getAttribute('aria-label') || '')); const b = a.getBoundingClientRect(); return { x: b.left + 8, y: b.top + 8 }; })()`);
    await cdp.click(pin.x, pin.y); await delay(1000);
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(2500);
    const grid = await cdp.webview(`!!document.querySelector('.grid .tile')`, 20000);
    const pinned = await grid.eval(`!!document.querySelector('.grid .tile[data-run=${JSON.stringify(c.run.id)}]')`);
    await s.screenshot('pinned-in-grid');
    check('Pin to Grid (row action) puts the agent on the grid', pinned);
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
