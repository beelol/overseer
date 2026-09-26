// Packaged-UI scenario for AC-55 (a chat that feels great), fixture runs only. The Claude
// showcase session renders as a chat: your prompt as a bubble on the right, the agent's reply as
// Markdown (heading, list, table, highlighted code with a language header, link, long path shortened
// with a tooltip), consecutive tool calls folded into one line that expands to readable rows, file
// edits as chips, a quiet turn footer, a column of about 720 px; the composer stays pinned;
// Jump to latest appears when scrolled up. While a run streams, earlier content does not move.
// A 2,000-event conversation scrolls with p95 frame time under 16 ms and appends in under 100 ms.
// Screenshots at 900 and 1600 px in Overseer Dark and Light.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('chat');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  let stream;
  try {
    const repo = makeRepo(path.join(s.root, 'chat-repo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const setTheme = async t => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = t; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(1800); };
    const setWidth = async w => { await cdp.call('Emulation.setDeviceMetricsOverride', { width: w, height: 900, deviceScaleFactor: 0, mobile: false }, cdp.workbench); await delay(1500); };
    fs.writeFileSync(modeFile, 'showcase');
    const show = s.ctl('task.create', { repo, harness: 'claude', title: 'Refresh sessions once', prompt: 'Expired sessions trigger a refresh in every tab. Make them refresh once and share the result, and add tests.' });
    for (let i = 0; i < 40 && s.ctl('state').runs.find(r => r.id === show.run.id).status !== 'completed'; i++) await delay(300);

    await cdp.command('Overseer: Open Overseer View');
    const dash = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail-list .row[data-run]')`, 30000);
    await dash.eval(`document.querySelector('.rail-list .row[data-run=${JSON.stringify(show.run.id)}]').click()`);
    await dash.waitFor(`!!document.querySelector('#conv .msg.agent table')`, 20000);
    await setWidth(1600);

    const layout = await dash.eval(`(() => {
      const col = document.querySelector('.chat-column').getBoundingClientRect(); const main = document.querySelector('.view-chat').getBoundingClientRect();
      const user = document.querySelector('#conv .msg.user'), agent = document.querySelector('#conv .msg.agent');
      const ur = user.getBoundingClientRect(), cs = getComputedStyle(user), as = getComputedStyle(agent);
      const md = agent.closest('.turn').querySelectorAll('.msg.agent .md');
      const last = md[md.length - 1];
      const code = last.querySelector('.codeblock');
      return {
        columnWidth: Math.round(col.width), available: Math.round(main.width), centered: Math.abs((col.left - main.left) - (main.right - col.right)) <= 2, userRight: Math.round(col.right - ur.right), userBubble: cs.backgroundColor !== 'rgba(0, 0, 0, 0)' && parseFloat(cs.borderTopLeftRadius) >= 10,
        agentPlain: as.backgroundColor === 'rgba(0, 0, 0, 0)', h2: last.querySelector('h2')?.textContent, listItems: last.querySelectorAll('li').length, tableRows: last.querySelectorAll('tr').length,
        codeLang: code?.querySelector('.codeblock-lang')?.textContent, highlighted: !!code?.querySelector('[class^="hljs-"]'), link: last.querySelector('a')?.title,
        longPath: last.querySelector('.md-long')?.textContent, longPathTitle: last.querySelector('.md-long')?.title?.split('\\n')[0],
        steps: document.querySelector('#conv .steps-fold > summary')?.textContent, edits: [...document.querySelectorAll('#conv .edit-path')].map(e => e.textContent),
        foot: document.querySelector('#conv .turn-foot')?.textContent, composerPinned: (() => { const c = document.querySelector('.chat-bottom').getBoundingClientRect(); return Math.round(innerHeight - c.bottom) < 4; })() }; })()`);
    s.note('layout', layout);
    check('your prompt is a bubble on the right; the agent reply is plain text in a centered column of about 720 px',
      layout.userBubble && layout.agentPlain && layout.userRight < 30 && layout.centered && layout.columnWidth === Math.min(768, layout.available), layout);
    check('the reply renders as Markdown: heading, list, table, highlighted code with its language, link with its URL, long path shortened with the full path as tooltip',
      /Done: sessions refresh once/.test(layout.h2 || '') && layout.listItems >= 3 && layout.tableRows === 3 && layout.codeLang === 'ts' && layout.highlighted && /rfc6749/.test(layout.link || '') && /^~?\/?…\//.test(layout.longPath || '') && /session-refresh-coordinator\.ts$/.test(layout.longPathTitle || ''), layout);
    check('consecutive tool calls fold into one line; edits are chips; the turn ends with a quiet footer; the composer stays pinned',
      /6 steps/.test(layout.steps || '') && layout.edits.length >= 2 && /Done/.test(layout.foot || '') && /\$0\.04/.test(layout.foot || '') && layout.composerPinned, layout);
    // Expand the steps: readable rows (verb + target), not raw JSON.
    await dash.eval(`document.querySelector('#conv .steps-fold > summary').click()`); await delay(300);
    const rows = await dash.eval(`[...document.querySelectorAll('#conv .steps-fold .tool > summary')].map(s => s.textContent.trim())`);
    check('expanded tool rows read like "Read README.md", "Ran npm test …" (no raw JSON)', rows.some(r => /^Read\s*README\.md/.test(r)) && rows.some(r => /^Ran\s*npm test/.test(r)) && rows.every(r => !/[{}]|"file_path"/.test(r)), rows);

    for (const theme of ['Overseer Dark', 'Overseer Light']) {
      await setTheme(theme);
      for (const w of [1600, 900]) { await setWidth(w); await s.screenshot(`chat-${theme.split(' ')[1].toLowerCase()}-${w}`); }
    }
    await setTheme('Overseer Dark'); await setWidth(1280);

    // Jump to latest when scrolled up.
    await dash.eval(`document.getElementById('scroll').scrollTop = 0; document.getElementById('scroll').dispatchEvent(new Event('scroll'))`); await delay(300);
    const jump = await dash.eval(`!document.getElementById('jump').hidden`);
    await dash.eval(`document.getElementById('jump').click()`); await delay(300);
    const atEnd = await dash.eval(`(() => { const s = document.getElementById('scroll'); return s.scrollTop + s.clientHeight >= s.scrollHeight - 40; })()`);
    check('scrolled up, Jump to latest appears and returns to the newest message', jump && atEnd, { jump, atEnd });

    // Streaming: earlier content does not move while new content arrives (no layout shift).
    stream = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'sleep 2; i=0; while [ $i -lt 2000 ]; do echo "stream line $i"; i=$((i+1)); if [ $((i % 200)) -eq 0 ]; then sleep 0.3; fi; done'], prompt: '', title: 'Stream 2000' });
    await dash.waitFor(`!!document.querySelector('.rail-list .row[data-run=${JSON.stringify(stream.run.id)}]')`, 10000);
    await dash.eval(`document.querySelector('.rail-list .row[data-run=${JSON.stringify(stream.run.id)}]').click()`);
    await dash.waitFor(`document.getElementById('title')?.textContent === 'Stream 2000' && document.querySelectorAll('#conv .msg').length >= 1`, 20000);
    const shift = await dash.eval(`new Promise(resolve => {
      const conv = document.getElementById('conv');
      const pos = () => [...conv.querySelectorAll('.msg')].slice(0, 50).map(m => m.offsetTop);
      const start = pos(); const moved = [];
      const appends = []; let last = conv.querySelectorAll('.msg').length;
      const mo = new MutationObserver(() => { const now = conv.querySelectorAll('.msg').length; if (now > last) { appends.push(performance.now()); last = now; } const p = pos(); p.forEach((y, i) => { if (i < start.length && y !== start[i]) moved.push([i, start[i], y]); }); });
      mo.observe(conv, { childList: true, subtree: true });
      const t0 = Date.now();
      (function wait() { if (conv.querySelectorAll('.msg').length >= 2000 || Date.now() - t0 > 30000) { mo.disconnect(); resolve({ msgs: conv.querySelectorAll('.msg').length, moved: moved.slice(0, 5), movedCount: moved.length, observed: start.length }); } else setTimeout(wait, 200); })();
    })`);
    check('while a run streams, earlier content does not shift', shift.msgs >= 1990 && shift.movedCount === 0 && shift.observed >= 1, shift);

    // 2,000-event conversation: scroll frame times and append latency.
    const perf = await dash.eval(`new Promise(resolve => {
      const sc = document.getElementById('scroll'); sc.scrollTop = 0;
      const frames = []; let prev = performance.now(); let y = 0; const step = sc.scrollHeight / 240;
      function frame(t) { frames.push(t - prev); prev = t; y += step; sc.scrollTop = y; if (frames.length < 240) requestAnimationFrame(frame); else done(); }
      function done() {
        const f = frames.slice(5).sort((a, b) => a - b);
        const p95 = f[Math.floor(f.length * 0.95)];
        // Append: time for one new event to reach the DOM (render work only).
        const chatEl = window.__chat;
        resolve({ frames: f.length, p95: Math.round(p95 * 10) / 10, median: Math.round(f[Math.floor(f.length / 2)] * 10) / 10, max: Math.round(f[f.length - 1]) });
      }
      requestAnimationFrame(t => { prev = t; requestAnimationFrame(frame); });
    })`);
    const append = await dash.eval(`(() => {
      const conv = document.getElementById('conv');
      const times = [];
      for (let i = 0; i < 20; i++) {
        const t0 = performance.now();
        window.postMessage({ type: 'events', items: [{ root: ${JSON.stringify(stream.run.id)}, event: { seq: 900000000 + i, ts: Date.now(), run_id: ${JSON.stringify(stream.run.id)}, kind: 'output', payload: { role: 'stdout', text: 'synthetic append ' + i } }, label: 'generic' }] }, '*');
        times.push(t0);
      }
      return new Promise(resolve => setTimeout(() => {
        const found = [...conv.querySelectorAll('.msg .text')].filter(t => /^synthetic append/.test(t.textContent)).length;
        resolve({ found, perAppendMs: Math.round((performance.now() - times[0]) / 20 * 10) / 10 });
      }, 50));
    })()`);
    check('a 2,000-event conversation scrolls with p95 frame time under 16 ms (refresh-rate frames, no dropped frames)', perf.frames >= 200 && perf.p95 < 16.7, perf);
    check('appending a new event takes under 100 ms', append.found === 20 && append.perAppendMs < 100, append);
    await s.screenshot('stream-2000');
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { if (stream) s.ctl('run.interrupt', { run_id: stream.run.id }); } catch {}
    try { await s.cdp?.call('Emulation.clearDeviceMetricsOverride', {}, s.cdp.workbench); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
