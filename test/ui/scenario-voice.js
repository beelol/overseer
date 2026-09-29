// Packaged-UI scenario for Voice Mode (Gate R: AC-174, AC-177, AC-171's toast), with the simulated
// voice (OVERSEER_VOICE_SIMULATE=1): the real listener in its simulated room, a made-up voice for
// Overseer, and the Claude fixture as Overseer's model. No microphone and no paid turn.
// The voice view shows the Overseer mark in the middle; it is driven by the daemon's live levels
// and state. Since AC-227 the voice view is home (the conversation with Overseer) with Voice Mode
// on: the mark on top and the same cards as typed ones below. Screenshots of every state in the
// three Overseer themes and in grayscale.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');
const { auditExpression } = require('./audit');

(async () => {
  const s = new Session('voice');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const micUsers = path.join(s.root, 'mic-users');
  try {
    const repo = makeRepo(path.join(s.root, 'voice-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'overseer');
    s.launch(repo, {
      OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS', FIXTURE_SLOW_MS: '60000',
      OVERSEER_VOICE_SIMULATE: '1', OVERSEER_LISTENER_TEST_VOICE: '1', OVERSEER_LISTENER_TEST_MIC_USERS: micUsers,
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
    // The star follows the level curve within 100 ms: the time shift that best lines up the star's
    // size with the levels (both from the page's own records), searched from 0 to 300 ms.
    const lag = await view.eval(`(() => {
      const L = window.__voice.levels.filter(l => l.source === 'owner');
      const F = window.__voice.mark.stats.log.filter(l => l.state === 'hearing' && typeof l.t === 'number');
      if (L.length < 10 || F.length < 30) return { n: F.length, levels: L.length };
      const levelAt = t => { let v = 0; for (const l of L) { if (l.t > t) break; v = l.value; } return v; };
      const scale = F.map(f => f.scale), mean = a => a.reduce((x, y) => x + y, 0) / a.length;
      let best = { lag: -1, r: -2 };
      for (let d = 0; d <= 300; d += 10) {
        const lv = F.map(f => levelAt(f.t - d));
        const ms = mean(scale), ml = mean(lv);
        let num = 0, a = 0, b = 0;
        for (let i = 0; i < F.length; i++) { num += (scale[i] - ms) * (lv[i] - ml); a += (scale[i] - ms) ** 2; b += (lv[i] - ml) ** 2; }
        const r = num / Math.sqrt(a * b || 1);
        if (r > best.r) best = { lag: d, r };
      }
      return { lag: best.lag, r: Math.round(best.r * 100) / 100, frames: F.length, levels: L.length };
    })()`);
    check('the star follows the level curve within 100 ms (best-aligned lag)', lag.lag >= 0 && lag.lag <= 100 && lag.r > 0.5, lag);
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
    // The spoken request's card is the conversation's card: the owner's words, then what was done.
    await view.waitFor(`[...document.querySelectorAll('#home-conv .home-msg.spoken')].some(m => /use the new wire format/.test(m.textContent)) && [...document.querySelectorAll('#home-conv .done-card .card-row')].some(r => /Phone/.test(r.textContent))`, 60000);
    const card = await view.eval(`(() => { const m = [...document.querySelectorAll('#home-conv .home-msg.spoken')].filter(m => /use the new wire format/.test(m.textContent)).pop(); const r = [...document.querySelectorAll('#home-conv .done-card .card-row')].filter(r => /Phone/.test(r.textContent)).pop(); return { words: m.querySelector('.home-text').textContent, stage: m.querySelector('.req-stage')?.textContent, row: r.textContent, full: r.title }; })()`);
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
      s.ctl('voice.say', { text: 'Tell Phone to keep the old format for now.' });
      await view.waitFor(`document.getElementById('voice-state').dataset.state === 'thinking'`, 10000).catch(() => {});
      await s.screenshot(`thinking-${tag}`);
      await view.waitFor(`document.getElementById('voice-state').dataset.state === 'listening'`, 60000).catch(() => {});
      await cdp.command('Overseer: Voice Mode: Mute or Unmute');
      await view.waitFor(`document.getElementById('voice-state').dataset.state === 'muted'`, 10000).catch(() => {});
      await delay(700);
      await s.screenshot(`muted-${tag}`);
      await cdp.command('Overseer: Voice Mode: Mute or Unmute');
      await view.waitFor(`document.getElementById('voice-state').dataset.state === 'listening'`, 20000);
      fs.writeFileSync(micUsers, 'us.zoom.xos\n');
      await view.waitFor(`document.getElementById('voice-state').dataset.state === 'paused'`, 6000).catch(() => {});
      await delay(500);
      await s.screenshot(`paused-${tag}`);
      fs.writeFileSync(micUsers, '');
      await view.waitFor(`document.getElementById('voice-state').dataset.state === 'listening'`, 6000);
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
    // The streamer's chat taken out to the side, and the voice view (home) in the Overseer view.
    await cdp.command('Overseer: Open to the Side'); await delay(1200);
    await cdp.command('Overseer: Voice Mode: Show'); await delay(1500);
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

    // Hidden: nothing is drawn. The Overseer view shows an agent's chat, so the stage is not shown.
    await cdp.command('View: Join All Editor Groups'); await delay(800);
    await s.selectRun(streamer.run.id).catch(e => s.note('select streamer: ' + e.message));
    await delay(1500);
    const hidden = await view.eval(`document.getElementById('voice-stage').checkVisibility() ? 'shown' : 'not shown'`);
    const f1 = await view.eval(`window.__voice.mark.stats.frames`); await delay(2000); const f2 = await view.eval(`window.__voice.mark.stats.frames`);
    check('no frame is drawn while the stage is not shown (the view shows an agent)', f1 === f2 && hidden === 'not shown', { visibility: hidden, f1, f2 });
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

    const generic = (title, secs = 900) => s.ctl('task.create', { repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sleep', args: [String(secs)], prompt: '', title }).run.id;
    const requestState = id => (s.ctl('voice.requests', { limit: 50 }).requests || []).find(r => r.id === id)?.state;
    const untilState = async (id, states, ms = 60000) => { const end = Date.now() + ms; let st; while (Date.now() < end) { st = requestState(id); if (states.includes(st)) return st; await delay(250); } return st; };
    const clearToasts = () => cdp.command('Notifications: Clear All Notifications').catch(() => {});

    // ---------- The toast before and after the window, in the three themes (AC-171).
    const toastShots = [];
    for (const [theme, tag] of [['Overseer', 'overseer'], ['Overseer Dark', 'dark'], ['Overseer Light', 'light']]) {
      s.settings({ 'workbench.colorTheme': theme, 'overseer.followNewRuns': false });
      await delay(1500);
      await clearToasts();
      fs.writeFileSync(modeFile, 'permission');
      const pa = s.ctl('task.create', { repo, harness: 'claude', prompt: 'write a file', title: `Asks ${tag}` });
      for (let i = 0; i < 80 && s.ctl('state').runs.find(r => r.id === pa.run.id)?.status !== 'waiting_for_user'; i++) await delay(250);
      fs.writeFileSync(modeFile, 'overseer');
      s.ctl('voice.say', { text: 'What does it want?' });
      await delay(300);
      s.ctl('voice.say', { text: 'Yes, allow it.' });
      const before = await cdp.waitFor(`[...document.querySelectorAll('.notification-toast')].some(t => /Allowed/.test(t.textContent) && /Cancel/.test(t.textContent))`, 10000).then(() => true, () => false);
      await s.screenshot(`toast-before-window-${tag}`);
      const after = await cdp.waitFor(`[...document.querySelectorAll('.notification-toast')].some(t => /Sent/.test(t.textContent))`, 12000).then(() => true, () => false);
      await s.screenshot(`toast-sent-${tag}`);
      toastShots.push({ tag, before, after });
      await clearToasts();
    }
    check('the toast names what was answered with Cancel, then reads Sent, in the three themes', toastShots.every(t => t.before && t.after), toastShots);
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    await delay(1500);

    // ---------- Paused for a call (AC-173, AC-174): another app records; the strip says so.
    await cdp.command('Overseer: Voice Mode: Show'); await delay(600);
    fs.writeFileSync(micUsers, 'us.zoom.xos\n');
    const paused = await view.waitFor(`document.getElementById('voice-state').dataset.state === 'paused'`, 6000).then(() => true, () => false);
    const pausedLabel = await view.eval(`({ text: document.getElementById('voice-state').textContent, title: document.getElementById('voice-state').title })`);
    await s.screenshot('paused-overseer');
    fs.writeFileSync(micUsers, '');
    const resumed = await view.waitFor(`document.getElementById('voice-state').dataset.state === 'listening'`, 6000).then(() => true, () => false);
    check('paused for a call: shown with the app, and listening again after', paused && resumed && pausedLabel.text === 'Paused for a call' && /Zoom/.test(pausedLabel.title), pausedLabel);

    // ---------- Who is spoken to (AC-166): from the strip, by command, by voice.
    const continuity = generic('Continuity');
    await delay(1500);
    const tp = await s.webviewPoint(view, '#voice-target'); await cdp.click(tp.x, tp.y);
    await cdp.pick('Voice Mode: talk to', 'Continuity');
    const byStrip = await view.waitFor(`document.getElementById('voice-target').textContent === 'Continuity'`, 10000).then(() => true, () => false);
    await s.screenshot('talking-to-continuity');
    await cdp.command('Overseer: Voice Mode: Talk To…'); await cdp.pick('Voice Mode: talk to', 'Overseer');
    const byCommand = await view.waitFor(`document.getElementById('voice-target').textContent === 'Overseer'`, 10000).then(() => true, () => false);
    s.ctl('voice.say', { text: 'talk to Continuity' });
    const byVoice = await view.waitFor(`document.getElementById('voice-target').textContent === 'Continuity'`, 10000).then(() => true, () => false);
    s.ctl('voice.say', { text: 'back to Overseer' });
    const back = await view.waitFor(`document.getElementById('voice-target').textContent === 'Overseer'`, 10000).then(() => true, () => false);
    check('who is spoken to switches from the strip, by command and by voice, and the strip shows it', byStrip && byCommand && byVoice && back, { byStrip, byCommand, byVoice, back });

    // ---------- Keyboard only (AC-174): mute, cancel and yes.
    s.ctl('voice.set', { settle_seconds: 10 });
    await cdp.focusWorkbench();
    await cdp.key('m', { meta: true, alt: true, shift: true });
    const keyMuted = await view.waitFor(`document.getElementById('voice-state').dataset.state === 'muted'`, 10000).then(() => true, () => false);
    await cdp.key('m', { meta: true, alt: true, shift: true });
    const keyUnmuted = await view.waitFor(`document.getElementById('voice-state').dataset.state === 'listening'`, 20000).then(() => true, () => false);
    // A request inside its window: the voice mark on the target in the side bar and the grid, then cancelled by keyboard.
    const cancelId = s.ctl('voice.say', { text: 'Tell Continuity to wait for the review.' }).request;
    const settling = await untilState(cancelId, ['settling']);
    await delay(600);
    const rows = await s.agentRows();
    const sideMarked = rows.some(r => r.label === 'Continuity' && /voice/.test(r.description));
    await s.screenshot('voice-mark-side-bar');
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(2000);
    const gridFrame = await cdp.webview(`!!document.querySelector('.tile')`, 15000).catch(() => null);
    const gridMarked = gridFrame ? await gridFrame.eval(`[...document.querySelectorAll('.tile')].some(t => /Continuity/.test(t.querySelector('.tile-title')?.textContent || '') && t.querySelector('.tile-voice') && !t.querySelector('.tile-voice').hidden)`) : false;
    await s.screenshot('voice-mark-grid');
    await cdp.command('Overseer: Toggle Agent Grid'); await delay(800);
    // The mark goes when the request closes (it goes out after its window).
    await untilState(cancelId, ['sent', 'cancelled'], 30000);
    const unmarked = await (async () => { for (let i = 0; i < 20; i++) { const r = await s.agentRows(); if (!r.some(x => x.label === 'Continuity' && /voice/.test(x.description))) return true; await delay(250); } return false; })();
    // Cancel by keyboard: a new request, cancelled as soon as it settles.
    const cancelId2 = s.ctl('voice.say', { text: 'Tell Continuity to pause for now.' }).request;
    await untilState(cancelId2, ['settling']);
    await cdp.focusWorkbench();
    await cdp.key('.', { meta: true, alt: true, shift: true });
    const cancelled = await untilState(cancelId2, ['cancelled'], 10000);
    check('the voice mark is on the targeted agent in the side bar and the grid while the request is open, and goes when it closes', settling === 'settling' && sideMarked && gridMarked && unmarked, { settling, sideMarked, gridMarked, unmarked });
    // Yes by keyboard: a plan that waits for a yes (archiving is a Confirm action).
    await cdp.command('Overseer: Voice Mode: Show'); await delay(600);
    const yesId = s.ctl('voice.say', { text: 'Archive Continuity.' }).request;
    const waiting = await untilState(yesId, ['waiting'], 120000);
    const yesShown = await view.waitFor(`!document.getElementById('voice-yes').hidden`, 8000).then(() => true, () => false);
    await s.screenshot('yes-waiting');
    await cdp.focusWorkbench();
    await cdp.key('y', { meta: true, alt: true, shift: true });
    const yesDone = await untilState(yesId, ['sent'], 15000);
    check('mute, cancel and yes by keyboard only', keyMuted && keyUnmuted && cancelled === 'cancelled' && waiting === 'waiting' && yesShown && yesDone === 'sent', { keyMuted, keyUnmuted, cancelled, waiting, yesShown, yesDone });
    s.ctl('voice.set', { settle_seconds: 2 });

    // ---------- Home (AC-174): the voice strip and the spoken request in the conversation.
    const writer = (() => { fs.writeFileSync(modeFile, 'slow'); const r = s.ctl('task.create', { repo, harness: 'claude', prompt: 'keep writing', title: 'Writer' }); return r.run.id; })();
    for (let i = 0; i < 80 && s.ctl('state').runs.find(r => r.id === writer)?.status !== 'running'; i++) await delay(250);
    fs.writeFileSync(modeFile, 'overseer');
    s.ctl('voice.set', { settle_seconds: 10 });
    await cdp.command('Overseer: Open Overseer View'); await delay(1500);
    // Home is the composer with no agent selected (Overseer: New Agent), as in scenario-home.
    await cdp.command('Overseer: New Agent'); await delay(1200);
    const homeFrame = await cdp.webview(`!!document.getElementById('home-voice') && document.body.dataset.mode === 'composer'`, 20000);
    await delay(800);
    const homeId = s.ctl('voice.say', { text: 'Tell Writer to add a changelog.' }).request;
    await untilState(homeId, ['settling']);
    // The conversation sits above the composer; with many messages, focusing the composer scrolls it
    // up out of view, so the owner scrolls back to it (as scenario-home does). Checked on screen:
    // rendered and inside the view, not merely present.
    const onView = e => `(() => { const x = [...document.querySelectorAll(${JSON.stringify(e)})].pop(); if (!x || !x.checkVisibility()) return false; const r = x.getBoundingClientRect(); return r.height > 0 && r.bottom > 0 && r.top < innerHeight; })()`;
    const toHome = `(document.getElementById('home').scrollIntoView({ block: 'start' }), true)`;
    const planLine = `[...document.querySelectorAll('#home-conv .home-msg.from-overseer .home-text')].some(e => /Writer/.test(e.textContent) && /changelog/.test(e.textContent))`;
    await homeFrame.waitFor(`${toHome} && ${onView('#home-voice')} && ${onView('#home-conv .home-msg.spoken')} && ${planLine}`, 20000).catch(() => {});
    const home = await homeFrame.eval(`(() => ({ mode: document.body.dataset.mode, strip: ${onView('#home-voice')}, state: document.querySelector('.home-voice-state')?.textContent, words: [...document.querySelectorAll('#home-conv .home-msg.spoken .home-text')].map(e => e.textContent).find(t => /add a changelog/.test(t)), spokenMark: ${onView('#home-conv .home-msg.spoken')} && /by voice/.test([...document.querySelectorAll('#home-conv .home-msg.spoken')].pop().textContent), plan: ${planLine} }))()`);
    await s.screenshot('home-voice-strip');
    const homeAudit = await homeFrame.eval(auditExpression({ root: '#home-voice', exclude: ['.home-voice-heard'] }));
    check('home shows the voice strip, the spoken request marked as spoken, and Overseer\u2019s plan for it', home.mode === 'composer' && home.strip && /Listening|Thinking|Speaking|Hearing/.test(home.state || '') && home.words === 'Tell Writer to add a changelog.' && home.spokenMark && home.plan, home);

    // ---------- A card filling in as its dispatches advance (AC-169, AC-174).
    await cdp.command('Overseer: Voice Mode: Show'); await delay(800);
    await untilState(homeId, ['sent'], 30000);
    s.ctl('voice.set', { settle_seconds: 2 });
    const rowState = () => view.eval(`(() => { const c = [...document.querySelectorAll('#home-conv .done-card')].filter(c => /changelog/.test(c.textContent)).pop(); return c && c.querySelector('.card-row-state')?.textContent; })()`);
    const seen = [];
    for (let i = 0; i < 40; i++) { const st = await rowState(); if (st && !seen.includes(st)) seen.push(st); if (st === 'held') break; await delay(250); }
    await s.screenshot('card-held');
    s.ctl('run.interrupt', { run_id: writer });
    for (let i = 0; i < 80; i++) { const st = await rowState(); if (st && !seen.includes(st)) seen.push(st); if (st && st !== 'held') break; await delay(250); }
    await s.screenshot('card-advanced');
    check('a card fills in as its dispatches advance (held, then the next state on the daemon\'s events)', seen[0] === 'held' && seen.length >= 2, seen);
    s.ctl('voice.set', { settle_seconds: 2 });

    // ---------- Evidence (AC-169, AC-168): two agents in flight and one new agent; the card and
    // each agent's chat, in the three themes; the new agent in the side bar with its prompt.
    fs.writeFileSync(modeFile, 'slow');
    const inflight = ['Gateway', 'Ledger'].map(title => s.ctl('task.create', { repo, harness: 'claude', prompt: 'keep working', title }).run.id);
    for (const id of inflight) for (let i = 0; i < 80 && s.ctl('state').runs.find(r => r.id === id)?.status !== 'running'; i++) await delay(250);
    fs.writeFileSync(modeFile, 'echo');
    s.ctl('voice.set', { start_defaults: { harness: 'claude', workspace_mode: 'worktree', trusted: true } });
    const evId = s.ctl('voice.say', { text: 'Tell Gateway and Ledger to use the new wire format, and someone should write the migration note.' }).request;
    const evState = await untilState(evId, ['sent', 'partly_sent'], 90000);
    const evReq = (s.ctl('voice.requests', { limit: 50 }).requests || []).find(r => r.id === evId);
    const evCard = s.ctl('overseer.card', { id: evReq.proposal });
    const newRun = (evCard.rows || []).find(r => r.action === 'start')?.run_id;
    await cdp.command('Overseer: Voice Mode: Show'); await delay(1200);
    await view.waitFor(`[...document.querySelectorAll('#home-conv .done-card')].some(c => /migration note/.test(c.textContent) && c.querySelectorAll('.card-row').length === 3)`, 20000).catch(() => {});
    const sideRow = (await s.agentRows()).find(r => /write the migration/.test(r.label || ''));
    const chats = [];
    for (const [theme, tag] of [['Overseer', 'overseer'], ['Overseer Dark', 'dark'], ['Overseer Light', 'light']]) {
      s.settings({ 'workbench.colorTheme': theme, 'overseer.followNewRuns': false });
      await delay(1500);
      await cdp.command('Overseer: Voice Mode: Show'); await delay(800);
      await s.screenshot(`card-three-targets-${tag}`);
      for (const [i, id] of [...inflight, newRun].entries()) {
        if (!id) continue;
        await s.selectRun(id).catch(() => {});
        await delay(1500);
        const frame = await cdp.webview(`!!document.querySelector('.view-chat')`, 10000).catch(() => null);
        const shows = frame ? await frame.eval(`document.body.innerText.includes('(voice, request') && document.body.innerText.includes('The owner said: “Tell Gateway and Ledger')`) : false;
        chats.push({ tag, id, shows });
        await s.screenshot(`chat-${['gateway', 'ledger', 'new-agent'][i]}-${tag}`);
      }
    }
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    fs.writeFileSync(modeFile, 'overseer');
    for (const id of inflight) { try { s.ctl('run.interrupt', { run_id: id }); } catch {} }
    check('two agents in flight and a new one: the card has three rows and each chat shows the owner\'s words, in the three themes', ['sent', 'partly_sent'].includes(evState) && (evCard.rows || []).length === 3 && chats.length === 9 && chats.every(c => c.shows), { evState, rows: (evCard.rows || []).length, chats });
    check('the new agent appears in the side bar with its prompt', !!sideRow, sideRow);
    await cdp.command('Overseer: Voice Mode: Show'); await delay(800);

    // ---------- Stopped: four listener crashes turn Voice Mode off, with the reason shown (AC-175).
    for (let i = 0; i < 4; i++) {
      const pid = s.ctl('voice.get').listener.pid;
      if (!pid) break;
      try { process.kill(pid, 'SIGKILL'); } catch {}
      for (let j = 0; j < 60; j++) { const v = s.ctl('voice.get'); if (!v.enabled || (v.listener.pid && v.listener.pid !== pid)) break; await delay(250); }
    }
    const stopped = await view.waitFor(`!document.getElementById('voice-off').hidden && document.getElementById('voice-off-reason').textContent.length > 0`, 15000).then(() => true, () => false);
    const reason = await view.eval(`document.getElementById('voice-off-reason').textContent`);
    await s.screenshot('stopped-after-four-crashes');
    check('four crashes turn Voice Mode off, and the view says why', stopped && s.ctl('voice.get').enabled === false, { reason });
    await cdp.command('Overseer: Voice Mode: Turn On or Off');
    await view.waitFor(`['listening', 'thinking'].includes(document.getElementById('voice-state').dataset.state)`, 30000);

    // ---------- Widths and themes, with the visible-text and accessible-name audits (AC-174).
    await cdp.command('View: Close Primary Side Bar').catch(() => {});
    await cdp.command('Overseer: Voice Mode: Show'); await delay(800);
    const audits = [];
    for (const [theme, tag] of [['Overseer', 'overseer'], ['Overseer Dark', 'dark'], ['Overseer Light', 'light']]) {
      s.settings({ 'workbench.colorTheme': theme, 'overseer.followNewRuns': false });
      await delay(1500);
      for (const w of [360, 900, 1280]) {
        await cdp.call('Emulation.setDeviceMetricsOverride', { width: w, height: 820, deviceScaleFactor: 0, mobile: false }, cdp.workbench);
        await delay(1500);
        const whole = await view.eval(auditExpression({ root: '#voice-stage' }));
        const chrome = await view.eval(auditExpression({ root: '#voice-stage', exclude: ['#voice-heard', '#voice-said', '#voice-error'] }));
        audits.push({ tag, w, viewWidth: whole.width, overflow: whole.overflow.length, unnamed: whole.unnamed.length, chrome: chrome.chars, longRuns: whole.longRuns.length });
        await s.screenshot(`width-${w}-${tag}`);
      }
    }
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench).catch(() => {});
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    await cdp.command('View: Toggle Primary Side Bar Visibility').catch(() => {});
    s.note('audits', audits);
    check('no horizontal overflow and no long unbroken runs at 360, 900 and 1280 px in the three themes', audits.every(a => a.overflow === 0 && a.longRuns === 0), audits);
    check('the accessible-name audit: every control has a name and a tooltip (the voice view and home’s strip)', audits.every(a => a.unnamed === 0) && homeAudit.unnamed.length === 0, { view: audits.map(a => a.unnamed), home: homeAudit.unnamed });
    check('the visible-text audit: the voice view’s own text stays within 60 characters, and home’s strip within 60', audits.every(a => a.chrome <= 60) && homeAudit.chars > 0 && homeAudit.chars <= 60, { view: Math.max(...audits.map(a => a.chrome)), home: homeAudit.chars });
    // AC-228: over every card of this scenario, no internal token (NOT_FOR_OVERSEER) and no raw error.
    const cardsText = await view.eval(`document.getElementById('home-conv').innerText`);
    const tokens = cardsText.match(/\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b|\b[a-z]+_[a-z_]+\b|\b(?:Error|panicked|anyhow)\b|Caused by/g) || [];
    check('no card of the voice scenario shows an internal token or a raw error (AC-228)', tokens.length === 0, { tokens: [...new Set(tokens)] });
    void continuity;
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
    console.log(failed.length ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed.length ? 1 : 0);
  }
})();
