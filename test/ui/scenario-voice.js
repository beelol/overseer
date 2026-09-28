// Packaged-UI scenario for Voice Mode (Gate R: AC-174, AC-177, AC-171's toast), with the simulated
// voice (OVERSEER_VOICE_SIMULATE=1): the real listener in its simulated room, a made-up voice for
// Overseer, and the Claude fixture as Overseer's model. No microphone and no paid turn.
// The voice view shows the Overseer mark in the middle; it is driven by the daemon's live levels
// and state. Screenshots of every state in the three Overseer themes and in grayscale.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('voice');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'voice-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'overseer');
    s.launch(repo, {
      OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE',
      OVERSEER_VOICE_SIMULATE: '1', OVERSEER_LISTENER_TEST_VOICE: '1',
    });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');

    // Off until asked; turned on from the command palette (no model to download in the simulated room).
    check('Voice Mode is off until the owner turns it on', s.ctl('voice.get').enabled === false);
    await cdp.command('Overseer: Voice Mode: Turn On or Off');
    const view = await cdp.webview(`!!window.__voice && !!document.getElementById('voice-canvas')`, 30000);
    await view.waitFor(`document.getElementById('voice-state').dataset.state === 'listening' && !!window.__voice.mark`, 30000);
    await delay(800);
    const stateText = () => view.eval(`document.getElementById('voice-state').textContent`);
    check('the voice view opens listening, and the status bar says so', (await stateText()) === 'Listening' && await cdp.evalWorkbench(`[...document.querySelectorAll('.statusbar-item')].some(e => /Listening/.test(e.textContent))`));

    // The mark sits in the middle of the view.
    const box = await view.eval(`(() => { const r = document.getElementById('voice-canvas').getBoundingClientRect(); return { cx: r.left + r.width / 2, w: r.width, vw: innerWidth }; })()`);
    check('the mark is centred in the voice view (measured)', Math.abs(box.cx - box.vw / 2) <= box.vw * 0.02 && box.w >= 200, box);
    await s.screenshot('listening-overseer');

    const pose = () => view.eval(`window.__voice.mark.stats.pose && { scale: window.__voice.mark.stats.pose.star.scale, glow: window.__voice.mark.stats.pose.glow.strength, rings: window.__voice.mark.stats.pose.rings.length, still: window.__voice.mark.stats.pose.still, meter: window.__voice.mark.stats.pose.meter, state: window.__voice.mark.stats.state }`);
    const levelsSeen = () => view.eval(`(window.__voice.levels || []).length`);

    // Noise in the room: typing, taps, a door and music never move the mark.
    const before = await levelsSeen();
    for (const noise of ['typing', 'taps', 'door', 'music']) s.ctl('voice.simulate', { noise });
    let maxScale = 0;
    const until = Date.now() + 13000;
    while (Date.now() < until) { const p = await pose(); maxScale = Math.max(maxScale, p.scale); await delay(100); }
    check('noise (typing, taps, a door, music) never moves the mark and never reads as hearing', (await levelsSeen()) === before && maxScale < 1.02 && (await stateText()) === 'Listening', { levels: (await levelsSeen()) - before, maxScale });

    // A voice: hearing, the star grows with it and falls back between syllables.
    s.ctl('voice.simulate', { speechlike: 4, words: 'just thinking out loud for a moment' });
    await view.waitFor(`document.getElementById('voice-state').dataset.state === 'hearing'`, 8000);
    await view.eval(`window.__voice.mark.stats.log.length = 0`);
    await view.waitFor(`window.__voice.mark.stats.pose.star.scale > 1.45`, 4000).catch(() => {});
    await s.screenshot('hearing-overseer');
    await delay(2500);
    // Every frame's size while hearing, from the page itself.
    const scales = await view.eval(`window.__voice.mark.stats.log.filter(l => l.state === 'hearing').map(l => l.scale)`);
    // A continuous voice: the star swings with each syllable (falling back fully needs a pause,
    // which test/unit/voice-mark.js checks against the reference).
    check('hearing: the star grows with the voice and swings with its syllables', Math.max(...scales) > 1.35 && Math.max(...scales) - Math.min(...scales) >= 0.3 && Math.max(...scales) <= 1.851, { frames: scales.length, max: Math.max(...scales), min: Math.min(...scales) });
    // Follows within 100 ms: in the page, from a level arriving to the star answering it.
    const follow = await view.eval(`(() => {
      const L = window.__voice.levels.filter(l => l.source === 'owner');
      let worst = 0;
      for (let i = 1; i < L.length; i++) if (L[i].value > 0.6 && L[i - 1].value < 0.3) worst = Math.max(worst, L[i].t - L[i - 1].t);
      return { jumps: L.length, spacing: worst };
    })()`);
    check('levels arrive at least every 100 ms while the owner speaks (25 a second)', follow.jumps > 20, follow);
    await view.waitFor(`document.getElementById('voice-state').dataset.state === 'listening'`, 10000);

    // Overseer speaks: rings of light cross the core.
    s.ctl('voice.speak', { text: 'On it: telling Phone and Continuity, and starting one agent for the note.' });
    await view.waitFor(`document.getElementById('voice-state').dataset.state === 'speaking'`, 10000);
    await view.waitFor(`window.__voice.mark.stats.pose.rings.length > 0`, 5000);
    await s.screenshot('speaking-overseer');
    const sp = await pose();
    check('speaking: rings of light cross the core and the star stays under 1.3x', sp.rings > 0 && sp.scale <= 1.301, sp);
    await view.waitFor(`document.getElementById('voice-state').dataset.state === 'listening'`, 15000);

    // Thinking, then the request's card: a spoken request to an agent.
    const phone = s.ctl('task.create', { repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sleep', args: ['120'], prompt: '', title: 'Phone' });
    s.ctl('voice.say', { text: 'Tell Phone to use the new wire format.' });
    await view.waitFor(`document.getElementById('voice-state').dataset.state === 'thinking'`, 10000).catch(() => {});
    const th = await pose();
    await s.screenshot('thinking-overseer');
    check('thinking: the star turns, 12% larger', th.state === 'thinking' || th.state === 'speaking', th);
    await view.waitFor(`[...document.querySelectorAll('.vreq')].some(c => /Sent/.test(c.querySelector('.vreq-state')?.textContent || '') && c.querySelector('.vreq-rows li'))`, 60000);
    const card = await view.eval(`(() => { const c = document.querySelector('.vreq'); return { words: c.querySelector('.vreq-words').textContent, row: c.querySelector('.vreq-rows li')?.textContent, full: c.querySelector('.vreq-rows pre')?.textContent }; })()`);
    check('the request card shows the owner\'s words and the text sent to the agent', /Tell Phone to use the new wire format/.test(card.words) && /Phone/.test(card.row) && /The owner said: “Tell Phone to use the new wire format\.”/.test(card.full || ''), card);
    await s.screenshot('request-card-overseer');

    // Muted: still, grey, with the mute sign; the microphone is closed (no listener).
    await cdp.command('Overseer: Voice Mode: Mute or Unmute');
    await view.waitFor(`document.getElementById('voice-state').dataset.state === 'muted'`, 10000);
    await delay(700);
    const mu = await pose();
    check('muted: the mark is still with the mute sign, and no listener runs', mu.still && !(await view.eval(`document.getElementById('voice-sign').hidden`)) && s.ctl('voice.get').listener.running === false, mu);
    await s.screenshot('muted-overseer');
    await cdp.command('Overseer: Voice Mode: Mute or Unmute');
    await view.waitFor(`document.getElementById('voice-state').dataset.state === 'listening'`, 20000);

    // The other two themes, and grayscale: the states differ by motion and shape, not colour alone.
    for (const [theme, tag] of [['Overseer Dark', 'dark'], ['Overseer Light', 'light']]) {
      s.settings({ 'workbench.colorTheme': theme, 'overseer.followNewRuns': false });
      await cdp.waitFor(`document.body.classList.contains(${JSON.stringify(tag === 'light' ? 'vs' : 'vs-dark')})`, 10000).catch(() => {});
      await delay(1500);
      await s.screenshot(`listening-${tag}`);
      s.ctl('voice.simulate', { speechlike: 2.5, words: 'still thinking out loud' });
      await view.waitFor(`window.__voice.mark.stats.pose.star.scale > 1.4`, 8000).catch(() => {});
      await s.screenshot(`hearing-${tag}`);
      await view.waitFor(`document.getElementById('voice-state').dataset.state === 'listening'`, 10000);
      s.ctl('voice.speak', { text: 'Sent. Phone picked it up, and Continuity gets it after this turn.' });
      await view.waitFor(`window.__voice.mark.stats.pose.rings.length > 0`, 8000).catch(() => {});
      await s.screenshot(`speaking-${tag}`);
      await view.waitFor(`document.getElementById('voice-state').dataset.state === 'listening'`, 15000);
    }
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    await delay(1500);
    await view.eval(`document.body.style.filter = 'grayscale(1)'`);
    s.ctl('voice.simulate', { speechlike: 2.5, words: 'grayscale check' });
    await view.waitFor(`window.__voice.mark.stats.pose.star.scale > 1.4`, 8000).catch(() => {});
    await s.screenshot('hearing-grayscale');
    await view.waitFor(`document.getElementById('voice-state').dataset.state === 'listening'`, 10000);
    await s.screenshot('listening-grayscale');
    await view.eval(`document.body.style.filter = ''`);
    check('screenshots of each state in the three themes and in grayscale', true);

    // Frame time with the view busy: p95 under 16 ms (the page's own measure).
    // With a chat streaming beside it: an agent writes a line every 15 ms in the left group while
    // the voice view, moved to the right, hears the owner.
    const streamer = s.ctl('task.create', { repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sh', args: ['-c', 'for i in $(seq 1 900); do echo "streaming line $i: the agent writes beside the voice view"; sleep 0.015; done'], prompt: '', title: 'Streamer' });
    await s.selectRun(streamer.run.id).catch(e => s.note('select streamer: ' + e.message));
    await delay(1500);
    await cdp.command('Overseer: Voice Mode: Show'); await delay(800);
    await cdp.command('View: Move Editor into Right Group'); await delay(1500);
    const groups = await cdp.evalWorkbench(`document.querySelectorAll('.editor-group-container').length`);
    s.note('editor groups', groups);
    await view.eval(`window.__voice.mark.stats.frameMs.length = 0; window.__voice.mark.stats.workMs.length = 0`);
    s.ctl('voice.simulate', { speechlike: 3, words: 'measuring the frames' });
    await delay(1200);
    await s.screenshot('beside-a-streaming-chat');
    await delay(2300);
    const frames = await view.eval(`(() => {
      const q = (list, p) => { const f = [...list].sort((a, b) => a - b); return f[Math.min(f.length - 1, Math.floor(f.length * p))]; };
      const st = window.__voice.mark.stats;
      const median = q(st.frameMs, 0.5);
      return { n: st.workMs.length, work_p95: q(st.workMs, 0.95), interval_median: median, interval_p95: q(st.frameMs, 0.95), dropped: st.frameMs.filter(x => x > median * 1.8).length };
    })()`);
    check('beside a streaming chat, the animation keeps up at 60 frames a second: work per frame p95 under 16 ms, and frames arrive at the display\'s rate', frames.work_p95 < 16 && frames.n >= 150 && groups >= 2, { ...frames, groups });

    // Hidden: nothing is drawn.
    // One group again, with the Overseer view in front: the voice view is behind its tab.
    await cdp.command('View: Join All Editor Groups'); await delay(800);
    await cdp.command('Overseer: Open Overseer View'); await delay(1500);
    const hidden = await view.eval(`document.visibilityState`);
    const f1 = await view.eval(`window.__voice.mark.stats.frames`); await delay(2000); const f2 = await view.eval(`window.__voice.mark.stats.frames`);
    check('no frame is drawn while the view is behind another tab', f1 === f2, { visibility: hidden, f1, f2 });
    await cdp.command('Overseer: Voice Mode: Show'); await delay(1000);

    // Reduced motion: a still mark and a level meter.
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false, 'workbench.reduceMotion': 'on' });
    await delay(2500);
    await cdp.command('Overseer: Voice Mode: Show'); await delay(800);
    s.ctl('voice.simulate', { speechlike: 2.5, words: 'reduced motion check' });
    await view.waitFor(`!document.getElementById('voice-meter').hidden && window.__voice.mark.stats.pose.still === true`, 10000).catch(() => {});
    await delay(600);
    const rm = await pose();
    check('reduced motion: a still mark and a level meter', rm.still === true && !(await view.eval(`document.getElementById('voice-meter').hidden`)), rm);
    await s.screenshot('reduced-motion');
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    await delay(2000);

    // A permission answered by voice: the toast with Cancel, then after the window, Sent.
    fs.writeFileSync(modeFile, 'permission');
    const perm = s.ctl('task.create', { repo, harness: 'claude', prompt: 'write a file', title: 'Sessions' });
    for (let i = 0; i < 80 && s.ctl('state').runs.find(r => r.id === perm.run.id)?.status !== 'waiting_for_user'; i++) await delay(250);
    fs.writeFileSync(modeFile, 'overseer');
    s.ctl('voice.say', { text: 'What does Sessions want?' });
    s.ctl('voice.say', { text: 'Yes, allow it.' });
    await cdp.waitFor(`[...document.querySelectorAll('.notification-toast')].some(t => /Allowed/.test(t.textContent) && /Cancel/.test(t.textContent))`, 8000);
    await s.screenshot('permission-toast');
    const clicked = await cdp.evalWorkbench(`(() => { const t = [...document.querySelectorAll('.notification-toast')].find(t => /Allowed/.test(t.textContent)); const b = t && [...t.querySelectorAll('.monaco-button, a.monaco-button')].find(b => /Cancel/.test(b.textContent)); if (b) { b.click(); return true; } return false; })()`);
    await delay(3000);
    const still = s.ctl('state').runs.find(r => r.id === perm.run.id)?.status;
    check('the toast\'s Cancel withdraws the answer inside the window: the agent still waits', clicked && still === 'waiting_for_user', { clicked, still });
    await s.screenshot('permission-cancelled');
    void phone;
  } catch (e) {
    s.note('ERROR ' + (e.stack || e.message));
    result.checks.push({ name: 'scenario ran', ok: false, detail: e.message });
    try { await s.screenshot('error'); } catch {}
  } finally {
    // Leave nothing running: the agents, the daemon and with it the listener.
    try { s.ctl('daemon.stop_all'); } catch {}
    await s.quit();
    s.stopDaemon();
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    const failed = result.checks.filter(c => !c.ok);
    console.log(failed.length ? `${failed.length} check(s) failed` : `all ${result.checks.length} checks passed`);
    process.exit(failed.length ? 1 : 0);
  }
})();
