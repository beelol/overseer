// Packaged-UI scenario for the owner's first Voice Mode session (2026-09-28), with the simulated
// voice and the Claude fixture as Overseer's model and as the agents (no microphone, no paid turn):
//   AC-227  one view: the chat (home) becomes the voice view when Voice Mode is turned on and the
//           chat again when it is turned off, nothing opens below, the same cards throughout, in
//           the three themes; the Needs-you badge, its list and a click focusing the agent;
//           "handle what needs me" answering a waiting permission.
//   AC-228  a request's card and the view pass through every stage in order (thinking, waiting
//           for the yes, starting the agent, working with live activity and elapsed time, done);
//           no card shows an internal token or a raw error; answering a permission and an agent
//           finishing each clear its Needs-you row within a second.
//   AC-226  "show me the draft agent", "what did it make?" and "open the file it made", typed and
//           spoken, each show the right thing; a card click opens its agent; a single agent
//           started by a request slides the view aside, and not with the setting off.
//   AC-217  home's Voice Mode button with its shortcut, home before, during and after turning it
//           on, and the removed line.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('one-view');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const micUsers = path.join(s.root, 'mic-users');
  const THEMES = [['Overseer', 'overseer'], ['Overseer Dark', 'dark'], ['Overseer Light', 'light']];
  const base = { 'overseer.followNewRuns': false };
  try {
    const repo = makeRepo(path.join(s.root, 'site-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer', ...base });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'overseer');
    s.launch(repo, {
      OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE,FIXTURE_WORKER_MS', FIXTURE_WORKER_MS: '9000',
      OVERSEER_VOICE_SIMULATE: '1', OVERSEER_LISTENER_TEST_VOICE: '1', OVERSEER_LISTENER_TEST_MIC_USERS: micUsers,
    });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const state = () => s.ctl('state');
    const runOf = id => state().runs.find(r => r.id === id);
    const newRun = async (before, ms = 30000) => { const end = Date.now() + ms; while (Date.now() < end) { const r = state().runs.find(x => !x.parent_run_id && !before.has(x.id)); if (r) return r; await delay(200); } return null; };
    const ids = () => new Set(state().runs.map(r => r.id));
    const overseerIdle = async (ms = 30000) => { const end = Date.now() + ms; while (Date.now() < end) { const x = s.ctl('overseer.session'); if (x.run_id && !['queued', 'starting', 'running'].includes(x.run_status)) return x; await delay(200); } return null; };
    const generic = title => s.ctl('task.create', { repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sleep', args: ['900'], prompt: '', title }).run.id;
    const claude = (mode, title, prompt = 'do the work') => { fs.writeFileSync(modeFile, mode); const id = s.ctl('task.create', { repo, harness: 'claude', prompt, title }).run.id; return id; };
    const settled = async id => { for (let i = 0; i < 60 && ['queued', 'starting'].includes(runOf(id)?.status); i++) await delay(200); await delay(400); fs.writeFileSync(modeFile, 'overseer'); };

    // ---------- Talk to Overseer is home: nothing docks below (AC-227).
    await cdp.command('Overseer: Talk to Overseer'); await delay(2000);
    const view = await s.editorView(`!!document.getElementById('home') && !!document.getElementById('home-voice-toggle')`);
    const panelViews = await cdp.evalWorkbench(`[...document.querySelectorAll('.part.panel .pane-header, .part.panel .composite-bar .action-label')].map(e => e.getAttribute('aria-label') || e.textContent.trim()).filter(t => /Overseer/.test(t))`);
    const target = await view.eval(`document.querySelector('#target')?.dataset.target`);
    check('Talk to Overseer opens home with the composer talking to Overseer, and no Overseer view is docked below', target === 'overseer' && panelViews.length === 0, { target, panelViews });
    const toggle = await view.eval(`(() => { const b = document.getElementById('home-voice-toggle'); return { text: b.textContent, label: b.getAttribute('aria-label'), pressed: b.getAttribute('aria-pressed'), visible: b.checkVisibility() }; })()`);
    check('home has a visible Voice Mode button with its shortcut shown (AC-217)', toggle.visible && /Voice/.test(toggle.text) && /⌥⌘⇧V/.test(toggle.text) && /Turn Voice Mode on/.test(toggle.label) && toggle.pressed === 'false', toggle);

    // A typed question, through the composer (the owner's keyboard).
    const phone = generic('Phone');
    const inTask = () => view.eval(`document.activeElement?.id === 'task' && document.hasFocus()`);
    const typeToOverseer = async text => {
      await cdp.command('Overseer: Talk to Overseer'); await delay(900);
      if (!(await inTask())) { const p = await s.webviewPoint(view, '#task'); await cdp.click(p.x, p.y); await delay(200); }
      await view.eval(`document.querySelector('#target')?.dataset.target`) === 'overseer' || s.note('composer target was not Overseer before typing', text);
      await cdp.type(text); await delay(200); await cdp.key('Enter');
    };
    await typeToOverseer('What is everyone doing?');
    await view.waitFor(`[...document.querySelectorAll('#home-conv .home-msg.from-overseer')].some(m => /Here is what everyone is doing/.test(m.textContent))`, 40000);
    await overseerIdle();
    const cardIds = () => view.eval(`[...document.querySelectorAll('#home-conv > [data-id]')].map(e => e.dataset.id)`);
    const stageShown = () => view.eval(`(() => { const st = document.getElementById('voice-stage'); return !!st && !st.hidden && st.checkVisibility(); })()`);
    const tabs = () => cdp.evalWorkbench(`[...document.querySelectorAll('.tabs-container .tab')].map(t => t.getAttribute('aria-label') || t.textContent.trim())`);
    const onPanel = () => cdp.evalWorkbench(`(() => { const p = document.querySelector('.part.panel'); return !!p && p.offsetParent !== null && p.getBoundingClientRect().height > 40; })()`);

    // ---------- One view in the three themes: the chat, Voice Mode on, Voice Mode off (AC-227, AC-217).
    const cycles = [];
    let spokenId;
    for (const [theme, tag] of THEMES) {
      s.settings({ 'workbench.colorTheme': theme, ...base });
      await delay(1500);
      await cdp.command('Overseer: Talk to Overseer'); await delay(800);
      await view.eval(`document.getElementById('home').scrollIntoView({ block: 'start' })`);
      const chat = await cardIds();
      const tabsBefore = await tabs(), panelBefore = await onPanel();
      await s.screenshot(`chat-${tag}`);
      // On, with the button: home turns into the voice view (the mark takes the top).
      const p = await s.webviewPoint(view, '#home-voice-toggle'); await cdp.click(p.x, p.y);
      if (tag === 'overseer') { for (let i = 1; i <= 3; i++) { await delay(140); await s.screenshot(`turning-on-${i}`); } }
      await view.waitFor(`document.getElementById('voice-state')?.dataset.state === 'listening' && !!window.__voice.mark`, 30000);
      await delay(900);
      const on = { stage: await stageShown(), voice: await view.eval(`document.body.dataset.voice`), mode: await view.eval(`document.body.dataset.mode`), pressed: await view.eval(`document.getElementById('home-voice-toggle').getAttribute('aria-pressed')`), cards: await cardIds(), tabs: await tabs(), panel: await onPanel() };
      const mark = await view.eval(`(() => { const r = document.getElementById('voice-canvas').getBoundingClientRect(); return { w: r.width, top: r.top }; })()`);
      if (tag === 'overseer') {
        // A spoken request: the same cards as typed ones, in the same list.
        s.ctl('voice.set', { settle_seconds: 2 });
        spokenId = s.ctl('voice.say', { text: 'Tell Phone to use the new wire format.' }).request;
        await view.waitFor(`[...document.querySelectorAll('#home-conv .home-msg.spoken')].some(m => /use the new wire format/.test(m.textContent)) && [...document.querySelectorAll('#home-conv .done-card')].some(c => /Phone/.test(c.textContent))`, 60000);
        await delay(800);
        await s.screenshot(`voice-spoken-card-${tag}`);
      }
      await s.screenshot(`voice-on-${tag}`);
      const withSpoken = await cardIds();
      // Off: the chat again.
      const q = await s.webviewPoint(view, '#home-voice-toggle'); await cdp.click(q.x, q.y);
      await view.waitFor(`document.body.dataset.voice === 'off'`, 15000);
      await delay(700);
      const off = { stage: await stageShown(), cards: await cardIds(), tabs: await tabs(), panel: await onPanel() };
      await s.screenshot(`voice-off-${tag}`);
      const same = chat.every(id => on.cards.includes(id)) && withSpoken.every(id => off.cards.includes(id));
      cycles.push({ tag, mark, same, on: { ...on, cards: on.cards.length }, off: { ...off, cards: off.cards.length }, opened: on.tabs.length - tabsBefore.length, panelBefore });
    }
    check('turning Voice Mode on turns the chat into the voice view (the mark on top, in the same view) and off returns to the chat, in the three themes', cycles.every(c => c.on.stage && c.on.voice === 'on' && c.on.mode === 'composer' && c.on.pressed === 'true' && c.mark.w >= 120 && !c.off.stage), cycles);
    check('nothing opens below or beside: no new editor tab and no panel', cycles.every(c => c.opened === 0 && !c.on.panel && !c.off.panel), cycles.map(c => ({ tag: c.tag, opened: c.opened, panel: c.on.panel })));
    check('the same cards throughout (chat, voice on, voice off), the spoken request among them', cycles.every(c => c.same) && !!spokenId, cycles.map(c => ({ tag: c.tag, same: c.same })));
    const removedLine = await view.eval(`!document.body.innerText.includes("Stop, mute and what's running still work")`);
    check('the line "Stop, mute and what\'s running still work" is gone (AC-217)', removedLine);
    s.settings({ 'workbench.colorTheme': 'Overseer', ...base });
    await delay(1200);

    // ---------- Needs you: a badge with a count, its list, a click focuses the agent (AC-227).
    const sessions = claude('permission', 'Sessions', 'write a file'); await settled(sessions);
    for (let i = 0; i < 80 && runOf(sessions)?.status !== 'waiting_for_user'; i++) await delay(250);
    await cdp.command('Overseer: Talk to Overseer'); await delay(800);
    await view.waitFor(`!document.getElementById('home-needs').hidden && Number(document.querySelector('.home-needs-count').textContent) >= 1`, 10000);
    const badge = await view.eval(`(() => { const b = document.getElementById('home-needs'); return { count: b.querySelector('.home-needs-count').textContent, label: b.getAttribute('aria-label'), visible: b.checkVisibility() }; })()`);
    await s.screenshot('needs-badge');
    { const p = await s.webviewPoint(view, '#home-needs'); await cdp.click(p.x, p.y); }
    await view.waitFor(`!!document.querySelector('.menu[aria-label="Needs you"]')`, 5000);
    const items = await view.eval(`[...document.querySelectorAll('.menu[aria-label="Needs you"] .menu-item')].map(b => ({ label: b.querySelector('.menu-label').textContent, hint: b.querySelector('.menu-hint')?.textContent }))`);
    await s.screenshot('needs-list');
    check('Needs you is a small badge with a count, and a click pops out the short list', badge.visible && Number(badge.count) >= 1 && items.some(i => i.label === 'Sessions' && i.hint === 'Approve'), { badge, items });
    { const p = await s.webviewPoint(view, `#needs-${sessions}`); await cdp.click(p.x, p.y); }
    await view.waitFor(`window.__overseer.selected() === ${JSON.stringify(sessions)} && window.__overseer.mode() === 'chat'`, 10000).catch(() => {});
    const focusedAgent = { selected: await view.eval(`window.__overseer.selected()`), mode: await view.eval(`window.__overseer.mode()`) };
    await delay(800);
    await s.screenshot('needs-item-focuses-agent');
    check('a click on a Needs-you item focuses that agent (its chat)', focusedAgent.selected === sessions && focusedAgent.mode === 'chat', focusedAgent);

    // "Handle what needs me": the one question it needs, then the owner's yes answers it.
    await typeToOverseer('Handle what needs me');
    await view.waitFor(`[...document.querySelectorAll('#home-conv .card-needs')].some(c => /Sessions wants to change perm\\.txt\\. Allow it\\?/.test(c.textContent)) && !!document.querySelector('#home-conv .proposal:not(.answered)')`, 15000);
    await s.screenshot('handle-what-needs-me-asks');
    const stillWaiting = runOf(sessions)?.status;
    // Needs you clears within a second of the answer (AC-228): the badge, measured in the view.
    const needsCount = () => view.eval(`document.getElementById('home-needs').hidden ? 0 : Number(document.querySelector('.home-needs-count').textContent)`);
    const before = await needsCount();
    await typeToOverseer('yes');
    const t0 = Date.now();
    let answeredAt = 0; for (let i = 0; i < 100 && !answeredAt; i++) { if (runOf(sessions)?.status !== 'waiting_for_user') answeredAt = Date.now(); else await delay(50); }
    let clearedAt = 0; for (let i = 0; i < 100 && !clearedAt; i++) { if (await needsCount() < before) clearedAt = Date.now(); else await delay(50); }
    await s.screenshot('handle-what-needs-me-answered');
    check('"handle what needs me" asks about the waiting permission and the owner\'s yes answers it', stillWaiting === 'waiting_for_user' && answeredAt > 0, { stillWaiting, answered: answeredAt - t0 });
    check('answering an agent\'s permission clears its Needs-you row within a second', clearedAt > 0 && clearedAt - answeredAt <= 1000, { before, after: await needsCount(), ms: clearedAt - answeredAt });
    // An agent that needs the owner and then finishes (it is stopped) leaves Needs you within a second.
    const writer = claude('permission', 'Writer', 'write a file'); await settled(writer);
    for (let i = 0; i < 80 && runOf(writer)?.status !== 'waiting_for_user'; i++) await delay(250);
    await view.waitFor(`!document.getElementById('home-needs').hidden`, 10000);
    const withWriter = await needsCount();
    s.ctl('run.interrupt', { run_id: writer });
    let endedAt = 0; for (let i = 0; i < 200 && !endedAt; i++) { if (!['waiting_for_user', 'running'].includes(runOf(writer)?.status)) endedAt = Date.now(); else await delay(50); }
    let goneAt = 0; for (let i = 0; i < 100 && !goneAt; i++) { if (await needsCount() < withWriter) goneAt = Date.now(); else await delay(50); }
    check('an agent finishing clears its Needs-you row within a second', endedAt > 0 && goneAt > 0 && goneAt - endedAt <= 1000, { status: runOf(writer)?.status, withWriter, after: await needsCount(), ms: goneAt - endedAt });

    // ---------- A request's stages, typed (AC-228), and the view sliding aside (AC-226).
    await cdp.command('Overseer: Talk to Overseer'); await delay(800);
    await view.eval(`(() => { window.__stages = []; const note = () => { const last = [...document.querySelectorAll('#home-conv .home-msg.from-owner')].filter(m => /draft the page/.test(m.querySelector('.home-text')?.textContent || '')).pop(); const st = last && last.querySelector('.req-stage'); if (!st || st.hidden) return; const cur = st.dataset.stage + '|' + st.textContent; const prev = window.__stages[window.__stages.length - 1]; if (!prev || prev.key !== cur) window.__stages.push({ key: cur, stage: st.dataset.stage, text: st.textContent, progress: document.getElementById('home-progress').textContent, t: Date.now() }); }; new MutationObserver(note).observe(document.getElementById('home-conv'), { subtree: true, childList: true, attributes: true, characterData: true }); setInterval(note, 100); })()`);
    let known = ids();
    fs.writeFileSync(modeFile, 'worker');
    await typeToOverseer('Someone should draft the page');
    await view.waitFor(`!!document.querySelector('#home-conv .proposal:not(.answered) [data-proposal="yes"]')`, 30000);
    await delay(600);
    await s.screenshot('stage-waiting-for-yes');
    await view.eval(`document.querySelector('#home-conv .proposal:not(.answered) [data-proposal="yes"]').scrollIntoView({ block: 'center' })`); await delay(300);
    { const p = await s.webviewPoint(view, '#home-conv .proposal:not(.answered) [data-proposal="yes"]'); await cdp.click(p.x, p.y); }
    const draft = await newRun(known);
    await settled(draft.id);
    await view.waitFor(`window.__overseer.aside() && window.__overseer.selected() === ${JSON.stringify(draft.id)}`, 15000).catch(() => {});
    await view.waitFor(`window.__stages.some(x => x.stage === 'working' && /· \\d+s · /.test(x.text))`, 20000).catch(() => {});
    await delay(1500);
    const aside = await view.eval(`(() => { const c = document.querySelector('.view-chat').getBoundingClientRect(), h = document.querySelector('.view-composer').getBoundingClientRect(), m = document.getElementById('voice-canvas').getBoundingClientRect(); return { aside: document.body.dataset.aside, selected: window.__overseer.selected(), mode: window.__overseer.mode(), chatLeft: c.left, chatWidth: c.width, homeLeft: h.left, homeWidth: h.width, homeVisible: !document.querySelector('.view-composer').hidden }; })()`);
    await s.screenshot('aside-working');
    await view.waitFor(`window.__stages.some(x => x.stage === 'done')`, 40000).catch(() => {});
    await delay(600);
    await s.screenshot('aside-done');
    const stages = await view.eval(`window.__stages.map(x => ({ stage: x.stage, text: x.text, progress: x.progress }))`);
    const order = ['thinking', 'waiting', 'starting', 'working', 'done'];
    const seen = stages.map(x => x.stage).filter((x, i, a) => a.indexOf(x) === i);
    const inOrder = order.every(st => seen.includes(st)) && seen.filter(x => order.includes(x)).every((x, i, a) => i === 0 || order.indexOf(x) > order.indexOf(a[i - 1]));
    const working = stages.find(x => x.stage === 'working' && /· \d+s · /.test(x.text));
    check('a typed request\'s card passes through every stage in order: thinking, waiting for the yes, starting the agent, working, done', inOrder, { seen, stages });
    check('while working, the card and the view show the agent\'s live activity and the elapsed time', !!working && /draft the page is working · \d+s · (Write|Read|Editing|Drafting)/i.test(working.text) && /working/.test(working.progress), working);
    check('a request that starts a single agent slides the view aside: the agent\'s chat on the left, the view with the mark on the right', aside.aside === '1' && aside.selected === draft.id && aside.mode === 'chat' && aside.homeVisible && aside.homeLeft > aside.chatLeft && aside.homeLeft >= aside.chatLeft + aside.chatWidth - 2, aside);

    // ---------- Overseer moves you around VS Code, typed (AC-226).
    const selected = () => view.eval(`window.__overseer.selected()`);
    const activeTab = () => cdp.evalWorkbench(`(() => { const t = document.querySelector('.editor-group-container.active .tab.active') || document.querySelector('.tab.active'); return t ? (t.getAttribute('aria-label') || t.textContent.trim()) : ''; })()`);
    const allTabs = () => tabs();
    const goHome = async () => { await cdp.command('Overseer: New Agent'); await delay(900); };
    const nav = [];
    // Closes the draft's file tabs between tries, so each "open the file it made" opens it anew.
    const closeFileTabs = () => cdp.evalWorkbench(`(() => { let n = 0; for (const t of [...document.querySelectorAll('.tabs-container .tab')]) { if (/draft\\.md/.test(t.getAttribute('aria-label') || t.textContent)) { const x = t.querySelector('.tab-actions .action-label'); if (x) { x.click(); n++; } } } return n; })()`);
    const lookFor = async (how, say) => {
      await closeFileTabs(); await delay(300);
      await goHome();
      if (how === 'typed') await typeToOverseer(say); else s.ctl('voice.say', { text: say });
    };
    await lookFor('typed', 'Show me the draft agent');
    await view.waitFor(`window.__overseer.selected() === ${JSON.stringify(draft.id)} && window.__overseer.mode() === 'chat'`, 30000).catch(() => {});
    nav.push({ how: 'typed', say: 'show me the draft agent', selected: await selected(), mode: await view.eval(`window.__overseer.mode()`) });
    await delay(700); await s.screenshot('typed-show-me-the-draft-agent');
    await lookFor('typed', 'What did it make?');
    await cdp.waitFor(`[...document.querySelectorAll('.tabs-container .tab')].some(t => /^Review/.test(t.getAttribute('aria-label') || t.textContent.trim()))`, 30000).catch(() => {});
    nav.push({ how: 'typed', say: 'what did it make?', tabs: await allTabs() });
    await delay(700); await s.screenshot('typed-what-did-it-make');
    await lookFor('typed', 'Open the file it made');
    await cdp.waitFor(`[...document.querySelectorAll('.tabs-container .tab')].some(t => /draft\\.md/.test(t.getAttribute('aria-label') || t.textContent))`, 30000).catch(() => {});
    nav.push({ how: 'typed', say: 'open the file it made', tabs: await allTabs() });
    await delay(700); await s.screenshot('typed-open-the-file-it-made');
    // The same, spoken.
    await goHome();
    { const p = await s.webviewPoint(view, '#home-voice-toggle'); await cdp.click(p.x, p.y); }
    await view.waitFor(`document.getElementById('voice-state')?.dataset.state === 'listening'`, 30000);
    await lookFor('spoken', 'Show me the draft agent.');
    await view.waitFor(`window.__overseer.selected() === ${JSON.stringify(draft.id)} && window.__overseer.mode() === 'chat'`, 30000).catch(() => {});
    nav.push({ how: 'spoken', say: 'show me the draft agent', selected: await selected(), mode: await view.eval(`window.__overseer.mode()`) });
    await delay(700); await s.screenshot('spoken-show-me-the-draft-agent');
    await lookFor('spoken', 'What did it make?');
    await cdp.waitFor(`[...document.querySelectorAll('.tabs-container .tab')].some(t => /^Review/.test(t.getAttribute('aria-label') || t.textContent.trim()))`, 30000).catch(() => {});
    nav.push({ how: 'spoken', say: 'what did it make?', tabs: await allTabs() });
    await delay(700); await s.screenshot('spoken-what-did-it-make');
    await lookFor('spoken', 'Open the file it made.');
    await cdp.waitFor(`[...document.querySelectorAll('.tabs-container .tab')].some(t => /draft\\.md/.test(t.getAttribute('aria-label') || t.textContent))`, 30000).catch(() => {});
    nav.push({ how: 'spoken', say: 'open the file it made', tabs: await allTabs() });
    await delay(700); await s.screenshot('spoken-open-the-file-it-made');
    const ok = n => n.say.startsWith('show me') ? n.selected === draft.id && n.mode === 'chat' : n.say.startsWith('what did') ? n.tabs.some(t => /^Review/.test(t)) : n.tabs.some(t => /draft\.md/.test(t));
    check('typed and spoken, "show me the draft agent", "what did it make?" and "open the file it made" focus or open the right thing', nav.length === 6 && nav.every(ok), nav);
    const noYes = s.ctl('overseer.session').proposals.filter(p => (p.actions || []).some(a => ['focus', 'show_work', 'open_file'].includes(a.action)));
    check('these are Look actions: none of them waited for a yes', noYes.length === 0, noYes);

    // ---------- A card click opens its agent (AC-226).
    await goHome();
    await view.eval(`[...document.querySelectorAll('#home-conv .done-card .card-row-agent')].find(b => /draft the page/.test(b.textContent))?.scrollIntoView({ block: 'center' })`); await delay(300);
    const rowSel = `#home-conv .done-card .card-row[data-run="${draft.id}"] .card-row-agent`;
    { const p = await s.webviewPoint(view, rowSel); await cdp.click(p.x, p.y); }
    // AC-257: the agent opens beside the conversation, which keeps the Overseer tab (its "Back to" chip names the agent).
    await view.waitFor(`document.getElementById('home-back-agent')?.dataset.run === ${JSON.stringify(draft.id)}`, 10000).catch(() => {});
    const clicked = { back: await view.eval(`document.getElementById('home-back-agent')?.dataset.run`), mode: await view.eval(`window.__overseer.mode()`) };
    await delay(600); await s.screenshot('card-click-opens-agent');
    check('clicking a request card opens the agent it started (beside the conversation, AC-257)', clicked.back === draft.id && clicked.mode === 'composer', clicked);

    // ---------- With "Show the agent I start" off, the view stays as it is.
    s.settings({ 'workbench.colorTheme': 'Overseer', ...base, 'overseer.showStartedAgent': false });
    await delay(1500);
    { const p = await s.webviewPoint(view, '#home-voice-toggle').catch(() => null); if (p && await view.eval(`document.body.dataset.voice === 'on' && document.body.dataset.mode === 'composer'`)) { await cdp.click(p.x, p.y); await delay(600); } }
    known = ids();
    fs.writeFileSync(modeFile, 'worker');
    await typeToOverseer('Someone should write the notes');
    await view.waitFor(`!!document.querySelector('#home-conv .proposal:not(.answered) [data-proposal="yes"]')`, 30000);
    await view.eval(`document.querySelector('#home-conv .proposal:not(.answered) [data-proposal="yes"]').scrollIntoView({ block: 'center' })`); await delay(300);
    { const p = await s.webviewPoint(view, '#home-conv .proposal:not(.answered) [data-proposal="yes"]'); await cdp.click(p.x, p.y); }
    const notes = await newRun(known);
    await settled(notes.id);
    await delay(3000);
    const stayed = { aside: await view.eval(`document.body.dataset.aside || ''`), mode: await view.eval(`window.__overseer.mode()`), selected: await selected() };
    await s.screenshot('setting-off-stays');
    check('with "Show the agent I start" off, the view does not slide aside', stayed.aside === '' && stayed.mode === 'composer' && stayed.selected !== notes.id, stayed);

    // ---------- Asides are kept as context and never said "On it."; no card shows a token or a raw error.
    { const p = await s.webviewPoint(view, '#home-voice-toggle'); if (!(await view.eval(`document.body.dataset.voice === 'on'`))) { await cdp.click(p.x, p.y); await view.waitFor(`document.getElementById('voice-state')?.dataset.state === 'listening'`, 30000); } }
    await view.eval(`window.__said = []; new MutationObserver(() => window.__said.push(document.getElementById('voice-said').textContent)).observe(document.getElementById('voice-said'), { childList: true, characterData: true, subtree: true })`);
    const asideId = s.ctl('voice.say', { text: 'Are you done with the dishes?' }).request;
    await view.waitFor(`[...document.querySelectorAll('#home-conv .req-stage')].some(e => /Not meant for Overseer/.test(e.textContent))`, 30000).catch(() => {});
    const said = await view.eval(`window.__said`);
    await delay(500); await s.screenshot('aside-kept-as-context');
    check('a request not for Overseer never says "On it." and is kept as context in plain words', !said.some(t => /On it/.test(t)) && (s.ctl('voice.requests', { limit: 50 }).requests || []).find(r => r.id === asideId)?.state === 'not_for_overseer', { said });
    const cards = await view.eval(`document.getElementById('home-conv').innerText`);
    const tokens = cards.match(/\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b|\b[a-z]+_[a-z_]+\b|\b(?:Error|panicked|anyhow)\b|Caused by/g) || [];
    check('no card shows an internal token (NOT_FOR_OVERSEER) or a raw error', tokens.length === 0, { tokens: [...new Set(tokens)] });
    void phone;
  } catch (e) {
    s.note('ERROR ' + (e.stack || e.message));
    result.checks.push({ name: 'scenario ran', ok: false, detail: e.message });
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { s.ctl('daemon.stop_all'); } catch {}
    await s.quit();
    s.stopDaemon();
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    const failed = result.checks.filter(c => !c.ok);
    console.log(failed.length ? `${failed.length} check(s) failed` : `all ${result.checks.length} checks passed`);
    console.log(failed.length ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed.length ? 1 : 0);
  }
})();
