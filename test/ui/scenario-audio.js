// Packaged-UI scenario for Audio Mode (Gate O), fixture harnesses only. No sound is played: the
// daemon writes each cue it would play to a log (OVERSEER_TEST_AUDIO_LOG), so the counts are exact.
// Audio Mode is off on a new install and its command sits in the Agents view's overflow menu (the
// title bar keeps its three icons). VS Code turns it on, picks a track and asks for a preview; the
// daemon is the one that plays. Two agents asking for permission at the same moment make one cue
// while the side bar, the Overseer icon and the status bar count two. With VS Code closed a new
// agent's start and completion still play once each.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('audio');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const audioLog = path.join(s.root, 'audio.log');
  const barrier = path.join(s.root, 'release-permissions');
  const played = () => (fs.existsSync(audioLog) ? fs.readFileSync(audioLog, 'utf8') : '').split('\n').filter(Boolean);
  const count = line => played().filter(l => l === line).length;
  const waitPlayed = async (n, ms = 10000) => { for (let t = 0; t < ms; t += 100) { if (played().length >= n) return true; await delay(100); } return false; };
  let closed = false;
  try {
    const repo = makeRepo(path.join(s.root, 'audio-demo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'window.dialogStyle': 'custom', 'window.menuStyle': 'custom', 'window.titleBarStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_TEST_AUDIO_LOG: audioLog, OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      FIXTURE_MODE: 'permission', FIXTURE_PERMISSION_BARRIER: barrier, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_MODE,FIXTURE_PERMISSION_BARRIER' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const state = id => s.ctl('state').runs.find(r => r.id === id);
    const waitStatus = async (id, re, ms = 30000) => { for (let t = 0; t < ms; t += 300) { if (re.test(state(id)?.status || '')) return true; await delay(300); } return false; };
    const toast = pattern => cdp.waitFor(`[...document.querySelectorAll('.notification-toast')].map(t => t.innerText).find(t => ${pattern}.test(t)) || null`, 20000).catch(() => null);

    // Off on a new install.
    const initial = s.ctl('audio.get');
    const warm = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'exit 0'], prompt: '', title: 'Quiet run' });
    await waitStatus(warm.run.id, /completed/);
    await delay(1000);
    check('Audio Mode is off on a new install: a finished agent makes no cue and no audio folder exists',
      initial.enabled === false && initial.track === 'reactor' && played().length === 0 && !fs.existsSync(path.join(s.home, 'audio')),
      { enabled: initial.enabled, track: initial.track, played: played(), audioFolder: fs.existsSync(path.join(s.home, 'audio')) });
    check('the daemon reports the 12 Reactor cues (31,488 bytes, each under 0.5 s) and three that play by themselves',
      initial.manifest.length === 12 && initial.manifest.reduce((n, c) => n + c.mp3_bytes, 0) === 31488 && initial.manifest.every(c => c.duration < 0.5) && initial.default_keys.join() === 'agent_started,agent_complete,agent_needs_attention',
      { cues: initial.manifest.length, bytes: initial.manifest.reduce((n, c) => n + c.mp3_bytes, 0), longest: Math.max(...initial.manifest.map(c => c.duration)), auto: initial.default_keys });

    // The title bar is the one from Gate K; Audio Mode is in the overflow menu.
    await cdp.command('View: Show Overseer');
    await delay(2500);
    await cdp.command('View: Focus on Agents View'); await delay(800);
    const paneHeader = `[...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || ''))?.querySelector('.pane-header')`;
    const title = await cdp.evalWorkbench(`(() => { const b = ${paneHeader}.querySelector('.title').getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`);
    await cdp.move(title.x, title.y); await delay(400); await cdp.move(title.x + 1, title.y); await delay(400);
    const header = await cdp.evalWorkbench(`(() => { const items = [...${paneHeader}.querySelectorAll('.actions .action-label')].map(a => ({ name: (a.getAttribute('aria-label') || '').trim(), box: a.getBoundingClientRect() }));
      const more = items.find(i => /^More Actions/.test(i.name));
      return { icons: items.filter(i => i.name && !/^More Actions/.test(i.name)).map(i => i.name), more: more && more.box.width ? { x: more.box.left + more.box.width / 2, y: more.box.top + more.box.height / 2 } : null }; })()`);
    let overflow = [];
    if (header.more) {
      await cdp.click(header.more.x, header.more.y);
      // A toolbar's menu is drawn inside a shadow root.
      const entries = `[document, ...[...document.querySelectorAll('.shadow-root-host')].map(h => h.shadowRoot).filter(Boolean)].flatMap(r => [...r.querySelectorAll('.monaco-menu .action-item .action-label')]).map(a => a.getAttribute('aria-label') || a.textContent.trim()).filter(Boolean)`;
      await cdp.waitFor(`${entries}.length > 0`, 10000, 'overflow menu').catch(() => {});
      overflow = await cdp.evalWorkbench(entries);
      await s.screenshot('overflow-menu');
      await cdp.key('Escape'); await delay(300);
    }
    const icons = header.icons.map(n => n.replace(/ \(.*$/, ''));
    check('the Agents title bar is unchanged (New Agent, Search Agents, Toggle Agent Grid and VS Code\'s Collapse All) and Audio Mode is in the overflow menu with the rest',
      icons.join() === 'New Agent,Search Agents,Toggle Agent Grid,Collapse All' && overflow.some(m => /^Audio Mode and Reactor Cues/.test(m)) && ['Show Archived', 'Refresh', 'Stop Agents and Daemon'].every(x => overflow.some(m => m.startsWith(x))),
      { icons: header.icons, overflow });

    // Turning it on is one explicit action in VS Code; the daemon keeps the setting.
    await cdp.command('Overseer: Audio Mode and Reactor Cues');
    await cdp.waitQuickTitle('Overseer Audio Mode');
    const menu = await cdp.quickInputState();
    const placeholder = await cdp.evalWorkbench(`document.querySelector('.quick-input-widget input')?.getAttribute('placeholder') || ''`);
    await s.screenshot('audio-mode-menu');
    await cdp.pick('Overseer Audio Mode', 'Turn Audio Mode on');
    const onToast = await toast('/Audio Mode on/');
    const enabled = s.ctl('audio.get');
    check('the Audio Mode menu says Reactor signals · Off, and Turn Audio Mode on enables it in the daemon',
      /Reactor signals · Off/.test(placeholder) && menu.rows.some(r => /Turn Audio Mode on/.test(r)) && menu.rows.some(r => /Preview cue/.test(r)) && !!onToast && enabled.enabled === true,
      { placeholder, rows: menu.rows, toast: onToast, enabled: enabled.enabled });
    await cdp.command('Notifications: Clear All Notifications');

    // A preview is played by the daemon, once.
    await cdp.command('Overseer: Audio Mode and Reactor Cues');
    await cdp.pick('Overseer Audio Mode', 'Preview cue');
    await cdp.waitQuickTitle('Preview Reactor signals cue');
    const previews = await cdp.quickInputState();
    const listed = await cdp.evalWorkbench(`Number(document.querySelector('.quick-input-widget .monaco-list-row')?.getAttribute('aria-setsize'))`);
    await s.screenshot('preview-cues');
    await cdp.pick('Preview Reactor signals cue', 'Review ready');
    await waitPlayed(1);
    await delay(500);
    check('Preview lists the 12 cues with their lengths and the daemon plays the chosen one once', listed === 12 && previews.rows.every(r => /, 0\.\d+s, /.test(r)) && played().join() === 'reactor:review_ready', { listed, shown: previews.rows, played: played() });

    // The track is a daemon setting too.
    await cdp.command('Overseer: Audio Mode and Reactor Cues');
    await cdp.pick('Overseer Audio Mode', 'Choose audio track');
    await cdp.pick('Choose audio track', 'System voice');
    await delay(800);
    const system = s.ctl('audio.get').track;
    await cdp.command('Overseer: Audio Mode and Reactor Cues');
    await cdp.pick('Overseer Audio Mode', 'Choose audio track');
    await cdp.pick('Choose audio track', 'Reactor signals');
    await delay(800);
    check('choosing System voice and then Reactor signals changes the daemon\'s track', system === 'system' && s.ctl('audio.get').track === 'reactor', { system, now: s.ctl('audio.get').track });

    // Two agents ask for permission at the same moment: one cue, a count of two.
    const before = played().length;
    const one = s.ctl('task.create', { repo, harness: 'claude', title: 'Write the release notes', prompt: 'Write the release notes.' });
    const two = s.ctl('task.create', { repo, harness: 'claude', title: 'Update the changelog', prompt: 'Update the changelog.' });
    await waitStatus(one.run.id, /running/); await waitStatus(two.run.id, /running/);
    await waitPlayed(before + 1);
    fs.writeFileSync(barrier, 'go');
    const bothWaiting = await waitStatus(one.run.id, /waiting_for_user/) && await waitStatus(two.run.id, /waiting_for_user/);
    await delay(2500);
    const rows = await s.agentRows();
    const needs = rows.find(r => r.label === 'Needs you');
    const badge = await cdp.evalWorkbench(`(() => { const a = [...document.querySelectorAll('.activitybar .action-item')].find(i => /Overseer/.test(i.querySelector('.action-label')?.getAttribute('aria-label') || '')); return a?.querySelector('.badge-content')?.textContent.trim(); })()`);
    const bar = await cdp.evalWorkbench(`[...document.querySelectorAll('.statusbar-item')].map(e => e.textContent.trim()).find(t => /Overseer \\d+ active/.test(t)) || ''`);
    await s.screenshot('two-need-you');
    check('two simultaneous permission requests make one attention cue', bothWaiting && count('reactor:agent_needs_attention') === 1, { bothWaiting, played: played().slice(before) });
    check('the side bar, the Overseer icon and the status bar each count two', needs?.description === '2' && badge === '2' && /\b2$/.test(bar), { needs: needs?.description, badge, statusBar: bar });

    // VS Code closed: the daemon still plays, once.
    for (const t of [one, two]) s.ctl('run.interrupt', { run_id: t.run.id });
    await waitStatus(one.run.id, /interrupted/); await waitStatus(two.run.id, /interrupted/);
    await delay(1200); // past the 800 ms window in which the same cue is dropped
    await s.quit(); closed = true;
    const atClose = played().length;
    const later = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'sleep 1; exit 0'], prompt: '', title: 'After closing' });
    await waitStatus(later.run.id, /completed/);
    await waitPlayed(atClose + 2);
    await delay(500);
    const after = played().slice(atClose);
    check('with VS Code closed a new agent\'s start and completion play once each', after.join() === 'reactor:agent_started,reactor:agent_complete' && s.ctl('audio.get').enabled === true, after);
    check('no Reactor cache and no private file was written while the daemon only logged cues', !fs.existsSync(path.join(s.home, 'audio')), fs.existsSync(path.join(s.home, 'audio')));
    result.played = played();
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    if (!closed) { try { await s.screenshot('error'); } catch {} }
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { if (!closed) await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
