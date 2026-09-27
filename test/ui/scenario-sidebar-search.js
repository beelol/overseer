// Packaged-UI scenario for AC-69 (search and filter in the side bar), fixture runs only: 300
// generic runs, each printing a unique word, plus a Claude fixture run that edits files, in two
// repositories. Keyboard only (command palette, typing, Enter, Escape): Search Agents filters the
// Agents view by title, prompt, message text, file, repository, account and status; results show
// in the tree in under 200 ms; Clear Search restores the tree and the selected agent.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('sidebar-search');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const fx = name => path.join(repoRoot, 'fixtures/fake-harness', name);
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'hist-repo'), { dirty: false });
    const other = makeRepo(path.join(s.root, 'billing-service'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: fx('claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode', CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const t0 = Date.now();
    for (let i = 0; i < 300; i++) {
      const n = String(i).padStart(3, '0');
      s.ctl('task.create', { repo, harness: 'generic', program: '/bin/echo', args: [`needle-${n} output line`], prompt: `prompt word pw${n}`, title: `History task ${n}`, workspace_mode: 'worktree' });
    }
    for (let i = 0; i < 120 && s.ctl('state').runs.some(r => ['queued', 'starting', 'running'].includes(r.status)); i++) await delay(500);
    fs.writeFileSync(modeFile, 'showcase');
    const claude = s.ctl('task.create', { repo: other, harness: 'claude', profile_id: 'system-claude', title: 'Refresh sessions once', prompt: 'Make sessions refresh once.' });
    for (let i = 0; i < 60 && s.ctl('state').runs.find(r => r.id === claude.run.id).status !== 'completed'; i++) await delay(300);
    const failed = s.ctl('task.create', { repo: other, harness: 'generic', program: '/bin/sh', args: ['-c', 'exit 3'], prompt: '', title: 'Broken migration' });
    for (let i = 0; i < 30 && s.ctl('state').runs.find(r => r.id === failed.run.id).status !== 'failed'; i++) await delay(300);
    s.note('created runs', { count: s.ctl('state').runs.length, ms: Date.now() - t0 });

    await cdp.command('View: Show Overseer'); await delay(2000);
    // Select one agent first: the selection must survive search and clear.
    await cdp.command('Overseer: Switch Agent…'); await cdp.waitQuickTitle('Switch to agent');
    await cdp.type('Refresh sessions once'); await delay(400); await cdp.key('Enter'); await delay(2500);
    const labels = () => cdp.evalWorkbench(`(() => { const pane = [...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || ''));
      return [...pane.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.getAttribute('aria-level') === '2').map(r => r.querySelector('.label-name')?.textContent.trim()); })()`);
    const selectedLabel = () => cdp.evalWorkbench(`[...document.querySelectorAll('.monaco-list-row.selected')].map(r => r.querySelector('.label-name')?.textContent.trim()).filter(Boolean).pop()`);
    const before = await labels(); const selBefore = await selectedLabel();
    const message = () => cdp.evalWorkbench(`[...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || ''))?.querySelector('.pane-header .description')?.textContent.trim() || ''`);

    // Search: open with the command palette, type, measure until the tree shows the match.
    const searchFor = async (q, expect, exact) => {
      await cdp.command('Overseer: Search Agents'); await cdp.waitQuickTitle('Search agents');
      await cdp.key('a', { meta: true }); await cdp.key('Backspace');
      // Start the clock when the text is in the field (no helper pause), stop when the tree shows the match.
      await cdp.call('Input.insertText', { text: q }, cdp.workbench);
      const start = Date.now();
      let got = [];
      // Done when the list is exactly this query's matches (not rows left from the previous search).
      const want = [...exact].sort().join('|');
      for (let i = 0; i < 400; i++) { got = await labels(); if ([...got].sort().join('|') === want) break; await delay(5); }
      const ms = Date.now() - start;
      await cdp.key('Enter'); await delay(200);
      return { q, ms, labels: got.slice(0, 5), count: got.length, message: await message() };
    };
    // Each query's exact result differs from the one before, so every timing is its own update.
    const cases = [
      ['by title', 'History task 123', 'History task 123', ['History task 123']],
      ['by prompt', 'pw201', 'History task 201', ['History task 201']],
      ['by message text (agent output)', 'needle-237', 'History task 237', ['History task 237']],
      ['by file the agent edited', 'coordinator', 'Refresh sessions once', ['Refresh sessions once']],
      ['by repository', 'billing-service', 'Broken migration', ['Broken migration', 'Refresh sessions once']],
      ['by account', 'existing login', 'Refresh sessions once', ['Refresh sessions once']],
      ['by status', 'failed', 'Broken migration', ['Broken migration']],
    ];
    const out = [];
    for (const [label, q, expect, exact] of cases) out.push({ label, expect, ...(await searchFor(q, expect, exact)) });
    s.note('searches', out);
    await s.screenshot('search-results');
    for (const o of out) check(`search ${o.label} ("${o.q}") shows ${o.expect} in the Agents view in under 200 ms`, o.labels.includes(o.expect) && o.count <= 2 && o.ms < 200, o);
    const narrowed = out.find(o => o.label === 'by message text (agent output)');
    check('the filtered tree says what it shows ("N matches for …") and hides the rest', narrowed.count < 10 && /match/.test(narrowed.message), { count: narrowed.count, message: narrowed.message });

    // Clear from the keyboard: the full tree and the selection come back.
    await cdp.command('Overseer: Clear Search'); await delay(1200);
    const after = await labels(); const selAfter = await selectedLabel();
    check('Clear Search restores the tree and the selected agent', after.length === before.length && selAfter === selBefore && !(await message()), { before: before.length, after: after.length, selBefore, selAfter });
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
