// Packaged-UI scenario for AC-105 (fixture runs only): track an agent from the grid. Two agents keep
// editing files; clicking a tile opens that agent's review beside the grid in follow mode and marks
// the tile as tracked; the review follows its edits; clicking the other tile switches; Escape, and
// the grid's "Grid alone" control, close the review and restore the exact grid layout. No second
// review, chat or window opens along the way.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

(async () => {
  const s = new Session('grid-track');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'track-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const writer = file => ['-c', `for i in $(seq 1 90); do echo "edit $i" >> ${file}; sleep 1; done`];
    const A = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: writer('alpha.txt'), prompt: '', title: 'Alpha writer' });
    const B = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: writer('bravo.txt'), prompt: '', title: 'Bravo writer' });
    await delay(2500);
    await s.openOverseerView(); await delay(800);
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(2500);
    const dash = await s.editorView(`document.body.dataset.mode === 'grid' && document.querySelectorAll('.grid .tile').length === 2`);
    const gridState = () => dash.eval(`({ layout: document.querySelector('.grid').dataset.layout, tracked: document.querySelector('.grid').dataset.tracked, marked: [...document.querySelectorAll('.tile.tracked')].map(t => t.dataset.run), bar: !document.querySelector('.grid-track').hidden })`);
    const workbench = () => cdp.evalWorkbench(`({ groups: document.querySelectorAll('.editor-group-container').length, tabs: [...document.querySelectorAll('.part.editor .tab')].map(t => t.getAttribute('aria-label')), windows: 1 })`);
    const reviewOf = async runId => cdp.webview(`!!document.getElementById('diffs') && document.body.dataset.runId === ${JSON.stringify(runId)}`, 20000).catch(() => null);
    const clickTile = async runId => { const p = await s.webviewPoint(dash, `.tile[data-run="${runId}"] .tile-body`); await cdp.click(p.x, p.y); await delay(150); await cdp.click(p.x, p.y); await delay(2500); };
    const before = await gridState();
    await s.screenshot('grid-alone');

    // Track A.
    await clickTile(A.run.id);
    const reviewA = await reviewOf(A.run.id);
    let g = await gridState(), wb = await workbench();
    const followA = reviewA && await reviewA.waitFor(`document.getElementById('follow')?.dataset.state === 'following'`, 10000).then(() => true, () => false);
    await s.screenshot('tracking-alpha');
    check('clicking a tile opens that agent\'s review beside the grid in follow mode, and the tile shows it is tracked', !!reviewA && followA && g.tracked === A.run.id && g.marked.join() === A.run.id && g.bar && wb.groups === 2, { g, wb, followA });
    // The review follows the agent's edits.
    const followed = reviewA && await reviewA.waitFor(`/alpha\\.txt/.test(document.getElementById('follow-state')?.textContent || '') || [...document.querySelectorAll('.diff-file .file-path')].some(e => e.textContent === 'alpha.txt')`, 15000).then(() => true, () => false);
    check('the tracked review follows the agent\'s edits (alpha.txt)', followed, await reviewA?.eval(`({ follow: document.getElementById('follow-state')?.textContent, files: [...document.querySelectorAll('.diff-file .file-path')].map(e => e.textContent) })`));

    // Switch to B.
    await clickTile(B.run.id);
    const reviewB = await reviewOf(B.run.id);
    g = await gridState(); wb = await workbench();
    const reviews = wb.tabs.filter(t => /^Review/.test(t || ''));
    await s.screenshot('tracking-bravo');
    check('clicking another tile switches tracking to that agent (one review, two groups, no chat editor or window opened)', !!reviewB && g.tracked === B.run.id && g.marked.join() === B.run.id && wb.groups === 2 && reviews.length === 1 && !wb.tabs.some(t => /writer/.test(t || '') && !/^Review/.test(t || '')), { g, wb });

    // Escape: back to the grid alone, exactly as it was.
    { const p = await s.webviewPoint(dash, '.grid-track-text'); await cdp.click(p.x, p.y); await delay(200); }
    await cdp.key('Escape'); await delay(2500);
    g = await gridState(); wb = await workbench();
    await s.screenshot('escape-grid-alone');
    check('Escape closes the review and restores the grid alone with the exact layout', !g.tracked && !g.bar && g.layout === before.layout && wb.groups === 1 && !wb.tabs.some(t => /^Review/.test(t || '')), { g, wb, before: before.layout });

    // The control does the same.
    await clickTile(A.run.id);
    await reviewOf(A.run.id);
    { const p = await s.webviewPoint(dash, '#grid-untrack'); await cdp.click(p.x, p.y); await delay(2500); }
    g = await gridState(); wb = await workbench();
    check('the grid\'s "Grid alone" control does the same', !g.tracked && g.layout === before.layout && wb.groups === 1 && !wb.tabs.some(t => /^Review/.test(t || '')), { g, wb });
    for (const t of [A, B]) s.ctl('run.interrupt', { run_id: t.run.id });
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
