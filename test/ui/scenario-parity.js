// Packaged-UI scenario for AC-60 (native-CLI parity), fixture Claude only (no paid tokens):
// from the chat composer, a pasted image, an @-mentioned worktree file (picked from the popup) and
// per-turn options (model, reasoning effort, permission mode) reach the harness (the echo fixture
// reports its argv and message content); a message sent while the agent works is queued and sent
// when the turn ends; ⌥Enter stops the agent and sends. The new-agent composer takes effort,
// permission mode and images too. Live checks with real Claude and Codex are in the session record.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

const RED_PNG = 'iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAIAAABLbSncAAAAEklEQVR42mP4z8DAgAEYRqEAAKXxAf9L4zNXAAAAAElFTkSuQmCC';

(async () => {
  const s = new Session('parity');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'parity-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, FIXTURE_SLOW_MS: '6000', OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const run = id => s.ctl('state').runs.find(r => r.id === id);
    const echoes = id => s.ctl('events.list', { run_id: id, limit: 5000 }).events.filter(e => e.kind === 'output' && /^ECHO /.test(e.payload.text || '')).map(e => JSON.parse(e.payload.text.slice(5)));
    fs.writeFileSync(modeFile, 'echo');
    const t = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title: 'Parity demo', prompt: 'first turn' });
    for (let i = 0; i < 30 && run(t.run.id).status !== 'completed'; i++) await delay(300);
    await cdp.command('Overseer: Open Overseer View');
    const dash = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail-list .row[data-run]')`, 30000);
    await dash.eval(`document.querySelector('.rail-list .row[data-run=${JSON.stringify(t.run.id)}]').click()`);
    await dash.waitFor(`document.getElementById('title')?.textContent === 'Parity demo' && !document.getElementById('send').disabled`, 20000);

    // Paste an image into the composer.
    await dash.eval(`(() => { const b = Uint8Array.from(atob(${JSON.stringify(RED_PNG)}), c => c.charCodeAt(0)); const f = new File([b], 'red.png', { type: 'image/png' });
      const dt = new DataTransfer(); dt.items.add(f); document.getElementById('prompt').dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })); return true; })()`);
    await dash.waitFor(`!!document.querySelector('.composer-tray .attach-chip img')`, 5000);
    // @-mention: type "@READ", pick README.md from the popup with Enter.
    await dash.eval(`document.getElementById('prompt').focus()`);
    await cdp.type('Compare the image with @READ'); await delay(200);
    await dash.eval(`document.getElementById('prompt').dispatchEvent(new Event('input'))`);
    await dash.waitFor(`!document.querySelector('.mention-pop').hidden && [...document.querySelectorAll('.mention-item')].some(i => i.title === 'README.md')`, 10000);
    await cdp.key('Enter'); await delay(200);
    const text = await dash.eval(`document.getElementById('prompt').value`);
    // Options for this message: model, effort, permission mode (through the options menu).
    const pickOpt = async label => { await dash.eval(`document.querySelector('.chat .composer-tools [data-action="tune"]').click()`); await delay(250);
      await dash.eval(`[...document.querySelectorAll('.menu .menu-item')].find(b => b.querySelector('.menu-label')?.textContent === ${JSON.stringify(label)}).click()`); await delay(250); };
    await pickOpt('opus'); await pickOpt('high'); await pickOpt('Plan only');
    const tray = await dash.eval(`document.querySelector('.composer-tray').textContent`);
    await s.screenshot('composer-tools');
    await cdp.key('Enter');
    let e; for (let i = 0; i < 30; i++) { const list = echoes(t.run.id); e = list[list.length - 1]; if (list.length >= 2) break; await delay(300); }
    const after = flag => { const i = e.argv.indexOf(flag); return i >= 0 ? e.argv[i + 1] : undefined; };
    check('a pasted image reaches the agent (an image block in the message)', e && e.kinds.some(k => /^image:image\/png:/.test(k)), e?.kinds);
    check('an @-mentioned worktree file is picked from the popup and named in the message', /@README\.md/.test(text) && /Files mentioned.*`README\.md`/.test(e?.text || ''), { text, sent: e?.text });
    check('per-turn model, reasoning effort and permission mode reach the harness', after('--model') === 'opus' && after('--effort') === 'high' && after('--permission-mode') === 'plan' && /opus/.test(tray) && /high effort/.test(tray), { argv: e?.argv, tray });

    // Steering: a message sent while the agent works is queued and sent when the turn ends.
    fs.writeFileSync(modeFile, 'slow');
    await dash.eval(`document.getElementById('prompt').focus()`);
    await cdp.type('start a slow turn'); await cdp.key('Enter');
    await dash.waitFor(`!document.getElementById('interrupt').hidden`, 10000);
    await cdp.type('queued message'); await cdp.key('Enter');
    const queued = await dash.waitFor(`!document.getElementById('queued').hidden && document.getElementById('queued').textContent`, 5000).catch(() => null);
    let turns; for (let i = 0; i < 60; i++) { turns = s.ctl('run.turns', { run_id: t.run.id }); if (turns.length >= 4 && turns[3].status !== 'running') break; await delay(300); }
    check('a message sent while the agent works is queued (shown) and sent when the turn ends', /Queued/.test(queued || '') && turns.length >= 4 && turns[3].prompt === 'queued message', { queued, turns: turns.map(x => [x.n, x.prompt, x.status]) });
    // ⌥Enter: stop and send now.
    for (let i = 0; i < 40 && ['running', 'starting'].includes(run(t.run.id).status); i++) await delay(300);
    await cdp.type('another slow turn'); await cdp.key('Enter');
    await dash.waitFor(`!document.getElementById('interrupt').hidden`, 10000);
    await cdp.type('stop and do this'); await cdp.key('Enter', { alt: true });
    for (let i = 0; i < 60; i++) { turns = s.ctl('run.turns', { run_id: t.run.id }); if (turns.length >= 6) break; await delay(300); }
    check('⌥Enter stops the agent and sends the message right away', turns.length >= 6 && turns[4].status === 'interrupted' && turns[5].prompt === 'stop and do this', turns.map(x => [x.n, x.prompt, x.status]));
    await s.screenshot('steered');
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
