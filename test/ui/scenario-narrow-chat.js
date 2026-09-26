// Packaged-UI scenario for AC-77 (a chat that works beside a diff), Claude fixture (no paid
// tokens): the showcase agent (Markdown, a table, code, six tool calls, two edits) is shown with
// its review, and the window is sized so the chat column is about 640, 480 and 360 px wide, in
// Overseer Dark and Overseer Light. At each width: nothing overflows sideways, no text run over
// 80 characters outside code, the agent's name is fully readable in the header, tool steps stay
// folded, code blocks scroll sideways inside themselves, and the composer fits.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');
const { auditExpression } = require('./audit');

const TARGETS = [{ chat: 640, window: 1960 }, { chat: 480, window: 1480 }, { chat: 360, window: 980 }];
const THEMES = ['Overseer Dark', 'Overseer Light'];

(async () => {
  const s = new Session('narrow-chat');
  const result = { checks: [], audits: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'narrow-repo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': THEMES[0], 'window.dialogStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode', CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer/.test(e.textContent))`, 60000, 'status bar');
    fs.writeFileSync(modeFile, 'showcase');
    const t = s.ctl('task.create', { repo, harness: 'claude', profile_id: 'system-claude', title: 'Refresh sessions once', prompt: 'Expired sessions trigger a refresh in every tab. Make them refresh once and share the result, and add tests.' });
    for (let i = 0; i < 40 && s.ctl('state').runs.find(r => r.id === t.run.id).status !== 'completed'; i++) await delay(300);
    await cdp.command('View: Close Primary Side Bar'); await delay(500);
    const theme = async name => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = name; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(1500); };
    for (const th of THEMES) {
      await theme(th);
      for (const target of TARGETS) {
        await cdp.call('Emulation.setDeviceMetricsOverride', { width: target.window, height: 1000, deviceScaleFactor: 0, mobile: false }, cdp.workbench); await delay(1200);
        // Select (again) so the arrangement sizes the chat for this window.
        await cdp.command('Overseer: Switch Agent…'); await cdp.waitQuickTitle('Switch to agent'); await cdp.type('Refresh sessions once'); await delay(300); await cdp.key('Enter'); await delay(2500);
        const chat = await cdp.webview(`document.getElementById('title')?.textContent === 'Refresh sessions once' && !!document.querySelector('#conv .msg')`, 30000);
        const a = await chat.eval(`(() => {
          const audit = ${auditExpression({ root: '.view-chat' })};
          const title = document.getElementById('title');
          const tb = title.getBoundingClientRect();
          const nameVisible = title.scrollWidth <= title.clientWidth + 1 && title.scrollHeight <= title.clientHeight + 2 && tb.width > 60;
          const steps = [...document.querySelectorAll('#conv details.steps-fold')].map(d => d.open);
          const code = [...document.querySelectorAll('#conv pre')].map(p => ({ scrolls: getComputedStyle(p).overflowX === 'auto' || getComputedStyle(p).overflowX === 'scroll' || p.scrollWidth <= p.clientWidth + 1, inside: p.getBoundingClientRect().right <= innerWidth + 1 }));
          const composer = document.querySelector('.composer, .chat-composer, #prompt')?.closest('form, .composer, .chat-composer') || document.getElementById('prompt').parentElement;
          const cb = composer.getBoundingClientRect();
          return { width: innerWidth, audit, nameVisible, name: title.textContent, steps, code, composerFits: cb.left >= -1 && cb.right <= innerWidth + 1,
            docOverflow: document.documentElement.scrollWidth > innerWidth + 1, scrollOverflow: (() => { const sc = document.getElementById('scroll'); return sc.scrollWidth > sc.clientWidth + 1; })() };
        })()`);
        result.audits.push({ theme: th, target: target.chat, ...a });
        await s.screenshot(`chat-${th.split(' ')[1].toLowerCase()}-${target.chat}`);
      }
    }
    await cdp.call('Emulation.clearDeviceMetricsOverride', {}, cdp.workbench).catch(() => {});
    const A = result.audits;
    s.note('audits', A.map(a => ({ theme: a.theme, target: a.target, width: a.width, overflow: a.audit.overflow, longRuns: a.audit.longRuns.length, nameVisible: a.nameVisible, steps: a.steps, docOverflow: a.docOverflow })));
    check('the chat measured about 640, 480 and 360 px wide (360 is the floor)', A.every(a => a.width >= 355 && Math.abs(a.width - a.target) <= 60), A.map(a => [a.target, a.width]));
    check('nothing in the chat overflows sideways at any width, in both themes', A.every(a => !a.docOverflow && !a.scrollOverflow && (a.audit.overflow || []).length === 0), A.map(a => [a.theme, a.target, a.docOverflow, a.scrollOverflow, a.audit.overflow]));
    check('no text run over 80 characters outside code', A.every(a => a.audit.longRuns.length === 0), A.map(a => a.audit.longRuns));
    check("the header keeps the agent's name fully readable", A.every(a => a.nameVisible && a.name === 'Refresh sessions once'), A.map(a => [a.target, a.nameVisible]));
    check('tool steps stay folded; code blocks scroll sideways inside themselves; the composer fits', A.every(a => a.steps.length > 0 && a.steps.every(o => !o) && a.code.every(c => c.scrolls && c.inside) && a.composerFits), A.map(a => [a.target, a.steps, a.code, a.composerFits]));
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
