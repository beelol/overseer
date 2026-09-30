// Packaged-UI scenario for AC-61 (Needs-you inbox and keyboard control), fixture runs only. Three
// or more concurrent runs need the user (two permission requests, a failure, a finished run with
// changes); the Needs you list and the status bar count them. Then, keyboard only (no clicks):
// next waiting agent (⌥⌘J), allow (⌥⌘Y), deny (⌥⌘⌫), review the rest, switch agents with a
// searchable quick pick (⌥⌘A), stop (⌥⌘.), start a new agent (⌥⌘N) and send a follow-up (Enter).
// Every control in the dashboard has an accessible name.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, until, repoRoot } = require('./harness');

const AUDIT = `(() => { const bad = []; for (const e of document.querySelectorAll('button, [role=radio], [role=treeitem], [role=tab], [role=menuitem], input, select, textarea, a[href]')) {
  if (e.closest('[hidden], [aria-hidden="true"]') || e.offsetParent === null) continue;
  const label = e.getAttribute('aria-label') || e.getAttribute('aria-labelledby') || e.getAttribute('title') || (e.id && document.querySelector('label[for="' + e.id + '"]')?.textContent) || e.closest('label')?.textContent.trim() || e.textContent.trim() || e.getAttribute('placeholder');
  if (!label) bad.push(e.outerHTML.slice(0, 100)); } return { checked: document.querySelectorAll('button, [role=treeitem], input, textarea').length, bad }; })()`;

