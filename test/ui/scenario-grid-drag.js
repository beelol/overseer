// Packaged-UI scenario for AC-104 (fixture runs only): build the grid by dragging. From one pinned
// agent the grid grows to 16 tiles: each agent is dragged from the side bar onto the grid (a real
// drag from VS Code's tree onto the grid's editor group, which places it on the grid's edge) and then
// dragged by its tile header to a chosen edge of another tile (drag events inside the grid's webview:
// CDP cannot intercept a drag that starts inside a webview). The layout is measured at each step;
// the drop preview shows the edge; a 17th agent is refused with "The grid is full (16)" and changes
// nothing; Alt+arrow moves a tile without a mouse; a window reload keeps the layout. Screenshots at
// 4, 9 and 16 tiles in the three Overseer themes.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, until } = require('./harness');

const THEMES = ['Overseer Dark', 'Overseer Light', 'Overseer'];

(async () => {
  const s = new Session('grid-drag');
  const result = { checks: [], steps: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'grid-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': THEMES[0] });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const settingsFile = path.join(s.profile, 'User/settings.json');
    // A theme is in place once the workbench carries its classes (15 s; a loaded machine applies it later).
    const themeClasses = () => s.cdp.evalWorkbench(`document.querySelector('.monaco-workbench')?.className || ''`);
    const setTheme = async theme => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); if (cur['workbench.colorTheme'] === theme) return; const was = await themeClasses(); cur['workbench.colorTheme'] = theme; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await until(themeClasses, c => c !== was, 15000, 100); };
    const titles = Array.from({ length: 17 }, (_, i) => `Agent ${String(i + 1).padStart(2, '0')}`);
    const ids = {};
    for (const t of titles) ids[t] = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/echo', args: [`${t} is done`], prompt: '', title: t }).run.id;
    const title = Object.fromEntries(Object.entries(ids).map(([t, id]) => [id, t]));
    await until(() => !s.ctl('state').runs.some(r => ['queued', 'starting', 'running'].includes(r.status)), Boolean, 60000, 300);

    // One agent to start from: select it and Pin to Grid, then show the grid.
    await s.selectAgent(titles[0], { settle: 1500 });
    await cdp.command('Overseer: Pin to Grid'); await delay(800);
    await cdp.command('Overseer: Toggle Agent Grid');
    let dash = await s.editorView(`document.body.dataset.mode === 'grid' && document.querySelectorAll('.grid .tile').length === 1`);
    const measure = () => dash.eval(`(() => { const out = {}; for (const t of document.querySelectorAll('.grid .tile')) { const r = t.getBoundingClientRect(); out[t.dataset.run] = { l: Math.round(r.left), t: Math.round(r.top), r: Math.round(r.right), b: Math.round(r.bottom) }; } return { count: Object.keys(out).length, rects: out, layout: document.querySelector('.grid').dataset.layout }; })()`);

    // A real drag from the side bar onto the grid's editor group (its tab strip).
    const sideDrop = async t => {
      const from = await cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.part.sidebar .monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === ${JSON.stringify(t)}).pop(); if (!r) return null; r.scrollIntoView({ block: 'nearest' }); const b = r.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 10000, 'row ' + t);
      const to = await cdp.evalWorkbench(`(() => { const tab = [...document.querySelectorAll('.part.editor .tab')].find(x => /^Overseer/.test(x.getAttribute('aria-label') || '')); const strip = tab.closest('.tabs-container').getBoundingClientRect(); return { x: strip.right - 30, y: strip.top + strip.height / 2 }; })()`);
      return cdp.drag(from, to);
    };
    // Drag a tile by its header to an edge of another tile (drag events in the webview).
    const tileMove = (id, target, edge) => dash.eval(`(() => {
      const head = document.querySelector('.tile[data-run="${id}"] .tile-head');
      const r = document.querySelector('.tile[data-run="${target}"]').getBoundingClientRect();
      const inset = v => Math.max(34, Math.min(v * 0.2, v / 2 - 4));
      const x = { left: r.left + inset(r.width), right: r.right - inset(r.width), top: r.left + r.width / 2, bottom: r.left + r.width / 2 }['${edge}'];
      const y = { left: r.top + r.height / 2, right: r.top + r.height / 2, top: r.top + inset(r.height), bottom: r.bottom - inset(r.height) }['${edge}'];
      const dt = new DataTransfer();
      const fire = (el, type) => el.dispatchEvent(new DragEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt }));
      fire(head, 'dragstart');
      const over = document.elementFromPoint(x, y);
      fire(over, 'dragenter'); fire(over, 'dragover');
      const p = document.querySelector('.grid-drop');
      const preview = { shown: !p.hidden, edge: p.dataset.edge, target: p.dataset.target };
      fire(over, 'drop'); fire(head, 'dragend');
      return preview;
    })()`);
    const beside = (m, t, edge) => {
      const overlapY = Math.min(m.b, t.b) - Math.max(m.t, t.t) > 10, overlapX = Math.min(m.r, t.r) - Math.max(m.l, t.l) > 10;
      return edge === 'left' ? m.r <= t.l + 2 && overlapY : edge === 'right' ? m.l >= t.r - 2 && overlapY : edge === 'top' ? m.b <= t.t + 2 && overlapX : m.t >= t.b - 2 && overlapX;
    };
    const shots = async n => { for (const theme of THEMES) { await setTheme(theme); await s.screenshot(`grid-${n}-${theme.toLowerCase().replace(/\s+/g, '-')}`); } await setTheme(THEMES[0]); };

    let allPlaced = true, allBeside = true, allPreview = true;
    for (let i = 2; i <= 16; i++) {
      const t = titles[i - 1];
      const data = await sideDrop(t);
      await dash.waitFor(`document.querySelectorAll('.grid .tile').length === ${i}`, 8000).catch(() => {});
      let m = await measure();
      const placed = !!data && m.count === i && !!m.rects[ids[t]];
      // Then place it at a chosen edge of another tile.
      // Chosen edges that build a 4×4: the first row to the right, then each agent below the one above it.
      const k = i - 1, target = k < 4 ? ids[titles[k - 1]] : ids[titles[k - 4]], edge = k < 4 ? 'right' : 'bottom';
      const preview = await tileMove(ids[t], target, edge);
      // The grid once it has laid the tile out beside its target (8 s; the step records what was there).
      m = await until(measure, x => x.count === i && !!x.rects[ids[t]] && !!x.rects[target] && beside(x.rects[ids[t]], x.rects[target], edge), 8000, 100);
      const ok = m.count === i && beside(m.rects[ids[t]], m.rects[target], edge);
      result.steps.push({ tiles: i, dragged: t, from: 'side bar', then: { edge, of: title[target] }, preview, beside: ok, layout: JSON.parse(m.layout) });
      allPlaced &&= placed; allBeside &&= ok; allPreview &&= preview.shown && preview.edge === edge && preview.target === target;
      if ([4, 9, 16].includes(i)) await shots(i);
    }
    check('each agent dragged from the side bar lands in the grid (1 to 16 tiles, counted at each step)', allPlaced, result.steps.map(x => x.tiles));
    const final = await measure();
    const distinct = xs => new Set(xs.map(x => Math.round(x / 6))).size;
    const shape = { cols: distinct(Object.values(final.rects).map(r => r.l)), rows: distinct(Object.values(final.rects).map(r => r.t)) };
    check('the chosen edges built a 4×4 grid of 16 tiles', final.count === 16 && shape.cols === 4 && shape.rows === 4, shape);
    check('each tile dragged to an edge of another tile ends up on that side of it (measured on screen at each step)', allBeside, result.steps.map(x => `${x.tiles}:${x.then.edge}:${x.beside}`));
    check('during each drag the drop preview shows the edge and tile it will land on', allPreview, result.steps.map(x => x.preview));

    // The 17th agent is refused and nothing changes.
    const before17 = await measure();
    await sideDrop(titles[16]);
    const fullShown = await dash.waitFor(`!document.querySelector('.grid-full').hidden && /The grid is full \\(16\\)/.test(document.querySelector('.grid-full').textContent)`, 4000).then(() => true, () => false);
    await s.screenshot('grid-full');
    await delay(800);
    const after17 = await measure();
    const tabs = await cdp.evalWorkbench(`[...document.querySelectorAll('.part.editor .tab')].map(t => t.getAttribute('aria-label'))`);
    check('a 17th agent is refused with "The grid is full (16)" and the layout does not change (no chat editor opens either)', fullShown && after17.count === 16 && after17.layout === before17.layout && !tabs.some(t => /Agent 17/.test(t || '')), { fullShown, count: after17.count, tabs });
    // The same refusal inside the grid: no drop zone once it is full.
    const external = await dash.eval(`(() => { const dt = new DataTransfer(); dt.setData('application/x-overseer-run', 'r-not-in-grid'); const g = document.querySelector('.grid'); const r = g.getBoundingClientRect(); const e = new DragEvent('dragover', { bubbles: true, cancelable: true, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, dataTransfer: dt }); g.dispatchEvent(e); return { accepted: e.defaultPrevented, preview: !document.querySelector('.grid-drop').hidden, full: !document.querySelector('.grid-full').hidden }; })()`);
    check('at 16 no drop zone appears for another agent; the drag says the grid is full', !external.preview && external.full, external);

    // Keyboard: Alt+Left moves the focused tile past its left neighbor.
    const kbId = Object.entries((await measure()).rects).sort((a, b) => b[1].l - a[1].l || a[1].t - b[1].t)[0][0];
    const kbBefore = (await measure()).rects[kbId];
    await dash.eval(`(() => { const c = document.createElement('div'); c.id = 'kb-spot'; c.style.cssText = 'position:fixed;left:1px;top:1px;width:3px;height:3px;z-index:99'; document.body.append(c); return true; })()`);
    { const p = await s.webviewPoint(dash, '#kb-spot'); await cdp.click(p.x, p.y); await delay(200); }
    await dash.eval(`document.querySelector('.tile[data-run="${kbId}"]').focus()`);
    await cdp.key('ArrowLeft', { alt: true });
    const kbAfter = (await until(measure, x => !!x.rects[kbId] && x.rects[kbId].l < kbBefore.l, 10000, 100)).rects[kbId];
    check('Alt+Left moves the focused tile to the left (a keyboard path to place tiles)', kbAfter && kbAfter.l < kbBefore.l, { before: kbBefore, after: kbAfter });

    // A reload keeps the layout.
    const beforeReload = (await measure()).layout;
    await cdp.command('Developer: Reload Window'); await delay(6000);
    cdp = await s.connect(); s.cdp = cdp;
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'after reload');
    dash = await cdp.webview(`document.body.dataset.mode === 'grid' && document.querySelectorAll('.grid .tile').length === 16`, 40000);
    const afterReload = (await until(measure, x => x.layout === beforeReload, 10000, 200)).layout;
    check('a window reload keeps the grid layout exactly', afterReload === beforeReload, { same: afterReload === beforeReload });
    await s.screenshot('after-reload');
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
