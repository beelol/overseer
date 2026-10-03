// Packaged UI AC-265: typed and spoken fixture messages stay paused after Stop; chat/grid owner
// controls resume FIFO, remove a chosen message or clear. Synthetic listener and Claude fixture only.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, until, repoRoot } = require('./harness');
(async () => {
  const s = new Session('queue-pause'); const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const mode = path.join(s.root, 'mode');
  try {
    const repo = makeRepo(path.join(s.root, 'queue-repo'), { dirty: false });
    fs.writeFileSync(mode, 'slow');
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' }); s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: mode, FIXTURE_SLOW_MS: '60000', FIXTURE_INTERRUPT_DELAY_MS: '1500', OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS,FIXTURE_INTERRUPT_DELAY_MS',
      OVERSEER_VOICE_SIMULATE: '1', OVERSEER_LISTENER: path.join(repoRoot, 'fixtures/fake-harness/queue-listener.js') });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer [0-9]+ active/.test(e.textContent))`, 90000, 'status bar');
    const run = s.ctl('task.create', { repo, harness: 'claude', title: 'Queue demo', prompt: 'original work' }).run.id;
    const turns = () => s.ctl('run.turns', { run_id: run });
    const queued = () => s.ctl('run.queued', { run_id: run });
    await cdp.command('Overseer: Open Overseer View'); await s.selectRun(run);
    const dash = await s.editorView();
    await dash.waitFor(`document.getElementById('title')?.textContent === 'Queue demo' && !document.getElementById('interrupt').hidden`, 20000);
    // Type the queued direction through the owner's composer.
    const point = await s.webviewPoint(dash, '#prompt'); await cdp.click(point.x, point.y);
    await cdp.type('typed direction'); await cdp.key('Enter', { alt: true });
    s.ctl('overseer.level', { level: 'auto' });
    s.ctl('voice.set', { enabled: true, target: run, delivery: 'add', settle_seconds: 0 });
    s.ctl('voice.say', { text: 'also add the spoken direction' });
    await until(() => queued().queued.length === 2, Boolean, 20000);
    check('typed AltEnter redirect and real spoken addition are queued before Stop mid-turn', queued().queued[0]?.redirect === true && queued().queued.length === 2 && s.ctl('state').runs.find(r => r.id === run)?.status === 'running');
    await dash.eval(`document.getElementById('interrupt').click()`);
    await until(() => s.ctl('state').runs.find(r => r.id === run)?.status === 'interrupted', Boolean, 20000);
    await delay(10000);
    check('Stop ends the current turn and no queued message is sent within 10 seconds', turns().length === 1 && queued().paused && s.ctl('state').runs.find(r => r.id === run)?.status === 'interrupted');
    await dash.waitFor(`document.getElementById('queued')?.dataset.paused === 'true' && document.querySelectorAll('#queued .queued-row').length === 2`, 10000);
    const chat = await dash.eval(`document.getElementById('queued').textContent`);
    check('chat shows both messages paused in order with Send queued, Clear and Remove', /Queue paused/.test(chat) && /Send queued/.test(chat) && chat.indexOf('typed direction') < chat.indexOf('spoken direction') && (chat.match(/Remove/g) || []).length === 2, chat);
    await s.screenshot('chat-paused-dark');
    await cdp.command('Overseer: Toggle Agent Grid');
    const selector = `.grid .tile[data-run="${run}"] .tile-queue`;
    await dash.waitFor(`document.querySelector(${JSON.stringify(selector)})?.dataset.paused === 'true' && document.querySelectorAll(${JSON.stringify(selector + ' .queued-row')}).length === 2`, 10000);
    const tile = await dash.eval(`document.querySelector(${JSON.stringify(selector)}).textContent`);
    check('stopped agent stays in grid with both paused messages in order', /Queue paused/.test(tile) && tile.indexOf('typed direction') < tile.indexOf('spoken direction'), tile);
    await s.screenshot('grid-paused-dark');
    fs.writeFileSync(mode, 'echo');
    await dash.eval(`document.querySelector(${JSON.stringify(selector + ' [data-queue-action="resume"]')}).click()`);
    await until(() => turns().length === 3 && s.ctl('state').runs.find(r => r.id === run)?.status === 'completed', Boolean, 20000);
    const sent = turns();
    check('Send queued sends first direction and spoken message follows only after its turn ends', sent[1].prompt === 'typed direction' && sent[2].prompt.includes('spoken direction') && sent[2].started_ms >= sent[1].ended_ms, sent.map(t => [t.n, t.prompt, t.status]));
    // Stop another turn, remove just the second message from chat, then Clear sends neither.
    await s.selectRun(run); await dash.waitFor(`document.getElementById('title')?.textContent === 'Queue demo'`, 10000);
    fs.writeFileSync(mode, 'slow'); s.ctl('run.follow_up', { run_id: run, prompt: 'second work' });
    await dash.waitFor(`!document.getElementById('interrupt').hidden`, 10000);
    s.ctl('run.queue', { run_id: run, text: 'keep until clear' }); s.ctl('run.queue', { run_id: run, text: 'remove only this one' });
    await dash.eval(`document.getElementById('interrupt').click()`);
    await dash.waitFor(`document.getElementById('queued')?.dataset.paused === 'true' && document.querySelectorAll('#queued .queued-row').length === 2`, 10000);
    await dash.eval(`document.querySelectorAll('#queued .queued-row [data-queue-action="cancel"]')[1].click()`);
    await until(() => queued().queued.length === 1, Boolean, 10000);
    check('Remove removes only the chosen message', queued().queued.length === 1 && queued().queued[0].text === 'keep until clear');
    await dash.eval(`document.querySelector('#queued [data-queue-action="clear"]').click()`);
    await until(() => queued().queued.length === 0, Boolean, 10000);
    await delay(10000);
    check('Clear retains pause and sends nothing', turns().length === 4 && queued().paused && queued().queued.length === 0);
    await s.screenshot('chat-cleared-paused');
  } catch (error) { result.error = error.message; s.note('ERROR ' + (error.stack || error.message)); try { await s.screenshot('error'); } catch {} }
  finally { s.writeLog(); fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2)); await s.quit(); s.stopDaemon(); const failed = result.error || result.checks.some(c => !c.ok); console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root); process.exit(failed ? 1 : 0); }
})();
