// Packaged-UI scenario for AC-63 (history that stays tidy), fixture runs only: 300 finished runs;
// the agents rail shows active and recent ones; search finds runs by title and by text that only
// appears in an agent's output, answering in under 200 ms; finished runs are archived and restored
// from the rail (keyboard); bulk cleanup of archived worktrees removes only clean ones unless the
// user confirms discarding uncommitted work (branches are always kept); automatic archiving after
// the chosen age hides old finished runs.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, git } = require('./harness');

(async () => {
  const s = new Session('history');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'hist-repo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'overseer.history.autoArchiveDays': 0 });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');

    // 300 finished runs, each printing a unique needle only its output contains.
    const t0 = Date.now();
    const tasks = [];
    for (let i = 0; i < 300; i++) {
      const n = String(i).padStart(3, '0');
      tasks.push(s.ctl('task.create', { repo, harness: 'generic', program: '/bin/echo', args: [`needle-${n} output line`], prompt: '', title: `History task ${n}`, workspace_mode: 'worktree' }));
    }
    for (let i = 0; i < 120 && s.ctl('state').runs.some(r => ['queued', 'starting', 'running'].includes(r.status)); i++) await delay(500);
    s.note('created 300 runs', { ms: Date.now() - t0 });

    await cdp.command('Overseer: Open Overseer View');
    const dash = await cdp.webview(`document.body.dataset.ready === '1' && document.querySelectorAll('.rail-list .row[data-run]').length >= 300`, 60000);
    // Search: time from the keystroke to the rail showing the match.
    const timeSearch = async (q, expectTitle) => dash.eval(`new Promise(resolve => {
      const input = document.querySelector('.rail-head input[type=search]');
      const t0 = performance.now();
      input.value = ${JSON.stringify(q)}; input.dispatchEvent(new Event('input'));
      (function poll() {
        const titles = [...document.querySelectorAll('.rail-list .row[data-run] .title')].map(e => e.textContent);
        if (titles.length && titles.every(t => t === ${JSON.stringify(expectTitle)})) return resolve({ ms: Math.round(performance.now() - t0), titles });
        if (performance.now() - t0 > 5000) return resolve({ ms: -1, titles });
        requestAnimationFrame(poll);
      })();
    })`);
    const byTitle = await timeSearch('History task 123', 'History task 123');
    const byOutput = await timeSearch('needle-237', 'History task 237');
    await s.screenshot('search');
    check('with 300 runs, search answers in under 200 ms, by title and by text only in the agent output', byTitle.ms >= 0 && byTitle.ms < 200 && byOutput.ms >= 0 && byOutput.ms < 200, { byTitle, byOutput });
    await dash.eval(`(() => { const i = document.querySelector('.rail-head input[type=search]'); i.value = ''; i.dispatchEvent(new Event('input')); return true; })()`);
    await delay(500);

    // Archive and restore from the rail with the keyboard (Delete on a finished run's row).
    const target = tasks[5];
    const rowFocus = `(() => { const r = document.querySelector('.rail-list .row[data-task=${JSON.stringify(target.task.id)}]'); r.tabIndex = 0; r.focus(); return !!r; })()`;
    await dash.eval(rowFocus);
    await cdp.key('Delete'); await delay(1200);
    const archived = s.ctl('state').tasks.find(t => t.id === target.task.id).archived_ms;
    const hidden = await dash.eval(`!document.querySelector('.rail-list .row[data-task=${JSON.stringify(target.task.id)}]')`);
    await dash.eval(`document.querySelector('[data-action="rail-more"]').click()`); await delay(300);
    await dash.eval(`[...document.querySelectorAll('.menu-item')].find(b => /Show archived/.test(b.textContent)).click()`); await delay(800);
    const inArchive = await dash.eval(`[...document.querySelectorAll('.rail-list .row[data-task] .title')].map(e => e.textContent)`);
    await dash.eval(rowFocus);
    await cdp.key('Delete'); await delay(1200);
    const restored = !s.ctl('state').tasks.find(t => t.id === target.task.id).archived_ms;
    check('a finished run is archived from the rail (hidden, still listed under Show archived) and restored', !!archived && hidden && inArchive.includes('History task 005') && restored, { archived, hidden, inArchive: inArchive.slice(0, 5), restored });

    // Bulk cleanup of archived worktrees: clean ones only, unless discarding is confirmed.
    const [a, b, c] = [tasks[10], tasks[11], tasks[12]];
    fs.writeFileSync(path.join(c.workspace.path, 'uncommitted.txt'), 'work in progress\n');
    for (const t of [a, b, c]) s.ctl('task.archive', { task_id: t.task.id, archived: true });
    await delay(800);
    const dialog = async button => {
      const d = await cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d) return null; const b = [...d.querySelectorAll('.monaco-button')].find(b => b.textContent.trim().startsWith(${JSON.stringify(button)})); if (!b) return null; const r = b.getBoundingClientRect(); return { text: d.innerText, x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, 20000, 'dialog ' + button);
      await cdp.click(d.x, d.y); await delay(1500); return d.text;
    };
    await cdp.command('Overseer: Clean Up Archived Worktrees');
    await delay(2500);
    s.note('after cleanup command', await cdp.evalWorkbench(`({ dialog: document.querySelector('.monaco-dialog-box')?.innerText, toasts: [...document.querySelectorAll('.notification-toast')].map(t => t.innerText), quick: document.querySelector('.quick-input-widget')?.style.display, quickValue: document.querySelector('.quick-input-widget input')?.value })`));
    const text = await dialog('Remove 2 Clean');
    const ws = id => s.ctl('state').workspaces.find(w => w.id === s.ctl('state').tasks.find(t => t.id === id).workspace_id);
    const branches = git(repo, 'branch', '--list');
    const cleaned = { a: !!ws(a.task.id).removed_ms, b: !!ws(b.task.id).removed_ms, c: !!ws(c.task.id).removed_ms, cFile: fs.existsSync(path.join(c.workspace.path, 'uncommitted.txt')),
      branchesKept: [a, b].every(t => branches.includes(t.workspace.branch)) };
    await s.screenshot('cleanup');
    check('bulk cleanup removes only the clean archived worktrees; uncommitted work is kept unless confirmed; branches stay', cleaned.a && cleaned.b && !cleaned.c && cleaned.cFile && cleaned.branchesKept && /uncommitted work/i.test(text), { cleaned, dialog: text.slice(0, 400) });
    // Confirming discards it (explicit second confirmation).
    await cdp.command('Overseer: Clean Up Archived Worktrees');
    await dialog('Remove All');
    await dialog('Discard and Remove');
    check('discarding uncommitted work needs an explicit second confirmation, then the worktree is removed (branch kept)', !!ws(c.task.id).removed_ms && git(repo, 'branch', '--list').includes(c.workspace.branch), { removed: !!ws(c.task.id).removed_ms });

    // Automatic archiving after the chosen age (here ~2 seconds), applied after a reload.
    const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['overseer.history.autoArchiveDays'] = 0.00002;
    fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2));
    await delay(3000);
    await cdp.command('Developer: Reload Window'); await delay(8000);
    cdp = await s.connect(); s.cdp = cdp;
    let archivedCount = 0;
    for (let i = 0; i < 40; i++) { archivedCount = s.ctl('state').tasks.filter(t => t.archived_ms).length; if (archivedCount >= 300) break; await delay(500); }
    const dash2 = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail')`, 30000);
    const shown = await dash2.eval(`document.querySelectorAll('.rail-list .row[data-run]').length`);
    check('finished runs older than overseer.history.autoArchiveDays are archived automatically; the rail shows active and recent runs only', archivedCount >= 300 && shown === 0, { archivedCount, shown });
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
