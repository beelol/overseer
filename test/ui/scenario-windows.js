// Packaged-UI scenario for AC-106 (fixture runs only): never lose track of windows. Every view is
// opened several times (from the side bar, commands and a drag): one Overseer view, one review, one
// chat taken out per agent and one New Task remain. The Where am I map (command, shortcut and header
// controls) lists each open view and jumps to it. Closing views leaves no empty editor group.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

(async () => {
  const s = new Session('windows');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'windows-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_CLAUDE_PATH: '/nonexistent/claude', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const edit = line => ['-c', `sed -i '' 's/^L2: original$/L2: ${line}/' a.txt; echo done`];
    const A = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: edit('alpha'), prompt: '', title: 'Alpha agent' });
    s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: edit('bravo'), prompt: '', title: 'Bravo agent' });
    for (let i = 0; i < 40 && s.ctl('state').runs.some(r => ['queued', 'starting', 'running'].includes(r.status)); i++) await delay(300);

    const inventory = () => cdp.evalWorkbench(`(() => {
      const tabs = [...document.querySelectorAll('.part.editor .tab')].map(t => t.getAttribute('aria-label') || '');
      const groups = [...document.querySelectorAll('.editor-group-container')];
      return { tabs, groups: groups.length, empty: groups.filter(g => g.classList.contains('empty')).length };
    })()`);
    const kinds = inv => ({
      overseer: inv.tabs.filter(t => /^Overseer\b/.test(t)).length,
      review: inv.tabs.filter(t => /^Review/.test(t)).length,
      chatOut: inv.tabs.filter(t => /^Alpha agent\b/.test(t)).length,
      newTask: inv.tabs.filter(t => /^New Agent/.test(t)).length,
    });
    const dragAlpha = async () => {
      await s.openOverseerView(); await delay(600);
      const from = await cdp.waitFor(`(() => { const r = [...document.querySelectorAll('.part.sidebar .monaco-list-row')].filter(r => r.offsetParent && r.querySelector('.label-name')?.textContent.trim() === 'Alpha agent').pop(); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 10000);
      const to = await cdp.evalWorkbench(`(() => { const t = [...document.querySelectorAll('.part.editor .tabs-container')].pop().getBoundingClientRect(); return { x: t.right - 30, y: t.top + t.height / 2 }; })()`);
      await cdp.drag(from, to); await delay(2500);
    };

    // Open everything, each more than once.
    for (let i = 0; i < 2; i++) { await cdp.command('Overseer: Open Overseer View'); await delay(1200); }
    for (let i = 0; i < 2; i++) await s.selectAgent('Alpha agent', { settle: 2500 });
    for (let i = 0; i < 2; i++) { await cdp.command('Overseer: Open Review'); await delay(1500); }
    for (let i = 0; i < 2; i++) await dragAlpha();
    for (let i = 0; i < 2; i++) { await cdp.command('Overseer: Start an Agent with the Full Form'); await delay(1500); }
    const opened = await inventory();
    const k = kinds(opened);
    await s.screenshot('everything-open');
    check('after opening each view twice, one of each remains (Overseer view, review, the chat taken out, New Task) and no editor group is empty',
      k.overseer === 1 && k.review === 1 && k.chatOut === 1 && k.newTask === 1 && opened.empty === 0, { k, opened });

    // Where am I: lists every open view; picking one goes there.
    await cdp.focusWorkbench();
    await cdp.key('m', { meta: true, alt: true });
    await cdp.waitQuickTitle('Where am I');
    const rows = await cdp.evalWorkbench(`[...document.querySelectorAll('.quick-input-widget .monaco-list-row')].map(r => r.getAttribute('aria-label') || r.textContent.trim())`);
    await s.screenshot('where-am-i');
    const listed = { chat: rows.some(r => /Chat|Grid|New agent/.test(r)), review: rows.some(r => /Review/.test(r)), newTask: rows.some(r => /Full form/.test(r)), here: rows.some(r => /you are here/.test(r)) };
    check('⌥⌘M opens Where am I, listing the Overseer view, the review and New Task, and where you are', listed.chat && listed.review && listed.newTask && listed.here, { rows });
    await cdp.key('Escape'); await delay(300);
    const jumps = [];
    for (const want of ['Review', 'Full form', 'Chat']) {
      await cdp.command('Overseer: Where Am I');
      await cdp.type(want); await delay(400); await cdp.key('Enter'); await delay(1500);
      const active = await cdp.evalWorkbench(`document.querySelector('.editor-group-container.active .tab.active')?.getAttribute('aria-label') || ''`);
      jumps.push({ want, active });
    }
    check('picking a view in the map goes to it (review, New Task, the Overseer view)', /^Review/.test(jumps[0].active) && /^New Agent/.test(jumps[1].active) && /^Overseer/.test(jumps[2].active), jumps);
    // The header controls open the same map.
    const dash = await s.editorView(`!!document.getElementById('where')`);
    const hasControls = { chat: await dash.eval(`!!document.getElementById('where') && !document.getElementById('where').hidden`),
      review: await cdp.webview(`!!document.getElementById('diffs') && !!document.getElementById('where')`, 5000).then(() => true, () => false) };
    check('the chat and the review each have a Where am I control in their own header', hasControls.chat && hasControls.review, hasControls);

    // Close views: no empty editor group is left behind.
    for (const want of ['Full form', 'Review']) {
      await cdp.command('Overseer: Where Am I');
      await cdp.type(want); await delay(400); await cdp.key('Enter'); await delay(1200);
      s.note('before closing ' + want, { active: await cdp.evalWorkbench(`document.querySelector('.editor-group-container.active .tab.active')?.getAttribute('aria-label') || ''`), tabs: (await inventory()).tabs });
      await cdp.focusWorkbench(); await cdp.key('w', { meta: true }); await delay(1500);
      s.note('after closing ' + want, (await inventory()).tabs);
    }
    const closed = await inventory();
    await s.screenshot('after-closing');
    check('closing views leaves no empty editor group behind, and the other views stay open', closed.empty === 0 && !closed.tabs.some(t => /^New Agent|^Review/.test(t)) && closed.tabs.some(t => /^Overseer\b/.test(t)), closed);
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