(async () => {
  const s = new Session('keyboard');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'kb-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(latestVsix());
    // Fixture harnesses only: Codex and OpenCode point nowhere so nothing real can start.
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode', CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const run = id => s.ctl('state').runs.find(r => r.id === id);
    const waitFor = async (id, re, ms = 20000) => { for (let t = 0; t < ms; t += 300) { if (re.test(run(id)?.status || '')) return run(id).status; await delay(300); } return run(id)?.status; };
    const claude = async (title, prompt) => { fs.writeFileSync(modeFile, 'permission'); const t = s.ctl('task.create', { repo, harness: 'claude', prompt, title }); await waitFor(t.run.id, /waiting_for_user/); return t; };
    const permA = await claude('Write file A', 'write perm.txt');
    const permB = await claude('Write file B', 'write perm.txt');
    const failed = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo boom; exit 2'], prompt: '', title: 'Broken build' });
    await waitFor(failed.run.id, /failed/);
    const changed = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', "sed -i '' 's/^L5: original$/L5: reviewed?/' a.txt"], prompt: '', title: 'Small edit' });
    await waitFor(changed.run.id, /completed/);
    const long = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'while true; do echo tick; sleep 1; done'], prompt: '', title: 'Long loop' });
    await waitFor(long.run.id, /running/);

    await cdp.command('Overseer: Open Overseer View');
    const dash = await s.editorView();
    await s.openOverseerView(); await delay(800);
    // Gate K: Needs you is the first section of the side bar's agents list.
    const needs = () => cdp.evalWorkbench(`(() => { const rows = [...document.querySelectorAll('.part.sidebar .monaco-list-row')].filter(r => r.offsetParent); const i = rows.findIndex(r => r.querySelector('.label-name')?.textContent.trim() === 'Needs you'); if (i < 0) return [];
      const out = []; for (const r of rows.slice(i + 1)) { if (r.getAttribute('aria-level') === '1') break; out.push({ title: r.querySelector('.label-name')?.textContent.trim(), why: r.querySelector('.label-description')?.textContent.trim() }); } return out; })()`);
    const badge = () => cdp.evalWorkbench(`[...document.querySelectorAll('.part.sidebar .monaco-list-row')].find(r => r.querySelector('.label-name')?.textContent.trim() === 'Needs you')?.querySelector('.label-description')?.textContent.trim()`);
    const status = () => cdp.evalWorkbench(`[...document.querySelectorAll('.statusbar-item')].map(e => e.getAttribute('aria-label') || e.textContent).find(t => /Overseer/.test(t)) || ''`);
    // Needs you is what waits for an answer (AC-246); the failure and the finished run carry the
    // "to review" mark instead (AC-254), counted in the Agents view's header.
    const header = () => cdp.evalWorkbench(`[...document.querySelectorAll('.pane')].find(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || ''))?.querySelector('.pane-header')?.innerText.replace(/\\s+/g, ' ') || ''`);
    let list = []; await until(async () => { list = await needs(); return list.length >= 2 && /1 to review · 1 failed/.test(await header()); }, Boolean, 30000, 300);
    const st = await status();
    await s.screenshot('needs-you');
    check('Needs you gathers the permission requests, counted on the view and in the status bar; the failure and the finished run are to review',
      list.length === 2 && list.every(x => x.why === 'Approve') && (await badge()) === '2' && /2/.test(st) && /1 to review · 1 failed/.test(await header()),
      { list, badge: await badge(), status: st, header: await header() });

    const selected = () => dash.eval(`window.__overseer.selected()`);
    const key = async (k, o = {}) => { await cdp.focusWorkbench(); await cdp.key(k, o); await delay(900); };
    // ⌥⌘J, then the selection it made (the dashboard learns it a moment after the key).
    const nextAgent = async () => { const prev = await selected(); await key('j', { meta: true, alt: true }); await dash.waitFor(`window.__overseer.selected() !== ${JSON.stringify(prev ?? null)} && !!window.__overseer.selected()`, 30000).catch(() => {}); return selected(); };
    // Next waiting agent, allow.
    const first = await nextAgent();
    s.note('after first ⌥⌘J', await dash.eval(`({ mode: document.body.dataset.mode, selected: window.__overseer.selected(), visible: document.visibilityState, chatRun: document.querySelector('#title')?.textContent })`).catch(e => 'eval failed: ' + e.message));
    s.note('frames', await cdp.webviews(`!!document.querySelector('.view-chat')`).then(fs => Promise.all(fs.map(f => f.eval(`({ mode: document.body.dataset.mode, selected: window.__overseer?.selected(), visible: document.visibilityState })`)))).catch(e => e.message));
    await key('y', { meta: true, alt: true });
    const firstDone = await waitFor(first, /completed/);
    // Next, deny.
    const second = await nextAgent();
    await key('Backspace', { meta: true, alt: true });
    const secondDone = await waitFor(second, /completed/);
    const denied = s.ctl('events.list', { run_id: second, limit: 500 }).events.some(e => e.kind === 'permission_answered' && e.payload.allow === false);
    check('⌥⌘J goes to the next agent that needs you; ⌥⌘Y allows and ⌥⌘⌫ denies its request', [permA.run.id, permB.run.id].includes(first) && [permA.run.id, permB.run.id].includes(second) && first !== second && firstDone === 'completed' && secondDone === 'completed' && denied, { first, second, firstDone, secondDone, denied });
    // Then the agents to review (the failure, the finished run with changes): ⌥⌘J visits each; visiting clears it.
    const visited = [];
    for (let i = 0; i < 6 && /to review|failed/.test(await header()); i++) { visited.push(await nextAgent()); await delay(600); }
    const left = await header();
    check('⌥⌘J goes on to the agents to review (the failure, then the finished run with changes); visiting clears each', visited.includes(failed.run.id) && visited.includes(changed.run.id) && !/to review|failed/.test(left), { visited, left });

    // Switch agents with the searchable quick pick, then stop it.
    await key('a', { meta: true, alt: true });
    await cdp.waitQuickTitle('Switch to agent');
    await cdp.type('Long loop'); await delay(400); await cdp.key('Enter'); await delay(1200);
    const switched = await selected();
    await key('.', { meta: true, alt: true });
    const stopped = await waitFor(long.run.id, /interrupted/);
    check('⌥⌘A switches agents from a searchable quick pick and ⌥⌘. stops the selected agent', switched === long.run.id && stopped === 'interrupted', { switched, stopped });

    // New agent, keyboard only: ⌥⌘N focuses the composer; typing and Enter start it.
    fs.writeFileSync(modeFile, 'showcase');
    await key('n', { meta: true, alt: true });
    await dash.waitFor(`document.body.dataset.mode === 'composer' && document.activeElement?.id === 'task'`, 10000);
    const before = s.ctl('state').runs.length;
    await cdp.type('Refresh sessions once'); await delay(300); await cdp.key('Enter');
    let created; for (let i = 0; i < 40 && !created; i++) { await delay(300); created = s.ctl('state').runs.find((r, j) => j >= before && !r.parent_run_id); }
    await waitFor(created?.id, /completed/);
    const nowSelected = await selected();
    check('⌥⌘N starts a new agent from the composer without the mouse; it becomes the selected agent', created && nowSelected === created.id, { created: created?.id, nowSelected });

    // Follow-up with Enter in the chat (focus lands in the composer after switching).
    fs.writeFileSync(modeFile, 'echo');
    // The agent's edits bring the review in beside the chat, which takes focus as it arrives (later
    // on a loaded machine): wait for it to be there, then click into the chat, then its prompt.
    await until(() => cdp.evalWorkbench(`[...document.querySelectorAll('.editor-group-container')].filter(g => g.offsetParent).some(g => /^Review/.test(g.querySelector('.tab.active')?.getAttribute('aria-label') || ''))`), Boolean, 30000);
    await dash.eval(`(() => { if (!document.getElementById('focus-spot')) { const c = document.createElement('div'); c.id = 'focus-spot'; c.style.cssText = 'position:fixed;right:2px;top:60px;width:3px;height:3px;z-index:9'; document.body.append(c); } return true; })()`);
    { const f = await s.webviewPoint(dash, '#focus-spot'); await cdp.click(f.x, f.y); await delay(200); }
    // Click into the prompt until it has focus (the chat may still be settling after the review came in).
    for (let i = 0; i < 3; i++) {
      const at = await s.webviewPoint(dash, '#prompt'); await cdp.click(at.x, at.y);
      if (await dash.waitFor(`document.activeElement?.id === 'prompt'`, 2000).then(() => true, () => false)) break;
    }
    await cdp.type('And add a test'); await delay(200);
    // Enter goes to the prompt: if something took focus while typing, click back into it (the text stays).
    for (let i = 0; i < 3 && !(await dash.eval(`document.activeElement?.id === 'prompt'`)); i++) { const at = await s.webviewPoint(dash, '#prompt'); await cdp.click(at.x, at.y); await dash.waitFor(`document.activeElement?.id === 'prompt'`, 2000).catch(() => {}); }
    s.note('prompt before Enter', await dash.eval(`({ value: document.getElementById('prompt').value, focused: document.activeElement?.id })`));
    await cdp.key('Enter');
    const turns = async () => s.ctl('run.turns', { run_id: created.id }).length;
    const n = await until(turns, x => x >= 2, 30000, 300);
    check('Enter in the chat composer sends a follow-up', n === 2, { turns: n });
    await s.screenshot('keyboard-done');

    // Gate K (AC-81): the shortcuts also work with keyboard focus in the side bar's Agents list.
    await cdp.command('Focus on Agents View'); await delay(500);
    const inList = await cdp.evalWorkbench(`!!document.activeElement?.closest('.part.sidebar')`);
    await cdp.key('a', { meta: true, alt: true }); await delay(700);
    const switcher = await cdp.waitQuickTitle('Switch to agent').then(() => true, () => false);
    await cdp.key('Escape'); await delay(300);
    await cdp.command('Focus on Agents View'); await delay(500);
    await cdp.key('n', { meta: true, alt: true }); await delay(900);
    const composer = await dash.waitFor(`document.body.dataset.mode === 'composer'`, 8000).then(() => true, () => false);
    check('the shortcuts also work from the side bar (⌥⌘A opens the agent switcher, ⌥⌘N the composer)', inList && switcher && composer, { inList, switcher, composer });

    const audit = await dash.eval(AUDIT);
    check('every control in the dashboard has a screen-reader label', audit.bad.length === 0 && audit.checked > 10, audit);
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
