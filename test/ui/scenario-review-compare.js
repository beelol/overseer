// Packaged-UI scenario for AC-263 (the review opens on "Since task start", with the other
// comparisons one click away), with generic fixture agents in a disposable repository (no paid turns).
// 1. An agent in its own worktree commits one.txt in its first turn and leaves an untracked two.txt
//    in its second; main moves on afterwards. Its review opens on Since task start (both files);
//    one click shows Latest run (two.txt) and one Entire worktree (its branch against the commit
//    it started from: both files, not main's later commit).
// 2. An agent in the owner's checkout, on a feature branch with a commit of its own and an edit the
//    owner left before the task: the agent writes c1.txt, the owner writes owner.txt, the agent
//    writes c2.txt. Its review opens on Since task start (c1, c2 and owner.txt) and says it includes
//    any edits made in this folder; Latest run is c2.txt; Entire worktree is the feature branch
//    against main (feature.txt and a.txt too).
// Each time the header names the comparison shown (the pressed button) and the file count matches.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, git } = require('./harness');

(async () => {
  const s = new Session('review-compare');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'shop'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const runState = id => s.ctl('state').runs.find(r => r.id === id);
    const waitDone = async id => { for (let i = 0; i < 80 && !['completed', 'failed', 'interrupted'].includes(runState(id).status); i++) await delay(250); return runState(id); };
    const turn = async id => { s.ctl('run.follow_up', { run_id: id, prompt: '' }); await delay(500); return waitDone(id); };
    const agent = (title, mode, script) => s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', script], prompt: '', title, workspace_mode: mode });
    const click = async (frame, selector) => { const p = await s.webviewPoint(frame, selector); await cdp.click(p.x, p.y); await delay(400); };
    const chatOf = runId => cdp.webview(`window.__overseer?.selected?.() === ${JSON.stringify(runId)} && !!document.getElementById('review')`, 30000);
    // What the review shows once it has settled on `mode` with `count` files.
    const shown = (frame, mode, count) => frame.waitFor(`(() => {
      const bar = document.getElementById('compare'); if (!bar || bar.hidden || bar.dataset.mode !== ${JSON.stringify(mode)} || document.body.dataset.checking !== 'false') return null;
      const files = [...document.querySelectorAll('.diff-file:not(.browsed) .file-path')].map(e => e.textContent).sort();
      if (files.length !== ${count}) return null;
      const pressed = [...bar.querySelectorAll('button[aria-pressed="true"]')];
      return { files, count: document.getElementById('total').dataset.count, total: document.getElementById('total').textContent, label: bar.dataset.label,
        pressed: pressed.map(b => b.textContent.trim()), pressedName: pressed.map(b => b.getAttribute('aria-label')), note: document.getElementById('compare-note').textContent,
        buttons: [...bar.querySelectorAll('button.cmp')].filter(b => !b.hidden).map(b => b.textContent.trim()), more: document.getElementById('base-label').textContent }; })()`, 30000).catch(async () => ({ timedOut: true, now: await frame.eval(`({ mode: document.getElementById('compare')?.dataset.mode, checking: document.body.dataset.checking, files: [...document.querySelectorAll('.diff-file .file-path')].map(e => e.textContent) })`).catch(e => e.message) }));
    const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
    const reviewOf = async (title, runId) => {
      await s.selectAgent(title, { settle: 2000 });
      const chat = await chatOf(runId);
      await click(chat, '#review');
      return cdp.webview(`document.body.dataset.runId === ${JSON.stringify(runId)} && !!document.getElementById('diffs') && !document.getElementById('compare').hidden`, 60000);
    };
    const choose = async (frame, mode) => { await frame.eval(`document.querySelector('#compare button.cmp[data-mode="${mode}"]').id = 'cmp-target'`); await click(frame, '#cmp-target'); await frame.eval(`document.getElementById('cmp-target')?.removeAttribute('id')`); };

    // ---- 1. An agent in its own worktree.
    const w = agent('Worktree agent', 'worktree', "if [ -f one.txt ]; then printf 'two\\n' > two.txt; else printf 'one\\n' > one.txt && git add one.txt && git -c user.name=A -c user.email=a@x.invalid commit -qm one; fi");
    await waitDone(w.run.id);
    await turn(w.run.id);
    fs.writeFileSync(path.join(repo, 'main-later.txt'), 'later\n'); git(repo, 'add', 'main-later.txt'); git(repo, 'commit', '-qm', 'main later');
    check('fixture: turn 1 committed one.txt, turn 2 left two.txt untracked, main moved on', git(w.workspace.path, 'log', '-1', '--format=%s') === 'one' && git(w.workspace.path, 'status', '--porcelain') === '?? two.txt', git(w.workspace.path, 'status', '--porcelain'));
    const wr = await reviewOf('Worktree agent', w.run.id);
    const w1 = await shown(wr, 'task_start', 2);
    await s.screenshot('worktree-since-task-start');
    check('a finished agent in its own worktree: the review opens on Since task start (one.txt and two.txt, 2 files)', same(w1.files, ['one.txt', 'two.txt']) && w1.count === '2' && /^2 files/.test(w1.total), w1);
    check('the header names it: Since task start pressed, Latest run and Entire worktree one click away, no folder note', same(w1.pressed, ['Since task start']) && same(w1.buttons, ['Since task start', 'Latest run', 'Entire worktree']) && w1.more === 'More…' && w1.note === '', w1);
    await choose(wr, 'latest_run');
    const w2 = await shown(wr, 'latest_run', 1);
    await s.screenshot('worktree-latest-run');
    check('one click: Latest run (two.txt, 1 file), named in the header', same(w2.files, ['two.txt']) && w2.count === '1' && same(w2.pressed, ['Latest run']) && w2.label === 'Latest run', w2);
    await choose(wr, 'entire_worktree');
    const w3 = await shown(wr, 'entire_worktree', 2);
    await s.screenshot('worktree-entire-worktree');
    check("one click: Entire worktree (the branch against where it started: one.txt and two.txt, not main's later commit), named in the header", same(w3.files, ['one.txt', 'two.txt']) && w3.count === '2' && same(w3.pressed, ['Entire worktree']) && w3.label === 'Entire worktree', w3);
    await choose(wr, 'task_start');
    const w4 = await shown(wr, 'task_start', 2);
    check('one click back to Since task start', same(w4.pressed, ['Since task start']), w4);

    // ---- 2. An agent in the owner's checkout, on a feature branch.
    git(repo, 'switch', '-q', '-c', 'feature');
    fs.writeFileSync(path.join(repo, 'feature.txt'), 'feature\n'); git(repo, 'add', 'feature.txt'); git(repo, 'commit', '-qm', 'feature');
    fs.writeFileSync(path.join(repo, 'a.txt'), fs.readFileSync(path.join(repo, 'a.txt'), 'utf8').replace('L3: original', 'L3: the owner, before the task'));
    const c = agent('Checkout agent', 'current', "if [ -f c2.txt ]; then :; elif [ -f c1.txt ]; then printf 'c2\\n' > c2.txt; else printf 'c1\\n' > c1.txt; fi");
    await waitDone(c.run.id);
    fs.writeFileSync(path.join(repo, 'owner.txt'), "the owner's own edit\n");
    await turn(c.run.id);
    const cr = await reviewOf('Checkout agent', c.run.id);
    const c1 = await shown(cr, 'task_start', 3);
    await s.screenshot('checkout-since-task-start');
    check("a finished agent in the owner's checkout: the review opens on Since task start (c1, c2 and the owner's edit between turns; not the edit before the task)", same(c1.files, ['c1.txt', 'c2.txt', 'owner.txt']) && c1.count === '3', c1);
    check('the header says it includes any edits made in this folder', same(c1.pressed, ['Since task start']) && c1.note === '(includes any edits made in this folder)' && same(c1.pressedName, ['Since task start (includes any edits made in this folder)']), c1);
    await choose(cr, 'latest_run');
    const c2 = await shown(cr, 'latest_run', 1);
    await s.screenshot('checkout-latest-run');
    check('one click: Latest run (c2.txt, 1 file), named in the header', same(c2.files, ['c2.txt']) && c2.count === '1' && same(c2.pressed, ['Latest run']) && c2.note === '(includes any edits made in this folder)', c2);
    await choose(cr, 'entire_worktree');
    const c3 = await shown(cr, 'entire_worktree', 5);
    await s.screenshot('checkout-entire-worktree');
    check('one click: Entire worktree (the feature branch against main: a.txt, c1, c2, feature.txt, owner.txt, 5 files), named in the header', same(c3.files, ['a.txt', 'c1.txt', 'c2.txt', 'feature.txt', 'owner.txt']) && c3.count === '5' && same(c3.pressed, ['Entire worktree']), c3);
    // Each agent keeps its own comparison: the checkout agent's Entire worktree does not carry over.
    const w5 = await reviewOf('Worktree agent', w.run.id);
    const again = await shown(w5, 'task_start', 2);
    check('each agent keeps its own choice (the worktree agent is still on Since task start)', same(again.pressed, ['Since task start']), again);
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
