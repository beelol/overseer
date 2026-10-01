// Packaged-UI scenario for AC-48 (Overseer view / command center) in the Gate K layout, no paid
// tokens: runs in two repositories that are NOT open in the window (Claude fixture with a native
// child and grandchild, generic runs with edits, one still active). The agents column is the
// Overseer side bar (AC-67 replaced the dashboard rail); expand/collapse at three levels, switching
// runs across repositories (also with the side bar closed, through Switch Agent), keyboard
// navigation in the tree, restore after a window reload, and narrow and wide windows.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, until, repoRoot } = require('./harness');

(async () => {
  const s = new Session('center');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  let busy;
  try {
    const opened = makeRepo(path.join(s.root, 'open-folder'), { dirty: false });
    const repoX = makeRepo(path.join(s.root, 'repo-x'), { dirty: false });
    const repoY = makeRepo(path.join(s.root, 'repo-y'), { dirty: false });
    s.settings();
    s.install(latestVsix());
    s.launch(opened, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE', CLAUDE_FIXTURE_MODE: 'nested' });
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const sh = (repo, title, script) => s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', script], prompt: '', title });
    const nested = s.ctl('task.create', { repo: repoX, harness: 'claude', prompt: 'delegate', title: 'X nested agents' });
    const editX = sh(repoX, 'X edits', `sed -i '' 's/^L3: original$/L3: edited in X/' a.txt`);
    busy = sh(repoY, 'Y live', `sed -i '' 's/^L7: original$/L7: edited in Y/' b.txt; i=0; while [ $i -lt 120 ]; do echo tick $i; sleep 1; i=$((i+1)); done`);
    const runState = id => s.ctl('state').runs.find(r => r.id === id);
    for (const t of [nested, editX]) await until(() => runState(t.run.id).status, st => st === 'completed', 60000, 300);

    // Gate K: the agents column is the Overseer side bar (it replaces the dashboard's rail, AC-67);
    // Explorer is not needed and the window's folder does not limit the list.
    // The list once it shows both repositories and the nested agents down to the grandchild.
    await s.openOverseerView();
    let rows = await until(() => s.agentRows(), rs => ['repo-x', 'repo-y', 'X nested agents', 'child task', 'grandchild task'].every(l => rs.some(r => r.label === l)), 30000, 200);
    const repos = rows.filter(r => r.level === 1).map(r => r.label);
    const explorerShown = await cdp.evalWorkbench(`[...document.querySelectorAll('.part.sidebar .pane-header')].some(h => h.offsetParent && /Folders|open-folder/i.test(h.textContent))`);
    check('the Overseer side bar replaces Explorer and lists repositories not open in the window', !explorerShown && repos.includes('repo-x') && repos.includes('repo-y') && !repos.includes('open-folder'), { explorerShown, repos });
    await s.screenshot('center-wide');

    // Expand/collapse at three levels: repository → task (its run) → native child → grandchild.
    const levels = rows.filter(r => ['grandchild task', 'child task', 'X nested agents', 'repo-x'].includes(r.label)).map(r => [r.label, r.level]);
    const at = (label, rs = rows) => rs.filter(r => r.label === label).pop();
    check('agents list shows repository → task → native child → grandchild', at('repo-x')?.level === 1 && at('X nested agents')?.level === 2 && at('child task')?.level === 3 && at('grandchild task')?.level === 4, levels);
    // Each click waits for the list to redraw with what it should show (15 s; the check then reads what was there).
    const twisty = async (label, ok) => { await s.clickAgentRow(label, { twisty: true, settle: 0 }); return until(() => s.agentRows(), ok, 15000, 100); };
    rows = await twisty('child task', rs => !rs.some(r => r.label === 'grandchild task') && at('child task', rs)?.expanded === 'false');
    const childCollapsed = !rows.some(r => r.label === 'grandchild task') && at('child task', rows).expanded === 'false';
    const childExpanded = (await twisty('child task', rs => rs.some(r => r.label === 'grandchild task'))).some(r => r.label === 'grandchild task');
    const taskCollapsed = !(await twisty('X nested agents', rs => !rs.some(r => r.label === 'child task'))).some(r => r.label === 'child task');
    await twisty('X nested agents', rs => rs.some(r => r.label === 'child task'));
    const repoCollapsed = !(await twisty('repo-y', rs => !rs.some(r => r.label === 'Y live'))).some(r => r.label === 'Y live');
    await twisty('repo-y', rs => rs.some(r => r.label === 'Y live'));
    check('expand/collapse at repository, task and native-child levels', childCollapsed && childExpanded && taskCollapsed && repoCollapsed, { childCollapsed, childExpanded, taskCollapsed, repoCollapsed });

    // Selecting runs across repositories switches the review and the conversation.
    const tabs = () => cdp.evalWorkbench(`[...document.querySelectorAll('.editor-group-container')].filter(g => g.offsetParent).map(g => g.querySelector('.tab.active')?.getAttribute('aria-label') || '')`);
    const reviewOf = id => cdp.webview(`!!document.getElementById('diffs') && document.body.dataset.runId === ${JSON.stringify(id)} && document.querySelectorAll('#tree .file').length > 0`, 20000).then(() => true, () => false);
    const chatOf = title => cdp.webview(`document.getElementById('title')?.textContent === ${JSON.stringify(title)} && !!document.querySelector('#conv .turn')`, 20000).then(() => true, () => false);
    // The review's tab says "Review" until its first snapshot has loaded, then names the agent:
    // read the tabs once the review and chat have loaded, waiting for that name.
    const tabsFor = title => until(tabs, t => t.some(x => new RegExp(`^Review.*${title}`).test(x)), 20000);
    await s.selectAgent('X edits', { settle: 2500 });
    const reviewX = await reviewOf(editX.run.id); const chatX = await chatOf('X edits'); const tabsX = await tabsFor('X edits');
    await s.screenshot('selected-x');
    await s.selectAgent('Y live', { settle: 2500 });
    const reviewY = await reviewOf(busy.run.id); const chatY = await chatOf('Y live'); const tabsY = await tabsFor('Y live');
    check('selecting a run in another repository switches the review and the conversation', tabsX.some(t => /^Review.*X edits/.test(t)) && tabsY.some(t => /^Review.*Y live/.test(t)) && reviewX && reviewY && chatX && chatY, { tabsX, tabsY, reviewX, reviewY, chatX, chatY });
    await s.screenshot('selected-y');
    rows = await s.agentRows();
    check('live status shown in the agents list', /●/.test(at('Y live', rows)?.aria || '') || /running|working/i.test(at('Y live', rows)?.aria || ''), at('Y live', rows));

    // With the side bar closed too, Switch Agent still moves between repositories.
    await cdp.command('View: Close Primary Side Bar'); await delay(800);
    await cdp.command('Overseer: Switch Agent…'); await cdp.waitQuickTitle('Switch to agent'); await cdp.type('X edits'); await delay(300); await cdp.key('Enter'); await delay(2500);
    const closedX = await reviewOf(editX.run.id) && await chatOf('X edits');
    await s.screenshot('sidebar-closed');
    check('with the side bar closed, Switch Agent switches the review and the conversation across repositories', closedX);
    await s.selectAgent('Y live', { settle: 2500 });

    // Keyboard: arrows move through labelled treeitems; Left collapses, then goes to the parent; Right expands.
    // Each key waits for the focus (or the expansion) to move before the next: a loaded machine
    // redraws the list later, and a key sent before it has would act on the old row.
    const current = async () => (await s.agentRows()).find(r => r.focused) || {};
    const after = async (key, ok) => { await cdp.key(key); return until(current, ok, 15000, 100); };
    await s.clickAgentRow('X nested agents', { settle: 500 });
    const start = await until(current, r => r.label === 'X nested agents', 15000, 100);
    const down = await after('ArrowDown', r => r.label !== 'X nested agents');
    await after('ArrowUp', r => r.label === 'X nested agents');
    const collapsedTask = (await after('ArrowLeft', r => r.expanded === 'false')).expanded; // collapses the task
    const reopened = (await after('ArrowRight', r => r.expanded === 'true')).expanded; // expands it again
    await after('ArrowLeft', r => r.expanded === 'false');
    const parent = await after('ArrowLeft', r => r.label !== 'X nested agents'); // collapsed: moves to its repository
    await after('ArrowRight', r => r.expanded === 'true');
    check('keyboard navigation: arrows move between labelled treeitems, Left collapses then goes to the parent, Right expands',
      start.label === 'X nested agents' && down.label === 'child task' && collapsedTask === 'false' && reopened === 'true' && parent.level === 1 && parent.label === 'repo-x' && !!start.aria, { start, down, collapsedTask, parent, reopened });
    await s.selectAgent('Y live', { settle: 2500 });

    // Restore after a window reload.
    await cdp.command('Developer: Reload Window');
    await delay(6000);
    cdp = await s.connect(); s.cdp = cdp;
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'after reload');
    const restoredChat = await chatOf('Y live');
    const restoredReview = await reviewOf(busy.run.id);
    await s.openOverseerView();
    const selectedRows = (await until(() => s.agentRows(), rs => rs.some(r => r.selected), 20000, 200)).filter(r => r.selected).map(r => r.label);
    check('Overseer restored after a window reload (same agent, its chat and review)', restoredChat && restoredReview && selectedRows.every(t => t === 'Y live'), { restoredChat, restoredReview, selectedRows });
    const view = await cdp.webview(`document.getElementById('title')?.textContent === 'Y live'`, 20000);

    // Narrow and wide windows.
    for (const [label, width, height] of [['narrow', 1024, 760], ['wide', 1900, 1100]]) {
      await cdp.call('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 0, mobile: false }, cdp.workbench);
      // Once the window has laid itself out at the new size.
      const dims = await until(() => cdp.evalWorkbench(`({ w: innerWidth, groups: [...document.querySelectorAll('.editor-group-container')].map(g => Math.round(g.getBoundingClientRect().width)) })`), d => d.w === width && d.groups.length >= 2 && d.groups.every(w => w > 150), 15000, 200);
      const fits = view && await view.eval(`document.documentElement.scrollWidth <= innerWidth + 1`);
      s.note('window ' + label, dims);
      await s.screenshot('window-' + label);
      check(`${label} window: review and chat columns remain usable and the chat does not overflow`, dims.w === width && dims.groups.length >= 2 && dims.groups.every(w => w > 150) && fits, { dims, fits });
    }
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench);
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { if (busy) s.ctl('run.interrupt', { run_id: busy.run.id }); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
