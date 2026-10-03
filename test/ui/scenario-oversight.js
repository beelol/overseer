// Packaged-UI scenario for Gate S's surfaces (AC-185, AC-187, AC-191, AC-193, AC-194, AC-199),
// fixture harnesses only (no paid tokens): what Overseer does to agents, on every VS Code surface
// and in the three Overseer themes. A held agent in the side bar, the grid and its chat; a watch
// on both agents (the subject watched, an idle agent the owner named watching) with a finding in
// both chats and in the conversation; a large diff shared as a patch file and a branch and commit,
// its card with a row per agent (delivery, state, time), then withdrawn; home with those cards.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');
const { initialSettings, settingOverride, runSettings } = require('./settings-qualification');

(async () => {
  const s = new Session('oversight');
  const result = { checks: [], screenshots: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const shot = async label => { await s.screenshot(label); result.screenshots.push(label); };
  try {
    const repo = makeRepo(path.join(s.root, 'oversight-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'echo');
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    initialSettings(s, result);
    const run = id => s.ctl('state').runs.find(r => r.id === id);
    const done = async id => { for (let i = 0; i < 80 && !['completed', 'failed', 'interrupted'].includes(run(id).status); i++) await delay(250); };
    const overseerIdle = async () => { for (let i = 0; i < 160; i++) { const x = s.ctl('overseer.session'); if (x.run_id && !['queued', 'starting', 'running'].includes(x.run_status)) return x; await delay(250); } throw new Error('Overseer stayed busy'); };

    // Overseer's conversation exists (the marks show once it does), and nothing checks in by itself.
    fs.writeFileSync(modeFile, 'overseer');
    s.ctl('agent.cadence', { cadence: 'off', by: 'owner' });
    settingOverride(s, result, 'check_ins', 'off', 'Oversight intentionally isolates holds, watches, shares and conflicts from cadence turns.');
    s.ctl('overseer.send', { text: 'What is everyone doing?', surface: 'ctl', harness: 'claude' });
    await overseerIdle();
    fs.writeFileSync(modeFile, 'echo');

    // A held agent, working (so it has a tile in the grid).
    const sessions = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo refreshing sessions; sleep 900'], prompt: '', title: 'Sessions' }).run.id;
    s.ctl('agent.hold', { run_id: sessions, reason: 'wait for the review', by: 'owner' });
    // A watch: the subject is working; an idle agent the owner named watches it and files a concern.
    const build = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo building the docs; echo hmm, not sure about the tests; sleep 900'], prompt: '', title: 'Docs build' }).run.id;
    const reviewer = s.ctl('task.create', { repo, harness: 'claude', prompt: 'hello', title: 'Reviewer' }).run.id;
    await done(reviewer);
    runSettings(s, result, reviewer);
    const watch = s.ctl('watch.start', { subject: build, watcher: reviewer, brief: 'the tests stay in place', by: 'owner' });
    const token = s.ctl('overseer.token', { run_id: reviewer, role: 'agent' }).token;
    s.ctl('overseer.tool', { token, name: 'finding', arguments: { result: 'concern', text: 'it sounds unsure about the tests' } });
    // A large diff shared, then withdrawn: a patch file and a branch and commit; the card's rows.
    const big = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', "yes 'a line of text for the patch' | head -c 102400 > big.txt"], prompt: '', title: 'Big change' }).run.id;
    const reader = s.ctl('task.create', { repo, harness: 'claude', prompt: 'hello', title: 'Reader' }).run.id;
    await done(big); await done(reader);
    await overseerIdle();
    for (const p of s.ctl('overseer.session').proposals) s.ctl('overseer.answer', { id: p.id, yes: false, surface: 'ctl', by: 'owner' });
    s.ctl('overseer.level', { level: 'steer' });
    s.ctl('overseer.send', { text: 'Share what Big change did with Reader', surface: 'ctl' });
    await overseerIdle();
    const shareProposal = s.ctl('overseer.propose', { actions: [{ action: 'share', to: reader, from: big, what: 'diff', why: 'named' }], source: 'ctl' }).proposal;
    let share;
    for (let i = 0; i < 60 && !(share = s.ctl('share.list', { run_id: reader }).shares.find(x => x.from === big)); i++) await delay(250);
    await done(reader);
    check('a large diff is shared as a patch file and as a branch and commit', !!share && !!share.file && /^overseer\/share\//.test(share.branch || '') && !!share.commit, share);
    const card = s.ctl('overseer.card', { id: shareProposal });
    check('the share\'s card has a row for the agent with its delivery, state and times', card.rows.length === 1 && card.rows[0].run_id === reader && card.rows[0].delivery && card.rows[0].sent_ms, card.rows);
    s.ctl('share.withdraw', { id: share.id, by: 'owner' });
    await done(reader);
    s.ctl('overseer.level', { level: 'ask_first' });
    for (const p of s.ctl('overseer.session').proposals) s.ctl('overseer.answer', { id: p.id, yes: false, surface: 'ctl', by: 'owner' });

    const state = s.ctl('state');
    check('the daemon\'s one state says held, watched and watching', state.oversight[sessions]?.held && state.oversight[build]?.watched && (state.oversight[reviewer]?.watching || []).includes(build), { sessions: state.oversight[sessions], build: state.oversight[build], reviewer: state.oversight[reviewer] });

    // The theme through the user's settings (VS Code applies it at once; the picker's filter can
    // fall through to the Marketplace); the change is read back from the workbench.
    const setTheme = async theme => {
      const bg = `getComputedStyle(document.querySelector('.part.activitybar') || document.body).backgroundColor`;
      const before = await cdp.evalWorkbench(bg);
      s.settings({ 'workbench.colorTheme': theme, 'overseer.followNewRuns': false });
      const want = /Light/.test(theme) ? 'vs' : 'vs-dark';
      await cdp.waitFor(`(() => { const w = document.querySelector('.monaco-workbench'); return !!w && w.classList.contains(${JSON.stringify(want)}) && ${bg} !== ${JSON.stringify(before)}; })()`, 20000, 'theme ' + theme);
      await delay(1500);
    };
    for (const theme of ['Overseer', 'Overseer Dark', 'Overseer Light']) {
      if (theme !== 'Overseer') await setTheme(theme);
      const t = theme.toLowerCase().replace(/\s+/g, '-');
      // The side bar: held, watched, watching.
      await s.openOverseerView(); await delay(1200);
      const rows = await s.agentRows();
      const desc = title => (rows.find(r => r.label === title) || {}).description || '';
      check(`${theme}: the side bar marks the held, the watched and the watching agent, and lists no run of Overseer's own`, /held/.test(desc('Sessions')) && /watched/.test(desc('Docs build')) && /watching/.test(desc('Reviewer')) && !rows.some(r => r.label === 'Talk to Overseer'), rows.map(r => [r.label, r.description]));
      // The held agent's chat.
      await s.selectAgent('Sessions', { settle: 2000 });
      let view = await s.editorView(`[...document.querySelectorAll('#conv .sys.oversight')].some(e => /Held by Overseer/.test(e.textContent))`);
      await shot(`held-chat-${t}`);
      check(`${theme}: the held agent's chat says it is held and why`, true);
      // The grid: the held and the watched tiles.
      await cdp.command('Overseer: Toggle Agent Grid'); await delay(1500);
      view = await s.editorView(`document.querySelectorAll('.grid .tile').length >= 2`);
      const tiles = await view.eval(`[...document.querySelectorAll('.grid .tile')].map(t => ({ title: t.querySelector('.tile-title, .who, .title')?.textContent || t.textContent.slice(0, 40), held: t.classList.contains('held'), watched: t.classList.contains('watched'), marks: t.querySelector('.tile-marks')?.textContent || '' }))`);
      await shot(`grid-${t}`);
      check(`${theme}: the grid's tiles show held and watched`, tiles.some(x => x.held && /held/.test(x.marks)) && tiles.some(x => x.watched && /watched/.test(x.marks)), tiles);
      await cdp.command('Overseer: Toggle Agent Grid'); await delay(1200);
      // The subject's chat and the watcher's: the watch and the finding on both.
      await s.selectAgent('Docs build', { settle: 2000 });
      await s.editorView(`[...document.querySelectorAll('#conv .sys.oversight')].some(e => /Reviewer: concern/.test(e.textContent))`);
      await shot(`watched-chat-${t}`);
      await s.selectAgent('Reviewer', { settle: 2000 });
      view = await s.editorView(`[...document.querySelectorAll('#conv .sys.oversight')].some(e => /Watched: the tests stay in place/.test(e.textContent))`);
      const watcherLines = await view.eval(`[...document.querySelectorAll('#conv .sys.oversight')].map(e => e.textContent.trim())`);
      await shot(`watcher-chat-${t}`);
      check(`${theme}: the watcher's chat shows the watch and its finding`, watcherLines.some(l => /concern/.test(l)), watcherLines);
      // The reader's chat: the share and its withdrawal.
      await s.selectAgent('Reader', { settle: 2000 });
      view = await s.editorView(`[...document.querySelectorAll('#conv .sys.oversight')].some(e => /A share was withdrawn/.test(e.textContent))`);
      const readerLines = await view.eval(`[...document.querySelectorAll('#conv .sys.oversight')].map(e => e.textContent.trim())`);
      await shot(`share-chat-${t}`);
      check(`${theme}: the receiving agent's chat shows the share and its withdrawal`, readerLines.some(l => /Overseer shared from Big change/.test(l)) && readerLines.some(l => /withdrawn/.test(l)), readerLines);
      // Home: the watch, the finding, the share's card with its row, the withdrawal.
      await cdp.command('Overseer: New Agent'); await delay(1500);
      view = await s.editorView(`!!document.querySelector('#home-conv .done-card .card-row')`);
      const home = await view.eval(`({ kinds: [...document.querySelectorAll('#home-conv .card')].map(c => c.dataset.kind), rows: [...document.querySelectorAll('#home-conv .done-card .card-row')].map(r => r.innerText.replace(/\\s+/g, ' ')) })`);
      await view.eval(`document.querySelector('#home-conv .done-card:last-of-type')?.scrollIntoView({ block: 'center' })`); await delay(300);
      await shot(`home-cards-${t}`);
      check(`${theme}: home shows the watch, the finding, the withdrawal and the share's card with its row`, ['watch', 'finding', 'withdrawn'].every(k => home.kinds.includes(k)) && home.rows.some(r => /Reader/.test(r) && /\d\d:\d\d/.test(r)), home);
    }
    s.ctl('watch.end', { id: watch.id, by: 'owner' });
    for (const id of [sessions, build]) s.ctl('run.interrupt', { run_id: id });
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
