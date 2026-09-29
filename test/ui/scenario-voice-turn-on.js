// Packaged-UI scenario for AC-217's gap (simulated voice, Claude fixture as Overseer, no paid turn):
// turning Voice Mode on from home, recorded in the three Overseer themes. In each theme home's
// Voice button is pressed while the window is recorded (Chrome's screencast of the workbench):
// the recording is kept as an animated GIF, and three frames of it (before, during and after) as
// screenshots; the mark's stage is measured growing in (its opacity and scale move from the start
// of the animation to rest); the line "Stop, mute and what's running still work" is absent.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('voice-turn-on');
  const result = { checks: [], recordings: {} };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const THEMES = [['Overseer', 'overseer'], ['Overseer Dark', 'dark'], ['Overseer Light', 'light']];
  const base = { 'overseer.followNewRuns': false };
  try {
    const repo = makeRepo(path.join(s.root, 'site-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer', ...base });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'overseer');
    s.launch(repo, {
      OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE',
      OVERSEER_VOICE_SIMULATE: '1', OVERSEER_LISTENER_TEST_VOICE: '1', OVERSEER_LISTENER_TEST_MIC_USERS: path.join(s.root, 'mic-users'),
    });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const ffmpeg = cp.spawnSync('ffmpeg', ['-version'], { encoding: 'utf8' }).status === 0;
    s.note('ffmpeg', ffmpeg ? 'found' : 'not found: frames only');

    // The workbench's screencast: every frame the compositor draws, with its time.
    let frames = null;
    cdp.socket.addEventListener('message', ev => {
      const m = JSON.parse(ev.data);
      if (m.method !== 'Page.screencastFrame' || !frames) return;
      frames.push({ data: m.params.data, at: m.params.metadata.timestamp, meta: m.params.metadata });
      cdp.call('Page.screencastFrameAck', { sessionId: m.params.sessionId }, m.sessionId).catch(() => {});
    });
    const record = async (ms, during) => {
      frames = [];
      await cdp.call('Page.startScreencast', { format: 'png', everyNthFrame: 1 }, cdp.workbench);
      await delay(250);
      const samples = await during();
      await delay(ms);
      await cdp.call('Page.stopScreencast', {}, cdp.workbench);
      const got = frames; frames = null;
      return { got, samples };
    };

    for (const [theme, tag] of THEMES) {
      s.settings({ 'workbench.colorTheme': theme, ...base });
      await delay(1800);
      await cdp.command('Overseer: Talk to Overseer'); await delay(1200);
      const view = await s.editorView(`!!document.getElementById('home-voice-toggle')`);
      if (await view.eval(`document.body.dataset.voice === 'on'`)) { const p = await s.webviewPoint(view, '#home-voice-toggle'); await cdp.click(p.x, p.y); await view.waitFor(`document.body.dataset.voice !== 'on'`, 15000); await delay(800); }
      await s.screenshot(`before-${tag}`);
      const toggle = await s.webviewPoint(view, '#home-voice-toggle');
      const rect = await cdp.evalWorkbench(`(() => { const f = [...document.querySelectorAll('iframe.webview')].map(f => f.getBoundingClientRect()).filter(r => r.width > 300).sort((a, b) => b.width * b.height - a.width * a.height)[0]; return f ? { x: f.left, y: f.top, w: f.width, h: f.height, vw: innerWidth, vh: innerHeight } : null; })()`);
      let clicked = 0;
      const { got, samples } = await record(1700, async () => {
        clicked = Date.now() / 1000;
        await cdp.click(toggle.x, toggle.y);
        // The stage's opacity and scale while it grows in, then at rest.
        const probe = `(() => { const st = document.getElementById('voice-stage'); if (!st || st.hidden) return null; const cs = getComputedStyle(st); const m = /matrix\\(([^,]+)/.exec(cs.transform); return { opacity: Number(cs.opacity), scale: m ? Number(m[1]) : 1 }; })()`;
        const out = [];
        for (const wait of [60, 140, 160, 700]) { await delay(wait); out.push(await view.eval(probe).catch(() => null)); }
        return out;
      });
      await view.waitFor(`document.body.dataset.voice === 'on'`, 15000);
      const moving = samples.filter(Boolean);
      const grew = moving.length >= 2 && (moving[0].opacity < 0.99 || moving[0].scale < 0.99) && moving[moving.length - 1].opacity >= 0.99 && Math.abs(moving[moving.length - 1].scale - 1) < 0.01;
      check(`${theme}: the mark's stage grows in when Voice Mode is turned on (opacity and scale move to rest)`, grew, samples);
      // Frames: kept as PNGs for the three moments and as an animated recording.
      const dir = path.join(s.root, `frames-${tag}`); fs.mkdirSync(dir, { recursive: true });
      got.forEach((f, i) => fs.writeFileSync(path.join(dir, `f${String(i).padStart(4, '0')}.png`), Buffer.from(f.data, 'base64')));
      const t0 = got.length ? got[0].at : 0;
      // Frames are stamped with the time they were drawn (seconds, as Date.now()): the moment
      // before the click, 150 ms into the 500 ms animation, and at rest.
      const pick = at => got.reduce((best, f, i) => (Math.abs(f.at - at) < Math.abs(got[best].at - at) ? i : best), 0);
      const before = got.reduce((b, f, i) => (f.at < clicked ? i : b), 0);
      const moments = got.length ? [['1-start', before], ['2-during', pick(clicked + 0.15)], ['3-after', got.length - 1]] : [];
      s.note(`${tag} frames`, { first: t0, clicked, during: got[pick(clicked + 0.15)]?.at, count: got.length });
      for (const [name, i] of moments) fs.copyFileSync(path.join(dir, `f${String(i).padStart(4, '0')}.png`), path.join(s.evidence, `turning-on-${tag}-${name}.png`));
      let gif = '';
      if (ffmpeg && got.length > 4 && rect) {
        // Only the Overseer view, at the frames' own pace (their timestamps), as a small GIF.
        const k = got[0].meta.deviceWidth / rect.vw;
        const concat = got.map((f, i) => `file 'f${String(i).padStart(4, '0')}.png'\nduration ${Math.max(0.01, (got[i + 1] ? got[i + 1].at - f.at : 0.2)).toFixed(3)}`).join('\n') + `\nfile 'f${String(got.length - 1).padStart(4, '0')}.png'\n`;
        fs.writeFileSync(path.join(dir, 'list.txt'), concat);
        gif = path.join(s.evidence, `turning-on-${tag}.gif`);
        const crop = `crop=${Math.round(rect.w * k)}:${Math.round(rect.h * k)}:${Math.round(rect.x * k)}:${Math.round(rect.y * k)}`;
        const r = cp.spawnSync('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i', 'list.txt', '-vf', `${crop},fps=25,scale=720:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128[p];[b][p]paletteuse`, '-loop', '0', gif], { cwd: dir, encoding: 'utf8' });
        if (r.status !== 0) { s.note('ffmpeg failed', r.stderr); gif = ''; }
      }
      result.recordings[tag] = { frames: got.length, seconds: got.length ? +(got[got.length - 1].at - t0).toFixed(2) : 0, gif: gif ? path.relative(repoRoot, gif) : '' };
      check(`${theme}: turning it on is recorded (${got.length} frames), with frames before, during and after`, got.length >= 10 && moments.length === 3 && (!ffmpeg || !!gif), result.recordings[tag]);
      await s.screenshot(`after-${tag}`);
      const removed = await view.eval(`!document.body.innerText.includes("Stop, mute and what's running still work")`);
      check(`${theme}: the line "Stop, mute and what's running still work" is absent`, removed);
      // Off again for the next theme.
      const off = await s.webviewPoint(view, '#home-voice-toggle'); await cdp.click(off.x, off.y);
      await view.waitFor(`document.body.dataset.voice === 'off'`, 15000); await delay(800);
    }
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
