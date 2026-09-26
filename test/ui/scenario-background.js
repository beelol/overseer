// LIVE scenario for AC-45 (one tiny Claude Haiku turn): an agent keeps running after VS Code
// closes and the user gets a macOS notification naming it; reopening VS Code shows it; the
// "Stop Agents and Daemon" command confirms, interrupts it and stops the daemon with no Overseer
// or harness processes left; with nothing running, closing VS Code sends no notification.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

const PROMPT = 'Use the Bash tool to run exactly this command in the foreground: for i in $(seq 1 24); do echo tick $i; sleep 5; done. Then reply exactly: done';

(async () => {
  const s = new Session('background');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const env = { OVERSEER_BACKGROUND_NOTICE_MS: '3000' };
  const daemonLog = () => { try { return fs.readFileSync(path.join(s.home, 'overseerd.log'), 'utf8'); } catch { return ''; } };
  const daemonPids = () => cp.spawnSync('pgrep', ['-f', `${s.extensions}/.*/overseerd-.* serve`], { encoding: 'utf8' }).stdout.trim().split('\n').filter(Boolean);
  const shimInfo = runId => {
    const dir = path.join(s.home, 'runs', runId);
    const gen = fs.readdirSync(dir).sort().pop();
    return JSON.parse(fs.readFileSync(path.join(dir, gen, 'shim.json'), 'utf8'));
  };
  const alive = pid => { try { process.kill(pid, 0); return true; } catch { return false; } };
  const toast = pattern => s.cdp.waitFor(`[...document.querySelectorAll('.notification-toast, .notifications-toasts .monaco-list-row')].map(e => e.textContent).find(t => ${pattern}.test(t)) || null`, 30000).catch(() => null);
  let run;
  try {
    const repo = makeRepo(path.join(s.root, 'bg-demo'), { dirty: false });
    s.settings({ 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, env);
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    check('VS Code window registered with the daemon', s.ctl('daemon.clients').vscode === 1, s.ctl('daemon.clients'));

    // A live Claude run that is busy for about two minutes.
    const created = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', model: 'haiku', prompt: PROMPT, title: 'tick loop' });
    run = created.run.id;
    for (let i = 0; i < 240; i++) {
      const r = s.ctl('state').runs.find(x => x.id === run);
      if (r.status === 'waiting_for_user' && r.attention?.request_id) { s.ctl('run.permission', { run_id: run, request_id: r.attention.request_id, allow: true }); await delay(1000); continue; }
      if (s.ctl('events.list', { run_id: run, limit: 5000 }).events.some(e => e.kind === 'tool' && e.payload.name === 'Bash')) break;
      if (!['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status)) break;
      await delay(500);
    }
    await delay(3000);
    const before = s.ctl('state').runs.find(x => x.id === run);
    check('live Claude run is busy before VS Code closes', before.status === 'running', { status: before.status });
    await s.screenshot('running-before-close');

    // Close VS Code (Cmd+Q). The agent keeps running and a notification names it.
    const t0 = Date.now();
    await s.quit();
    await delay(6000);
    const notices = s.ctl('events.list', { after: 0, limit: 5000 }).events.filter(e => e.kind === 'background_notice');
    const logLine = daemonLog().split('\n').find(l => l.includes('background notice ('));
    check('notification posted after the last window closed', notices.length === 1 && /(overseer-notifier|osascript) \(ok\)/.test(notices[0]?.payload.delivered_via || ''), { notice: notices[0]?.payload, logLine, afterMs: Date.now() - t0 });
    check('notification names the running agent and how to stop it', notices[0] && /claude: tick loop/.test(notices[0].payload.body) && /Stop Agents and Daemon/.test(notices[0].payload.body), notices[0]?.payload.body);
    const shot = path.join(s.evidence, '02-macos-notification.png');
    const cap = cp.spawnSync('/usr/sbin/screencapture', ['-x', shot], { encoding: 'utf8' });
    s.note('screencapture of the desktop (banner, if screen recording is permitted)', { status: cap.status, stderr: cap.stderr.trim() });
    const whileClosed = s.ctl('state').runs.find(x => x.id === run);
    check('agent keeps running while VS Code is closed', whileClosed.status === 'running', { status: whileClosed.status });

    // Reopen: the run is shown and the window says it kept running.
    s.launch(repo, env);
    cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer 1 active/.test(e.textContent))`, 60000, 'status after reopen');
    const message = await toast('/kept running while VS Code was closed/');
    check('reopening VS Code says which agents kept running', message && /claude: tick loop/.test(message), message);
    await s.openOverseerView();
    const row = await cdp.waitFor(`[...document.querySelectorAll('.monaco-list-row')].map(r => r.textContent).find(t => /claude/.test(t) && /running/.test(t)) || null`, 20000).catch(() => null);
    check('reopened Agents view shows the running agent', !!row, row);
    await s.screenshot('reopened');

    // Stop everything from the command, with confirmation.
    const shim = shimInfo(run);
    await cdp.command('Overseer: Stop Agents and Daemon');
    const dialog = await cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d) return null; const b = [...d.querySelectorAll('.monaco-button')].find(b => /Stop Agents and Daemon/.test(b.textContent)); if (!b) return null; const r = b.getBoundingClientRect(); return { text: d.textContent, x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, 20000);
    check('stop asks for confirmation and lists the agent', /Stop 1 running agent/.test(dialog.text) && /claude: tick loop/.test(dialog.text), dialog.text);
    await s.screenshot('confirm-stop');
    await cdp.click(dialog.x, dialog.y);
    for (let i = 0; i < 60 && daemonPids().length; i++) await delay(500);
    await delay(1500);
    check('daemon stopped', daemonPids().length === 0, daemonPids());
    check('harness and supervisor processes gone', !alive(shim.child_pid) && !alive(shim.shim_pid), shim);
    const leftovers = cp.spawnSync('pgrep', ['-fl', s.home], { encoding: 'utf8' }).stdout.trim();
    check('no Overseer processes remain for this home', !leftovers, leftovers);
    const status = await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].map(e => e.textContent).find(t => /Overseer stopped/.test(t)) || null`, 15000).catch(() => null);
    await delay(4000);
    check('window shows Overseer stopped and does not respawn the daemon', !!status && daemonPids().length === 0, { status, pids: daemonPids() });
    await s.screenshot('stopped');

    // Nothing running: start the daemon again, close VS Code, and expect no notification.
    await cdp.command('Overseer: Start Daemon');
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer 0 active/.test(e.textContent))`, 30000, 'restarted');
    const final = s.ctl('state').runs.find(x => x.id === run);
    check('stopped run recorded as interrupted', final.status === 'interrupted', { status: final.status, reason: final.exit_reason });
    const count = s.ctl('events.list', { after: 0, limit: 5000 }).events.filter(e => e.kind === 'background_notice').length;
    await s.quit();
    await delay(6000);
    const count2 = s.ctl('events.list', { after: 0, limit: 5000 }).events.filter(e => e.kind === 'background_notice').length;
    check('no notification when nothing is running', count2 === count && /no active agents, no notice/.test(daemonLog()), { before: count, after: count2 });
    fs.writeFileSync(path.join(s.evidence, 'overseerd.log'), daemonLog().split('\n').filter(l => /background notice|stop_all|no notice|shutdown/.test(l)).join('\n') + '\n');
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
    try { if (run) s.ctl('run.interrupt', { run_id: run }); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) await s.quit();
    s.stopDaemon();
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
