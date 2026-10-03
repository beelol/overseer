// Packaged-UI scenario for AC-252 (the zero-friction loop, measured), the Claude fixture as Overseer
// and as the agents, the simulated voice (no microphone, no paid turn). From a cold window, the five
// things the owner does most, each counted (keys, commands and sentences; a pick from the list a
// key opens and the words of a request are what the action says, not more actions) and timed:
//   1. open Overseer                ⌥⌘⇧O                    "Open Overseer."
//   2. tell it to do something      ⌥⌘O, the words, Enter   "Overseer, someone should …" (+ one yes)
//   3. follow an agent              ⌥⌘A, its name           "Follow the notes agent."
//   4. Follow ⇄ Manual edit         ⌥⌘E (both ways)          "Manual edit." / "Back to follow."
//   5. change course                ⌥⌘. (stop)              "Tell … to …" (redirect), "Stop …"
// Typed first (keyboard only, from an editor of the owner's own file where it makes sense), then,
// back in the owner's own layout, the same five by voice. Each step passes with at most one action
// and at most one yes; Manual edit is shown to be editable (a save lands in the agent's worktree).
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, until, repoRoot } = require('./harness');
const L = require('./overseer-window-helpers');

(async () => {
  const s = new Session('zero-friction');
  const result = { checks: [], steps: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const conns = [];
  try {
    const repo = makeRepo(path.join(s.root, 'zf-repo'), { dirty: false });
    // Follow is the default (AC-233); the agent a request starts is not opened for the owner, so
    // following it is the owner's own action, counted here.
    s.settings({ 'workbench.colorTheme': 'Overseer Dark', 'overseer.agent.openIn': undefined, 'overseer.followNewRuns': false, 'overseer.showStartedAgent': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'worker');
    s.launch(repo, {
      OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE,FIXTURE_WORKER_MS', FIXTURE_WORKER_MS: '100000',
      OVERSEER_VOICE_SIMULATE: '1', OVERSEER_LISTENER_TEST_VOICE: '1', OVERSEER_LISTENER_TEST_MIC_USERS: path.join(s.root, 'mic-users'),
    });
    let c = await s.connect(); conns.push(c);
    const ready = x => x.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 90000, 'status bar');
    await ready(c);
    await L.size(c, 1920, 1080);
    const state = () => s.ctl('state');
    const runOf = id => state().runs.find(r => r.id === id);
    const worktreeOf = id => { const st = state(); const r = st.runs.find(x => x.id === id); return st.workspaces.find(w => w.id === r?.workspace_id)?.path; };
    // An agent already at work in the repository (the owner's day has begun; Overseer places new work beside it).
    s.ctl('task.create', { repo, harness: 'generic', workspace_mode: 'worktree', program: '/bin/sleep', args: ['900'], prompt: '', title: 'Docs' });

    // The window reopened as (or back from) the Overseer layout is a new page: attach to it.
    const reopened = async overseer => {
      const end = Date.now() + 90000;
      while (Date.now() < end) {
        const x = await L.attach(s, t => /zf-repo/.test(t), 5000).catch(() => null);
        if (x) {
          const file = await x.evalWorkbench(`globalThis.vscode?.context?.configuration?.()?.workspace?.configPath?.path || ''`).catch(() => null);
          if (file !== null && /\/layouts\//.test(file) === overseer) { conns.push(x); s.cdp = x; await ready(x); await L.size(x, 1920, 1080); return x; }
          x.close();
        }
        await delay(500);
      }
      throw new Error(`the window did not reopen ${overseer ? 'as the Overseer layout' : 'on the folder'}`);
    };
    const windowTitle = () => c.evalWorkbench('document.title');
    const quickOpen = () => c.quickInputState().catch(() => null);
    const editorOf = () => c.evalWorkbench(`(() => { const g = document.querySelector('.part.editor .editor-group-container.active'); const ed = g && [...g.querySelectorAll('.monaco-editor')].find(e => e.offsetParent);
      return { title: document.title, editor: !!ed, focused: !!ed && ed.contains(document.activeElement), status: [...document.querySelectorAll('.statusbar-item')].map(e => e.textContent.trim()).find(t => /^(Follow|Diffs only|Manual edit)$/.test(t)) || '',
      about: [...document.querySelectorAll('.statusbar-item, .statusbar-item a')].map(e => e.getAttribute('aria-label') || '').find(t => /^(Follow|Diffs only|Manual edit): /.test(t)) || '' }; })()`);
    const followOf = async runId => {
      const f = await c.webview(`!!document.getElementById('follow-view') && document.body.dataset.runId === ${JSON.stringify(runId)} && document.visibilityState === 'visible'`, 3000);
      return f.eval(`({ view: document.body.dataset.view, path: document.body.dataset.followPath || '', source: document.body.dataset.followSource || '', visible: document.visibilityState })`);
    };
    // What the review showed on the way (each change, with when), for a slow step's evidence.
    const waitFollow = async (runId, ok, ms = 30000) => {
      let f; const t0 = Date.now(), seen = [];
      for (const end = t0 + ms; Date.now() < end;) {
        f = await followOf(runId).catch(e => ({ none: e.message.slice(0, 60) }));
        const k = JSON.stringify(f); if (seen[seen.length - 1]?.k !== k) seen.push({ k, ms: Date.now() - t0 });
        if (f && !f.none && ok(f)) return { ...f, seen: seen.map(x => ({ ms: x.ms, ...JSON.parse(x.k) })) };
        await delay(300);
      }
      return { ...f, seen: seen.map(x => ({ ms: x.ms, ...JSON.parse(x.k) })) };
    };
    const key = async (k, o) => { await c.focusWorkbench(); await c.key(k, o); };

    // One step: its actions counted, its time from the first action to the result on screen.
    const step = async (part, name, how, run) => {
      const actions = { keys: 0, commands: 0, said: 0, picks: 0, words: 0, yes: 0 };
      // count('shown') marks the moment the result is on screen (before any screenshot).
      let shown;
      const count = (kind, n = 1) => { if (kind === 'shown') shown = shown || Date.now(); else actions[kind] += n; };
      const t0 = Date.now();
      let ok = false, detail;
      try { ({ ok, detail } = await run(count)); } catch (error) { detail = { error: error.message }; }
      const ms = (shown || Date.now()) - t0;
      const one = actions.keys + actions.commands + actions.said;
      const row = { part, name, how, actions, ms, ok: !!ok && one <= 1 && actions.yes <= 1 };
      result.steps.push(row);
      check(`${part}: ${name} — ${how}: ${one} action${one === 1 ? '' : 's'}${actions.yes ? ' and one yes' : ''}, ${ms} ms`, row.ok, { actions, ...detail });
      return row;
    };

    // ======================================================================== typed (keyboard)
    s.note('typed: from a cold window, keyboard only');
    await s.screenshot('typed-0-cold-window');
    await step('typed', 'open Overseer', '⌥⌘⇧O', async count => {
      await key('o', { meta: true, alt: true, shift: true }); count('keys');
      try { c.close(); } catch {}
      c = await reopened(true);
      const home = await (async () => { let h; for (let i = 0; i < 80; i++) { h = await L.overseerView(c).catch(() => null); if (h?.mode === 'composer') return h; await delay(250); } return h; })();
      const lay = await L.layout(c);
      count('shown'); await s.screenshot('typed-1-open');
      return { ok: home?.mode === 'composer' && lay.groups.length >= 1, detail: { home, groups: lay.groups.map(g => g.active) } };
    });

    // From anywhere: the owner is in a file of their own (a.txt) when they ask.
    await L.openFile(c, 'a.txt');
    const known = new Set(state().runs.map(r => r.id));
    let draft;
    await step('typed', 'tell it to do something', '⌥⌘O, "Someone should draft the page", Enter; Yes on the proposal', async count => {
      await key('o', { meta: true, alt: true }); count('keys');
      let view = await s.editorView(`document.body.dataset.mode === 'composer'`);
      const focused = await view.waitFor(`document.activeElement?.id === 'task' && document.hasFocus()`, 8000).then(() => true, () => false);
      await c.type('Someone should draft the page'); await c.key('Enter'); count('words');
      await view.waitFor(`!!document.querySelector('#home-conv .proposal:not(.answered) [data-proposal="yes"]')`, 45000);
      await view.eval(`document.querySelector('#home-conv .proposal:not(.answered) [data-proposal="yes"]').scrollIntoView({ block: 'center' })`); await delay(300);
      const p = await s.webviewPoint(view, '#home-conv .proposal:not(.answered) [data-proposal="yes"]'); await c.click(p.x, p.y); count('yes');
      draft = await until(() => state().runs.find(r => !r.parent_run_id && !known.has(r.id) && r.title !== 'Docs'), Boolean, 30000, 200);
      count('shown'); await s.screenshot('typed-2-told');
      return { ok: focused && !!draft, detail: { composerFocusedByTheKey: focused, started: draft && { id: draft.id, title: draft.title } } };
    });
    if (!draft) throw new Error('no agent started');
    // The agent writes draft.md a quarter of the way through its work.
    await until(() => worktreeOf(draft.id) && fs.existsSync(path.join(worktreeOf(draft.id), 'draft.md')), Boolean, 60000, 300);
    await delay(1500);

    await step('typed', 'follow an agent', '⌥⌘A, "draft", Enter', async count => {
      await key('a', { meta: true, alt: true }); count('keys');
      await c.waitQuickTitle('Switch to agent');
      await c.type('draft'); await delay(400); await c.key('Enter'); count('picks');
      // On screen: the review in Follow (the head's status item; reading the webview itself is slow for the harness).
      await until(editorOf, e => e.about === 'Follow: draft the page', 30000, 100); count('shown');
      const f = await waitFollow(draft.id, f => f.view === 'follow' && f.path === 'draft.md');
      count('shown'); await delay(2000); await s.screenshot('typed-3-follow');
      return { ok: f?.view === 'follow' && f.path === 'draft.md', detail: { follow: f } };
    });

    const draftFile = path.join(worktreeOf(draft.id), 'draft.md');
    await step('typed', 'Follow → Manual edit', '⌥⌘E', async count => {
      await key('e', { meta: true, alt: true }); count('keys');
      const ed = await until(editorOf, e => e.editor && e.focused && /^draft\.md\b/.test(e.title), 15000, 200);
      count('shown'); await s.screenshot('typed-4-manual-edit');
      return { ok: ed.editor && ed.focused && /^draft\.md\b/.test(ed.title) && ed.status === 'Manual edit', detail: { editor: ed } };
    });
    // Manual edit is the real file: a save lands in the agent's worktree.
    // The editor opened at Follow's line (line 1 here): typed there, then saved. No ⌘Home: in the
    // background test window a CDP ⌘Home sometimes opened VS Code's About dialog over the editor,
    // which then took every key that followed.
    await c.type('Owner: keep it short.\n'); await c.key('s', { meta: true }); await delay(800);
    const saved = fs.readFileSync(draftFile, 'utf8');
    check('Manual edit is the agent\'s real file: the owner\'s line, saved, is in its worktree', /^Owner: keep it short\.\n# Draft/.test(saved), { file: draftFile, text: saved.slice(0, 80) });

    await step('typed', 'Manual edit → Follow', '⌥⌘E', async count => {
      await key('e', { meta: true, alt: true }); count('keys');
      const f = await waitFollow(draft.id, f => f.view === 'follow' && f.path === 'draft.md', 15000);
      const ed = await editorOf();
      count('shown'); await delay(2000); await s.screenshot('typed-5-follow-again');
      return { ok: f?.view === 'follow' && f.path === 'draft.md' && !/^draft\.md\b/.test(ed.title) && ed.status === 'Follow', detail: { follow: f, editor: ed } };
    });

    await step('typed', 'change course (stop the agent)', '⌥⌘.', async count => {
      const before = runOf(draft.id)?.status;
      await key('.', { meta: true, alt: true }); count('keys');
      const status = await until(() => runOf(draft.id)?.status, st => st === 'interrupted', 20000, 100);
      count('shown');
      // No "which agent?" pick: the one on screen is stopped.
      const asked = await until(quickOpen, Boolean, 1000, 100);
      await s.screenshot('typed-6-stopped');
      return { ok: before !== 'interrupted' && status === 'interrupted' && !asked, detail: { before, after: status, pickShown: asked } };
    });

    // ======================================================================== spoken (simulated voice)
    // Back to the owner's own layout, then Voice Mode on (as the owner leaves it on); the same five by voice.
    await key('o', { meta: true, alt: true, shift: true });
    try { c.close(); } catch {}
    c = await reopened(false);
    s.ctl('voice.set', { enabled: true, settle_seconds: 2 });
    await until(() => s.ctl('voice.get').state, st => st === 'listening', 30000, 300);
    await delay(1500);
    await s.screenshot('voice-0-own-layout');
    const say = text => s.ctl('voice.say', { text });
    const request = id => (s.ctl('voice.requests', { limit: 50 }).requests || []).find(r => r.id === id);

    await step('voice', 'open Overseer', '"Open Overseer."', async count => {
      const r = say('Open Overseer.'); count('said');
      try { c.close(); } catch {}
      c = await reopened(true);
      let home; for (let i = 0; i < 80; i++) { home = await L.overseerView(c).catch(() => null); if (home?.mode === 'composer') break; await delay(250); }
      count('shown'); await s.screenshot('voice-1-open');
      return { ok: r.place === 'overseer' && home?.mode === 'composer', detail: { said: r, home } };
    });

    const known2 = new Set(state().runs.map(r => r.id));
    let notes, spokenYes = false;
    await step('voice', 'tell it to do something', '"Overseer, someone should write the notes."', async count => {
      const r = say('Overseer, someone should write the notes.'); count('said');
      let row;
      for (const end = Date.now() + 60000; Date.now() < end;) {
        row = request(r.request);
        notes = state().runs.find(x => !x.parent_run_id && !known2.has(x.id));
        if (notes) break;
        // A plan that waits for a spoken yes gets the one yes.
        if (row?.state === 'waiting' && !spokenYes) { spokenYes = true; say('Yes.'); count('yes'); }
        await delay(300);
      }
      count('shown'); await s.screenshot('voice-2-told');
      return { ok: !!notes, detail: { said: r, request: row && { state: row.state, done: row.done }, started: notes && { id: notes.id, title: notes.title } } };
    });
    if (!notes) throw new Error('no agent started by voice');
    await until(() => worktreeOf(notes.id) && fs.existsSync(path.join(worktreeOf(notes.id), 'draft.md')), Boolean, 60000, 300);
    await delay(1500);

    await step('voice', 'follow an agent', '"Follow the notes agent."', async count => {
      const r = say('Follow the notes agent.'); count('said');
      // On screen: the review in Follow (the head's status item; reading the webview itself is slow for the harness).
      await until(editorOf, e => e.about === 'Follow: write the notes', 30000, 100); count('shown');
      const f = await waitFollow(notes.id, f => f.view === 'follow' && f.path === 'draft.md');
      count('shown'); await delay(2000); await s.screenshot('voice-3-follow');
      return { ok: r.place === 'follow' && r.run === notes.id && f?.view === 'follow' && f.path === 'draft.md', detail: { said: r, follow: f } };
    });

    await step('voice', 'Follow → Manual edit', '"Manual edit."', async count => {
      const r = say('Manual edit.'); count('said');
      const ed = await until(editorOf, e => e.editor && /^draft\.md\b/.test(e.title), 15000, 200);
      count('shown'); await s.screenshot('voice-4-manual-edit');
      return { ok: r.place === 'manual_edit' && ed.editor && /^draft\.md\b/.test(ed.title) && ed.status === 'Manual edit', detail: { said: r, editor: ed } };
    });

    await step('voice', 'Manual edit → Follow', '"Back to follow."', async count => {
      const r = say('Back to follow.'); count('said');
      const f = await waitFollow(notes.id, f => f.view === 'follow' && f.path === 'draft.md', 15000);
      const ed = await editorOf();
      count('shown'); await delay(2000); await s.screenshot('voice-5-follow-again');
      return { ok: r.place === 'follow' && f?.view === 'follow' && !/^draft\.md\b/.test(ed.title), detail: { said: r, follow: f, editor: ed } };
    });

    await step('voice', 'change course (redirect the agent)', '"Tell the notes agent to keep it short."', async count => {
      const r = say('Tell the notes agent to keep it short.'); count('said');
      const row = await until(() => request(r.request), x => x && ['sent', 'done'].includes(x.state), 45000, 300);
      count('shown');
      // A message to a working agent waits for the end of its turn (queued, no yes).
      const queued = s.ctl('events.list', { run_id: notes.id, limit: 500 }).events.find(x => x.kind === 'queued' && /keep it short/.test(x.payload?.text || ''));
      return { ok: ['sent', 'done'].includes(row?.state) && !!queued, detail: { said: r, request: row && { state: row.state, done: row.done }, queued: !!queued } };
    });

    await step('voice', 'change course (stop the agent)', '"Stop the notes agent."', async count => {
      const r = say('Stop the notes agent.'); count('said');
      // The turn it was on ends at once; the redirect said before it is then its next turn (stop + new direction).
      const ended = await until(() => s.ctl('events.list', { run_id: notes.id, limit: 500 }).events.find(x => x.kind === 'turn_done' && x.payload?.summary === 'interrupted'), Boolean, 20000, 200);
      count('shown'); await s.screenshot('voice-6-stopped');
      const next = await until(() => s.ctl('run.turns', { run_id: notes.id }).find(t => /keep it short/.test(t.prompt || '')), Boolean, 20000, 300);
      return { ok: r.built_in === 'stop' && !!ended, detail: { said: r, interrupted: !!ended, redirectIsTheNextTurn: !!next } };
    });

    // The table the criterion asks for: actions and timings per step.
    fs.writeFileSync(path.join(s.evidence, 'loop.json'), JSON.stringify(result.steps, null, 2));
    s.note('the loop', result.steps.map(x => `${x.part} · ${x.name} · ${x.how} · keys ${x.actions.keys} said ${x.actions.said} yes ${x.actions.yes} · ${x.ms} ms · ${x.ok ? 'ok' : 'FAIL'}`));
    check('all five, typed and spoken, at most one action each plus at most one yes', result.steps.length >= 13 && result.steps.every(x => x.ok), result.steps.map(x => [x.part, x.name, x.ok]));
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    try { s.ctl('voice.set', { enabled: false }); } catch {}
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    for (const x of conns) { try { x.close(); } catch {} }
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(x => !x.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
