// Packaged-UI scenario for AC-247 (Claude Code fixture as Overseer, no paid tokens): home's input is
// always on screen. A 20-message conversation with Overseer, with Continuity's one-time notice
// showing; at 1280×800 and 1440×900 home's text box (#task) and its choices are fully inside the
// view, the conversation scrolls above them, and the notice is one line. Screenshots at both sizes.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('home-input');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'site-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'overseer');
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const idle = async (ms = 40000) => { const end = Date.now() + ms; while (Date.now() < end) { const x = s.ctl('overseer.session'); if (x.run_id && !['queued', 'starting', 'running'].includes(x.run_status)) return x; await delay(200); } return null; };

    // A 20-message conversation: ten questions, ten answers.
    for (let i = 1; i <= 10; i++) {
      s.ctl('overseer.send', { text: `Question ${i}: what is everyone doing?`, surface: 'vscode' });
      await delay(300); await idle();
    }
    const messages = s.ctl('overseer.session').messages.filter(m => !m.card).length;
    check('the conversation has at least 20 messages', messages >= 20, { messages });

    await cdp.command('Overseer: Talk to Overseer'); await delay(2500);
    const view = await s.editorView(`!!document.querySelector('#task') && document.querySelectorAll('#home-conv .home-msg').length >= 20`, 40000);
    const notice = await view.eval(`(() => { const n = document.querySelector('[data-continuity="notice"]'); return n ? { shown: n.checkVisibility(), height: Math.round(n.getBoundingClientRect().height) } : { shown: false }; })()`);
    s.note('Continuity notice', notice);

    const measure = () => view.eval(`(() => {
      const r = sel => { const e = document.querySelector(sel); if (!e || !e.checkVisibility()) return null; const b = e.getBoundingClientRect(); return { top: Math.round(b.top), bottom: Math.round(b.bottom), height: Math.round(b.height) }; };
      const list = document.getElementById('home-conv');
      const chips = [...document.querySelectorAll('.composer-choices .chip')].filter(c => c.checkVisibility()).map(c => { const b = c.getBoundingClientRect(); return Math.round(b.bottom); });
      const n = document.querySelector('[data-continuity="notice"]');
      return { vh: innerHeight, vw: innerWidth, task: r('#task'), box: r('.composer.big'), chips, choices: r('.composer-choices'), notice: n && n.checkVisibility() ? Math.round(n.getBoundingClientRect().height) : 0,
        list: list ? { scrolls: list.scrollHeight > list.clientHeight + 4, top: list.scrollTop, height: list.clientHeight, full: list.scrollHeight, bottom: Math.round(list.getBoundingClientRect().bottom) } : null };
    })()`);
    const sizes = {};
    for (const [w, h] of [[1280, 800], [1440, 900]]) {
      await cdp.call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 0, mobile: false }, cdp.workbench); await delay(1800);
      const m = await measure();
      sizes[`${w}x${h}`] = m;
      await s.screenshot(`home-20-messages-${w}x${h}`);
      const inView = !!m.task && m.task.top >= 0 && m.task.bottom <= m.vh && m.chips.length > 0 && m.chips.every(b => b <= m.vh);
      check(`at ${w}×${h} home's text box and its choices are fully on screen with a 20-message conversation`, inView, m);
      check(`at ${w}×${h} the conversation scrolls above the input`, !!m.list && m.list.scrolls && m.list.bottom <= (m.box ? m.box.top + 2 : m.vh), m.list);
      // Continuity's one-time notice, when shown, is one line and never pushes the input off.
      check(`at ${w}×${h} the Continuity notice (when shown) is one line`, !m.notice || m.notice <= 44, { notice: m.notice });
    }
    // Scrolling the conversation to its top leaves the input where it is.
    await view.eval(`document.getElementById('home-conv').scrollTop = 0`); await delay(400);
    const top = await measure();
    check('scrolling the conversation to its top leaves the input on screen', !!top.task && top.task.bottom <= top.vh, top.task);
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench).catch(() => {});
    result.sizes = sizes;
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
