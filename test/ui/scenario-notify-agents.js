// Packaged-UI scenario for AC-240 (you hear about it outside VS Code), fixture runs only; no banner
// is shown: the daemon's notify command is a logger (as a dev daemon's notifications.log). The test
// window opens behind the owner's apps, so it is not focused: a fixture permission, finish and
// failure each write one entry naming the agent, with that agent's click URL. Opening that URL in
// VS Code (Open URL, the same handler a click reaches) opens that agent's chat. Inside VS Code the
// permission toast names the agent and shows even while the Overseer view is open on another agent.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('notify-agents');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const log = path.join(s.root, 'notifications.log');
  const notify = path.join(s.root, 'notify.sh');
  fs.writeFileSync(notify, `#!/bin/sh\nprintf '%s|%s|%s\\n' "$1" "$2" "$3" >> '${log}'\n`, { mode: 0o755 });
  const lines = () => { try { return fs.readFileSync(log, 'utf8').trim().split('\n').filter(Boolean); } catch { return []; } };
  try {
    const repo = makeRepo(path.join(s.root, 'site'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'echo');
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE', OVERSEER_TEST_SYSTEM_HOME: path.join(s.root, 'system'), OVERSEER_NOTIFY_COMMAND: notify });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const run = id => s.ctl('state').runs.find(r => r.id === id);
    const waitFor = async (id, re, ms = 20000) => { for (let t = 0; t < ms; t += 300) { if (re.test(run(id)?.status || '')) return run(id).status; await delay(300); } return run(id)?.status; };
    // The test window never takes the owner's focus, but the harness makes its page act focused
    // (CDP focus emulation) so keys reach it. Here that is turned off and on, and the blur and focus
    // events macOS sends when the owner switches apps are emitted from VS Code's main process
    // (VS Code asks the page whether it has focus when one arrives); nothing is activated.
    const setFocus = async on => {
      await cdp.call('Emulation.setFocusEmulationEnabled', { enabled: on }, cdp.workbench).catch(() => {});
      await s.quiet?.main(`require('electron').BrowserWindow.getAllWindows().forEach(w => w.emit(${JSON.stringify(on ? 'focus' : 'blur')})), true`);
      let st; for (let i = 0; i < 30; i++) { st = s.ctl('notices.get'); if (st.vscode_focused === on) break; await delay(300); }
      return st;
    };
    const focusedState = await setFocus(true);
    check('a focused window tells the daemon so', focusedState.vscode_focused === true, focusedState);
    fs.writeFileSync(modeFile, 'echo');
    const quiet = s.ctl('task.create', { repo, harness: 'claude', prompt: 'summarise', title: 'Finished while you watch' });
    await waitFor(quiet.run.id, /completed/);
    await delay(1500);
    check('a focused window writes no notification', lines().length === 0, lines());
    const focus = await setFocus(false);
    check('the window, behind the owner\'s apps, tells the daemon it is not focused', focus.vscode_focused === false, focus);

    // A permission, a finish and a failure while VS Code is not focused.
    fs.writeFileSync(modeFile, 'permission');
    const asks = s.ctl('task.create', { repo, harness: 'claude', prompt: 'write perm.txt', title: 'Write the changelog' });
    await waitFor(asks.run.id, /waiting_for_user/);
    fs.writeFileSync(modeFile, 'echo');
    const done = s.ctl('task.create', { repo, harness: 'claude', prompt: 'summarise', title: 'Summarise the notes' });
    await waitFor(done.run.id, /completed/);
    const broken = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo boom; exit 2'], prompt: '', title: 'Broken build' });
    await waitFor(broken.run.id, /failed/);
    for (let i = 0; i < 30 && lines().length < 3; i++) await delay(200);
    await delay(800);
    const got = lines();
    const one = (title, re, id) => got.filter(l => l.startsWith(title + '|')).length === 1 && got.some(l => l.startsWith(title + '|') && re.test(l) && l.endsWith(`open-agent?run=${id}`));
    check('with the window unfocused, a fixture permission, finish and failure each write one notification naming the agent, with its click URL',
      got.length === 3 && one('Write the changelog', /Needs your permission to use Write · site/, asks.run.id) && one('Summarise the notes', /Finished · site/, done.run.id) && one('Broken build', /Stopped with an error · site/, broken.run.id), got);
    fs.writeFileSync(path.join(s.evidence, 'notifications.log'), got.join('\n') + '\n');
    await setFocus(true); // keys and the command palette again

    // The click: VS Code opens the notification's URL (Open URL reaches the same handler).
    await cdp.command('Overseer: Open Overseer View');
    const view = await s.editorView();
    await cdp.command('Developer: Open URL');
    await cdp.input('', `vscode://beelol.overseer/open-agent?run=${broken.run.id}`);
    // VS Code asks once before an extension handles a vscode:// link (as in scenario-notify, AC-52).
    const prompt = await cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d) return null; const open = [...d.querySelectorAll('.monaco-button')].find(b => b.textContent.trim() === 'Open'); if (!open) return null; const r = open.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, 10000, 'URI prompt').catch(() => null);
    if (prompt) await cdp.click(prompt.x, prompt.y);
    const opened = await view.waitFor(`document.body.dataset.mode === 'chat' && window.__overseer.selected() === ${JSON.stringify(broken.run.id)}`, 15000).then(() => true, () => false);
    await s.screenshot('click-opens-the-agent');
    check('opening the notification\'s URL focuses that agent (its chat in the Overseer view)', opened, { opened, selected: await view.eval(`window.__overseer.selected()`) });

    // Inside VS Code: the permission toast names the agent, even with the view open on another agent.
    await cdp.command('Notifications: Clear All Notifications'); await delay(300);
    fs.writeFileSync(modeFile, 'permission');
    const other = s.ctl('task.create', { repo, harness: 'claude', prompt: 'write perm.txt', title: 'Add the release notes' });
    await waitFor(other.run.id, /waiting_for_user/);
    const toast = await cdp.waitFor(`[...document.querySelectorAll('.notification-toast, .notifications-toasts .monaco-list-row')].map(t => t.innerText).find(t => /release notes/.test(t)) || null`, 10000).catch(() => null);
    await s.screenshot('toast-names-the-agent');
    check('the in-VS Code permission toast names the agent and shows while the Overseer view is open on another agent',
      /“Add the release notes” is waiting for permission to use Write/.test(toast || ''), { toast, showing: await view.eval(`window.__overseer.selected()`) });
    for (const id of [asks.run.id, other.run.id]) s.ctl('run.interrupt', { run_id: id });
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
