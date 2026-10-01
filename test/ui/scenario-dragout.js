// Packaged-UI scenario for AC-71 (take an agent out), fixture runs only. An agent row is dragged
// from the side bar into the editor area with real HTML drag events: its chat opens there as an
// editor. The alternatives work too: Open to the Side (context menu) opens the chat beside, and
// Pin to Grid (row action) puts the agent on the grid.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, until } = require('./harness');

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
    await until(() => !s.ctl('state').runs.some(r => ['queued', 'starting', 'running'].includes(r.status)), Boolean, 60000, 300);
    await s.openOverseerView();
    const rowPoint = title => cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === ${JSON.stringify(title)}).pop(); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 70, y: b.top + b.height / 2 }; })()`, 15000, title);
    const editorPoint = () => cdp.evalWorkbench(`(() => { const e = document.querySelector('.part.editor') .getBoundingClientRect(); return { x: e.left + e.width / 2, y: e.top + e.height / 2 }; })()`);

    // Drag "Drag me" into the editor area, once its row has stopped moving (the list draws its rows
    // as they arrive; two equal readings 200 ms apart).
    let lastFrom = null;
    const from = await until(async () => { const p = await rowPoint('Drag me'); const same = lastFrom && p.x === lastFrom.x && p.y === lastFrom.y; lastFrom = p; return same && p; }, Boolean, 15000, 200) || lastFrom;
    const data = await cdp.drag(from, await editorPoint());
    const opened = await cdp.webview(`document.body.dataset.runId === ${JSON.stringify(a.run.id)} && /says hello/.test(document.body.innerText)`, 30000).then(() => true, () => false);
    const tabs = await until(() => cdp.evalWorkbench(`[...document.querySelectorAll('.tab')].map(t => t.getAttribute('aria-label'))`), ts => ts.some(t => /^Drag me/.test(t || '')), 15000, 200);
    await s.screenshot('dragged-into-editor');
    check('dragging an agent from the side bar into the editor area opens its chat there', !!data && opened && tabs.some(t => /^Drag me/.test(t || '')),
      { dragTypes: data?.items?.map(i => i.mimeType), opened, tabs });

    // Open to the Side from the context menu.
    const pt = await rowPoint('Open me beside');
    await cdp.click(pt.x, pt.y, { button: 'right' });
    const item = await cdp.waitFor(`(() => { const a = [...document.querySelectorAll('.monaco-menu .action-item .action-label')].find(a => /^Open to the Side/.test(a.getAttribute('aria-label') || a.textContent.trim())); if (!a) return null; const r = a.getBoundingClientRect(); return { x: r.left + 20, y: r.top + r.height / 2 }; })()`, 15000, 'menu item');
    // VS Code's menu ignores a click that comes too soon after it opened: click until the menu has
    // taken it (it closes), noting each extra click.
    const menuOpen = () => cdp.evalWorkbench(`!!document.querySelector('.monaco-menu .action-item')`);
    for (let i = 0; i < 5; i++) {
      await cdp.click(item.x, item.y);
      if (!(await until(menuOpen, open => !open, 3000, 100))) break;
      s.note('the menu did not take the click; clicking Open to the Side again');
    }
    const beside = await cdp.webview(`document.body.dataset.runId === ${JSON.stringify(b.run.id)} && /says hello/.test(document.body.innerText)`, 30000).then(() => true, () => false);
    check('Open to the Side (context menu) opens the agent\'s chat beside', beside);

    // Pin to Grid from the row action.
    // The row's action once the hover has drawn it; the grid once it shows the pinned tile.
    const pp = await rowPoint('Pin me'); await cdp.move(pp.x, pp.y);
    const pin = await cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === 'Pin me').pop(); const a = r && [...r.querySelectorAll('.actions .action-label')].find(a => a.offsetParent && /^Pin to Grid/.test(a.getAttribute('aria-label') || '')); if (!a) return null; const b = a.getBoundingClientRect(); return { x: b.left + 8, y: b.top + 8 }; })()`, 15000, 'Pin to Grid action');
    await cdp.click(pin.x, pin.y);
    await cdp.command('Overseer: Toggle Agent Grid');
    const grid = await cdp.webview(`!!document.querySelector('.grid .tile')`, 20000);
    const pinned = await grid.waitFor(`!!document.querySelector('.grid .tile[data-run=${JSON.stringify(c.run.id)}]')`, 20000).then(() => true, () => false);
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
