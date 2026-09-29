// Packaged-UI scenario for AC-254, AC-255 and AC-256 (generic fixture programs, no paid tokens),
// across two repositories, site and notes:
//   AC-256  a repository whose last working agent finishes unreviewed keeps a nonzero badge before
//           and after; it reaches nothing only once every agent in it is reviewed or archived.
//   AC-254  six agents finish with none reviewed: each carries the "to review" mark (✦, not the
//           reviewed ✓), with a count per repository and in the Agents view's header; opening one
//           agent's review clears only its own mark. Screenshots at 6 unreviewed and at 3.
//   AC-255  with 11 agents at their end and none working, the rollup reads nonzero counts in the
//           side bar while the grid shows no tile (the grid command goes home, and its note gives the
//           same counts); with one agent pinned, the grid's header shows the same rollup as the side bar.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('review-marks');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const site = makeRepo(path.join(s.root, 'site'), { dirty: false });
    const notes = makeRepo(path.join(s.root, 'notes'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'overseer.followNewRuns': false });
    s.install(latestVsix());
    s.launch(site, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const run = id => s.ctl('state').runs.find(r => r.id === id);
    const waitStatus = async (id, re, ms = 30000) => { for (let t = 0; t < ms; t += 250) { if (re.test(run(id)?.status || '')) return run(id).status; await delay(250); } return run(id)?.status; };
    // An agent that edits a file in its worktree and finishes (after `wait` seconds).
    const agent = (repo, title, wait = 0) => s.ctl('task.create', { repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sh', args: ['-c', `sleep ${wait}; echo "${title}" >> a.txt`], prompt: '', title }).run.id;

    await cdp.command('View: Show Overseer'); await delay(2000);
    const rows = () => cdp.evalWorkbench(`(() => {
      const pane = [...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || ''));
      if (!pane) return { header: '', rows: [] };
      const list = [...pane.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).map(r => ({ label: r.querySelector('.label-name')?.textContent.trim(), description: r.querySelector('.label-description')?.textContent.trim() || '', level: Number(r.getAttribute('aria-level')), aria: r.getAttribute('aria-label'),
        badge: (() => { for (const e of [r.querySelector('.monaco-icon-label'), r.querySelector('.label-name'), r.querySelector('.monaco-icon-label-container')]) { const c = e && getComputedStyle(e, '::after').content; if (c && c !== 'none' && c !== 'normal') return c.replace(/^"|"$/g, ''); } return ''; })() }));
      return { header: pane.querySelector('.pane-header')?.innerText.replace(/\\s+/g, ' ').trim() || '', rows: list };
    })()`);
    const repoRow = (v, name) => v.rows.find(r => r.level === 1 && r.label === name);
    const agentRow = (v, title) => v.rows.filter(r => r.label === title).pop();
    const settle = async pred => { let v; for (let i = 0; i < 40; i++) { v = await rows(); if (pred(v)) return v; await delay(250); } return v; };

    // ---------- AC-256: the last working agent of notes finishes, unreviewed.
    const first = agent(notes, 'Notes index', 4);
    await waitStatus(first, /running/);
    let v = await settle(x => /1/.test(repoRow(x, 'notes')?.description || ''));
    const before = repoRow(v, 'notes')?.description;
    await waitStatus(first, /completed/);
    v = await settle(x => /to review/.test(repoRow(x, 'notes')?.description || ''));
    const after = repoRow(v, 'notes')?.description;
    check('a repository whose last working agent finishes unreviewed shows a nonzero badge before and after', /^1$/.test(before || '') && /^1 to review$/.test(after || ''), { before, after });

    // ---------- AC-254: six agents finish across the two repositories, none reviewed.
    const six = [first, agent(notes, 'Notes search'), agent(notes, 'Notes export'), agent(site, 'Site header'), agent(site, 'Site footer'), agent(site, 'Site pricing')];
    for (const id of six) await waitStatus(id, /completed/);
    v = await settle(x => /6 to review/.test(x.header) && x.rows.filter(r => r.badge === '✦').length === 6);
    const titles = ['Notes index', 'Notes search', 'Notes export', 'Site header', 'Site footer', 'Site pricing'];
    const marks = titles.map(t => ({ t, badge: agentRow(v, t)?.badge, description: agentRow(v, t)?.description }));
    check('six finished agents, none reviewed: each carries the "to review" mark (✦) and says so', marks.every(m => m.badge === '✦' && /to review/.test(m.description)), marks);
    check('the count per repository and in the Agents view\'s header: 3 and 3, 6 overall', repoRow(v, 'site')?.description === '3 to review' && repoRow(v, 'notes')?.description === '3 to review' && /6 to review/.test(v.header), { site: repoRow(v, 'site')?.description, notes: repoRow(v, 'notes')?.description, header: v.header });
    const rollupRow = x => x.rows.find(r => r.level === 1 && /\d+ (working|needs you|to review|reviewed|failed)/.test(r.label || ''));
    check('the side bar\'s rollup row (AC-255) reads the same 6 to review', rollupRow(v)?.label === '6 to review', rollupRow(v));
    await s.screenshot('six-to-review');

    // Opening one agent's review clears only its own mark.
    await cdp.command('Overseer: Open Overseer View'); await delay(1500);
    await s.selectAgent('Site header', { settle: 2500 });
    const reviewOpen = await cdp.evalWorkbench(`[...document.querySelectorAll('.tab')].some(t => /Review/.test(t.getAttribute('aria-label') || t.textContent))`);
    v = await settle(x => /5 to review/.test(x.header));
    const cleared = { own: agentRow(v, 'Site header')?.badge, others: titles.filter(t => t !== 'Site header').map(t => agentRow(v, t)?.badge), site: repoRow(v, 'site')?.description, notes: repoRow(v, 'notes')?.description, header: v.header, reviewOpen };
    check('opening one agent\'s review clears only its own mark: ✓ for it, ✦ for the other five, site 2 and notes 3', reviewOpen && cleared.own === '✓' && cleared.others.every(b => b === '✦') && cleared.site === '2 to review' && cleared.notes === '3 to review' && /5 to review/.test(cleared.header), cleared);
    for (const t of ['Site footer', 'Notes index']) await s.selectAgent(t, { settle: 2500 });
    v = await settle(x => /3 to review/.test(x.header));
    check('at three reviewed, three still carry the mark', v.rows.filter(r => r.badge === '✦').length === 3 && /3 to review/.test(v.header), { header: v.header, marked: v.rows.filter(r => r.badge === '✦').map(r => r.label) });
    await s.screenshot('three-to-review');

    // AC-256: the badge reaches nothing only once every agent in the repository is reviewed or archived.
    await s.selectAgent('Site pricing', { settle: 2500 });
    v = await settle(x => !repoRow(x, 'site')?.description);
    check('site\'s badge is empty once every agent in it is reviewed', repoRow(v, 'site') && repoRow(v, 'site').description === '', repoRow(v, 'site'));
    const notesTasks = s.ctl('state').tasks.filter(t => t.repo_root === notes && ['Notes search', 'Notes export'].includes(t.title));
    s.ctl('task.archive', { task_id: notesTasks[0].id, archived: true });
    v = await settle(x => repoRow(x, 'notes')?.description === '1 to review');
    const oneLeft = repoRow(v, 'notes')?.description;
    s.ctl('task.archive', { task_id: notesTasks[1].id, archived: true });
    v = await settle(x => !repoRow(x, 'notes')?.description);
    check('notes\'s badge counts down as its agents are archived and is empty when none is left to review', oneLeft === '1 to review' && repoRow(v, 'notes')?.description === '', { oneLeft, after: repoRow(v, 'notes')?.description });

    // ---------- AC-255: 11 agents at their end, none working: the rollup still counts them.
    const more = ['Site blog', 'Site search', 'Notes tags', 'Notes sync', 'Site docs', 'Notes print', 'Site legal'].map((t, i) => agent(i % 2 ? notes : site, t));
    for (const id of more) await waitStatus(id, /completed/);
    const failedId = s.ctl('task.create', { repo: notes, harness: 'generic', program: '/bin/sh', args: ['-c', 'exit 3'], prompt: '', title: 'Notes backup' }).run.id;
    await waitStatus(failedId, /failed/);
    v = await settle(x => /7 to review/.test(rollupRow(x)?.label || '') && /1 failed/.test(rollupRow(x)?.label || ''));
    const sideRollup = rollupRow(v)?.label;
    const done = s.ctl('state').runs.filter(r => !r.parent_run_id && ['completed', 'failed'].includes(r.status)).length;
    check('with 12 agents at their end (11 finished, 1 failed) and none working, the side bar\'s rollup reads nonzero counts', done >= 12 && sideRollup === '7 to review · 4 reviewed · 1 failed', { sideRollup, done });
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(2000);
    const dash = await s.editorView();
    const home = await dash.eval(`({ mode: document.body.dataset.mode, tiles: document.querySelectorAll('.grid .tile').length, note: document.querySelector('.view-composer .composer-note')?.innerText || '' })`);
    check('the grid\'s tile view shows none (the grid command goes home), and its note gives the same counts', home.mode === 'composer' && home.tiles === 0 && home.note.includes('7 to review · 4 reviewed · 1 failed'), home);
    await s.screenshot('grid-has-nothing-rollup');
    const pinned = more[0];
    // Pinned as the tile's pin does (the host's pin message), without opening the agent.
    await dash.eval(`window.overseerApi.postMessage({ type: 'pin', runId: ${JSON.stringify(pinned)}, on: true })`); await delay(1200);
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(2500);
    const grid = await dash.waitFor(`document.body.dataset.mode === 'grid' && !!document.getElementById('grid-rollup')?.dataset.text`, 15000).then(() => dash.eval(`({ text: document.getElementById('grid-rollup').dataset.text, tiles: document.querySelectorAll('.grid .tile').length })`), () => null);
    v = await rows();
    check('with one agent pinned, the grid\'s header shows the same rollup as the side bar', !!grid && grid.text === rollupRow(v)?.label, { grid, side: rollupRow(v)?.label });
    await s.screenshot('grid-header-rollup');
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
