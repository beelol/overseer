// Packaged-UI scenario for AC-48 (Overseer view / command center), no paid tokens: runs in
// two repositories that are NOT open in the window (Claude fixture with a native child and
// grandchild, generic runs with edits, one still active), the native sidebar closed, the
// Overseer view opened as the dashboard (agents rail and conversation | review), expand/collapse
// at three levels, switching runs across repositories, keyboard navigation, restore after a
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
    let view = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail-list .row')`, 30000);
    const repos = await view.eval(`[...document.querySelectorAll('.rail-list .row.repo .title')].map(e => e.textContent)`);
    check('Overseer view works with the native sidebar closed and lists repositories not open in the window', sidebarHidden && repos.includes('repo-x') && repos.includes('repo-y') && !repos.includes('open-folder'), { sidebarHidden, repos });
    const groups = await cdp.evalWorkbench(`document.querySelectorAll('.editor-group-container').length`);
    check('opens as the dashboard (agents rail and conversation) with the review column beside it', groups >= 2 && await view.eval(`!!document.querySelector('.rail') && !!document.querySelector('.main')`), groups);
    await s.screenshot('center-wide');

    // Expand/collapse at three levels: repository → task (its run) → native child → grandchild.
    const visible = () => view.eval(`[...document.querySelectorAll('.rail-list .row')].map(r => ({ id: r.dataset.id, level: Number(r.dataset.level), label: r.querySelector('.title').textContent, expanded: r.getAttribute('aria-expanded') }))`);
    const clickRow = async (label, part = '.title') => {
      await view.eval(`(() => { const r = [...document.querySelectorAll('.rail-list .row')].find(r => r.querySelector('.title').textContent === ${JSON.stringify(label)}); document.querySelectorAll('#target').forEach(e => e.removeAttribute('id')); r.querySelector(${JSON.stringify(part)}).id = 'target'; })()`);
      const p = await s.webviewPoint(view, '#target'); await cdp.click(p.x, p.y); await delay(700);
    };
    let rows = await visible();
    const levels = rows.filter(r => r.label === 'grandchild task' || r.label === 'child task' || r.label === 'X nested agents').map(r => [r.label, r.level]);
    check('agents rail shows repository → task → native child → grandchild', rows.some(r => r.label === 'grandchild task' && r.level === 4) && rows.some(r => r.label === 'child task' && r.level === 3) && rows.some(r => r.label === 'X nested agents' && r.level === 2), levels);
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
    const runRow = title => `(() => { const r = [...document.querySelectorAll('.rail-list .row[data-run]')].find(r => r.querySelector('.title').textContent === ${JSON.stringify(title)}); if (!r) return null; document.querySelectorAll('#target').forEach(e => e.removeAttribute('id')); r.querySelector('.title').id = 'target'; return r.dataset.run; })()`;
    const selectIn = async title => { const id = await view.eval(runRow(title)); const p = await s.webviewPoint(view, '#target'); await cdp.click(p.x, p.y); await delay(2500); return id; };
    const activeTabs = () => cdp.evalWorkbench(`[...document.querySelectorAll('.editor-group-container')].map(g => g.querySelector('.tab.active')?.getAttribute('aria-label') || '')`);
    const chatOf = () => view.eval(`({ title: document.getElementById('title')?.textContent, turns: document.querySelectorAll('#conv .turn').length })`);
    await selectIn('X edits');
    const tabsX = await activeTabs(); const chatX = await chatOf();
    const reviewX = await cdp.webview(`document.getElementById('workspace-note')?.textContent.includes(${JSON.stringify(editX.workspace.path)}) && document.querySelectorAll('.diff-file').length > 0`, 20000).then(() => true, () => false);
    await s.screenshot('selected-x');
    await selectIn('Y live');
    const tabsY = await activeTabs();
    const reviewY = await cdp.webview(`document.getElementById('workspace-note')?.textContent.includes(${JSON.stringify(busy.workspace.path)}) && document.querySelectorAll('.diff-file').length > 0`, 20000).then(() => true, () => false);
    const convY = await view.waitFor(`document.getElementById('title')?.textContent === 'Y live' && !!document.querySelector('#conv .turn')`, 20000).then(() => true, () => false);
    check('selecting a run in another repository switches the review column and the dashboard conversation', /Review: X edits/.test(tabsX[1] || '') && chatX.title === 'X edits' && /Review: Y live/.test(tabsY[1] || '') && reviewX && reviewY && convY, { tabsX, tabsY, chatX, reviewX, reviewY, convY });
    await s.screenshot('selected-y');
    const status = await view.eval(`document.querySelector('.rail-list .row[data-run=${JSON.stringify(busy.run.id)}] .status')?.className`);
    check('live status shown in the agents rail', /st-running/.test(status || ''), status);

    // Keyboard: arrows move through labelled treeitems; Left collapses, then goes to the parent; Right expands.
    const current = () => view.eval(`({ label: document.activeElement.querySelector('.title')?.textContent, role: document.activeElement.getAttribute('role'), aria: document.activeElement.getAttribute('aria-label'), level: document.activeElement.getAttribute('aria-level'), expanded: document.activeElement.getAttribute('aria-expanded') })`);
    await view.eval(`(() => { const r = [...document.querySelectorAll('.rail-list .row')].find(r => r.querySelector('.title').textContent === 'X nested agents'); r.tabIndex = 0; r.focus(); return true; })()`);
    const start = await current();
    await cdp.key('ArrowDown'); await delay(200);
    const down = await current();
    await cdp.key('ArrowUp'); await delay(200);
    await cdp.key('ArrowLeft'); await delay(200); // collapses the task
    const collapsedTask = (await current()).expanded;
    await cdp.key('ArrowRight'); await delay(200); // expands it again
    const reopened = (await current()).expanded;
    await cdp.key('ArrowLeft'); await delay(200);
    await cdp.key('ArrowLeft'); await delay(200); // collapsed: moves to its repository
    const parent = await current();
    check('keyboard navigation: arrows move between labelled treeitems, Left collapses then goes to the parent, Right expands',
      start.role === 'treeitem' && start.label === 'X nested agents' && down.label === 'child task' && collapsedTask === 'false' && Number(parent.level) === 1 && parent.label === 'repo-x' && reopened === 'true' && !!start.aria, { start, down, collapsedTask, parent, reopened });

    // Restore after a window reload.
    await cdp.command('Developer: Reload Window');
    await delay(6000);
    cdp = await s.connect(); s.cdp = cdp;
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'after reload');
    view = await cdp.webview(`document.body.dataset.ready === '1' && document.querySelectorAll('.rail-list .row').length > 3`, 30000).catch(() => null);
    const restored = view && await view.eval(`[...document.querySelectorAll('.rail-list .row[aria-selected="true"] .title')].map(e => e.textContent)`);
    check('Overseer view restored after a window reload (same agent selected)', !!view && restored.length >= 1 && restored.every(t => t === 'Y live'), restored);

    // Narrow and wide windows.
    for (const [label, width, height] of [['narrow', 1024, 760], ['wide', 1900, 1100]]) {
      await cdp.call('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 0, mobile: false }, cdp.workbench);
      await delay(1500);
      const dims = await cdp.evalWorkbench(`({ w: innerWidth, groups: [...document.querySelectorAll('.editor-group-container')].map(g => Math.round(g.getBoundingClientRect().width)) })`);
      const fits = view && await view.eval(`document.documentElement.scrollWidth <= innerWidth + 1`);
      s.note('window ' + label, dims);
      await s.screenshot('window-' + label);
      check(`${label} window: dashboard and review columns remain usable and the dashboard does not overflow`, dims.w === width && dims.groups.length >= 2 && dims.groups.every(w => w > 150) && fits, { dims, fits });
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
