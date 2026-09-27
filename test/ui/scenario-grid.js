// Packaged-UI scenario for AC-58 (agent grid), fixture runs only (no paid tokens): nine concurrent
// generic runs stream timestamped lines while the grid shows all nine; each tile must show a line
// within 250 ms of when it was printed, with webview event-loop lag p95 under 50 ms. A Claude
// fixture waiting for permission is answered from its tile; a pinned finished run stays; arrow
// keys move between tiles and Enter opens one; screenshots at 4 and 9 tiles in both themes.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('grid');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const runs = [];
  try {
    const repo = makeRepo(path.join(s.root, 'grid-repo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'overseer.grid.maxTiles': 9 });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE', CLAUDE_FIXTURE_MODE: 'permission' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const setSetting = async (k, v) => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur[k] = v; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(1500); };
    const state = () => s.ctl('state');
    // A finished run to pin.
    const done = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo finished work'], prompt: '', title: 'Finished and pinned', workspace_mode: 'worktree' });
    for (let i = 0; i < 30 && state().runs.find(r => r.id === done.run.id).status !== 'completed'; i++) await delay(300);
    // A Claude fixture waiting for permission.
    const perm = s.ctl('task.create', { repo, harness: 'claude', prompt: 'write perm.txt', title: 'Needs permission' });
    for (let i = 0; i < 60 && state().runs.find(r => r.id === perm.run.id).status !== 'waiting_for_user'; i++) await delay(300);

    await cdp.command('Overseer: Open Overseer View');
    const dash = await s.editorView();
    // Pin the finished run from its tile later; first open the grid.
    await cdp.command('Overseer: Toggle Agent Grid');
    await dash.waitFor(`document.querySelectorAll('.grid .tile').length >= 1`, 20000);
    // Pin the finished run through the dashboard message (as the tile's pin button does).
    await dash.eval(`window.overseerApi.postMessage({ type: 'pin', runId: ${JSON.stringify(done.run.id)}, on: true })`);
    await dash.waitFor(`!!document.querySelector('.grid .tile[data-run=${JSON.stringify(done.run.id)}]')`, 20000);

    // Seven streaming runs (with the permission run and the pinned one: nine tiles).
    const script = `perl -MTime::HiRes=time -e '$|=1; for (1..90) { printf "tick %d\\n", time*1000; select(undef,undef,undef,0.2) }'`;
    for (let i = 0; i < 7; i++) runs.push(s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', script], prompt: '', title: `Stream ${i + 1}` }));
    for (let i = 0; i < 30; i++) {
      const d = await dash.eval(`({ tiles: document.querySelectorAll('.grid .tile').length, mode: window.__overseer.mode(), active: window.__overseer.state().runs.filter(r => !r.parent_run_id && ['queued','starting','running','waiting_for_user'].includes(r.status)).length, pinned: window.__overseer.state().pinned, gridMax: window.__overseer.state().gridMax })`);
      if (d.tiles === 9) break;
      if (i % 5 === 0) s.note('grid wait', d);
      await delay(1000);
    }
    await dash.waitFor(`document.querySelectorAll('.grid .tile').length === 9`, 5000);
    // Measure: when a "tick <ms>" line appears in a tile, how late is it; and event-loop lag.
    await dash.eval(`(() => {
      window.__lat = []; window.__lag = []; window.__stages = [];
      // Where the time goes: printed → daemon event timestamp → message reaches the webview.
      window.addEventListener('message', e => { const d = e.data; if (d?.type !== 'events' || d.channel !== 'grid') return; const now = Date.now();
        for (const it of d.items) for (const m of String(it.event?.payload?.text || '').matchAll(/tick (\\d+)/g)) window.__stages.push([Number(m[1]), it.event.ts, now]); });
      // Lines already on the tiles are not new: only lines that appear from now on are timed.
      const seen = new Set(document.querySelectorAll('.grid .tile .msg .text'));
      const mo = new MutationObserver(() => { const now = Date.now(); for (const t of document.querySelectorAll('.grid .tile .msg .text')) { const m = /^tick (\\d+)$/.exec(t.textContent.trim()); if (m && !seen.has(t)) { seen.add(t); window.__lat.push(now - Number(m[1])); } } });
      mo.observe(document.querySelector('.grid'), { childList: true, subtree: true, characterData: true });
      (function tick() { const t0 = performance.now(); if (window.__lag.length < 400) setTimeout(() => { window.__lag.push(performance.now() - t0 - 25); tick(); }, 25); })();
      return true; })()`);
    await delay(9000);
    const layout = await dash.eval(`(() => { const g = document.querySelector('.grid'); return { tiles: document.querySelectorAll('.grid .tile').length, cols: g.style.getPropertyValue('--cols'), rows: g.style.getPropertyValue('--rows') }; })()`);
    await s.screenshot('grid-9-dark');
    const perf = await dash.eval(`(() => { const p = (a, q) => { const x = a.slice().sort((m, n) => m - n); return Math.round(x[Math.floor(x.length * q)] || 0); }; return { lines: window.__lat.length, latP95: p(window.__lat, .95), latMax: Math.max(...window.__lat), lagN: window.__lag.length, lagP95: p(window.__lag, .95), lagMax: Math.round(Math.max(...window.__lag)) }; })()`);
    const stages = await dash.eval(`(() => { const p = (a, q) => { const x = a.slice().sort((m, n) => m - n); return Math.round(x[Math.floor(x.length * q)] || 0); };
      const st = window.__stages; const toDaemon = st.map(([t, d]) => d - t), toWebview = st.map(([, d, w]) => w - d), total = st.map(([t, , w]) => w - t);
      return { n: st.length, toDaemonP50: p(toDaemon, .5), toDaemonP95: p(toDaemon, .95), daemonToWebviewP50: p(toWebview, .5), daemonToWebviewP95: p(toWebview, .95), arriveP95: p(total, .95) }; })()`);
    s.note('grid latency by stage (ms)', stages);
    perf.stages = stages;
    check('nine agents tile as 3×3', layout.tiles === 9 && layout.cols === '3' && layout.rows === '3', layout);
    check('nine concurrent streams: each tile shows a line within 250 ms (p95) and event-loop lag p95 stays under 50 ms', perf.lines > 100 && perf.latP95 < 250 && perf.lagP95 < 50, perf);
    await setSetting('workbench.colorTheme', 'Overseer Light');
    await s.screenshot('grid-9-light');

    // Permission answered from its tile.
    await dash.waitFor(`!!document.querySelector('.grid .tile[data-run=${JSON.stringify(perm.run.id)}] .tile-perm:not([hidden]) [data-permission="allow"]')`, 20000);
    await dash.eval(`document.querySelector('.grid .tile[data-run=${JSON.stringify(perm.run.id)}] .tile-perm [data-permission="allow"]').id = 'tile-allow'`);
    const allow = await s.webviewPoint(dash, '#tile-allow'); await cdp.click(allow.x, allow.y);
    let permDone; for (let i = 0; i < 40; i++) { permDone = state().runs.find(r => r.id === perm.run.id); if (permDone.status === 'completed') break; await delay(300); }
    const permFile = fs.existsSync(path.join(perm.workspace.path, 'perm.txt'));
    check('a permission request is answered from its tile (Allow → the agent continues and writes the file)', permDone.status === 'completed' && permFile, { status: permDone.status, permFile });

    // The pinned finished run stays; unpinned finished runs leave.
    await delay(1500);
    const pinned = await dash.eval(`({ pinnedTile: !!document.querySelector('.grid .tile[data-run=${JSON.stringify(done.run.id)}]'), pressed: document.querySelector('.grid .tile[data-run=${JSON.stringify(done.run.id)}] .tile-pin')?.getAttribute('aria-pressed') })`);
    check('a pinned finished run stays on the grid', pinned.pinnedTile && pinned.pressed === 'true', pinned);

    // Four tiles.
    for (let i = 0; i < 60 && state().runs.some(r => runs.some(x => x.run.id === r.id) && ['running', 'starting', 'queued'].includes(r.status)); i++) await delay(500);
    await setSetting('overseer.grid.maxTiles', 4);
    for (let i = 0; i < 3; i++) runs.push(s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'for i in 1 2 3 4 5 6 7 8 9 10; do echo "working $i"; sleep 1; done'], prompt: '', title: `Short ${i + 1}` }));
    await dash.waitFor(`document.querySelectorAll('.grid .tile').length === 4`, 20000);
    const four = await dash.eval(`({ tiles: document.querySelectorAll('.grid .tile').length, cols: document.querySelector('.grid').style.getPropertyValue('--cols') })`);
    check('with a maximum of 4, the grid shows 2×2', four.tiles === 4 && four.cols === '2', four);
    await s.screenshot('grid-4-light');
    await setSetting('workbench.colorTheme', 'Overseer Dark');
    await s.screenshot('grid-4-dark');

    // Keyboard: arrows move between tiles; Enter opens the agent's chat.
    // Put keyboard focus inside the webview (the search field), then on the first tile.
    // Put keyboard focus inside the webview with a click on an empty corner of the grid (not a tile).
    await dash.eval(`(() => { const g = document.querySelector('.grid'); const c = document.createElement('div'); c.id = 'grid-corner'; c.style.cssText = 'position:fixed;left:1px;top:1px;width:3px;height:3px;z-index:9'; document.body.append(c); return true; })()`);
    { const at = await s.webviewPoint(dash, '#grid-corner'); await cdp.click(at.x, at.y); await delay(200); }
    await dash.eval(`document.querySelector('.grid .tile').focus()`);
    const first = await dash.eval(`document.activeElement.dataset.run`);
    await cdp.key('ArrowRight'); await delay(200);
    const second = await dash.eval(`document.activeElement.dataset.run`);
    await cdp.key('Enter'); await delay(1500);
    const opened = await dash.eval(`({ mode: document.body.dataset.mode, title: document.getElementById('title')?.textContent, selected: window.__overseer.selected() })`);
    check('arrow keys move between tiles and Enter opens the agent in the chat', first && second && first !== second && opened.mode === 'chat' && opened.selected === second, { first, second, opened });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    for (const r of runs) { try { s.ctl('run.interrupt', { run_id: r.run.id }); } catch {} }
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
