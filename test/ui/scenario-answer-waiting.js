// Packaged-UI scenario for AC-241 (a waiting agent can always be answered), fixture runs only.
// Claude fixture agents wait on a Write permission:
// - in the chat, the reply box is enabled and its reply denies the request with the reply as the
//   note; the fixture's stdin log shows the note reached the harness as the reason;
// - an agent whose harness offers its session rule shows Allow once, Allow for this session and
//   Deny; Allow for this session is not asked again when the agent asks for Write a second time;
// - in the grid, the waiting agent's tile takes a reply the same way;
// - Overseer's proposal to an agent blocked on a permission reads "blocked on your permission" on
//   home, and its stage asks to answer it instead of saying it was sent.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('answer-waiting');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const stdinLog = path.join(s.root, 'stdin');
  fs.mkdirSync(stdinLog, { recursive: true });
  try {
    const repo = makeRepo(path.join(s.root, 'answer-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'overseer');
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, FIXTURE_STDIN_LOG_DIR: stdinLog, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE,FIXTURE_STDIN_LOG_DIR', OVERSEER_TEST_SYSTEM_HOME: path.join(s.root, 'system') });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const state = () => s.ctl('state');
    const run = id => state().runs.find(r => r.id === id);
    const waitFor = async (id, re, ms = 20000) => { for (let t = 0; t < ms; t += 300) { if (re.test(run(id)?.status || '')) return run(id).status; await delay(300); } return run(id)?.status; };
    const agent = async (mode, title) => {
      fs.writeFileSync(modeFile, mode);
      const id = s.ctl('task.create', { repo, harness: 'claude', prompt: 'write the file', title }).run.id;
      await waitFor(id, /waiting_for_user/);
      fs.writeFileSync(modeFile, 'overseer');
      return id;
    };
    const worktree = id => { const st = state(); const t = st.tasks.find(x => x.id === run(id).task_id); return st.workspaces.find(w => w.id === t.workspace_id).path; };
    const sent = id => { try { return fs.readFileSync(path.join(stdinLog, path.basename(worktree(id)) + '.log'), 'utf8').split('\n').filter(Boolean).map(l => { try { return JSON.parse(l); } catch { return {}; } }); } catch { return []; } };
    const answerOf = id => sent(id).filter(m => m.type === 'control_response').map(m => m.response.response);

    const docs = await agent('permission', 'Docs');
    const writer = await agent('permission-twice', 'Writer');
    await cdp.command('Overseer: Open Overseer View');
    const view = await s.editorView();

    // ---------- The chat: the reply box is enabled while a permission waits; the reply denies it with a note.
    await s.selectAgent('Docs');
    await view.waitFor(`document.body.dataset.mode === 'chat' && window.__overseer.selected() === ${JSON.stringify(docs)} && !document.getElementById('perm').hidden`, 15000);
    const chat = await view.eval(`({ disabled: document.getElementById('prompt').disabled, send: document.getElementById('send')?.disabled, placeholder: document.getElementById('prompt').placeholder, buttons: [...document.querySelectorAll('#perm [data-permission]')].map(b => b.dataset.permission + ':' + b.textContent) })`);
    await s.screenshot('chat-waiting');
    check('the chat\'s reply box is enabled while the agent waits on a permission, and says a reply denies it with a note', chat.disabled === false && /Deny with a note/.test(chat.placeholder) && chat.buttons.join() === 'allow:Allow once,deny:Deny', chat);
    const note = 'Not perm.txt: write docs/notes.md instead.';
    for (let i = 0; i < 3; i++) {
      const at = await s.webviewPoint(view, '#prompt'); await cdp.click(at.x, at.y);
      if (await view.waitFor(`document.activeElement?.id === 'prompt'`, 2000).then(() => true, () => false)) break;
    }
    await cdp.type(note); await delay(200);
    await cdp.key('Enter');
    const docsDone = await waitFor(docs, /completed|failed|interrupted/);
    const docsAnswer = answerOf(docs)[0] || {};
    await delay(800);
    await s.screenshot('chat-denied-with-note');
    check('Deny with a note: the note reaches the agent as the reason (fixture stdin log)', docsDone === 'completed' && docsAnswer.behavior === 'deny' && docsAnswer.message === note, { docsDone, docsAnswer });

    // ---------- Allow once, Allow for this session (the harness's rule) and Deny; the session rule is not asked again.
    await s.selectAgent('Writer');
    await view.waitFor(`window.__overseer.selected() === ${JSON.stringify(writer)} && !!document.querySelector('#perm [data-permission="always"]')`, 15000);
    const offered = await view.eval(`[...document.querySelectorAll('#perm [data-permission]')].map(b => b.dataset.permission + ':' + b.textContent + (b.title ? ' (' + b.title + ')' : ''))`);
    await s.screenshot('chat-allow-for-session');
    check('a permission offers Allow once, Allow for this session (the harness\'s rule) and Deny', offered.length === 3 && /^allow:Allow once/.test(offered[0]) && /^always:Allow for this session \(Allow and don't ask again: Write · this session\)/.test(offered[1]) && /^deny:Deny/.test(offered[2]), offered);
    await view.eval(`document.querySelector('#perm [data-permission="always"]').id = 'perm-always'`);
    // A first click from another frame can be taken by focus alone: click until the answer is sent.
    for (let i = 0; i < 3 && run(writer).status === 'waiting_for_user' && !answerOf(writer).length; i++) {
      const always = await s.webviewPoint(view, '#perm-always'); await cdp.click(always.x, always.y);
      for (let t = 0; t < 10 && !answerOf(writer).length; t++) await delay(200);
    }
    const writerDone = await waitFor(writer, /completed|failed|interrupted/);
    const evs = s.ctl('events.list', { run_id: writer, limit: 5000 }).events;
    const asked = evs.filter(e => e.kind === 'status' && e.payload.status === 'waiting_for_user').length;
    const auto = evs.filter(e => e.kind === 'permission' && e.payload.auto_allowed === 'allowed for this session').length;
    const wrote = ['one.txt', 'two.txt'].every(f => fs.existsSync(path.join(worktree(writer), f)));
    await view.waitFor(`[...document.querySelectorAll('.perm-card')].some(c => /Allowed for this session/.test(c.textContent))`, 10000).catch(() => {});
    const shown = await view.eval(`[...document.querySelectorAll('.perm-card .perm-head')].map(h => h.textContent)`);
    await s.screenshot('chat-session-allowed');
    check('Allow for this session is not asked again for the same tool (the agent asked for Write twice, the owner once)', writerDone === 'completed' && asked === 1 && auto === 1 && wrote && answerOf(writer).length === 2 && answerOf(writer).every(a => a.behavior === 'allow' && a.updatedPermissions), { writerDone, asked, auto, wrote, answers: answerOf(writer), shown });

    // ---------- The grid: the waiting agent's tile takes a reply.
    const tileAgent = await agent('permission', 'Tile asks');
    await cdp.command('Overseer: Toggle Agent Grid');
    await view.waitFor(`!!document.querySelector('.grid .tile[data-run=${JSON.stringify(tileAgent)}] .tile-perm:not([hidden])')`, 20000);
    const tileSel = `.grid .tile[data-run=${JSON.stringify(tileAgent)}]`;
    const tile = await view.eval(`(() => { const t = document.querySelector(${JSON.stringify(tileSel)}); const i = t.querySelector('.tile-input input'); i.id = 'tile-reply'; return { disabled: i.disabled, placeholder: i.placeholder, title: i.title, buttons: [...t.querySelectorAll('.tile-perm [data-permission]')].map(b => b.dataset.permission + ':' + b.textContent) }; })()`);
    await s.screenshot('grid-tile-waiting');
    check('the grid tile\'s reply box is enabled on a waiting permission agent', tile.disabled === false && /Why not\?/.test(tile.placeholder) && /Denies the request/.test(tile.title || '') && tile.buttons.join() === 'allow:Allow,deny:Deny', tile);
    const tileNote = 'Leave perm.txt alone.';
    for (let i = 0; i < 3; i++) {
      const at = await s.webviewPoint(view, '#tile-reply'); await cdp.click(at.x, at.y);
      if (await view.waitFor(`document.activeElement?.id === 'tile-reply'`, 2000).then(() => true, () => false)) break;
    }
    await cdp.type(tileNote); await delay(200);
    await cdp.key('Enter');
    const tileDone = await waitFor(tileAgent, /completed|failed|interrupted/);
    const tileAnswer = answerOf(tileAgent)[0] || {};
    check('a reply in the tile denies with the note (fixture stdin log)', tileDone === 'completed' && tileAnswer.behavior === 'deny' && tileAnswer.message === tileNote, { tileDone, tileAnswer });
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(800);

    // ---------- Overseer's message to a blocked agent: "blocked on your permission", never "Done".
    const phone = await agent('permission', 'Phone');
    s.ctl('overseer.session', {});
    const proposed = s.ctl('overseer.propose', { actions: [{ action: 'message', agent: phone, text: 'Also add tests.' }], source: 'test' });
    await cdp.key('n', { meta: true, alt: true }); await delay(900);
    await view.waitFor(`document.body.dataset.mode === 'composer' && /blocked on your permission/.test(document.body.innerText)`, 15000).catch(() => {});
    const home = await view.eval(`(() => { const p = [...document.querySelectorAll('.proposal')].pop(); return { proposal: p ? p.innerText : '', page: document.body.innerText.includes('Phone is blocked on your permission to use Write') }; })()`);
    await s.screenshot('home-blocked-proposal');
    check('a proposal to a blocked agent reads "blocked on your permission", and Overseer\'s turn is told to ask whether to answer it', /Phone is blocked on your permission to use Write: answer the permission first\?/.test(home.proposal) && /ask whether to answer the permission/.test(proposed.result || ''), { home, result: proposed.result });
    s.ctl('overseer.answer', { id: proposed.proposal, yes: true });
    await delay(1500);
    const card = s.ctl('overseer.card', { id: proposed.proposal });
    const stage = await view.eval(`[...document.querySelectorAll('.req-stage:not([hidden]), .proposal-status')].map(e => e.textContent).filter(t => /Phone/.test(t))`);
    await s.screenshot('home-blocked-after-yes');
    check('said yes anyway, the result is that the message waits for the permission, never "Done" or sent', /^Waiting on you: Phone is blocked on your permission/.test(card.result || '') && /waits until you answer it/.test(card.result || '') && !/Done/.test(card.result || ''), { result: card.result, stage });
    try { s.ctl('run.permission', { run_id: phone, request_id: String(run(phone).attention.request_id), allow: false }); } catch {}
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
