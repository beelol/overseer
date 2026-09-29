// Packaged-UI scenario for AC-253 (Overseer leads with what happened while you were away), with
// the Claude fixture as Overseer's model and generic agents (no paid turn):
//   home opens on its empty hero; while it is closed, eight agents finish across two
//   repositories (one fails); reopening it shows one line grouped by repository and outcome as
//   the conversation's newest message instead of the hero; asking "what happened while I was
//   away" in the composer gives the same line, with no model turn.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('away');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const site = makeRepo(path.join(s.root, 'site'), { dirty: false });
    const notes = makeRepo(path.join(s.root, 'notes'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'overseer');
    s.launch(site, {
      OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE',
    });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');

    // Home, before anything happened: the empty hero.
    await cdp.command('Overseer: Talk to Overseer'); await delay(2000);
    let view = await s.editorView(`!!document.getElementById('home')`);
    const before = await view.eval(`document.body.dataset.conversation || ''`);
    check('home opens on its empty hero while nothing has happened', before === '', { conversation: before });
    await cdp.command('View: Close Editor'); await delay(1500);

    // Eight agents finish across two repositories while home is closed; one fails.
    const start = (repo, title, script) => s.ctl('task.create', { repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sh', args: ['-c', script], prompt: '', title }).run.id;
    const runs = [];
    for (const t of ['Header', 'Footer', 'Pricing', 'Blog']) runs.push(start(site, t, 'echo done'));
    runs.push(start(site, 'Search', 'exit 3'));
    for (const t of ['Inbox', 'Tags', 'Export']) runs.push(start(notes, t, 'echo done'));
    const ended = () => runs.every(id => ['completed', 'failed'].includes((s.ctl('state').runs.find(r => r.id === id) || {}).status));
    for (let i = 0; i < 150 && !ended(); i++) await delay(200);
    check('eight agents finished across two repositories, one failed', ended(), s.ctl('state').runs.map(r => [r.title, r.status]));

    // Reopened: the line leads, not the hero.
    const line = 'While you were away: 4 finished and 1 failed in site; 3 finished in notes.';
    await cdp.command('Overseer: Talk to Overseer'); await delay(1500);
    view = await s.editorView(`!!document.getElementById('home')`);
    await view.waitFor(`[...document.querySelectorAll('#home-conv > [data-id]')].some(e => e.textContent.includes(${JSON.stringify(line)}))`, 20000).catch(() => {});
    await delay(1200);
    const shown = await view.eval(`(() => {
      const items = [...document.querySelectorAll('#home-conv > [data-id]')];
      const last = items[items.length - 1];
      // On screen: inside the conversation's own visible box, not only the page's.
      const r = last && last.getBoundingClientRect(), box = document.getElementById('home-conv').getBoundingClientRect();
      return { conversation: document.body.dataset.conversation || '', last: last ? last.textContent.trim() : '', onScreen: !!r && r.top >= box.top - 1 && r.bottom <= box.bottom + 1 && r.bottom <= innerHeight && last.checkVisibility(), r: r && [r.top, r.bottom], box: [box.top, box.bottom] };
    })()`);
    await delay(300); await s.screenshot('reopened-while-away');
    check('reopening home shows the summary as the newest message, on screen, instead of the empty hero', shown.conversation === '1' && shown.last.includes(line) && shown.onScreen, shown);

    // Asked in the composer: the same line, with no model turn.
    const p = await s.webviewPoint(view, '#task');
    await cdp.click(p.x, p.y); await delay(200);
    await cdp.type('What happened while I was away?'); await delay(200); await cdp.key('Enter');
    await view.waitFor(`[...document.querySelectorAll('#home-conv > [data-id]')].filter(e => e.textContent.includes(${JSON.stringify(line)})).length >= 2`, 20000).catch(() => {});
    const count = await view.eval(`[...document.querySelectorAll('#home-conv > [data-id]')].filter(e => e.textContent.includes(${JSON.stringify(line)})).length`);
    const session = s.ctl('overseer.session');
    await delay(300); await s.screenshot('asked-what-happened');
    check('asking "what happened while I was away" gives the same line, with no model turn', count >= 2 && !session.run_id, { count, run: session.run_id });
  } catch (e) {
    s.note('ERROR ' + (e.stack || e.message));
    result.checks.push({ name: 'scenario ran', ok: false, detail: e.message });
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { s.ctl('daemon.stop_all'); } catch {}
    await s.quit();
    s.stopDaemon();
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    const failed = result.checks.filter(c => !c.ok);
    console.log(failed.length ? `${failed.length} check(s) failed` : `all ${result.checks.length} checks passed`);
    console.log(failed.length ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed.length ? 1 : 0);
  }
})();
