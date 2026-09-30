// Packaged-UI scenario for AC-99 and AC-264 (fixture runs only): the review is where an agent's files
// live. In Diffs only the list is Changed; a changed file is edited and saved in the review (disk
// checked) with no editor tab. In Follow the list is All files, the whole worktree one folder at a
// time, and a file picked there shows in the review's middle (the owner, 2026-09-30), with no VS Code
// editor. On a 10,000-file worktree All files' first level shows in under 500 ms.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, git } = require('./harness');

(async () => {
  const s = new Session('review-files');
  const result = { checks: [], timings: {} };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'files-repo'), { dirty: false });
    fs.mkdirSync(path.join(repo, 'src/deep'), { recursive: true });
    fs.writeFileSync(path.join(repo, 'src/deep/nested.txt'), Array.from({ length: 30 }, (_, i) => `nested ${i + 1}`).join('\n') + '\n');
    fs.writeFileSync(path.join(repo, 'src/index.js'), 'module.exports = 1;\n');
    git(repo, 'add', '.'); git(repo, 'commit', '-q', '-m', 'nested files');
    // 10,000 files: 100 folders of 100 files each at the top level.
    const big = makeRepo(path.join(s.root, 'big-repo'), { dirty: false });
    for (let d = 0; d < 100; d++) {
      const dir = path.join(big, `pkg-${String(d).padStart(3, '0')}`); fs.mkdirSync(dir);
      for (let f = 0; f < 100; f++) fs.writeFileSync(path.join(dir, `file-${f}.txt`), `${d}/${f}\n`);
    }
    git(big, 'add', '.'); git(big, 'commit', '-q', '-m', '10,000 files');

    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const edit = "sed -i '' 's/^L2: original$/L2: agent edit/' a.txt; echo done";
    const t = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', edit], prompt: '', title: 'Files demo' });
    const tb = s.ctl('task.create', { repo: big, harness: 'generic', program: '/bin/sh', args: ['-c', edit], prompt: '', title: 'Big worktree' });
    const ws = t.workspace.path;
    for (let i = 0; i < 60 && ['queued', 'starting', 'running'].includes(s.ctl('state').runs.find(r => r.id === t.run.id).status); i++) await delay(250);

    await s.selectAgent('Files demo', { settle: 2000 });
    const review = await cdp.webview(`!!document.getElementById('view-mode') && document.querySelectorAll('.diff-file').length > 0`, 30000);
    await review.waitFor(`document.body.dataset.nav === 'changes' && !!document.querySelector('#tree .file[data-path="a.txt"] .status-M') && /\\+1/.test(document.querySelector('#tree .file[data-path="a.txt"] .counts')?.textContent || '')`, 20000);
    const changesTree = await review.eval(`[...document.querySelectorAll('#tree .file')].map(b => b.dataset.path)`);
    const listName = await review.eval(`document.getElementById('list-title').textContent`);
    check('in Diffs only the list is Changed: the changed file, with its status and counts', listName === 'Changed' && changesTree.length === 1 && changesTree[0] === 'a.txt', { listName, changesTree });
    await s.screenshot('changes-only');

    const editLine = async (file, text, pattern) => {
      await review.waitFor(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === ${JSON.stringify(file)}); if (!e) return false; e.scrollIntoView(); const l = [...e.querySelectorAll('.editor.modified .view-lines .view-line')].filter(l => !l.closest('.view-zones')).find(l => ${pattern}.test(l.textContent)); if (!l) return false; l.scrollIntoView({ block: 'center' }); document.getElementById('edit-target')?.removeAttribute('id'); document.getElementById('save-target')?.removeAttribute('id'); l.id = 'edit-target'; e.querySelector('.save-file').id = 'save-target'; return true; })()`, 15000);
      await delay(500);
      const p = await s.webviewPoint(review, '#edit-target');
      await cdp.click(p.x - 20, p.y); await delay(150); await cdp.click(p.x - 20, p.y);
      const tabsNow = () => cdp.evalWorkbench(`[...document.querySelectorAll('.tabs-container .tab')].map(t => (t.getAttribute('aria-label') || '') + ' [' + t.className.replace(/\\s+/g, ' ') + ']')`);
      s.note('tabs before typing', await tabsNow());
      await cdp.key('End'); await cdp.type(text); await delay(900);
      s.note('tabs after typing', await tabsNow());
      const save = await s.webviewPoint(review, '#save-target');
      await cdp.click(save.x, save.y);
      await delay(1500);
      s.note('tabs after save', await tabsNow());
    };
    await editLine('a.txt', ' EDITED-CHANGED', '/agent.edit/');
    let aDisk = '';
    for (let i = 0; i < 40 && !/L2: agent edit EDITED-CHANGED/.test(aDisk); i++) { await delay(250); aDisk = fs.readFileSync(path.join(ws, 'a.txt'), 'utf8'); }
    check('the changed file is edited and saved from the review', /L2: agent edit EDITED-CHANGED/.test(aDisk), aDisk.split('\n').slice(0, 3));
    const tabs = await cdp.evalWorkbench(`[...document.querySelectorAll('.tabs-container .tab')].map(t => t.getAttribute('aria-label') || t.textContent.trim())`);
    check('editing the agent\'s changed file in the review opened no editor tab', !tabs.some(t => /(^|[^\w])a\.txt/.test(t)), tabs);
    await s.screenshot('edited');

    // Follow (AC-264): its list is All files, the whole worktree, folders listed when opened.
    const toView = v => review.eval(`document.querySelector('#view-mode .seg[data-view="${v}"]').click()`);
    await toView('follow');
    await review.waitFor(`document.body.dataset.view === 'follow' && document.body.dataset.nav === 'all' && [...document.querySelectorAll('#tree > details.folder > summary')].some(s => s.textContent === 'src')`, 10000);
    const top = await review.eval(`({ list: document.getElementById('list-title').textContent, folders: [...document.querySelectorAll('#tree > details.folder > summary')].map(s => s.textContent), files: [...document.querySelectorAll('#tree > .file')].map(b => b.dataset.path), changed: document.querySelector('#tree > .file[data-path="a.txt"] .status')?.textContent })`);
    check('in Follow the list is All files: the worktree top level (folders first, .git left out), the changed file still marked', top.list === 'All files' && top.folders.join() === 'src' && ['README.md', 'a.txt', 'b.txt', 'c.txt'].every(f => top.files.includes(f)) && top.changed === 'M', top);
    await toView('diffs');
    await review.waitFor(`document.body.dataset.nav === 'changes'`, 5000);
    const onlyChanged = await review.eval(`[...document.querySelectorAll('#tree .file')].map(b => b.dataset.path).sort()`);
    check('back in Diffs only, Changed shows just the changed files again', onlyChanged.join() === 'a.txt', onlyChanged);

    // AC-264 (the owner, 2026-09-30): a file picked in All files shows in the review's middle, in
    // place of the agent's file; no VS Code editor opens and the review stays.
    await toView('follow');
    await review.waitFor(`document.body.dataset.nav === 'all' && [...document.querySelectorAll('#tree details.folder > summary')].some(s => s.textContent === 'src')`, 10000);
    await review.eval(`[...document.querySelectorAll('#tree details.folder > summary')].find(s => s.textContent === 'src').click()`);
    await review.waitFor(`[...document.querySelectorAll('#tree details.folder > summary')].some(s => s.textContent === 'deep')`, 10000);
    await review.eval(`[...document.querySelectorAll('#tree details.folder > summary')].find(s => s.textContent === 'deep').click()`);
    await review.waitFor(`!!document.querySelector('#tree .file[data-path="src/deep/nested.txt"]')`, 10000);
    await review.eval(`document.querySelector('#tree .file[data-path="src/deep/nested.txt"]').click()`);
    await review.waitFor(`document.body.dataset.followPath === 'src/deep/nested.txt' && document.body.dataset.followState === 'shown' && /nested 1/.test(document.getElementById('follow-editor').textContent)`, 15000).catch(() => {});
    await delay(600);
    const shown = await review.eval(`({ path: document.body.dataset.followPath, source: document.body.dataset.followSource, state: document.body.dataset.followState, active: document.querySelector('#tree .file.active')?.dataset.path || '',
      text: [...document.querySelectorAll('#follow-editor .view-line')].map(l => l.textContent.replace(/\\u00a0/g, ' ')).join('\\n').slice(0, 200), again: !document.getElementById('follow-again').hidden })`);
    const work = await cdp.evalWorkbench(`({ tabs: [...document.querySelectorAll('.tabs-container .tab')].map(t => t.getAttribute('aria-label') || t.textContent.trim()), editors: [...document.querySelectorAll('.part.editor .monaco-editor')].filter(e => e.offsetParent).length })`);
    check('a nested unchanged file picked in All files shows in the review\'s middle (selected in the list, "Follow the agent" to go back); no VS Code editor or tab opens and the review stays',
      shown.path === 'src/deep/nested.txt' && shown.source === 'user' && shown.state === 'shown' && /nested 1/.test(shown.text) && shown.active === 'src/deep/nested.txt' && shown.again
      && work.editors === 0 && !work.tabs.some(t => /nested\.txt/.test(t)) && work.tabs.some(t => /^Review/.test(t)), { shown, work });
    await s.screenshot('all-files-picked-file');

    // 10,000 files: the first level of All files, measured in the webview from the click.
    for (let i = 0; i < 60 && ['queued', 'starting', 'running'].includes(s.ctl('state').runs.find(r => r.id === tb.run.id).status); i++) await delay(250);
    await s.selectAgent('Big worktree', { settle: 2000 });
    const bigReview = await cdp.webview(`!!document.getElementById('view-mode') && (document.body.dataset.workspace || '').includes(${JSON.stringify(path.basename(tb.workspace.path))}) && document.querySelectorAll('.diff-file').length > 0`, 30000);
    await bigReview.waitFor(`document.body.dataset.nav === 'changes'`, 10000);
    const firstLevel = await bigReview.eval(`(async () => { const t0 = performance.now(); document.querySelector('#view-mode .seg[data-view="follow"]').click();
      while (document.querySelectorAll('#tree > details.folder').length < 100) { if (performance.now() - t0 > 5000) return { ms: -1 }; await new Promise(r => setTimeout(r, 2)); }
      return { ms: Math.round(performance.now() - t0), folders: document.querySelectorAll('#tree > details.folder').length }; })()`);
    result.timings.firstLevel10k = firstLevel.ms;
    check('on a 10,000-file worktree Follow\'s All files shows its first level in under 500 ms', firstLevel.ms >= 0 && firstLevel.ms < 500 && firstLevel.folders === 100, firstLevel);
    await s.screenshot('all-files-10k');
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
