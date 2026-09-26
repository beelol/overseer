// Packaged-UI scenario for AC-78 (quiet turn endings), fixture harnesses only. Live-shaped cases:
// a Claude turn stopped mid-way (Claude Code 2.1.x answers a stop with an error result that has no
// text), a Codex exec turn stopped mid-way (SIGINT, no turn event), a harness line the parser does
// not understand, and a failed Claude turn with a reason. Stopped turns read "Stopped" (no empty
// error card, no "Failed"); the failed turn shows its reason once; the unparsed line is in the event
// log, not the chat. Screenshots in Overseer Dark and Light.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('endings');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const cli = path.join(repoRoot, 'fixtures/fake-harness/account-cli.js');
  const sys = path.join(s.root, 'desktop-home'); const next = path.join(s.root, 'next-login');
  fs.mkdirSync(sys, { recursive: true }); fs.writeFileSync(next, 'desk:pro');
  cp.execFileSync(cli, ['login'], { env: { ...process.env, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next } });
  try {
    const repo = makeRepo(path.join(s.root, 'end-repo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'window.dialogStyle': 'custom', 'window.menuStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: cli, OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next, CLAUDE_FIXTURE_MODE_FILE: modeFile, FIXTURE_CODEX_DELAY_MS: '15000',
      OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE,OVERSEER_TEST_SYSTEM_HOME,FIXTURE_LOGIN_ACCOUNT_FILE,FIXTURE_CODEX_DELAY_MS' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    const run = id => s.ctl('state').runs.find(r => r.id === id);
    const waitStatus = async (id, re, ms = 30000) => { for (let t = 0; t < ms; t += 250) { if (re.test(run(id)?.status || '')) return run(id).status; await delay(250); } return run(id)?.status; };
    const open = async title => { await cdp.command('Overseer: Switch Agent…'); await cdp.waitQuickTitle('Switch to agent'); await cdp.type(title); await delay(300); await cdp.key('Enter'); await delay(2000);
      return cdp.webview(`document.getElementById('title')?.textContent === ${JSON.stringify(title)} && !!document.querySelector('#conv .turn')`, 30000); };
    const chatState = chat => chat.eval(`({ errors: [...document.querySelectorAll('#conv .error-block')].map(e => e.innerText.trim()), foot: [...document.querySelectorAll('#conv .turn-foot .done')].map(d => d.innerText.trim()),
      statusLines: [...document.querySelectorAll('#conv .status-line')].map(l => l.innerText.trim()), unparsed: document.querySelectorAll('#conv .unparsed').length, text: document.getElementById('conv').innerText })`);
    // Stop from the chat's own Stop button.
    const stopFromChat = async chat => { await chat.waitFor(`!document.getElementById('interrupt').hidden`, 15000); const pt = await s.webviewPoint(chat, '#interrupt'); await cdp.click(pt.x, pt.y); };

    // Claude, stopped mid-turn (live-shaped).
    fs.writeFileSync(modeFile, 'stop-live');
    const c = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title: 'Claude stopped', prompt: 'Write a long list.' });
    await waitStatus(c.run.id, /running/);
    let chat = await open('Claude stopped'); await delay(800);
    await stopFromChat(chat); await waitStatus(c.run.id, /interrupted|failed|completed/); await delay(1500);
    const cs = await chatState(chat);
    await s.screenshot('claude-stopped-dark');
    check('a stopped Claude turn reads "Stopped": no empty error card, no "Failed", one ending line', cs.errors.length === 0 && cs.foot.some(f => /^Stopped/.test(f)) && !cs.foot.some(f => /Failed/.test(f)) && cs.statusLines.length === 0,
      { status: run(c.run.id).status, ...cs, text: undefined });

    // Codex exec, stopped mid-turn.
    const x = s.ctl('task.create', { repo, harness: 'codex', profile_id: 'system-codex', title: 'Codex stopped', prompt: 'Write a long list.' });
    await waitStatus(x.run.id, /running/);
    chat = await open('Codex stopped'); await delay(800);
    await stopFromChat(chat); await waitStatus(x.run.id, /interrupted|failed|completed/); await delay(1500);
    const xs = await chatState(chat);
    await s.screenshot('codex-stopped-dark');
    check('a stopped Codex turn reads "Stopped" (no "Failed", no error card)', xs.errors.length === 0 && xs.foot.some(f => /^Stopped/.test(f)) && !xs.foot.some(f => /Failed/.test(f)) && !xs.statusLines.some(l => /Failed/.test(l)),
      { status: run(x.run.id).status, ...xs, text: undefined });

    // An unparsed harness line stays out of the chat and is in the event log.
    fs.writeFileSync(modeFile, 'unparsed');
    const u = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title: 'Odd output', prompt: 'hi' });
    await waitStatus(u.run.id, /completed|failed/);
    chat = await open('Odd output');
    const us = await chatState(chat);
    const raw = s.ctl('events.list', { run_id: u.run.id, limit: 500 }).events.find(e => e.kind === 'raw_unparsed');
    await chat.eval(`document.getElementById('more').click()`); await delay(400);
    await chat.eval(`[...document.querySelectorAll('.menu .menu-item')].find(b => /Show event log/.test(b.textContent))?.click()`); await delay(800);
    const log = await chat.eval(`document.body.innerText`);
    await chat.eval(`document.getElementById('more').click()`); await delay(300);
    await chat.eval(`[...document.querySelectorAll('.menu .menu-item')].find(b => /Show conversation/.test(b.textContent))?.click()`); await delay(300);
    check('a line the parser does not understand stays out of the chat and is in the event log', !!raw && us.unparsed === 0 && !/telemetry flush skipped/.test(us.text) && /telemetry flush skipped/.test(log), { raw: raw?.payload?.text, inChat: us.unparsed });

    // A failed turn shows its reason once.
    fs.writeFileSync(modeFile, 'failed-reason');
    const f = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title: 'Failed migration', prompt: 'Run the migration.' });
    await waitStatus(f.run.id, /failed|completed/);
    chat = await open('Failed migration');
    const fsx = await chatState(chat);
    const times = (fsx.text.match(/relation users_v2 does not exist/g) || []).length;
    await s.screenshot('failed-dark');
    check('a failed turn shows its reason once', times === 1 && fsx.foot.some(x => /Failed/.test(x)), { times, foot: fsx.foot, errors: fsx.errors, statusLines: fsx.statusLines });

    // Light theme.
    const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = 'Overseer Light'; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(1500);
    await open('Claude stopped'); await s.screenshot('claude-stopped-light');
    await open('Failed migration'); await s.screenshot('failed-light');
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
