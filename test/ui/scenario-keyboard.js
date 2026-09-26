// Packaged-UI scenario for AC-61 (Needs-you inbox and keyboard control), fixture runs only. Three
// or more concurrent runs need the user (two permission requests, a failure, a finished run with
// changes); the Needs you list and the status bar count them. Then, keyboard only (no clicks):
// next waiting agent (⌥⌘J), allow (⌥⌘Y), deny (⌥⌘⌫), review the rest, switch agents with a
// searchable quick pick (⌥⌘A), stop (⌥⌘.), start a new agent (⌥⌘N) and send a follow-up (Enter).
// Every control in the dashboard has an accessible name.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

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
    const dash = await cdp.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.rail-list .row')`, 30000);
    const needs = () => dash.eval(`[...document.querySelectorAll('.rail-list .row.needs-row')].map(r => ({ run: r.dataset.run, why: r.querySelector('.why-chip')?.textContent }))`);
    const badge = () => dash.eval(`document.querySelector('.rail-list .row.section.needs .badge')?.textContent`);
    const status = () => cdp.evalWorkbench(`[...document.querySelectorAll('.statusbar-item')].map(e => e.getAttribute('aria-label') || e.textContent).find(t => /Overseer/.test(t)) || ''`);
    let list = []; for (let i = 0; i < 20; i++) { list = await needs(); if (list.length >= 4) break; await delay(500); }
    const st = await status();
    await s.screenshot('needs-you');
    check('Needs you gathers permission requests, the failure and the finished run with changes, counted on the view and in the status bar',
      list.length === 4 && list.filter(x => x.why === 'Approve').length === 2 && list.some(x => x.why === 'Failed') && list.some(x => x.why === 'Review') && (await badge()) === '4' && /4/.test(st),
      { list, badge: await badge(), status: st });

    const selected = () => dash.eval(`document.querySelector('.rail-list .row[aria-selected="true"]')?.dataset.run`);
    const key = async (k, o = {}) => { await cdp.focusWorkbench(); await cdp.key(k, o); await delay(900); };
    // Next waiting agent, allow.
    await key('j', { meta: true, alt: true });
    const first = await selected();
    await key('y', { meta: true, alt: true });
    const firstDone = await waitFor(first, /completed/);
    // Next, deny.
    await key('j', { meta: true, alt: true });
    const second = await selected();
    await key('Backspace', { meta: true, alt: true });
    const secondDone = await waitFor(second, /completed/);
    const denied = s.ctl('events.list', { run_id: second, limit: 500 }).events.some(e => e.kind === 'permission_answered' && e.payload.allow === false);
    check('⌥⌘J goes to the next agent that needs you; ⌥⌘Y allows and ⌥⌘⌫ denies its request', [permA.run.id, permB.run.id].includes(first) && [permA.run.id, permB.run.id].includes(second) && first !== second && firstDone === 'completed' && secondDone === 'completed' && denied, { first, second, firstDone, secondDone, denied });
    // The rest (the failure, the finished runs with changes): ⌥⌘J visits each; visiting clears it.
    const visited = []; let left = await needs();
    for (let i = 0; i < 6 && left.length; i++) { await key('j', { meta: true, alt: true }); visited.push(await selected()); await delay(600); left = await needs(); }
    check('⌥⌘J walks the rest of Needs you (the failure and finished runs with changes); visiting clears each', visited.includes(failed.run.id) && visited.includes(changed.run.id) && left.length === 0, { visited, left });

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
    await dash.eval(`document.getElementById('prompt').focus()`);
    await cdp.type('And add a test'); await delay(200); await cdp.key('Enter');
    const turns = async () => s.ctl('run.turns', { run_id: created.id }).length;
    let n = 0; for (let i = 0; i < 30 && n < 2; i++) { n = await turns(); await delay(300); }
    check('Enter in the chat composer sends a follow-up', n === 2, { turns: n });
    await s.screenshot('keyboard-done');

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
