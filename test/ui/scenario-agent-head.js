// Packaged-UI scenario for AC-233 (clicking an agent puts you in its head) and AC-257 (following an
// agent sits beside Overseer's conversation), Claude fixture only (no paid turns). The fixture
// agent edits a.txt and b.txt one step at a time (each step waits for this scenario's go file).
//   AC-257: the agent is opened from its card in Overseer's conversation: its head opens beside the
//   conversation, which keeps its tab. ⌥⌘U goes to the conversation leaving the head as it was; the
//   conversation's "Back to" chip returns to the head exactly where it was left; the same from an
//   agent opened in the side bar, whose chat the Overseer tab shows again on the way back.
//   AC-233: the Worktree view lists the agent's worktree; Follow opens the file the agent edits at
//   the changed line with inline annotations (tint and gutter bar, "was:" on changed lines, a marker
//   for removed lines); screenshots in the three themes; an edit typed and saved in the head lands
//   in the agent's worktree (not the window's folder); the toggle switches to Diffs only (the
//   review) and back to the same file and line; the window's folder and window count never change.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot, git } = require('./harness');

const THEMES = ['Overseer Dark', 'Overseer Light', 'Overseer'];

(async () => {
  const s = new Session('agent-head');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const barrier = path.join(s.root, 'edit-barrier');
  fs.mkdirSync(barrier);
  const go = n => fs.writeFileSync(path.join(barrier, `go-${n}`), '');
  try {
    const repo = makeRepo(path.join(s.root, 'head-repo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    // The default (Follow) is what is tested: the harness's Diffs-only default for older scenarios is dropped.
    s.settings({ 'workbench.colorTheme': THEMES[0], 'overseer.agent.openIn': undefined, 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'editor');
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, FIXTURE_EDIT_BARRIER: barrier, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE,FIXTURE_EDIT_BARRIER' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const setTheme = async t => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = t; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(2500); };

    // What the window has open: its title, its Explorer root and how many windows there are.
    const windowState = async () => ({
      title: await cdp.evalWorkbench('document.title'),
      windows: (await cdp.call('Target.getTargets')).targetInfos.filter(t => t.type === 'page' && /workbench/.test(t.url)).length,
    });
    const window0 = await windowState();
    const repoBefore = { a: fs.readFileSync(path.join(repo, 'a.txt'), 'utf8'), b: fs.readFileSync(path.join(repo, 'b.txt'), 'utf8'), status: git(repo, 'status', '--porcelain') };

    // The editor groups: each one's active tab, and for a text editor its annotations and lines.
    const groups = () => cdp.evalWorkbench(`(() => [...document.querySelectorAll('.editor-group-container')].filter(g => g.offsetParent).map(g => {
      const tab = g.querySelector('.tab.active');
      const ed = [...g.querySelectorAll('.monaco-editor')].find(e => e.offsetParent && !e.closest('.monaco-diff-editor'));
      // Text a decoration adds after a line (VS Code renders it as injected text in the line, or as ::after content).
      const content = () => {
        if (!ed) return [];
        const out = [];
        for (const span of ed.querySelectorAll('.view-lines span[class*="ced-"]')) {
          const c = getComputedStyle(span, '::after').content;
          if (c && c !== 'none' && c !== 'normal') out.push(c.replace(/^"|"$/g, ''));
          else if (span.children.length === 0 && /^\s*(was:|replaced|− \d)/.test(span.textContent)) out.push(span.textContent.trim());
        }
        return out;
      };
      const numbers = ed ? [...ed.querySelectorAll('.line-numbers')].map(n => Number(n.textContent)).filter(Boolean) : [];
      // Lines tinted by a decoration (the agent's added and changed lines): overlay cells with a background.
      const tinted = ed ? [...ed.querySelectorAll('.view-overlays > div > div')].filter(d => /ced-/.test(d.className) && !/rgba\\(0, 0, 0, 0\\)|transparent/.test(getComputedStyle(d).backgroundColor)).length : 0;
      return { tab: tab?.getAttribute('aria-label') || '', title: (tab?.getAttribute('aria-label') || '').split(',')[0], active: g.classList.contains('active'), tabs: [...g.querySelectorAll('.tab')].map(t => t.querySelector('.label-name')?.textContent || t.getAttribute('aria-label')),
        editor: !!ed, after: content(), tinted, first: numbers.length ? Math.min(...numbers) : 0, last: numbers.length ? Math.max(...numbers) : 0 };
    }))()`);
    const cursor = () => cdp.evalWorkbench(`(() => { const t = [...document.querySelectorAll('.statusbar-item')].map(e => e.textContent).find(x => /Ln \\d+, Col \\d+/.test(x)); const m = /Ln (\\d+), Col (\\d+)/.exec(t || ''); return m ? { line: Number(m[1]), col: Number(m[2]) } : null; })()`);
    const headGroup = async file => (await groups()).find(g => g.editor && (!file || g.title === file));
    const waitHead = async (file, extra = () => true, ms = 30000) => {
      for (let t = 0; t < ms; t += 250) { const g = await headGroup(file); if (g && extra(g)) return g; await delay(250); }
      return headGroup(file);
    };
    const worktreeRows = () => cdp.evalWorkbench(`(() => {
      const pane = [...document.querySelectorAll('.pane')].find(p => /^Worktree/i.test(p.querySelector('.pane-header .title')?.textContent.trim() || ''));
      if (!pane) return null;
      return { description: pane.querySelector('.pane-header .description')?.textContent || '', rows: [...pane.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).map(r => {
        const label = r.querySelector('.monaco-icon-label');
        const badge = label ? getComputedStyle(label, '::after').content.replace(/^"|"$/g, '') : '';
        return { name: r.querySelector('.label-name')?.textContent || '', badge: badge === 'none' ? '' : badge, selected: r.classList.contains('selected'),
          color: getComputedStyle(r.querySelector('.label-name') || r).color, title: [label?.title, label?.getAttribute('aria-label'), r.getAttribute('aria-label')].filter(Boolean).join(' | ') };
      }) };
    })()`);

    // Overseer's conversation, with some history: a question and Overseer's answer.
    await cdp.command('Overseer: Open Overseer View'); await delay(2000);
    let home = await s.editorView(`!!document.querySelector('#home-conv')`);
    await home.eval(`window.overseerApi.postMessage({ type: 'overseerSend', text: 'what is everyone doing?' })`);
    await home.waitFor(`document.querySelectorAll('#home-conv .home-msg.from-overseer').length >= 1`, 45000).catch(() => {});
    // The agent, started by hand: its card appears in the conversation.
    const task = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title: 'Head agent',
      prompt: 'edit a.txt:40; edit b.txt:120-121; add a.txt:200; remove b.txt:250-252' });
    const runId = task.run.id;
    await home.waitFor(`!!document.querySelector('#home-conv .card[data-run=${JSON.stringify(runId)}]')`, 30000);
    const conversation = () => home.eval(`({ mode: window.__overseer.mode(), items: [...document.querySelectorAll('#home-conv .home-msg, #home-conv .card, #home-conv .proposal')].map(e => e.textContent.trim().slice(0, 80)),
      visible: !document.querySelector('.view-composer').hidden, back: (() => { const b = document.getElementById('home-back-agent'); return b && !b.hidden ? b.textContent : ''; })() })`);
    const conv0 = await conversation();
    s.note('conversation before', conv0);

    // ---- AC-257: opened from the conversation, the agent's head sits beside it.
    const card = await s.webviewPoint(home, `#home-conv .card[data-run=${JSON.stringify(runId)}]`);
    await cdp.click(card.x, card.y);
    let head = await waitHead(undefined, () => true, 30000);
    await delay(1500);
    const conv1 = await conversation();
    const g1 = await groups();
    check('AC-257: opening the agent from its card in the conversation opens its head (a file of its worktree) beside the conversation, which keeps its tab and its history',
      !!head && g1.length === 2 && g1.some(g => /^Overseer/.test(g.tab)) && conv1.mode === 'composer' && conv1.visible && JSON.stringify(conv1.items) === JSON.stringify(conv0.items), { groups: g1, conv1 });
    await s.openOverseerView();
    const rows = await worktreeRows();
    check('AC-233: the Worktree view shows the agent\'s worktree (its files, named after the agent)',
      !!rows && /Head agent/.test(rows.description) && ['a.txt', 'b.txt', 'README.md'].every(n => rows.rows.some(r => r.name === n)), rows);
    await s.screenshot('opened-from-conversation');

    // ---- AC-233: Follow goes to the file the agent edits, at the line, annotated.
    go(1);
    head = await waitHead('a.txt', g => g.after.some(a => /^was: L40: original/.test(a)), 30000);
    const c1 = await cursor();
    check('AC-233: Follow opens the file the agent is editing (a.txt) at the changed line, the line tinted and saying what it was',
      !!head && head.first <= 40 && head.last >= 40 && head.tinted >= 1 && head.after.some(a => /^was: L40: original/.test(a)) && c1?.line === 40, { head, cursor: c1 });
    const rows1 = await worktreeRows();
    // Marked: git's or Overseer's "M", or the changed colour (Overseer leaves the letter to git when git knows the worktree).
    const plainColor = rows1?.rows.find(r => r.name === 'c.txt')?.color;
    check('AC-233: the changed file is marked in the Worktree view', rows1?.rows.some(r => r.name === 'a.txt' && (/M/.test(r.badge) || r.color !== plainColor)), rows1?.rows.filter(r => /txt|md/.test(r.name)));
    for (const [i, theme] of THEMES.entries()) {
      if (i) await setTheme(theme);
      await s.screenshot(`follow-${theme.toLowerCase().replace(/\s+/g, '-')}`);
    }
    await setTheme(THEMES[0]);

    go(2);
    head = await waitHead('b.txt', g => g.after.filter(a => /^was: L12[01]: original/.test(a)).length === 2, 30000);
    const c2 = await cursor();
    check('AC-233: Follow moves with the agent to b.txt, at its two changed lines (120 and 121)',
      !!head && head.first <= 120 && head.last >= 121 && head.after.filter(a => /^was: L12[01]: original/.test(a)).length === 2 && c2?.line === 120, { head, cursor: c2 });

    // ---- AC-233: an edit typed and saved in the head lands in the agent's worktree.
    const ws = s.ctl('state').workspaces.find(w => w.id === s.ctl('state').runs.find(r => r.id === runId).workspace_id);
    // Into the head's file (Follow never takes the focus), then line 10.
    await cdp.command('Overseer: Back to the Agent'); await delay(800);
    await cdp.command('Go to Line/Column'); await delay(300); await cdp.type('10'); await cdp.key('Enter'); await delay(400);
    await cdp.key('Home'); await cdp.type('owner edit: '); await delay(200);
    await cdp.key('s', { meta: true }); await delay(1500);
    const saved = fs.readFileSync(path.join(ws.path, 'b.txt'), 'utf8').split('\n')[9];
    check('AC-233: an edit typed in the head and saved lands in the agent\'s worktree, not the window\'s folder',
      saved === 'owner edit: L10: original' && fs.readFileSync(path.join(repo, 'b.txt'), 'utf8') === repoBefore.b, { worktree: ws.path, line10: saved });
    await delay(3500); // Follow waits while the owner is typing; the next edit is after that

    // ---- AC-233: the toggle switches to Diffs only (the review) and back.
    go(3);
    head = await waitHead('a.txt', g => g.first <= 200 && g.last >= 201 && g.tinted >= 2, 30000);
    const beforeToggle = { head: await headGroup(), cursor: await cursor() };
    await cdp.command('Overseer: Switch Between Follow and Diffs Only');
    const review = await cdp.webview(`!!document.getElementById('diffs') && document.body.dataset.runId === ${JSON.stringify(runId)}`, 30000);
    await delay(1500);
    const g3 = await groups();
    check('AC-233: the toggle switches to Diffs only: the review of what changed takes the head\'s place and the files close',
      g3.some(g => /^Review/.test(g.tab)) && !g3.some(g => /^(a|b)\.txt/.test(g.title)), g3);
    await s.screenshot('diffs-only');
    const back = await s.webviewPoint(review, '#head-follow');
    await cdp.click(back.x, back.y);
    const again = await waitHead('a.txt', () => true, 20000);
    await delay(800);
    const afterToggle = { head: await headGroup(), cursor: await cursor() };
    check('AC-233: and back to Follow, at the same file, line and scroll',
      !!again && afterToggle.head?.title === beforeToggle.head?.title && afterToggle.cursor?.line === beforeToggle.cursor?.line && afterToggle.head?.first === beforeToggle.head?.first && !(await groups()).some(g => /^Review/.test(g.tab)), { beforeToggle, afterToggle });

    go(4);
    head = await waitHead('b.txt', g => g.after.some(a => /^− 3 lines removed/.test(a)), 30000);
    check('AC-233: removed lines leave a marker where they were', head?.after.some(a => /^− 3 lines removed/.test(a)), head);
    await s.screenshot('follow-removed-lines');

    // ---- AC-257: one action to the conversation and one back, the head as it was left.
    const left = { head: await headGroup(), cursor: await cursor() };
    await cdp.key('u', { meta: true, alt: true }); await delay(1500);
    home = await s.editorView(`!!document.querySelector('#home-conv')`);
    const conv2 = await conversation();
    const g4 = await groups();
    const activeTab = g4.find(g => g.active)?.tab || '';
    check('AC-257: ⌥⌘U goes to Overseer\'s conversation (its history intact, a "Back to" chip for the agent) and leaves the agent\'s head as it was',
      /^Overseer/.test(activeTab) && conv2.mode === 'composer' && conv2.visible && conv0.items.every(i => conv2.items.includes(i)) && /Head agent/.test(conv2.back)
      && g4.some(g => g.title === left.head.title && g.first === left.head.first), { activeTab, conv2, groups: g4 });
    await s.screenshot('back-in-conversation');
    const chip = await s.webviewPoint(home, '#home-back-agent');
    await cdp.click(chip.x, chip.y); await delay(1500);
    const returned = { head: await headGroup(), cursor: await cursor(), active: (await groups()).find(g => g.active) };
    const conv3 = await conversation();
    check('AC-257: one click on the chip goes back to the agent: the same file, line and scroll, with the conversation intact beside it',
      returned.active?.title === left.head.title && returned.cursor?.line === left.cursor?.line && returned.head.first === left.head.first && JSON.stringify(conv3.items) === JSON.stringify(conv2.items) && conv3.visible,
      { left, returned, conv3: conv3.items.length });
    await s.screenshot('back-to-agent');

    // ---- AC-257 from the side bar: the Overseer tab shows the agent's chat; the way back brings it back too.
    await s.selectAgent('Head agent', { settle: 2500 });
    const chatMode = await home.eval(`window.__overseer.mode()`);
    const left2 = { head: await headGroup(), cursor: await cursor() };
    await cdp.key('u', { meta: true, alt: true }); await delay(1500);
    const conv4 = await conversation();
    const g5 = await groups();
    await s.screenshot('sidebar-agent-to-conversation');
    await cdp.key('u', { meta: true, alt: true }); await delay(1500);
    const mode5 = await home.eval(`({ mode: window.__overseer.mode(), selected: window.__overseer.selected() })`);
    const returned2 = { head: await headGroup(), cursor: await cursor() };
    check('AC-257: opened from the side bar (the Overseer tab shows its chat), ⌥⌘U shows the conversation beside the head as left, and ⌥⌘U again brings back its chat and its head as left',
      chatMode === 'chat' && conv4.mode === 'composer' && conv4.visible && g5.some(g => g.title === left2.head?.title && g.first === left2.head?.first)
      && mode5.mode === 'chat' && mode5.selected === runId && returned2.head?.title === left2.head?.title && returned2.head?.first === left2.head?.first,
      { chatMode, conv4: conv4.mode, mode5, left2, returned2 });
    await s.screenshot('sidebar-back-to-agent');

    const window1 = await windowState();
    const repoAfter = { a: fs.readFileSync(path.join(repo, 'a.txt'), 'utf8'), b: fs.readFileSync(path.join(repo, 'b.txt'), 'utf8'), status: git(repo, 'status', '--porcelain') };
    const folderOf = t => String(t).split(' — ').pop();
    check('AC-233: the window\'s own folder is unchanged (same folder in its title, one window, the folder\'s files and status untouched)',
      folderOf(window1.title) === folderOf(window0.title) && folderOf(window0.title) === 'head-repo' && window1.windows === 1 && window0.windows === 1 && JSON.stringify(repoAfter) === JSON.stringify(repoBefore), { window0, window1 });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || !result.checks.length || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
