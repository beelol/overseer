// Packaged-UI scenario for AC-100 (fixture runs only): nothing is shown twice. Overseer offers once
// to take Explorer's place in the side bar; then, in every arrangement (chat alone, review beside
// the chat, the grid, the dashboard) an inventory lists where each kind of information appears:
// agents, the agent's files, its changed files and unsaved edits. Each appears in exactly one place
// (none where the arrangement does not show it); the chat has no Files pane and no changed-files
// strip, and its edit chips still open the review.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

// Where each kind of information shows, read from the workbench and every webview.
const WORKBENCH = `(() => {
  const shown = e => !!e && e.offsetWidth > 0 && e.offsetHeight > 0;
  const side = document.querySelector('.part.sidebar');
  const title = side?.querySelector('.composite.title .title-label')?.textContent.trim() || '';
  const agentsPane = [...document.querySelectorAll('.part.sidebar .pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || ''));
  return { sideBar: shown(side) ? title : '', agents: shown(agentsPane) && agentsPane.querySelectorAll('.monaco-list-row').length > 0,
    explorer: shown(side) && /Explorer/i.test(title),
    dirtyTabs: [...document.querySelectorAll('.tabs-container .tab.dirty')].filter(shown).map(t => t.getAttribute('aria-label')) };
})()`;
const WEBVIEW = `(() => {
  const shown = e => !!e && e.offsetWidth > 0 && e.offsetHeight > 0 && !e.closest('[hidden]');
  const any = sel => [...document.querySelectorAll(sel)].some(shown);
  return {
    kind: document.getElementById('diffs') ? 'review' : document.querySelector('.view-chat, .view-grid, .view-composer') ? 'overseer' : 'other',
    agentsList: any('[data-audit-view="agents"], .agents-list, #tree .row[data-run]'),
    filesPane: any('[data-audit-view="files"], .files-panel, #files-toggle'),
    changesStrip: any('.changes-bar, #changes'),
    reviewFiles: !!document.getElementById('diffs') && any('#tree .file'),
    reviewChanged: !!document.getElementById('diffs') && any('#tree .file.changed'),
    unsaved: [...document.querySelectorAll('.diff-file .unsaved')].filter(e => shown(e) && e.textContent.trim()).length,
  };
})()`;

