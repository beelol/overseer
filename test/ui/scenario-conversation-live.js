// LIVE scenario for AC-43 (tiny prompts: Claude Haiku, Codex gpt-5.6-luna): Codex exec,
// Codex app-server and Claude Code runs, each with a native child and a file edit; permission
// requests (Claude, app-server) are answered from the conversation's inline buttons. Checks the
// conversation structure, the child nested under its spawning tool call, and that clicking the
// file edit opens that run's worktree review at the hunk. One attempt per harness, no retries.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay } = require('./harness');

const LUNA = process.env.CODEX_MODEL || 'gpt-5.6-luna';
const SPECS = [
  { harness: 'codex', title: 'live exec', model: LUNA, file: 'hello.txt', prompt: 'Spawn exactly one sub-agent whose only job is to reply with exactly: hi. After it finishes, create hello.txt containing exactly one line: hello from codex. Then reply exactly: done' },
  { harness: 'codex-app', title: 'live app', model: LUNA, file: 'app.txt', approval_policy: 'untrusted', prompt: 'Spawn exactly one sub-agent whose only job is to reply with exactly: hi. After it finishes, create app.txt containing exactly one line: hello from app-server. Then reply exactly: done' },
  { harness: 'claude', title: 'live claude', model: 'haiku', file: 'hello.md', prompt: 'Use the Agent tool to launch one general-purpose subagent whose only job is to reply with exactly: hi. After it returns, use the Write tool to create hello.md containing exactly one line: hello from claude. Then reply exactly: done' },
];

