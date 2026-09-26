// Packaged-UI scenario for AC-43 with SYNTHETIC fixture harnesses (no paid tokens):
// codex-app fixture (child + grandchild threads, command approval, file change), Claude
// fixture (Write tool with a permission request) and generic bursts at the retention bound.
// Checks the run panel reads as a conversation: turns, collapsible tool calls with inputs and
// results, children nested under the spawning tool, inline permission decisions, file edits
// that open the right worktree's review at the hunk, errors/usage, the event log tab, and
// responsiveness with truncation visible. The live counterpart is scenario-conversation-live.js.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('conversation');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const fx = name => path.join(repoRoot, 'fixtures/fake-harness', name);
  try {
    const repo = makeRepo(path.join(s.root, 'conv-demo'), { dirty: false });
    s.settings();
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_CODEX_PATH: fx('codex-app-fixture.js'), OVERSEER_CLAUDE_PATH: fx('claude-fixture.js'), OVERSEER_HARNESS_ENV_PASSTHROUGH: 'FIXTURE_MODE,CLAUDE_FIXTURE_MODE', FIXTURE_MODE: 'tree', CLAUDE_FIXTURE_MODE: 'permission' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const app = s.ctl('task.create', { repo, harness: 'codex-app', prompt: 'spawn a child, then touch approved.txt', title: 'app tree', approval_policy: 'on-request' });
    const claude = s.ctl('task.create', { repo, harness: 'claude', prompt: 'write perm.txt', title: 'claude write' });
    const burst = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'i=0; while [ $i -lt 6000 ]; do echo "burst line $i"; i=$((i+1)); done'], prompt: '', title: 'burst history' });
    const runState = id => s.ctl('state').runs.find(r => r.id === id);
    for (let i = 0; i < 60 && runState(app.run.id).status !== 'waiting_for_user'; i++) await delay(500);
    for (let i = 0; i < 60 && runState(claude.run.id).status !== 'waiting_for_user'; i++) await delay(500);
    for (let i = 0; i < 60 && runState(burst.run.id).status !== 'completed'; i++) await delay(500);

    await s.openOverseerView();
    const selectRun = async (title, harness) => {
      const pt = await cdp.waitFor(`(() => { const rows = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top);
        const i = rows.findIndex(r => r.textContent.includes(${JSON.stringify(title)})); const r = rows[i + 1]; if (!r || !r.textContent.includes(${JSON.stringify(harness)})) return null; const b = r.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 20000, 'run row ' + title);
      await cdp.click(pt.x, pt.y);
      await delay(1500);
    };
    const panel = runId => cdp.webview(`document.body.dataset.runId === ${JSON.stringify(runId)} && !!document.querySelector('#conv .turn')`, 30000);
    const clickIn = async (frame, selector) => { const p = await s.webviewPoint(frame, selector); await cdp.click(p.x, p.y); await delay(400); };
    const tag = (frame, expr, id) => frame.eval(`(() => { const e = ${expr}; if (!e) return false; e.id = ${JSON.stringify(id)}; e.scrollIntoView({ block: 'center' }); return true; })()`);

    // --- codex-app fixture: turns, nested children, inline permission, file edit.
    await selectRun('app tree', 'codex-app');
    const a = await panel(app.run.id);
    const tree = await a.eval(`(() => { const spawn = [...document.querySelectorAll('#conv details.tool')].find(t => t.querySelector('.tool-name').textContent === 'collab:spawn_agent');
      const kids = spawn && spawn.nextElementSibling; const child = kids && kids.querySelector(':scope > details.child'); const grand = child && child.querySelector('details.child');
      return { turns: document.querySelectorAll('#conv .turn').length, prompt: document.querySelector('#conv .msg.user .text')?.textContent, spawn: !!spawn, child: child?.querySelector('.child-title').textContent,
        childText: child?.querySelector('.child-body .msg .text')?.textContent, grand: grand?.querySelector('.child-title').textContent }; })()`);
    check('conversation shows the turn with its prompt', tree.turns >= 1 && /spawn a child/.test(tree.prompt || ''), tree);
    check('native child nested under the spawning tool call, grandchild under the child, with their own output', tree.spawn && tree.child === 'child task' && tree.childText === 'child output' && tree.grand === 'grandchild task', tree);
    const pending = await a.eval(`(() => { const c = document.querySelector('#conv .perm-card.pending'); return c && { head: c.querySelector('.perm-head').textContent, buttons: [...c.querySelectorAll('button')].map(b => b.textContent) }; })()`);
    check('permission request inline in the conversation with Allow/Deny', pending && /Waiting for your permission: commandExecution|Waiting for your permission/.test(pending.head) && pending.buttons.includes('Allow once'), pending);
    await s.screenshot('codex-app-pending');
    await tag(a, `[...document.querySelectorAll('#conv .perm-card.pending button')].find(b => b.textContent === 'Allow once')`, 'inline-allow');
    await clickIn(a, '#inline-allow');
    await a.waitFor(`[...document.querySelectorAll('#conv .perm-card')].some(c => /Allowed/.test(c.querySelector('.perm-head').textContent))`, 20000);
    await a.waitFor(`!!document.querySelector('#conv .turn-foot .done.ok') && !!document.querySelector('#conv .edit-path')`, 20000);
    const after = await a.eval(`({ decision: [...document.querySelectorAll('#conv .perm-card .perm-head')].map(h => h.textContent), done: document.querySelector('#conv .turn-foot .done').textContent, usage: document.querySelector('#conv .turn-foot .usage').textContent, edits: [...document.querySelectorAll('#conv .edit-path')].map(b => b.textContent) })`);
    check('decision recorded inline, turn completed with usage, file edit listed', after.decision.some(d => /Allowed/.test(d)) && /completed/.test(after.done) && /1 in/.test(after.usage) && after.edits.includes('approved.txt'), after);
    // Expand and collapse a tool call.
    await tag(a, `[...document.querySelectorAll('#conv details.tool')].find(t => t.querySelector('.tool-name').textContent === 'shell')?.querySelector('summary')`, 'shell-summary');
    await clickIn(a, '#shell-summary');
    const open = await a.eval(`(() => { const d = document.getElementById('shell-summary').parentElement; return { open: d.open, input: d.querySelector('.tool-section pre')?.textContent, status: d.querySelector('.badge').textContent }; })()`);
    await s.screenshot('tool-expanded');
    await clickIn(a, '#shell-summary');
    const closed = await a.eval(`!document.getElementById('shell-summary').parentElement.open`);
    check('tool call expands to its input and status, and collapses again', open.open && /touch approved.txt/.test(open.input || '') && open.status === 'completed' && closed, { open, closed });
    // File edit -> review of the right worktree at the hunk.
    await tag(a, `[...document.querySelectorAll('#conv .edit-path')].find(b => b.textContent === 'approved.txt')`, 'edit-approved');
    await clickIn(a, '#edit-approved');
    const reviewA = await cdp.webview(`document.body.dataset.revealed === 'approved.txt:1' && document.getElementById('workspace-note').textContent.includes(${JSON.stringify(app.workspace.path)})`, 30000).catch(() => null);
    check('clicking a file edit opens that hunk in the run\'s worktree review', !!reviewA, app.workspace.path);
    await s.screenshot('edit-opened-in-review');

    // --- Claude fixture: Write tool with a permission request, answered inline; edit opens the Claude worktree.
    await selectRun('claude write', 'claude');
    const c = await panel(claude.run.id);
    await c.waitFor(`!!document.querySelector('#conv .perm-card.pending')`, 20000);
    const write = await c.eval(`(() => { const t = [...document.querySelectorAll('#conv details.tool')].find(t => t.querySelector('.tool-name').textContent === 'Write'); return t && { summary: t.querySelector('.tool-summary').textContent }; })()`);
    check('Claude Write tool call shown before its permission request', write && /perm.txt/.test(write.summary), write);
    await tag(c, `[...document.querySelectorAll('#conv .perm-card.pending button')].find(b => b.textContent === 'Allow once')`, 'inline-allow');
    await clickIn(c, '#inline-allow');
    await c.waitFor(`!!document.querySelector('#conv .turn-foot .done.ok')`, 20000);
    await tag(c, `[...document.querySelectorAll('#conv details.tool')].find(t => t.querySelector('.tool-name').textContent === 'Write')?.querySelector('summary')`, 'write-summary');
    for (let i = 0; i < 3 && !(await c.eval(`document.getElementById('write-summary').parentElement.open`)); i++) { await c.eval(`document.getElementById('write-summary').scrollIntoView({ block: 'center' })`); await delay(300); await clickIn(c, '#write-summary'); }
    const writeOpen = await c.eval(`(() => { const d = document.getElementById('write-summary').parentElement; return { pres: [...d.querySelectorAll('pre')].map(p => p.textContent), status: d.querySelector('.badge').textContent }; })()`);
    check('Claude tool call shows input and result', writeOpen.pres.some(p => /"file_path"/.test(p)) && writeOpen.pres.some(p => /File created successfully/.test(p)) && writeOpen.status === 'completed', writeOpen);
    await tag(c, `[...document.querySelectorAll('#conv .edit-path')].find(b => b.textContent === 'perm.txt')`, 'edit-perm');
    await clickIn(c, '#edit-perm');
    const reviewC = await cdp.webview(`document.body.dataset.revealed === 'perm.txt:1' && document.getElementById('workspace-note').textContent.includes(${JSON.stringify(claude.workspace.path)})`, 30000).catch(() => null);
    check('Claude file edit opens the Claude worktree review (not the codex-app one)', !!reviewC, claude.workspace.path);
    // Event log tab keeps the raw event stream.
    await clickIn(c, '#tab-log');
    const logRows = await c.eval(`document.getElementById('log').hidden ? -1 : document.querySelectorAll('#log .ev').length`);
    await s.screenshot('event-log-tab');
    await clickIn(c, '#tab-conv');
    check('Event log tab lists the raw events; conversation returns', logRows > 5 && await c.eval(`!document.getElementById('conv').hidden`), logRows);

    // --- Retention-bound history and a live burst stay responsive; truncation is visible.
    await selectRun('burst history', 'generic');
    const b = await panel(burst.run.id);
    const hist = await b.eval(`({ last: [...document.querySelectorAll('#conv .msg .text')].pop()?.textContent, ms: Number(document.body.dataset.historyMs), events: Number(document.body.dataset.historyEvents), banner: document.querySelector('#conv .conv-banner:not([hidden])')?.textContent, msgs: document.querySelectorAll('#conv .msg').length })`);
    check('history at the retention bound renders within 1.5 s with truncation visible', hist.ms < 1500 && hist.events >= 4900 && /truncated/.test(hist.banner || '') && hist.last === 'burst line 5999', hist);
    const live = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'sleep 4; i=0; while [ $i -lt 6000 ]; do echo "live line $i"; i=$((i+1)); done; sleep 1'], prompt: '', title: 'burst live' });
    await selectRun('burst live', 'generic');
    const l = await panel(live.run.id).catch(async () => { await delay(3000); return cdp.webview(`document.body.dataset.runId === ${JSON.stringify(live.run.id)}`, 30000); });
    await l.eval(`window.__lag = []; (function tick() { const t0 = performance.now(); if (window.__lag.length < 600) setTimeout(() => { window.__lag.push(performance.now() - t0 - 50); tick(); }, 50); })()`);
    for (let i = 0; i < 60 && runState(live.run.id).status !== 'completed'; i++) await delay(500);
    await delay(1500);
    const lag = await l.eval(`(() => { const a = window.__lag.slice().sort((x, y) => x - y); return { n: a.length, p95: Math.round(a[Math.floor(a.length * 0.95)] || 0), max: Math.round(a[a.length - 1] || 0), msgs: document.querySelectorAll('#conv .msg').length }; })()`);
    check('live burst keeps the panel responsive (event-loop lag p95 under 250 ms, the AC-35 bound) and shows the newest output', lag.n > 40 && lag.p95 < 250 && lag.msgs >= 4900, lag);
    await s.screenshot('burst');
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