(async () => {
  const s = new Session('inventory');
  const result = { checks: [], inventory: {} };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  try {
    const repo = makeRepo(path.join(s.root, 'inventory-repo'), { dirty: false });
    // Not decided yet: Overseer asks once (the harness otherwise answers "Keep Explorer").
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'overseer.sideBar.openOnStartup': null });
    s.install(latestVsix());
    s.launch(repo);
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const settingsFile = path.join(s.profile, 'User/settings.json');

    // The one-time offer.
    const offer = await cdp.waitFor(`(() => { const t = [...document.querySelectorAll('.notification-toast')].find(t => /instead of Explorer/.test(t.textContent)); if (!t) return false; const b = [...t.querySelectorAll('.monaco-button')].find(b => b.textContent.trim() === 'Use Overseer'); if (!b) return false; const r = b.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2, text: t.innerText }; })()`, 20000, 'side bar offer');
    await s.screenshot('side-bar-offer');
    await cdp.move(offer.x, offer.y); await delay(300);
    await cdp.evalWorkbench(`[...document.querySelectorAll('.notification-toast')].find(t => /instead of Explorer/.test(t.textContent))?.querySelectorAll('.monaco-button').forEach(b => { if (b.textContent.trim() === 'Use Overseer') b.click(); })`);
    await delay(1500);
    const chosen = JSON.parse(fs.readFileSync(settingsFile, 'utf8'))['overseer.sideBar.openOnStartup'];
    const wbAfter = await cdp.evalWorkbench(WORKBENCH);
    check('Overseer offers once to take Explorer\'s place; "Use Overseer" shows it in the side bar and remembers the choice', chosen === true && /Overseer/i.test(wbAfter.sideBar), { chosen, sideBar: wbAfter.sideBar, offer: offer.text });
    await cdp.command('Developer: Reload Window'); await delay(6000);
    cdp = await s.connect(); s.cdp = cdp;
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'after reload');
    await delay(4000);
    const reopened = await cdp.evalWorkbench(WORKBENCH);
    const askedAgain = await cdp.evalWorkbench(`[...document.querySelectorAll('.notification-toast')].some(t => /instead of Explorer/.test(t.textContent))`);
    check('after a reload the side bar opens on Overseer and the offer is not repeated (Explorer is still in the activity bar)', /Overseer/i.test(reopened.sideBar) && !askedAgain && await cdp.evalWorkbench(`!!document.querySelector('.activitybar [aria-label^="Explorer"]')`), { reopened, askedAgain });

    const changed = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', "sed -i '' 's/^L2: original$/L2: agent edit/' a.txt; echo done"], prompt: '', title: 'Changes agent' });
    const quiet = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo nothing to change'], prompt: '', title: 'Quiet agent' });
    for (const t of [changed, quiet]) for (let i = 0; i < 60 && ['queued', 'starting', 'running'].includes(s.ctl('state').runs.find(r => r.id === t.run.id).status); i++) await delay(250);

    const inventory = async label => {
      await delay(1200);
      const wb = await cdp.evalWorkbench(WORKBENCH);
      const views = [];
      for (const frame of await cdp.webviews().catch(() => [])) views.push(await frame.eval(WEBVIEW).catch(() => null));
      const live = views.filter(Boolean);
      const places = {
        agents: [wb.agents && 'side bar', ...live.filter(v => v.agentsList).map(v => v.kind + ' agent list')].filter(Boolean),
        files: [wb.explorer && 'Explorer', ...live.filter(v => v.filesPane).map(() => 'chat Files pane'), ...live.filter(v => v.reviewFiles).map(() => 'review navigator')].filter(Boolean),
        changedFiles: [...live.filter(v => v.reviewChanged).map(() => 'review navigator'), ...live.filter(v => v.changesStrip).map(() => 'chat changed-files strip'), ...live.filter(v => v.filesPane).map(() => 'chat Files pane')],
        unsaved: [...live.filter(v => v.unsaved).map(() => 'review'), ...(wb.dirtyTabs.length ? ['editor tab'] : [])],
      };
      result.inventory[label] = places;
      await s.screenshot('arrangement-' + label);
      return places;
    };
    const once = (places, kind, expected) => places[kind].length === expected;

    await s.selectAgent('Quiet agent', { settle: 2500 });
    const chatOnly = await inventory('chat-alone');
    check('chat alone: agents once (side bar); no files or changed files listed anywhere (no Files pane, no strip)', once(chatOnly, 'agents', 1) && once(chatOnly, 'files', 0) && once(chatOnly, 'changedFiles', 0), chatOnly);

    await s.selectAgent('Changes agent', { settle: 3000 });
    await cdp.webview(`!!document.getElementById('diffs') && document.querySelectorAll('#tree .file.changed').length > 0`, 30000);
    const split = await inventory('review-and-chat');
    check('review beside the chat: agents once, files once (review navigator), changed files once (not repeated under the conversation)', once(split, 'agents', 1) && once(split, 'files', 1) && once(split, 'changedFiles', 1) && split.files[0] === 'review navigator', split);
    // The edit chip in the turn still opens the review at that hunk.
    const dash = await s.editorView(`!!document.querySelector('.view-chat') && !document.querySelector('.view-chat').hidden`);
    const chip = await dash.eval(`!!document.querySelector('#conv .edit-chip, #conv [data-edit-path], #conv .file-chip')`);
    s.note('edit chip present', chip);

    // Unsaved edit typed in the review (not saved).
    const review = await cdp.webview(`!!document.getElementById('diffs') && document.querySelectorAll('.diff-file').length > 0`, 20000);
    await review.waitFor(`(() => { const e = [...document.querySelectorAll('.diff-file')].find(e => e.querySelector('.file-path').textContent === 'a.txt'); if (!e || e.dataset.loadState !== 'rendered') return false; const l = [...e.querySelectorAll('.editor.modified .view-lines .view-line')].find(l => /agent.edit/.test(l.textContent)); if (!l) return false; l.id = 'edit-target'; return true; })()`, 20000);
    const p = await s.webviewPoint(review, '#edit-target');
    await cdp.click(p.x - 20, p.y); await delay(150); await cdp.click(p.x - 20, p.y);
    await cdp.key('End'); await cdp.type(' UNSAVED');
    await review.waitFor(`[...document.querySelectorAll('.diff-file .unsaved')].some(e => e.textContent.trim())`, 10000).catch(() => {});
    const unsaved = await inventory('unsaved-edit');
    check('an unsaved edit is marked once among Overseer\'s views (the review)', unsaved.unsaved.filter(p => p === 'review').length === 1, unsaved);
    await review.eval(`document.querySelector('.diff-file .save-file').click()`); await delay(1500);

    await cdp.command('Overseer: Toggle Agent Grid'); await delay(2000);
    const grid = await inventory('grid');
    check('grid: agents once (side bar); no file lists', once(grid, 'agents', 1) && once(grid, 'files', 0) && once(grid, 'changedFiles', 0), grid);
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(1500);

    await cdp.command('Overseer: Enter Focus Mode'); await delay(3000);
    await s.selectAgent('Changes agent', { settle: 3000 });
    const dashboard = await inventory('dashboard');
    check('dashboard: agents once, files once, changed files once', once(dashboard, 'agents', 1) && once(dashboard, 'files', 1) && once(dashboard, 'changedFiles', 1), dashboard);
    await cdp.command('Overseer: Exit Focus Mode'); await delay(1500);
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
