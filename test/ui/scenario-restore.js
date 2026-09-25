// Packaged-UI scenario for AC-49 (no paid tokens; generic-harness runs): open several runs,
// their reviews and run panels, choose a comparison, turn Follow on, scroll, type an unsent
// follow-up and collapse a task; then reload the window and restart VS Code and check that
// the same runs, comparisons, positions and tree shape return, Follow comes back paused, and
// a run whose worktree was removed is explained instead of failing.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

(async () => {
  const s = new Session('restore');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  let r2;
  try {
    const repoA = makeRepo(path.join(s.root, 'repoA'), { dirty: false });
    const repoB = makeRepo(path.join(s.root, 'repoB'), { dirty: false });
    s.settings();
    s.install(latestVsix());
    s.launch(repoA);
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const sh = script => ({ harness: 'generic', program: '/bin/sh', args: ['-c', script], prompt: '' });
    const t1 = s.ctl('task.create', { repo: repoA, title: 'R1 long review', ...sh(`i=0; while [ $i -lt 200 ]; do echo "log line $i"; i=$((i+1)); done
awk 'NR%10==0{print "L" NR ": edited by R1"; next}{print}' a.txt > a.tmp && mv a.tmp a.txt
awk 'NR%15==0{print "L" NR ": edited by R1"; next}{print}' b.txt > b.tmp && mv b.tmp b.txt
echo done`) });
    const t3 = s.ctl('task.create', { repo: repoA, title: 'R3 removed later', ...sh('echo changed > c.txt; echo done') });
    const t2 = s.ctl('task.create', { repo: repoB, title: 'R2 other repo', ...sh('i=0; while [ $i -lt 150 ]; do echo tick $i >> progress.txt; echo tick $i; sleep 2; i=$((i+1)); done') });
    r2 = t2.run.id;
    const runState = id => s.ctl('state').runs.find(r => r.id === id);
    for (const t of [t1, t3]) for (let i = 0; i < 60 && runState(t.run.id).status !== 'completed'; i++) await delay(500);
    s.note('runs', { r1: t1.run.id, r2: t2.run.id, r3: t3.run.id, ws1: t1.workspace.path, ws2: t2.workspace.path, ws3: t3.workspace.path });

    await s.openOverseerView();
    const rowBelow = title => `(() => { const rows = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top);
      const i = rows.findIndex(r => r.textContent.includes(${JSON.stringify(title)})); return i < 0 ? null : { task: rows[i], run: rows[i + 1] }; })()`;
    const selectRun = async title => {
      const pt = await cdp.waitFor(`(() => { const x = ${rowBelow(title)}; if (!x || !x.run || !/generic/.test(x.run.textContent)) return null; const b = x.run.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 20000, 'run row ' + title);
      await cdp.click(pt.x, pt.y);
      await delay(2000);
    };
    const tabs = () => cdp.evalWorkbench(`[...document.querySelectorAll('.tab')].map(t => t.getAttribute('aria-label') || t.textContent)`);
    const clickTab = async pattern => {
      const pt = await cdp.waitFor(`(() => { const t = [...document.querySelectorAll('.tab')].find(t => ${pattern}.test(t.getAttribute('aria-label') || t.textContent)); if (!t) return null; t.scrollIntoView({ inline: 'nearest' });
        const b = t.getBoundingClientRect(), c = t.closest('.tabs-container').getBoundingClientRect(); const left = Math.max(b.left, c.left), right = Math.min(b.right, c.right); if (right - left < 20) return null; return { x: left + Math.min(40, (right - left) / 2), y: b.top + b.height / 2 }; })()`, 20000, 'tab ' + pattern);
      await cdp.click(pt.x, pt.y);
      await delay(1200);
    };
    const reviewFrame = runId => cdp.webview(`document.body.dataset.runId === ${JSON.stringify(runId)} && !!document.getElementById('diffs') && document.querySelectorAll('.diff-file').length > 0`, 30000);
    const outputFrame = runId => cdp.webview(`document.body.dataset.runId === ${JSON.stringify(runId)} && !!document.getElementById('conv') && document.querySelectorAll('#conv .msg').length > 0`, 30000);

    await selectRun('R3 removed later');
    await selectRun('R2 other repo');
    await selectRun('R1 long review');
    // R1: comparison "Since task start", Follow on, review scrolled, run panel scrolled with an unsent draft.
    let review1 = await reviewFrame(t1.run.id);
    for (let attempt = 0; attempt < 3; attempt++) {
      const base = await s.webviewPoint(review1, '#base');
      await cdp.click(base.x, base.y);
      await cdp.waitQuickTitle('Compare the working tree with');
      await cdp.focusWorkbench();
      await cdp.evalWorkbench(`document.querySelector('.quick-input-widget input').focus()`);
      await cdp.type('task start');
      await delay(500);
      s.note('comparison pick', await cdp.quickInputState());
      await cdp.key('Enter');
      if (await review1.waitFor(`document.getElementById('base-label').textContent === 'Since task start'`, 8000).catch(() => false)) break;
      await cdp.key('Escape');
    }
    await review1.waitFor(`document.getElementById('base-label').textContent === 'Since task start' && document.querySelectorAll('.diff-file').length === 2`, 20000);
    const follow = await s.webviewPoint(review1, '#follow');
    await cdp.click(follow.x, follow.y);
    await review1.waitFor(`document.getElementById('follow').checked`, 10000);
    await review1.waitFor(`document.getElementById('diffs').scrollHeight > document.getElementById('diffs').clientHeight + 1500`, 20000);
    await review1.eval(`document.getElementById('diffs').scrollTop = 1200`);
    await delay(1500);
    const before = { reviewTop: await review1.eval(`document.getElementById('diffs').scrollTop`), comparison: await review1.eval(`document.getElementById('base-label').textContent`) };
    let out1 = await outputFrame(t1.run.id);
    await out1.waitFor(`document.body.scrollHeight > innerHeight + 600`, 20000);
    await out1.eval(`window.scrollTo(0, 400); const p = document.getElementById('prompt'); p.value = 'unsent follow-up draft'; p.dispatchEvent(new Event('input'));`);
    await delay(800);
    before.outputY = await out1.eval(`window.scrollY`);
    // Collapse the R2 task in the Agents view.
    const taskRow = await cdp.waitFor(`(() => { const x = ${rowBelow('R2 other repo')}; if (!x) return null; const b = x.task.getBoundingClientRect(); return { x: b.left + 80, y: b.top + b.height / 2 }; })()`, 10000);
    await cdp.click(taskRow.x, taskRow.y);
    await cdp.waitFor(`${rowBelow('R2 other repo')}.task.getAttribute('aria-expanded') === 'false'`, 5000);
    before.tabs = await tabs();
    s.note('before', before);
    await s.screenshot('before-reload');

    const verify = async (label, cdpNow) => {
      cdp = cdpNow; s.cdp = cdpNow;
      await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status after ' + label);
      await delay(3000);
      const now = await tabs();
      const expected = [/Review: R1 long review/, /Review: R2 other repo/, /generic: R1 long review/, /generic: R2 other repo/, /generic: R3 removed later/];
      check(`${label}: review and run panels reopened`, expected.every(re => now.some(t => re.test(t))), now);
      await s.openOverseerView();
      const tree = await cdp.waitFor(`(() => { const r1 = ${rowBelow('R1 long review')}, r2 = ${rowBelow('R2 other repo')}; if (!r1 || !r2) return null;
        return { r1Selected: r1.run.classList.contains('selected') || r1.run.getAttribute('aria-selected') === 'true', r2Collapsed: r2.task.getAttribute('aria-expanded') === 'false', r2RunHidden: !r2.run || !/generic/.test(r2.run.textContent) || r2.run.textContent.includes('R1') || r2.run.textContent.includes('R3') }; })()`, 20000);
      check(`${label}: selected run and collapsed task restored in the Agents view`, tree.r1Selected && tree.r2Collapsed, tree);
      await clickTab(/Review: R1 long review/);
      const rv = await reviewFrame(t1.run.id);
      await rv.waitFor(`document.getElementById('base-label').textContent === 'Since task start'`, 20000);
      await delay(1500);
      const r = await rv.eval(`({ rows: [...document.querySelectorAll('.diff-file')].map(e => [e.querySelector('.file-path').textContent, e.offsetTop, e.offsetHeight, e.dataset.loadState]), restore: document.body.dataset.restore, top: document.getElementById('diffs').scrollTop, comparison: document.getElementById('base-label').textContent, follow: document.getElementById('follow').checked, resume: !document.getElementById('resume').hidden, followState: document.getElementById('follow-state').textContent })`);
      check(`${label}: review comparison mode restored`, r.comparison === before.comparison, r.comparison);
      check(`${label}: review scroll position restored`, Math.abs(r.top - before.reviewTop) <= 80, { before: before.reviewTop, after: r.top, restore: r.restore, rows: r.rows });
      check(`${label}: Follow restored paused, not auto-resumed`, r.follow && r.resume && /paused/.test(r.followState), r);
      await clickTab(/generic: R1 long review/);
      const ov = await outputFrame(t1.run.id);
      await delay(1200);
      const o = await ov.eval(`({ y: window.scrollY, draft: document.getElementById('prompt').value })`);
      check(`${label}: run panel scroll position and unsent draft restored`, Math.abs(o.y - before.outputY) <= 60 && o.draft === 'unsent follow-up draft', { before: before.outputY, after: o });
      await clickTab(/Review: R2 other repo/);
      const rv2 = await cdp.webview(`document.body.dataset.runId === ${JSON.stringify(t2.run.id)} && document.getElementById('workspace-note').textContent.includes(${JSON.stringify(t2.workspace.path)})`, 30000).catch(() => undefined);
      check(`${label}: review for a run in another repository (not open in the window) restored`, !!rv2, t2.workspace.path);
      check(`${label}: active run kept running`, runState(t2.run.id).status === 'running', runState(t2.run.id).status);
      await s.screenshot(label);
    };

    // 1) Window reload.
    await cdp.command('Developer: Reload Window');
    await delay(6000);
    await verify('after-reload', await s.connect());

    // 2) Remove R3's worktree, then restart VS Code (Cmd+Q and relaunch with the same profile).
    s.ctl('workspace.cleanup', { workspace_id: t3.workspace.id, discard_dirty: true });
    check('R3 worktree removed before restart', !fs.existsSync(t3.workspace.path), t3.workspace.path);
    await s.quit();
    await delay(3000);
    s.launch(repoA);
    await verify('after-restart', await s.connect());
    const r3tab = (await tabs()).find(t => /Review: R3/.test(t));
    await clickTab(/Review: R3/);
    const unavailable = await cdp.webview(`!!document.getElementById('restore-error')`, 20000).catch(() => undefined);
    const text = unavailable && await unavailable.eval(`document.getElementById('restore-error').textContent`);
    check('removed worktree is explained instead of failing', r3tab && /removed/.test(text || '') && text.includes(t3.workspace.path) && /branch overseer\//.test(text), { tab: r3tab, text });
    await s.screenshot('removed-worktree-explained');
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { if (r2) s.ctl('run.interrupt', { run_id: r2 }); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
