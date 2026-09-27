// Packaged-UI scenario for AC-49 in the Gate K layout (no paid tokens; generic-harness runs):
// select a run (its review opens beside its chat), choose a comparison, turn Follow on, scroll the
// review and the chat, type an unsent follow-up, open two more runs' chats to the side and collapse a
// repository in the side bar; then reload the window and restart VS Code and check that the same
// agent, comparison, positions, side chats and tree shape return, Follow comes back paused, and a
// run whose worktree was removed is explained instead of failing.
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
    s.settings({ 'window.menuStyle': 'custom' });
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

    const tabs = () => cdp.evalWorkbench(`[...document.querySelectorAll('.tab')].map(t => t.getAttribute('aria-label') || t.textContent)`);
    const clickTab = async pattern => {
      const pt = await cdp.waitFor(`(() => { const t = [...document.querySelectorAll('.tab')].find(t => ${pattern}.test(t.getAttribute('aria-label') || t.textContent)); if (!t) return null; t.scrollIntoView({ inline: 'nearest' });
        const b = t.getBoundingClientRect(), c = t.closest('.tabs-container').getBoundingClientRect(); const left = Math.max(b.left, c.left), right = Math.min(b.right, c.right); if (right - left < 20) return null; return { x: left + Math.min(40, (right - left) / 2), y: b.top + b.height / 2 }; })()`, 15000, 'tab ' + pattern);
      await cdp.click(pt.x, pt.y);
      await delay(1200);
    };
    const reviewFrame = runId => cdp.webview(`document.body.dataset.runId === ${JSON.stringify(runId)} && !!document.getElementById('diffs') && document.querySelectorAll('.diff-file').length > 0`, 30000);
    const chatFrame = title => cdp.webview(`document.getElementById('title')?.textContent === ${JSON.stringify(title)} && document.querySelectorAll('#conv .msg').length > 0 && !document.body.dataset.runId`, 30000);
    const sideChat = runId => cdp.webview(`document.body.dataset.runId === ${JSON.stringify(runId)} && !!document.getElementById('conv')`, 30000);
    const rowOf = label => cdp.evalWorkbench(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === ${JSON.stringify(label)}).pop(); return r ? { selected: r.classList.contains('selected'), expanded: r.getAttribute('aria-expanded') } : null; })()`);
    const openToSide = async title => {
      const pt = await cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === ${JSON.stringify(title)}).pop(); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 80, y: b.top + b.height / 2 }; })()`, 15000, title);
      const menuItem = `(() => { const a = [...document.querySelectorAll('.monaco-menu .action-item .action-label')].find(a => /^Open to the Side/.test(a.getAttribute('aria-label') || a.textContent.trim())); if (!a) return null; const r = a.getBoundingClientRect(); return { x: r.left + 20, y: r.top + r.height / 2 }; })()`;
      let item = null;
      for (let attempt = 0; attempt < 2 && !item; attempt++) {
        // Move off first so a row hover does not stand in the way of the context menu.
        await cdp.move(pt.x + 600, pt.y + 300); await delay(300); await cdp.key('Escape'); await delay(200);
        await cdp.click(pt.x, pt.y, { button: 'right' }); await delay(800);
        item = await cdp.waitFor(menuItem, 4000, 'menu item').catch(() => null);
      }
      if (!item) throw new Error('Open to the Side menu item not found for ' + title);
      await cdp.click(item.x, item.y); await delay(2000);
    };

    // R1 selected: its review (left) and chat (right).
    await s.selectAgent('R1 long review', { settle: 2500 });
    let review1 = await reviewFrame(t1.run.id);
    for (let attempt = 0; attempt < 3; attempt++) {
      const base = await s.webviewPoint(review1, '#base');
      await cdp.click(base.x, base.y);
      await cdp.waitQuickTitle('Compare the working tree with');
      await cdp.focusWorkbench();
      await cdp.evalWorkbench(`document.querySelector('.quick-input-widget input').focus()`);
      await cdp.type('task start');
      await delay(500);
      await cdp.key('Enter');
      if (await review1.waitFor(`document.getElementById('base-label').textContent === 'Since task start'`, 8000).catch(() => false)) break;
      await cdp.key('Escape');
    }
    await review1.waitFor(`document.getElementById('base-label').textContent === 'Since task start' && document.querySelectorAll('.diff-file').length === 2`, 20000);
    const follow = await s.webviewPoint(review1, '#follow');
    await cdp.click(follow.x, follow.y);
    await review1.waitFor(`(document.getElementById('follow').dataset.state !== 'off')`, 10000);
    await review1.waitFor(`document.getElementById('diffs').scrollHeight > document.getElementById('diffs').clientHeight + 1500`, 20000);
    await review1.eval(`document.getElementById('diffs').scrollTop = 1200`);
    await delay(1500);
    const before = { reviewTop: await review1.eval(`document.getElementById('diffs').scrollTop`), comparison: await review1.eval(`document.getElementById('base-label').textContent`) };
    const chat1 = await chatFrame('R1 long review');
    await chat1.waitFor(`document.getElementById('scroll').scrollHeight > document.getElementById('scroll').clientHeight + 600`, 20000);
    await chat1.eval(`document.getElementById('scroll').scrollTop = 400; document.getElementById('scroll').dispatchEvent(new Event('scroll')); const p = document.getElementById('prompt'); p.value = 'unsent follow-up draft'; p.dispatchEvent(new Event('input'));`);
    await delay(800);
    before.chatY = await chat1.eval(`document.getElementById('scroll').scrollTop`);
    // R2 and R3: their chats opened to the side.
    await openToSide('R2 other repo');
    await openToSide('R3 removed later');
    // The side bar: collapse repoB.
    await s.clickAgentRow('repoB', { twisty: true });
    before.repoB = await rowOf('repoB');
    await s.selectAgent('R1 long review', { settle: 2000 });
    before.tabs = await tabs();
    s.note('before', before);
    await s.screenshot('before-reload');

    const verify = async (label, cdpNow) => {
      cdp = cdpNow; s.cdp = cdpNow;
      await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status after ' + label);
      await delay(3500);
      const now = await tabs();
      check(`${label}: the review and the two side chats reopened`, [/Review.*R1 long review/, /^R2 other repo/, /^R3 removed later/].every(re => now.some(t => re.test(t))), now);
      check(`${label}: one Overseer view (no second copy)`, now.filter(t => /^Overseer\b/.test(t)).length === 1, now);
      await s.openOverseerView(); await delay(800);
      const r1 = await rowOf('R1 long review'), rb = await rowOf('repoB');
      check(`${label}: the selected agent and the collapsed repository restored in the side bar`, r1?.selected && rb?.expanded === 'false', { r1, repoB: rb });
      await clickTab(/Review.*R1 long review/);
      const rv = await reviewFrame(t1.run.id);
      await rv.waitFor(`document.getElementById('base-label').textContent === 'Since task start'`, 20000);
      await delay(1500);
      const r = await rv.eval(`({ top: Math.round(document.getElementById('diffs').scrollTop), comparison: document.getElementById('base-label').textContent, follow: document.getElementById('follow').dataset.state })`);
      check(`${label}: review comparison restored`, r.comparison === before.comparison, r.comparison);
      check(`${label}: review scroll position restored`, Math.abs(r.top - before.reviewTop) <= 80, { before: before.reviewTop, after: r.top });
      check(`${label}: Follow restored paused, not auto-resumed`, r.follow === 'paused', r.follow);
      await clickTab(/^Overseer\b/);
      const c = await chatFrame('R1 long review');
      await delay(1200);
      const o = await c.eval(`({ y: document.getElementById('scroll').scrollTop, draft: document.getElementById('prompt').value })`);
      check(`${label}: chat scroll position and unsent draft restored`, Math.abs(o.y - before.chatY) <= 60 && o.draft === 'unsent follow-up draft', { before: before.chatY, after: o });
      await clickTab(/^R2 other repo/);
      const side2 = await sideChat(t2.run.id).then(() => true, () => false);
      check(`${label}: a side chat for a run in another repository (not open in the window) restored`, side2);
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
    // R3's chat still shows its history; its review is explained instead of failing.
    await clickTab(/^R3 removed later/);
    const side3 = await sideChat(t3.run.id).then(f => f.eval(`document.querySelectorAll('#conv .msg').length`), () => 0);
    await s.selectAgent('R3 removed later', { settle: 2000 });
    await cdp.command('Overseer: Open Review'); await delay(1500);
    const text = await cdp.waitFor(`[...document.querySelectorAll('.notification-toast, .notifications-list-container .monaco-list-row')].map(t => t.innerText).find(t => /was removed/.test(t))`, 10000).catch(() => '');
    await s.screenshot('removed-explained');
    check('removed worktree is explained instead of failing (its chat keeps its history)', side3 > 0 && /was removed/.test(text || '') && (text || '').includes(t3.workspace.path) && /branch overseer\//.test(text || ''), { side3, text });
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
