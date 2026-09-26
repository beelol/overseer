// Packaged-UI scenario for AC-75 (one place for changes), fixtures only. The Workspace Dirty view
// is gone; the review's scope picker shows All changes, Staged, Unstaged and Untracked. The fixture
// is a current checkout with a merge conflict, a staged edit, a staged rename, an unstaged edit, an
// unstaged deletion, an untracked file and an unsaved editor: each shows under the right scope,
// staged files are read-only (their right side is the index), conflicted and unsaved files carry
// markers, and the choice is remembered per agent.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, git } = require('./harness');

(async () => {
  const s = new Session('scopes');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'scope-repo'), { dirty: false });
    fs.writeFileSync(path.join(repo, 'gone.txt'), 'to be deleted\n'); fs.writeFileSync(path.join(repo, 'conflict.txt'), 'base\n');
    git(repo, 'add', '.'); git(repo, 'commit', '-q', '-m', 'more files');
    // A merge conflict on conflict.txt.
    git(repo, 'checkout', '-q', '-b', 'other'); fs.writeFileSync(path.join(repo, 'conflict.txt'), 'theirs\n'); git(repo, 'commit', '-q', '-am', 'theirs');
    git(repo, 'checkout', '-q', 'main'); fs.writeFileSync(path.join(repo, 'conflict.txt'), 'ours\n'); git(repo, 'commit', '-q', '-am', 'ours');
    try { git(repo, 'merge', 'other'); } catch { /* conflict expected */ }
    // Staged edit, staged rename, unstaged edit, unstaged deletion, untracked file.
    fs.appendFileSync(path.join(repo, 'c.txt'), 'staged line\n'); git(repo, 'add', 'c.txt');
    git(repo, 'mv', 'b.txt', 'b-renamed.txt');
    fs.writeFileSync(path.join(repo, 'a.txt'), fs.readFileSync(path.join(repo, 'a.txt'), 'utf8').replace('L5: original', 'L5: unstaged edit'));
    fs.rmSync(path.join(repo, 'gone.txt'));
    fs.writeFileSync(path.join(repo, 'notes.txt'), 'untracked notes\n');
    s.note('git status', git(repo, 'status', '--porcelain=v1'));

    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const views = await cdp.evalWorkbench(`[...document.querySelectorAll('.pane-header')].map(h => h.textContent.trim())`);
    const t = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/echo', args: ['looked around'], prompt: '', title: 'Dirty checkout', workspace_mode: 'current' });
    for (let i = 0; i < 30 && s.ctl('state').runs.find(r => r.id === t.run.id).status !== 'completed'; i++) await delay(300);

    // An unsaved editor on README.md in the checkout.
    await cdp.command('Go to File'); await delay(600); await cdp.type('README.md'); await delay(900); await cdp.key('Enter'); await delay(1500);
    s.note('unsaved editor', await cdp.evalWorkbench(`[...document.querySelectorAll('.tab')].map(t => t.getAttribute('aria-label') + (t.classList.contains('dirty') ? ' (dirty)' : ''))`));
    await cdp.key('End', { meta: true }); await cdp.type('unsaved draft line'); await delay(500);

    await cdp.command('Overseer: Switch Agent…'); await cdp.waitQuickTitle('Switch to agent'); await cdp.type('Dirty checkout'); await delay(300); await cdp.key('Enter'); await delay(1500);
    await cdp.command('Overseer: Open Review'); await delay(2500);
    const review = await cdp.webview(`!!document.getElementById('scope') && document.body.dataset.runId === ${JSON.stringify(t.run.id)}`, 30000);
    await cdp.command('View: Show Overseer'); await delay(800);
    const sideViews = await cdp.evalWorkbench(`[...document.querySelectorAll('.pane-header')].filter(h => h.offsetParent).map(h => h.textContent.trim())`);
    check('the Workspace Dirty view is gone; the side bar shows Agents and Accounts', !sideViews.some(v => /Workspace Dirty/i.test(v)) && sideViews.some(v => /^Agents/.test(v)) && sideViews.some(v => /^Accounts/.test(v)), sideViews);

    const files = () => review.eval(`[...document.querySelectorAll('#tree .file')].map(b => ({ name: b.querySelector('.file-name')?.textContent, status: b.querySelector('.status')?.textContent, conflicted: b.classList.contains('conflicted'), unsaved: !!b.querySelector('.marker'), title: b.title }))`);
    const pick = async scope => {
      await review.eval(`(() => { const s = document.getElementById('scope'); s.value = ${JSON.stringify(scope)}; s.dispatchEvent(new Event('change')); return true; })()`);
      await review.waitFor(`document.body.dataset.scope === ${JSON.stringify(scope)}`, 10000).catch(() => {});
      await delay(2500);
      return files();
    };
    const results = {};
    for (const scope of ['staged', 'unstaged', 'untracked', 'all']) { results[scope] = await pick(scope); await s.screenshot('scope-' + scope); }
    s.note('scopes', results);
    const names = scope => results[scope].map(f => f.name);
    check('Staged shows the staged edit and the staged rename', names('staged').includes('c.txt') && names('staged').includes('b-renamed.txt') && !names('staged').includes('a.txt'), results.staged);
    check('Unstaged shows the unstaged edit and the unstaged deletion', names('unstaged').includes('a.txt') && names('unstaged').includes('gone.txt') && results.unstaged.find(f => f.name === 'gone.txt')?.status === 'D' && !names('unstaged').includes('c.txt'), results.unstaged);
    check('Untracked shows the untracked file only', names('untracked').includes('notes.txt') && !names('untracked').includes('a.txt'), results.untracked);
    check('All changes marks the conflicted file and the unsaved editor', results.all.some(f => f.name === 'conflict.txt' && f.conflicted) && results.all.some(f => f.name === 'README.md' && f.unsaved), results.all);

    // Staged files are read-only in the review (no Save; the right side is the index).
    await pick('staged');
    await review.eval(`[...document.querySelectorAll('#tree .file')].find(b => b.querySelector('.file-name')?.textContent === 'c.txt')?.click()`); await delay(2500);
    const staged = await review.waitFor(`(() => { const f = [...document.querySelectorAll('.diff-file')].find(e => /c\\.txt/.test(e.querySelector('.file-path')?.textContent || '')); return f && f.dataset.editable !== undefined && { save: !f.querySelector('.save-file').hidden, editable: f.dataset.editable }; })()`, 15000).catch(() => null);
    const unstagedEditable = await (async () => { await pick('unstaged'); await review.eval(`[...document.querySelectorAll('#tree .file')].find(b => b.querySelector('.file-name')?.textContent === 'a.txt')?.click()`); return review.waitFor(`(() => { const f = [...document.querySelectorAll('.diff-file')].find(e => /a\\.txt/.test(e.querySelector('.file-path')?.textContent || '')); return f && f.dataset.editable; })()`, 15000).catch(() => null); })();
    check('a staged file is read-only (no Save); an unstaged file stays editable', staged && staged.editable === 'false' && !staged.save && unstagedEditable === 'true', { staged, unstagedEditable });

    // Remembered per agent.
    await pick('untracked');
    await cdp.command('Overseer: Open Review'); await delay(1500);
    const again = await review.eval(`document.getElementById('scope').value`);
    check('the scope is remembered for the agent', again === 'untracked', again);
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
