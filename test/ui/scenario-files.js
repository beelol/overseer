// Packaged-UI scenario for AC-51 (no paid tokens; generic-harness runs): the Overseer view's
// Files pane browses the selected run's worktree in repositories that are not open in the
// window, marks changed/added/deleted files and folders containing changes, opens files from
// two different repositories in the editor, and stays responsive in a large repository.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, git } = require('./harness');

(async () => {
  const s = new Session('files');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const opened = makeRepo(path.join(s.root, 'open-folder'), { dirty: false });
    const repoX = makeRepo(path.join(s.root, 'repo-x'), { dirty: false });
    const repoY = makeRepo(path.join(s.root, 'repo-y'), { dirty: false });
    // A large repository: 10,000 tracked files (6,000 in one folder, the rest nested).
    const repoZ = makeRepo(path.join(s.root, 'repo-z'), { dirty: false });
    fs.mkdirSync(path.join(repoZ, 'big'));
    for (let i = 0; i < 6000; i++) fs.writeFileSync(path.join(repoZ, 'big', `f${String(i).padStart(5, '0')}.txt`), `${i}\n`);
    for (let d = 0; d < 40; d++) { const dir = path.join(repoZ, 'pkg', `m${d}`); fs.mkdirSync(dir, { recursive: true }); for (let i = 0; i < 100; i++) fs.writeFileSync(path.join(dir, `x${i}.txt`), 'x\n'); }
    git(repoZ, 'add', '.'); git(repoZ, 'commit', '-qm', 'large');
    s.settings();
    s.install(latestVsix());
    s.launch(opened);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const sh = (repo, title, script) => s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', script], prompt: '', title });
    const x = sh(repoX, 'X files', `sed -i '' 's/^L3: original$/L3: changed in X/' a.txt; mkdir -p sub && printf 'new in X\\n' > sub/new.txt; rm c.txt`);
    const y = sh(repoY, 'Y files', `sed -i '' 's/^L9: original$/L9: changed in Y/' b.txt`);
    const z = sh(repoZ, 'Z large', `printf 'deep change\\n' >> pkg/m39/x99.txt`);
    const runState = id => s.ctl('state').runs.find(r => r.id === id);
    for (const t of [x, y, z]) for (let i = 0; i < 60 && runState(t.run.id).status !== 'completed'; i++) await delay(300);

    await cdp.command('Overseer: Open Overseer View');
    const view = await cdp.webview(`document.body.dataset.ready === '1' && !!document.getElementById('files')`, 30000);
    const pick = async label => {
      const runId = (() => { const st = s.ctl('state'); const t = st.tasks.find(t => t.title === label); return st.runs.find(r => r.task_id === t.id && !r.parent_run_id).id; })();
      await s.selectAgent(label);
      await view.waitFor(`document.getElementById('title')?.textContent === ${JSON.stringify(label)}`, 20000);
      if (await view.eval(`document.querySelector('.files-panel').hidden`)) await view.eval(`document.getElementById('files-toggle').click()`);
      await view.waitFor(`document.getElementById('files').dataset.run === ${JSON.stringify(runId)} && document.body.dataset.filesReady === '1'`, 20000);
      // An agent with changes also brings its review forward (Gate K: review left, chat right); let that settle first.
      await cdp.waitFor(`[...document.querySelectorAll('.editor-group-container .tab.active')].some(t => (t.getAttribute('aria-label') || '').startsWith(${JSON.stringify('Review: ' + label)}))`, 20000, 'review for ' + label);
      await delay(500);
    };
    const entries = () => view.eval(`[...document.querySelectorAll('#files .row[role=treeitem]')].map(r => ({ path: r.dataset.path, level: Number(r.dataset.level), status: r.querySelector('.fstatus')?.textContent || '', inside: r.querySelector('.fcount')?.textContent || '', deleted: r.classList.contains('deleted'), dir: !!r.dataset.dir }))`);
    const clickEntry = async p => {
      await view.eval(`(() => { const r = [...document.querySelectorAll('#files .row[role=treeitem]')].find(r => r.dataset.path === ${JSON.stringify(p)}); document.querySelectorAll('#ftarget').forEach(e => e.removeAttribute('id')); r.querySelector('.label').id = 'ftarget'; r.scrollIntoView({ block: 'center' }); })()`);
      const pt = await s.webviewPoint(view, '#ftarget');
      await cdp.click(pt.x, pt.y); await delay(900);
    };
    // The tab label has no path; the editor's breadcrumbs show the folder chain of the open file.
    const activeTab = () => cdp.evalWorkbench(`(() => { const g = document.querySelector('.editor-group-container.active'); const groups = document.querySelectorAll('.editor-group-container').length; const t = g?.querySelector('.tab.active'); const crumbs = [...(g?.querySelectorAll('.breadcrumbs-control .monaco-breadcrumb-item') || [])].map(i => i.textContent.trim()).join('/'); return t ? { label: t.getAttribute('aria-label'), path: crumbs, groups } : null; })()`);

    // Repository X.
    await pick('X files');
    let ex = await entries();
    const get = (list, p) => list.find(e => e.path === p) || {};
    check('Files pane lists the selected run\'s worktree (repository not open in the window) with changes marked',
      get(ex, 'sub').inside === '1' && get(ex, 'a.txt').status === 'M' && get(ex, 'c.txt').status === 'D' && get(ex, 'c.txt').deleted && get(ex, 'b.txt').status === '' && !ex.some(e => e.path === '.git'),
      ex.map(e => `${e.path}${e.status ? ' ' + e.status : ''}${e.inside ? ' (' + e.inside + ')' : ''}`));
    await clickEntry('sub');
    await view.waitFor(`[...document.querySelectorAll('#files .row')].some(r => r.dataset.path === 'sub/new.txt')`, 10000);
    ex = await entries();
    check('folders expand lazily and show their changed files', get(ex, 'sub/new.txt').status === 'A' && get(ex, 'sub/new.txt').level === 2, get(ex, 'sub/new.txt'));
    await clickEntry('a.txt');
    const tabX = await activeTab();
    check('clicking a file opens it in the editor from the X worktree (in the review\'s group, no third column)', /a\.txt/.test(tabX?.label || '') && (tabX?.path || '').includes('repo-x') && tabX.groups <= 2, tabX);
    await s.screenshot('files-x');

    // Repository Y.
    await pick('Y files');
    const ey = await entries();
    // Injected (CDP) clicks right after focus moves between webviews are sometimes not delivered to
    // the webview's document (observed: no click event at all); the retry covers that test-input quirk.
    let tabY;
    for (let i = 0; i < 3; i++) { await clickEntry('b.txt'); tabY = await activeTab(); if (/b\.txt/.test(tabY?.label || '')) break; s.note('b.txt click did not open the file yet; retrying', tabY); }
    check('switching runs switches the Files pane; files open from the Y worktree too', get(ey, 'b.txt').status === 'M' && !ey.some(e => e.path === 'sub') && /b\.txt/.test(tabY?.label || '') && (tabY?.path || '').includes('repo-y'), { ey: ey.map(e => e.path + ' ' + e.status), tabY });
    await s.screenshot('files-y');

    // Large repository.
    await pick('Z large');
    const ez = await entries();
    check('large repository: the deep change is visible from the root (folder counts)', get(ez, 'pkg').inside === '1' && get(ez, 'big').inside === '', ez.map(e => e.path + ' ' + e.inside));
    const t0 = Date.now();
    await clickEntry('big');
    await view.waitFor(`document.querySelectorAll('#files .row[role=treeitem]').length > 5000`, 15000);
    const openMs = Date.now() - t0;
    const truncated = await view.eval(`[...document.querySelectorAll('#files .row.muted')].map(r => r.textContent + ' (' + (r.title || '') + ')').find(t => /more entries/.test(t)) || ''`);
    await view.eval(`window.__lag = []; (function tick() { const t0 = performance.now(); if (window.__lag.length < 80) setTimeout(() => { window.__lag.push(performance.now() - t0 - 25); tick(); }, 25); })()`);
    const pane = await s.webviewPoint(view, '#files');
    for (let i = 0; i < 12; i++) { await cdp.wheel(pane.x, pane.y + 40, 800); await delay(60); }
    await delay(800);
    const lag = await view.eval(`(() => { const a = window.__lag.slice().sort((x, y) => x - y); return { n: a.length, p95: Math.round(a[Math.floor(a.length * 0.95)] || 0), max: Math.round(a[a.length - 1] || 0) }; })()`);
    check('a 6,000-entry folder opens within 2 s, is capped with a visible note, and scrolling stays responsive (p95 lag < 250 ms)', openMs < 2000 && /1000 more entries/.test(truncated) && lag.n > 20 && lag.p95 < 250, { openMs, truncated, lag });
    await clickEntry('big');
    await clickEntry('pkg');
    await view.waitFor(`[...document.querySelectorAll('#files .row')].some(r => r.dataset.path === 'pkg/m39')`, 10000);
    await clickEntry('pkg/m39');
    await view.waitFor(`[...document.querySelectorAll('#files .row')].some(r => r.dataset.path === 'pkg/m39/x99.txt')`, 10000);
    const deep = get(await entries(), 'pkg/m39/x99.txt');
    await clickEntry('pkg/m39/x99.txt');
    const tabZ = await activeTab();
    check('nested change found and opened in the large repository', deep.status === 'M' && /x99\.txt/.test(tabZ?.label || ''), { deep, tabZ });
    await s.screenshot('files-large');

    // Keyboard.
    await view.eval(`document.querySelector('#files .row[role=treeitem]').focus()`);
    const kb = [];
    for (const k of ['ArrowDown', 'ArrowDown']) { await view.eval(`document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: ${JSON.stringify(k)}, bubbles: true }))`); kb.push(await view.eval(`document.activeElement.getAttribute('aria-label')`)); }
    check('Files pane is a keyboard tree with labelled items', kb.every(Boolean) && kb[0] !== kb[1], kb);
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
