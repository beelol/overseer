// Packaged-UI scenario for AC-236 (Claude Code fixture as Overseer and as the agent, no paid tokens):
// home talks to Overseer first. A fresh profile's home sends to Overseer: the conversation's head
// and a one-line hint of what to ask are there from the first visit, with no repository, agent,
// model or workspace choice; the first Enter reaches Overseer and starts no agent. The labelled
// "Start an agent directly" choice starts one as before and is remembered for the owner
// (overseer.home.sendTo); New Agent starts one directly without changing it. Screenshots in the
// three Overseer themes.
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('home-overseer');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  const THEMES = [['Overseer', 'overseer'], ['Overseer Dark', 'dark'], ['Overseer Light', 'light']];
  const settingsFile = path.join(s.profile, 'User/settings.json');
  const userSettings = () => { try { return JSON.parse(fs.readFileSync(settingsFile, 'utf8')); } catch { return {}; } };
  try {
    const repo = makeRepo(path.join(s.root, 'site-repo'), { dirty: false });
    // A fresh profile: nothing says where home's box sends.
    s.settings({ 'workbench.colorTheme': 'Overseer', 'overseer.followNewRuns': false });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'overseer');
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const agents = () => { const st = s.ctl('state'); const ov = s.ctl('overseer.session').run_id; return st.runs.filter(r => !r.parent_run_id && r.id !== ov && (st.oversight || {})[r.id]?.role !== 'overseer'); };
    const setTheme = async theme => {
      const bg = `getComputedStyle(document.querySelector('.part.activitybar') || document.body).backgroundColor`;
      const before = await cdp.evalWorkbench(bg);
      s.settings({ ...userSettings(), 'workbench.colorTheme': theme });
      await cdp.waitFor(`${bg} !== ${JSON.stringify(before)}`, 20000, 'theme ' + theme).catch(() => {});
      await delay(1500);
    };

    // ---------- The first visit: home talks to Overseer.
    await cdp.command('Overseer: Open Overseer View'); await delay(2500);
    const view = await s.editorView(`!!document.querySelector('#task') && !!document.querySelector('#target')`);
    await view.waitFor(`document.querySelector('#target')?.dataset.target === 'overseer' && !!document.querySelector('[data-chip="repo"]')`, 20000).catch(() => {});
    const first = await view.eval(`(() => {
      const shown = sel => { const e = document.querySelector(sel); return !!e && e.checkVisibility(); };
      return { target: document.querySelector('#target')?.dataset.target, label: document.querySelector('#target .chip-label')?.textContent, hint: shown('#composer-hint') ? document.querySelector('#composer-hint').textContent : '',
        head: shown('#home .home-title'), placeholder: document.querySelector('#task').placeholder, ariaLabel: document.querySelector('#task').getAttribute('aria-label'),
        choices: [...document.querySelectorAll('.composer-choices .chip')].filter(c => c.checkVisibility()).map(c => c.dataset.chip) };
    })()`);
    check('a fresh profile\'s home sends to Overseer, and says so in words', first.target === 'overseer' && first.label === 'Overseer' && /Overseer/.test(first.placeholder) && first.ariaLabel === 'Message to Overseer', first);
    check('home shows the conversation\'s head and a one-line hint of what to ask from the first visit', first.head && /Ask what your agents are doing/.test(first.hint), first);
    check('with Overseer as the target there is no repository, agent, model or workspace choice to make', JSON.stringify(first.choices) === JSON.stringify(['target']), first.choices);
    for (const [theme, tag] of THEMES) { if (tag !== 'overseer') await setTheme(theme); await s.screenshot(`first-visit-${tag}`); }
    await setTheme('Overseer');

    // The first Enter reaches Overseer and starts no agent.
    const inTask = () => view.eval(`document.activeElement?.id === 'task' && document.hasFocus()`);
    const focusTask = async () => { if (!(await inTask())) { const p = await s.webviewPoint(view, '#task'); await cdp.click(p.x, p.y); await delay(250); } };
    await focusTask();
    await cdp.type('What is everyone doing?'); await delay(200); await cdp.key('Enter');
    await view.waitFor(`[...document.querySelectorAll('#home-conv .home-msg.from-owner')].some(m => /What is everyone doing/.test(m.textContent))`, 20000);
    await view.waitFor(`[...document.querySelectorAll('#home-conv .home-msg.from-overseer')].length > 0`, 40000);
    const said = s.ctl('overseer.session').messages.filter(m => m.source === 'owner').map(m => m.text);
    await delay(1500);
    check('the first Enter on home reaches Overseer, which answers, and no agent is started', said.includes('What is everyone doing?') && agents().length === 0, { said, agents: agents().map(r => r.title) });
    const after = await view.eval(`({ target: document.querySelector('#target')?.dataset.target, hint: document.querySelector('#composer-hint')?.checkVisibility() })`);
    check('the box still sends to Overseer, and the hint gives way to the conversation', after.target === 'overseer' && !after.hint, after);
    for (const [theme, tag] of THEMES) { if (tag !== 'overseer') await setTheme(theme); await s.screenshot(`conversation-${tag}`); }
    await setTheme('Overseer');

    // ---------- "Start an agent directly": one labelled choice away, starts one as before, remembered.
    fs.writeFileSync(modeFile, 'echo');
    const pick = async label => {
      const p = await s.webviewPoint(view, '#target'); await cdp.click(p.x, p.y); await delay(400);
      const items = await view.eval(`[...document.querySelectorAll('.menu[role=menu] .menu-label')].map(e => e.textContent)`);
      const at = await view.eval(`(() => { const i = [...document.querySelectorAll('.menu[role=menu] .menu-item')].find(e => e.querySelector('.menu-label')?.textContent === ${JSON.stringify(label)}); if (!i) return null; const b = i.getBoundingClientRect(); return { x: b.left + 20, y: b.top + b.height / 2 }; })()`);
      const frame = await s.webviewPoint(view, '#target'), chip = await view.eval(`(() => { const b = document.querySelector('#target').getBoundingClientRect(); return { x: b.left + Math.min(b.width / 2, 40), y: b.top + Math.min(b.height / 2, 12) }; })()`);
      await cdp.click(frame.x - chip.x + at.x, frame.y - chip.y + at.y); await delay(600);
      return items;
    };
    const items = await pick('Start an agent directly');
    const direct = await view.eval(`({ target: document.querySelector('#target')?.dataset.target, label: document.querySelector('#target')?.getAttribute('aria-label'), choices: [...document.querySelectorAll('.composer-choices .chip')].filter(c => c.checkVisibility()).map(c => c.dataset.chip) })`);
    check('the Send to menu offers Overseer and "Start an agent directly"; the direct choice brings back the repository, agent, model and workspace choices', JSON.stringify(items) === JSON.stringify(['Overseer', 'Start an agent directly']) && direct.target === 'agent' && direct.label === 'Send to: Start directly' && ['repo', 'agent', 'mode'].every(c => direct.choices.includes(c)), { items, direct });
    await s.screenshot('start-directly');
    await focusTask();
    await cdp.type('tidy the docs'); await delay(200); await cdp.key('Enter');
    let started;
    for (let i = 0; i < 80 && !(started = agents().find(r => /tidy the docs/.test(r.title))); i++) await delay(250);
    check('"Start an agent directly" starts one as before', !!started, { started: started?.title });
    await delay(1500);
    check('the choice is remembered for the owner (overseer.home.sendTo in the user settings)', userSettings()['overseer.home.sendTo'] === 'agent', { sendTo: userSettings()['overseer.home.sendTo'] });
    await cdp.command('Overseer: Open Overseer View'); await delay(1500);
    await cdp.command('Overseer: Talk to Overseer'); await delay(300);
    // Talk to Overseer sends this message to Overseer; back home afterwards, the remembered choice holds.
    const talk = await view.eval(`document.querySelector('#target')?.dataset.target`);
    check('Talk to Overseer always talks to Overseer', talk === 'overseer', { talk });

    // Back to Overseer, remembered; New Agent starts one directly without changing it.
    await pick('Overseer');
    await delay(800);
    check('choosing Overseer again is remembered too', userSettings()['overseer.home.sendTo'] === 'overseer', { sendTo: userSettings()['overseer.home.sendTo'] });
    await cdp.command('Overseer: New Agent'); await delay(1500);
    const viaNew = await view.eval(`document.querySelector('#target')?.dataset.target`);
    await focusTask();
    await cdp.type('write the changelog'); await delay(200); await cdp.key('Enter');
    let second;
    for (let i = 0; i < 80 && !(second = agents().find(r => /write the changelog/.test(r.title))); i++) await delay(250);
    await delay(1000);
    await cdp.command('Overseer: Open Overseer View'); await delay(1200);
    const home = await view.eval(`(() => { document.body.dataset.mode === 'composer' || null; return document.querySelector('#target')?.dataset.target; })()`);
    check('New Agent starts one agent directly, then home sends to Overseer again (the remembered choice is unchanged)', viaNew === 'agent' && !!second && userSettings()['overseer.home.sendTo'] === 'overseer' && home === 'overseer', { viaNew, second: second?.title, home, sendTo: userSettings()['overseer.home.sendTo'] });
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
