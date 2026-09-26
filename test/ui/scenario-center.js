// Packaged-UI scenario for AC-48 (Overseer view / command center), no paid tokens: runs in
// two repositories that are NOT open in the window (Claude fixture with a native child and
// grandchild, generic runs with edits, one still active), the native sidebar closed, the
// Overseer view opened as three columns (agents | review | conversation), expand/collapse at
// three levels, switching runs across repositories, keyboard navigation, restore after a
// window reload, and narrow and wide windows.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

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
    for (const t of [nested, editX]) for (let i = 0; i < 40 && runState(t.run.id).status !== 'completed'; i++) await delay(300);

    // The native sidebar is closed; the Overseer view does not need it.
    const sidebarIsHidden = () => cdp.evalWorkbench(`(() => { const p = document.querySelector('.part.sidebar'); return !p || p.offsetWidth === 0 || getComputedStyle(p).display === 'none' || p.classList.contains('hidden'); })()`);
    for (let i = 0; i < 2 && !(await sidebarIsHidden()); i++) { await cdp.focusWorkbench(); await cdp.key('b', { meta: true }); await delay(800); }
    const sidebarHidden = await sidebarIsHidden();
    await cdp.command('Overseer: Open Overseer View');
    let view = await cdp.webview(`document.body.dataset.ready === '1' && !!document.getElementById('tree')`, 30000);
    const repos = await view.eval(`[...document.querySelectorAll('.row.repo .label')].map(e => e.textContent)`);
    check('Overseer view works with the native sidebar closed and lists repositories not open in the window', sidebarHidden && repos.includes('repo-x') && repos.includes('repo-y') && !repos.includes('open-folder'), { sidebarHidden, repos });
    const groups = await cdp.evalWorkbench(`document.querySelectorAll('.editor-group-container').length`);
    check('opens as three columns (agents | review | conversation)', groups >= 3, groups);
    await s.screenshot('center-wide');

    // Expand/collapse at three levels: task → run → native child → grandchild.
    const visible = () => view.eval(`[...document.querySelectorAll('#tree .row')].map(r => ({ id: r.dataset.id, level: Number(r.dataset.level), label: r.querySelector('.label').textContent, expanded: r.getAttribute('aria-expanded') }))`);
    const clickRow = async (label, part = '.label') => {
      await view.eval(`(() => { const r = [...document.querySelectorAll('#tree .row')].find(r => r.querySelector('.label').textContent === ${JSON.stringify(label)}); document.querySelectorAll('#target').forEach(e => e.removeAttribute('id')); r.querySelector(${JSON.stringify(part)}).id = 'target'; })()`);
      const p = await s.webviewPoint(view, '#target'); await cdp.click(p.x, p.y); await delay(700);
    };
    let rows = await visible();
    const levels = rows.filter(r => r.label === 'grandchild task' || r.label === 'child task' || r.label === 'X nested agents').map(r => [r.label, r.level]);
    check('agents column shows task → run → native child → grandchild', rows.some(r => r.label === 'grandchild task' && r.level === 5) && rows.some(r => r.label === 'child task' && r.level === 4), levels);
    await clickRow('child task', '.twisty');
    rows = await visible();
    const childCollapsed = !rows.some(r => r.label === 'grandchild task') && rows.find(r => r.label === 'child task').expanded === 'false';
    await clickRow('child task', '.twisty');
    const childExpanded = (await visible()).some(r => r.label === 'grandchild task');
    await clickRow('X nested agents', '.twisty');
    rows = await visible();
    const taskCollapsed = !rows.some(r => r.label === 'child task');
    await clickRow('X nested agents', '.twisty');
    await clickRow('repo-y', '.twisty');
    const repoCollapsed = !(await visible()).some(r => r.label === 'Y live');
    await clickRow('repo-y', '.twisty');
    check('expand/collapse at repository, task and native-child levels', childCollapsed && childExpanded && taskCollapsed && repoCollapsed, { childCollapsed, childExpanded, taskCollapsed, repoCollapsed });

    // Selecting runs across repositories switches the review and the conversation.
    const runRow = title => `(() => { const rows = [...document.querySelectorAll('#tree .row')]; const i = rows.findIndex(r => r.querySelector('.label').textContent === ${JSON.stringify(title)}); const r = rows[i + 1]; if (!r?.dataset.run) return null; document.querySelectorAll('#target').forEach(e => e.removeAttribute('id')); r.querySelector('.label').id = 'target'; return r.dataset.run; })()`;
    const selectIn = async title => { const id = await view.eval(runRow(title)); const p = await s.webviewPoint(view, '#target'); await cdp.click(p.x, p.y); await delay(2500); return id; };
    const activeTabs = () => cdp.evalWorkbench(`[...document.querySelectorAll('.editor-group-container')].map(g => g.querySelector('.tab.active')?.getAttribute('aria-label') || '')`);
    await selectIn('X edits');
    const tabsX = await activeTabs();
    const reviewX = await cdp.webview(`document.getElementById('workspace-note')?.textContent.includes(${JSON.stringify(editX.workspace.path)}) && document.querySelectorAll('.diff-file').length > 0`, 20000).then(() => true, () => false);
    await s.screenshot('selected-x');
    await selectIn('Y live');
    const tabsY = await activeTabs();
    const reviewY = await cdp.webview(`document.getElementById('workspace-note')?.textContent.includes(${JSON.stringify(busy.workspace.path)}) && document.querySelectorAll('.diff-file').length > 0`, 20000).then(() => true, () => false);
    const convY = await cdp.webview(`document.body.dataset.runId === ${JSON.stringify(busy.run.id)} && !!document.querySelector('#conv .turn')`, 20000).then(() => true, () => false);
    check('selecting a run in another repository switches the review column and the conversation column', /Review: X edits/.test(tabsX[1] || '') && /X edits/.test(tabsX[2] || '') && /Review: Y live/.test(tabsY[1] || '') && /Y live/.test(tabsY[2] || '') && reviewX && reviewY && convY, { tabsX, tabsY, reviewX, reviewY, convY });
    await s.screenshot('selected-y');
    const status = await view.eval(`[...document.querySelectorAll('#tree .row')].find(r => r.dataset.run === ${JSON.stringify(busy.run.id)})?.querySelector('.dot').className`);
    check('live status shown in the agents column', /status-running/.test(status || ''), status);

    // Keyboard: a click puts focus in the tree; arrows move through labelled treeitems.
    const current = () => view.eval(`({ label: document.activeElement.querySelector('.label')?.textContent, role: document.activeElement.getAttribute('role'), aria: document.activeElement.getAttribute('aria-label'), level: document.activeElement.getAttribute('aria-level') })`);
    const start = await current();
    await cdp.key('ArrowUp'); await delay(200);
    const up = await current();
    await cdp.key('ArrowLeft'); await delay(200); // collapses the task
    const collapsedTask = await view.eval(`document.activeElement.getAttribute('aria-expanded')`);
    await cdp.key('ArrowLeft'); await delay(200); // moves to its repository
    const parent = await current();
    await cdp.key('ArrowDown'); await delay(200);
    await cdp.key('ArrowRight'); await delay(200); // expands the task again
    const reopened = await view.eval(`document.activeElement.getAttribute('aria-expanded')`);
    check('keyboard navigation: arrows move between labelled treeitems, Left collapses then goes to the parent, Right expands',
      start.role === 'treeitem' && up.label !== start.label && Number(up.level) < Number(start.level) && collapsedTask === 'false' && Number(parent.level) === 1 && reopened === 'true' && !!up.aria, { start, up, collapsedTask, parent, reopened });

    // Restore after a window reload.
    await cdp.command('Developer: Reload Window');
    await delay(6000);
    cdp = await s.connect(); s.cdp = cdp;
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'after reload');
    view = await cdp.webview(`document.body.dataset.ready === '1' && document.querySelectorAll('#tree .row').length > 3`, 30000).catch(() => null);
    check('Overseer view restored after a window reload', !!view && (await view.eval(`[...document.querySelectorAll('#tree .row[aria-selected="true"] .label')].map(e => e.textContent)`)).length === 1, view && await view.eval(`[...document.querySelectorAll('#tree .row[aria-selected="true"] .label')].map(e => e.textContent)`));

    // Narrow and wide windows.
    for (const [label, width, height] of [['narrow', 1024, 760], ['wide', 1900, 1100]]) {
      await cdp.call('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 0, mobile: false }, cdp.workbench);
      await delay(1500);
      const dims = await cdp.evalWorkbench(`({ w: innerWidth, groups: [...document.querySelectorAll('.editor-group-container')].map(g => Math.round(g.getBoundingClientRect().width)) })`);
      const fits = view && await view.eval(`document.documentElement.scrollWidth <= innerWidth + 1`);
      s.note('window ' + label, dims);
      await s.screenshot('window-' + label);
      check(`${label} window: three columns remain usable and the agents column does not overflow`, dims.w === width && dims.groups.length >= 3 && dims.groups.every(w => w > 150) && fits, { dims, fits });
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
