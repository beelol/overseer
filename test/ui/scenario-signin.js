// Packaged-UI scenario for AC-11 with a SYNTHETIC account CLI (no real logins touched):
// a new Claude account is added and named through the UI and starts with a missing login
// (shown, and not offered as a usable tile in New Task); it is signed in through its own flow;
// a task runs with it; the login then "expires"; the next turn fails with a classified auth
// error whose "Sign in again" button reauthenticates that account; the follow-up then works.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('signin');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const cli = path.join(repoRoot, 'fixtures/fake-harness/account-cli.js');
  const sys = path.join(s.root, 'desktop-home'); const next = path.join(s.root, 'next-login');
  fs.mkdirSync(sys, { recursive: true });
  try {
    const repo = makeRepo(path.join(s.root, 'signin-demo'), { dirty: false });
    s.settings({ 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: cli, OVERSEER_CLAUDE_PATH: cli, OVERSEER_TEST_SYSTEM_HOME: sys, FIXTURE_LOGIN_ACCOUNT_FILE: next,
      OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    await s.openOverseerView();
    const accountRow = name => cdp.evalWorkbench(`(() => { const r = [...document.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent && (r.querySelector('.label-name')?.textContent || '').trim() === ${JSON.stringify(name)}); return r ? (r.querySelector('.label-description')?.textContent || '') : null; })()`);
    const refreshUntil = async (name, re) => { let last; for (let i = 0; i < 8; i++) { await cdp.command('Overseer: Refresh Account Status'); await delay(2000); last = await accountRow(name); if (re.test(last || '')) return last; } return last; };

    // Add and name a Claude account; skip signing in for now (missing login).
    await cdp.command('Overseer: Add Account');
    await cdp.pick('Add account: provider', 'Anthropic');
    await cdp.input('Name for the Anthropic', 'Claude work');
    await delay(1500);
    await cdp.command('Notifications: Clear All Notifications');
    const missing = await refreshUntil('Claude work', /not signed in/);
    await cdp.command('Overseer: New Task');
    const form = await cdp.webview(`document.body.dataset.ready === '1' && !!document.getElementById('harnesses')`, 30000);
    await form.waitFor(`document.querySelectorAll('#harnesses .tile').length >= 3`, 20000);
    await form.eval(`[...document.querySelectorAll('#harnesses .tile')].find(t => /Claude Code/.test(t.textContent)).click()`);
    await delay(500);
    const tile = await form.eval(`(() => { const t = [...document.querySelectorAll('#accounts .tile')].find(t => /Claude work/.test(t.textContent)); return t && { disabled: t.getAttribute('aria-disabled') === 'true', why: t.querySelector('.tile-why')?.textContent, pill: t.querySelector('.pill')?.textContent }; })()`);
    await s.screenshot('missing-login');
    check('a missing login is shown in Accounts and the account tile is disabled with the reason', /not signed in/.test(missing || '') && tile?.disabled && /Sign in first/.test(tile.why || ''), { missing, tile });

    // Sign in through the UI (the account's own login flow).
    fs.writeFileSync(next, 'claudia:max');
    await cdp.command('Overseer: Sign In');
    await cdp.pick('', 'Claude work');
    const signed = await refreshUntil('Claude work', /signed in · max/);
    check('sign in through the UI', /signed in · max/.test(signed || ''), signed);
    const profile = s.ctl('profile.list').find(p => p.name === 'Claude work');
    const created = s.ctl('task.create', { repo, harness: 'claude', profile_id: profile.id, prompt: 'say hello', title: 'signin run' });
    const runState = id => s.ctl('state').runs.find(r => r.id === id);
    const waitDone = async id => { for (let i = 0; i < 60 && ['queued', 'starting', 'running'].includes(runState(id).status); i++) await delay(300); return runState(id); };
    const first = await waitDone(created.run.id);
    check('a task runs with the signed-in account', first.status === 'completed', { status: first.status });

    // The login expires (credential gone); the next turn fails with a classified auth error.
    fs.rmSync(path.join(profile.home, 'claude/.fixture-login.json'));
    s.ctl('run.follow_up', { run_id: created.run.id, prompt: 'again' });
    const failed = await waitDone(created.run.id);
    await s.openOverseerView();
    await cdp.command('Overseer: Open Overseer View');
    const center = await cdp.webview(`document.body.dataset.ready === '1' && !!document.getElementById('tree')`, 30000);
    await center.eval(`[...document.querySelectorAll('#tree .row')].find(r => r.dataset.run === ${JSON.stringify(created.run.id)})?.click()`);
    const conv = await cdp.webview(`document.body.dataset.runId === ${JSON.stringify(created.run.id)} && !!document.querySelector('#conv .sign-in-again')`, 30000);
    const errText = await conv.eval(`document.querySelector('#conv .error-block').textContent`);
    await s.screenshot('expired-login');
    check('an expired login fails the turn with an auth error that offers "Sign in again"', failed.status === 'failed' && /auth/.test(errText) && /expired/.test(errText), { status: failed.status, reason: failed.exit_reason, errText });

    // Reauthenticate from the conversation, then the follow-up works.
    fs.writeFileSync(next, 'claudia-again:max');
    await conv.eval(`document.querySelector('#conv .sign-in-again').id = 'again'`);
    const p = await s.webviewPoint(conv, '#again'); await cdp.click(p.x, p.y);
    const again = await refreshUntil('Claude work', /signed in · max/);
    s.ctl('run.follow_up', { run_id: created.run.id, prompt: 'after re-sign-in' });
    const ok = await waitDone(created.run.id);
    const hello = s.ctl('events.list', { run_id: created.run.id, limit: 5000 }).events.filter(e => e.kind === 'output' && /hello from/.test(e.payload.text || '')).map(e => e.payload.text);
    check('"Sign in again" reauthenticates only that account and the follow-up then succeeds', /signed in/.test(again || '') && ok.status === 'completed' && hello.some(t => t.includes('claudia-again')), { again, status: ok.status, hello });
    await s.screenshot('after-resign-in');
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failedAll = result.error || result.checks.some(c => !c.ok);
    console.log(failedAll ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failedAll ? 1 : 0);
  }
})();