(async () => {
  const s = new Session('conversation-live');
  const result = { checks: [], runs: {} };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const ACTIVE = ['queued', 'starting', 'running', 'waiting_for_user'];
  try {
    const repo = makeRepo(path.join(s.root, 'live-conv'), { dirty: false });
    s.settings();
    s.install(latestVsix());
    s.launch(repo);
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    await s.openOverseerView();
    const runState = id => s.ctl('state').runs.find(r => r.id === id);
    const selectRun = async (title, harness) => {
      const pt = await cdp.waitFor(`(() => { const rows = [...document.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top);
        const i = rows.findIndex(r => r.textContent.includes(${JSON.stringify(title)})); const r = rows[i + 1]; if (!r || !r.textContent.includes(${JSON.stringify(harness)})) return null; const b = r.getBoundingClientRect(); return { x: b.left + 60, y: b.top + b.height / 2 }; })()`, 30000, 'run row ' + title);
      await cdp.click(pt.x, pt.y);
      await delay(1500);
    };
    const clickIn = async (frame, selector) => { const p = await s.webviewPoint(frame, selector); await cdp.click(p.x, p.y); await delay(500); };
    for (const spec of SPECS.filter(x => !process.env.ONLY || process.env.ONLY.split(',').includes(x.harness))) {
      const created = s.ctl('task.create', { repo, harness: spec.harness, profile_id: spec.harness === 'claude' ? 'system-claude' : 'system-codex', model: spec.model, prompt: spec.prompt, title: spec.title, approval_policy: spec.approval_policy });
      const run = created.run.id;
      await selectRun(spec.title, spec.harness);
      const panel = await cdp.webview(`document.body.dataset.runId === ${JSON.stringify(run)} && !!document.getElementById('conv')`, 60000);
      const decisions = [];
      for (let i = 0; i < 480; i++) {
        const r = runState(run);
        if (!ACTIVE.includes(r.status)) break;
        const pending = await panel.eval(`(() => { const b = [...document.querySelectorAll('#conv .perm-card.pending button')].find(b => b.textContent === 'Allow once'); if (!b) return null; b.id = 'inline-allow'; b.scrollIntoView({ block: 'center' }); return b.closest('.perm-card').querySelector('.perm-head').textContent; })()`).catch(() => null);
        if (pending) { decisions.push(pending); await s.screenshot(`${spec.harness}-permission`); await clickIn(panel, '#inline-allow'); await delay(1500); continue; }
        await delay(500);
      }
      const done = runState(run);
      await delay(2000);
      // Permission toasts can cover the panel; clear them before clicking inside it.
      await cdp.command('Notifications: Clear All Notifications');
      await delay(500);
      const conv = await panel.eval(`(() => {
        const tools = [...document.querySelectorAll('#conv details.tool')].map(t => t.querySelector('.tool-name').textContent);
        const nested = [...document.querySelectorAll('#conv .tool-children > details.child')].map(c => ({ title: c.querySelector('.child-title').textContent, spawnedBy: c.parentElement.previousElementSibling?.querySelector('.tool-name')?.textContent, replies: [...c.querySelectorAll('.child-body .msg .text')].map(t => t.textContent.trim()).filter(t => !t.startsWith('[tool ') && !t.startsWith('Prompt: ')).slice(-3) }));
        const loose = [...document.querySelectorAll('#conv .turn-body > details.child')].map(c => c.querySelector('.child-title').textContent);
        return { turns: document.querySelectorAll('#conv .turn').length, prompt: document.querySelector('#conv .msg.user .text')?.textContent.slice(0, 60), agent: [...document.querySelectorAll('#conv .msg.agent .text')].map(t => t.textContent).pop(),
          tools, nested, loose, perms: [...document.querySelectorAll('#conv .perm-card .perm-head')].map(h => h.textContent), edits: [...document.querySelectorAll('#conv .edit-path')].map(b => b.textContent),
          done: document.querySelector('#conv .turn-foot .done')?.textContent, usage: document.querySelector('#conv .turn-foot .usage')?.textContent, errors: [...document.querySelectorAll('#conv .error-block')].map(e => e.textContent) }; })()`);
      result.runs[spec.harness] = { run, status: done.status, reason: done.exit_reason, decisions, conv };
      s.note(spec.harness + ' conversation', result.runs[spec.harness]);
      await s.screenshot(`${spec.harness}-conversation`);
      check(`${spec.harness}: run completed`, done.status === 'completed', { status: done.status, reason: done.exit_reason });
      check(`${spec.harness}: conversation has the prompt, agent reply, tool calls and per-turn usage`, conv.turns >= 1 && conv.prompt && conv.agent && conv.tools.length >= 1 && /completed/.test(conv.done || '') && !!conv.usage, conv);
      check(`${spec.harness}: native child nested under the tool call that spawned it, with its own output`, conv.nested.length >= 1 && conv.nested.every(c => /Agent|Task|spawn/.test(c.spawnedBy || '')) && conv.nested.some(c => c.replies.some(t => /^hi\.?$/i.test(t))), { nested: conv.nested, loose: conv.loose });
      if (spec.harness !== 'codex') check(`${spec.harness}: permission request answered inline and recorded`, decisions.length >= 1 && conv.perms.some(p => /Allowed/.test(p)), { decisions, perms: conv.perms });
      // Expand the first tool call and collapse it again.
      const picked = await panel.eval(`(() => { const s = [...document.querySelectorAll('#conv details.tool > summary')].find(x => x.getBoundingClientRect().height > 0); s.id = 'first-tool'; s.scrollIntoView({ block: 'center' }); return s.textContent; })()`);
      await delay(400);
      await clickIn(panel, '#first-tool');
      const opened = await panel.eval(`document.getElementById('first-tool').parentElement.open && document.getElementById('first-tool').parentElement.querySelectorAll('.tool-section *').length > 0`);
      await clickIn(panel, '#first-tool');
      const collapsed = await panel.eval(`!document.getElementById('first-tool').parentElement.open`);
      check(`${spec.harness}: tool call expands and collapses`, opened && collapsed, { picked, opened, collapsed });
      // File edit -> this run's worktree review at the hunk.
      const has = await panel.eval(`(() => { const b = [...document.querySelectorAll('#conv .edit-path')].find(b => b.textContent.endsWith(${JSON.stringify(spec.file)})); if (!b) return false; b.id = 'edit-link'; b.scrollIntoView({ block: 'center' }); return true; })()`);
      if (has) await clickIn(panel, '#edit-link');
      const review = has && await cdp.webview(`(document.body.dataset.revealed || '').startsWith(${JSON.stringify(spec.file)}) && document.getElementById('workspace-note').textContent.includes(${JSON.stringify(created.workspace.path)})`, 30000).catch(() => null);
      check(`${spec.harness}: clicking the file edit opens ${spec.file} in this run's worktree review`, !!review, { edits: conv.edits, workspace: created.workspace.path });
      await s.screenshot(`${spec.harness}-edit-in-review`);
    }
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { for (const r of s.ctl('state').runs.filter(r => !r.parent_run_id && ACTIVE.includes(r.status))) s.ctl('run.interrupt', { run_id: r.id }); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
