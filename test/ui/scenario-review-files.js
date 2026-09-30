// Packaged-UI scenario for AC-99 (fixture runs only): the review is where an agent's files live.
// The navigator starts on Changed while the agent has changes; a changed file is edited and saved
// in the review (disk checked) with no editor tab; All files lists the whole worktree one folder at
// a time, and (AC-264) a file picked there opens as the real file, the agent in Follow. On a
// 10,000-file worktree the first level shows in under 500 ms.
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
    const review = await cdp.webview(`!!document.getElementById('changes-only') && document.querySelectorAll('.diff-file').length > 0`, 30000);
    await review.waitFor(`document.body.dataset.nav === 'changes' && !!document.querySelector('#tree .file[data-path="a.txt"] .status-M') && /\\+1/.test(document.querySelector('#tree .file[data-path="a.txt"] .counts')?.textContent || '')`, 20000);
    const changesTree = await review.eval(`[...document.querySelectorAll('#tree .file')].map(b => b.dataset.path)`);
    check('the navigator starts on Changes only while the agent has changes, with the status and counts beside the changed file', changesTree.length === 1 && changesTree[0] === 'a.txt', changesTree);
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

    // All files: the whole worktree, folders listed when opened.
    await review.eval(`document.getElementById('changes-only').click()`);
    await review.waitFor(`document.body.dataset.nav === 'all' && [...document.querySelectorAll('#tree > details.folder > summary')].some(s => s.textContent === 'src')`, 10000);
    const top = await review.eval(`({ folders: [...document.querySelectorAll('#tree > details.folder > summary')].map(s => s.textContent), files: [...document.querySelectorAll('#tree > .file')].map(b => b.dataset.path), changed: document.querySelector('#tree > .file[data-path="a.txt"] .status')?.textContent })`);
    check('All files lists the worktree top level (folders first, .git left out) and keeps the changed file marked', top.folders.join() === 'src' && ['README.md', 'a.txt', 'b.txt', 'c.txt'].every(f => top.files.includes(f)) && top.changed === 'M', top);
    await review.eval(`document.getElementById('changes-only').click()`);
    await review.waitFor(`document.body.dataset.nav === 'changes'`, 5000);
    const onlyChanged = await review.eval(`[...document.querySelectorAll('#tree .file')].map(b => b.dataset.path).sort()`);
    check('Changed shows just the changed files again', onlyChanged.join() === 'a.txt', onlyChanged);

    // AC-264 (the owner, 2026-09-29): All files is Follow's list, so a file picked there opens as
    // the real file, the agent in Follow (the review gives way).
    await review.eval(`document.getElementById('changes-only').click()`);
    await review.waitFor(`document.body.dataset.nav === 'all' && [...document.querySelectorAll('#tree details.folder > summary')].some(s => s.textContent === 'src')`, 10000);
    await review.eval(`[...document.querySelectorAll('#tree details.folder > summary')].find(s => s.textContent === 'src').click()`);
    await review.waitFor(`[...document.querySelectorAll('#tree details.folder > summary')].some(s => s.textContent === 'deep')`, 10000);
    await review.eval(`[...document.querySelectorAll('#tree details.folder > summary')].find(s => s.textContent === 'deep').click()`);
    await review.waitFor(`!!document.querySelector('#tree .file[data-path="src/deep/nested.txt"]')`, 10000);
    await review.eval(`document.querySelector('#tree .file[data-path="src/deep/nested.txt"]').click()`);
    const realTab = async () => cdp.evalWorkbench(`(() => { const t = [...document.querySelectorAll('.tabs-container .tab')]; const a = t.find(x => x.classList.contains('active') && x.closest('.editor-group-container.active'));
      return { active: a ? (a.getAttribute('aria-label') || a.textContent.trim()) : '', review: t.some(x => /^Review/.test(x.textContent.trim())), text: [...document.querySelectorAll('.editor-group-container.active .monaco-editor .view-line')].map(l => l.textContent.replace(/\u00a0/g, ' ')).join('\n') }; })()`);
    let opened = await realTab();
    for (let i = 0; i < 80 && !/nested\.txt/.test(opened.active); i++) { await delay(250); opened = await realTab(); }
    await delay(800); opened = await realTab();
    check('a nested unchanged file picked in All files opens as the real file in an editor, the agent in Follow and its review closed',
      /nested\.txt/.test(opened.active) && /nested 1/.test(opened.text) && !opened.review, { active: opened.active, review: opened.review, firstLines: opened.text.split('\n').slice(0, 2) });
    await s.screenshot('all-files-real-file');

    // 10,000 files: the first level of All files, measured in the webview from the click.
    for (let i = 0; i < 60 && ['queued', 'starting', 'running'].includes(s.ctl('state').runs.find(r => r.id === tb.run.id).status); i++) await delay(250);
    await s.selectAgent('Big worktree', { settle: 2000 });
    const bigReview = await cdp.webview(`!!document.getElementById('changes-only') && (document.body.dataset.workspace || '').includes(${JSON.stringify(path.basename(tb.workspace.path))}) && document.querySelectorAll('.diff-file').length > 0`, 30000);
    await bigReview.waitFor(`document.body.dataset.nav === 'changes'`, 10000);
    const firstLevel = await bigReview.eval(`(async () => { const t0 = performance.now(); document.getElementById('changes-only').click();
      while (document.querySelectorAll('#tree > details.folder').length < 100) { if (performance.now() - t0 > 5000) return { ms: -1 }; await new Promise(r => setTimeout(r, 2)); }
      return { ms: Math.round(performance.now() - t0), folders: document.querySelectorAll('#tree > details.folder').length }; })()`);
    result.timings.firstLevel10k = firstLevel.ms;
    check('on a 10,000-file worktree the navigator\'s first level shows in under 500 ms', firstLevel.ms >= 0 && firstLevel.ms < 500 && firstLevel.folders === 100, firstLevel);
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
