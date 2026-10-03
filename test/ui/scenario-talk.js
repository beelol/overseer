// Packaged-UI scenario for AC-107 (Claude Code fixture, no paid tokens): Talk to Overseer. Since
// AC-227 it is home, the one view for talking to Overseer (nothing docks below); it runs on the
// Claude harness as a task Overseer keeps for itself (hidden from the side bar). "What is everyone doing?" gets a summary that matches the daemon's state; "tell API
// tests to add tests" gets a proposal, and on Yes that agent's chat shows the follow-up as coming from
// Overseer; a declined proposal changes nothing; an agent's waiting permission comes up by itself
// as a yes/no that a click answers (AC-230). (The one live run on the Claude account waits for
// the owner's Claude sign-in.)
const fs = require('fs');
const path = require('path');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');

(async () => {
  const s = new Session('talk');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const modeFile = path.join(s.root, 'claude-mode');
  try {
    const repo = makeRepo(path.join(s.root, 'talk-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer' });
    s.install(latestVsix());
    fs.writeFileSync(modeFile, 'echo');
    s.launch(repo, { OVERSEER_CLAUDE_PATH: path.join(repoRoot, 'fixtures/fake-harness/claude-fixture.js'), OVERSEER_CODEX_PATH: '/nonexistent/codex', OVERSEER_OPENCODE_PATH: '/nonexistent/opencode',
      CLAUDE_FIXTURE_MODE_FILE: modeFile, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CLAUDE_FIXTURE_MODE_FILE' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const api = s.ctl('task.create', { repo, harness: 'claude', prompt: 'write the API', title: 'API tests' });
    const front = s.ctl('task.create', { repo, harness: 'claude', prompt: 'fix the header', title: 'Frontend fixer' });
    const watch = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo watching; sleep 600'], prompt: '', title: 'Build watcher' });
    for (let i = 0; i < 40 && [api, front].some(t => s.ctl('state').runs.find(r => r.id === t.run.id).status !== 'completed'); i++) await delay(300);
    fs.writeFileSync(modeFile, 'overseer');
    const turns = id => s.ctl('run.turns', { run_id: id });

    // Open the chat (home, the composer talking to Overseer) and ask.
    await cdp.command('Overseer: Talk to Overseer'); await delay(2000);
    const chat = await s.editorView(`!!document.getElementById('home-conv') && document.querySelector('#target')?.dataset.target === 'overseer'`);
    const focusPrompt = async () => { if (!(await chat.eval(`document.activeElement?.id === 'task' && document.hasFocus()`))) { const p = await s.webviewPoint(chat, '#task'); await cdp.click(p.x, p.y); await delay(150); } };
    await focusPrompt();
    await cdp.type('What is everyone doing?'); await cdp.key('Enter');
    await chat.waitFor(`/What is everyone doing/.test(document.getElementById('home-conv').textContent)`, 30000);
    await chat.waitFor(`/Here is what everyone is doing/.test(document.getElementById('home-conv').textContent)`, 20000);
    const summary = await chat.eval(`document.getElementById('home-conv').innerText`);
    const state = s.ctl('state');
    const wanted = [api, front, watch].map(t => { const r = state.runs.find(x => x.id === t.run.id); return { title: r.title, status: r.status }; });
    await s.screenshot('summary');
    check('"What is everyone doing?" gets a summary that matches the daemon\'s state (each agent and its status)', wanted.every(w => summary.includes(`${w.title}: ${w.status}`)), { wanted, summary: summary.slice(0, 600) });
    // The agents' state goes with the message to Overseer's run, and is never shown as text.
    const sent = s.ctl('run.turns', { run_id: s.ctl('overseer.session').run_id }).pop()?.prompt || '';
    check('the agents\' state goes with the message but is not shown as text', sent.includes('<overseer-state>') && ['API tests', 'Frontend fixer', 'Build watcher'].every(t => sent.includes(t)) && !summary.includes('overseer-state'), { sent: sent.length });
    await s.openOverseerView(); await delay(1000);
    const sideBar = await cdp.evalWorkbench(`[...document.querySelectorAll('.part.sidebar .monaco-list-row .label-name')].map(e => e.textContent.trim())`);
    check('Overseer\'s own conversation is not listed as an agent', !sideBar.includes('Talk to Overseer') && sideBar.includes('API tests'), sideBar);

    // A proposal accepted.
    const send = async text => { await cdp.command('Overseer: Talk to Overseer'); await delay(900); await focusPrompt(); await cdp.type(text); await delay(200); await cdp.key('Enter'); };
    const before = { api: turns(api.run.id).length, front: turns(front.run.id).length };
    await send('Tell API tests to add tests');
    await chat.waitFor(`document.querySelectorAll('.proposal:not(.answered)').length === 1`, 20000);
    const card = await chat.eval(`document.querySelector('.proposal:not(.answered)').innerText`);
    await s.screenshot('proposal');
    check('asking Overseer to act gets a proposal saying exactly what it will do, and nothing happens yet', /Send API tests: “Please add tests\.”/.test(card) && turns(api.run.id).length === before.api, { card, turns: turns(api.run.id).length });
    await chat.eval(`document.querySelector('.proposal:not(.answered)').scrollIntoView({ block: 'center' })`); await delay(300);
    { const c = await s.webviewPoint(chat, '.proposal:not(.answered) .proposal-head'); await cdp.click(c.x, c.y); await delay(200);
      const p = await s.webviewPoint(chat, '.proposal:not(.answered) [data-proposal="yes"]'); await cdp.click(p.x, p.y); }
    await chat.waitFor(`[...document.querySelectorAll('.proposal.answered .proposal-status')].some(e => /^Sent /.test(e.textContent))`, 20000);
    let t = []; for (let i = 0; i < 30 && t.length <= before.api; i++) { await delay(300); t = turns(api.run.id); }
    const last = t[t.length - 1];
    check('on Yes, the follow-up goes to that agent as coming from Overseer', t.length === before.api + 1 && /^From Overseer: Please add tests\./.test(last?.prompt || ''), { turns: t.length, prompt: last?.prompt });
    await s.selectAgent('API tests', { settle: 2500 });
    const agentChat = await s.editorView(`!!document.querySelector('#conv .msg.user.from-overseer')`);
    const shown = await agentChat.eval(`(() => { const m = [...document.querySelectorAll('#conv .msg.user.from-overseer')].pop(); return { who: m.querySelector('.msg-from')?.textContent, text: m.querySelector('.text')?.textContent }; })()`);
    await s.screenshot('agent-chat-from-overseer');
    check('the agent\'s chat shows the message as coming from Overseer', /From Overseer/.test(shown.who || '') && shown.text === 'Please add tests.', shown);

    // A proposal declined.
    await cdp.command('Overseer: Talk to Overseer'); await delay(1200);
    await send('Tell Frontend fixer to update the docs');
    await chat.waitFor(`document.querySelectorAll('.proposal:not(.answered)').length === 1`, 20000);
    await chat.eval(`document.querySelector('.proposal:not(.answered)').scrollIntoView({ block: 'center' })`); await delay(300);
    { const c = await s.webviewPoint(chat, '.proposal:not(.answered) .proposal-head'); await cdp.click(c.x, c.y); await delay(200);
      const p = await s.webviewPoint(chat, '.proposal:not(.answered) [data-proposal="no"]'); await cdp.click(p.x, p.y); }
    await chat.waitFor(`[...document.querySelectorAll('.proposal.answered .proposal-status')].some(e => /Declined/.test(e.textContent))`, 20000);
    await delay(2000);
    await s.screenshot('declined');
    check('a declined proposal changes nothing', turns(front.run.id).length === before.front, { turns: turns(front.run.id).length });

    // AC-230: an agent's waiting permission comes up by itself as a yes/no in the view, with
    // nothing asked of Overseer, and a click on Yes answers it.
    fs.writeFileSync(modeFile, 'permission');
    const asks = s.ctl('task.create', { repo, harness: 'claude', prompt: 'write perm.txt', title: 'Sessions' });
    for (let i = 0; i < 60 && s.ctl('state').runs.find(r => r.id === asks.run.id).status !== 'waiting_for_user'; i++) await delay(300);
    fs.writeFileSync(modeFile, 'overseer');
    const ownerTurns = () => s.ctl('run.turns', { run_id: s.ctl('overseer.session').run_id }).length;
    const turnsBefore = ownerTurns();
    await cdp.command('Overseer: Talk to Overseer'); await delay(1200);
    await chat.waitFor(`[...document.querySelectorAll('.home-card.card-needs')].some(e => /Sessions wants to change perm\.txt\. Allow it\?/.test(e.textContent)) && [...document.querySelectorAll('.proposal:not(.answered)')].some(e => /Allow Sessions to change perm\.txt/.test(e.textContent))`, 20000);
    await chat.eval(`document.querySelector('.proposal:not(.answered)').scrollIntoView({ block: 'center' })`); await delay(400);
    const asked = await chat.eval(`(() => { const p = [...document.querySelectorAll('.proposal:not(.answered)')].pop(); return { card: [...document.querySelectorAll('.home-card.card-needs')].pop().innerText, proposal: p.innerText, yes: !!p.querySelector('[data-proposal="yes"]'), no: !!p.querySelector('[data-proposal="no"]') }; })()`);
    await s.screenshot('permission-comes-up');
    check('a waiting permission comes up by itself as a yes/no, with no request id and no turn of Overseer\'s', asked.yes && asked.no && !/toolu_|req-/.test(asked.card + asked.proposal) && ownerTurns() === turnsBefore, { ...asked, turns: ownerTurns(), before: turnsBefore });
    { const p = await s.webviewPoint(chat, '.proposal:not(.answered) [data-proposal="yes"]'); await cdp.click(p.x, p.y); }
    let done = ''; for (let i = 0; i < 60 && done !== 'completed'; i++) { await delay(300); done = s.ctl('state').runs.find(r => r.id === asks.run.id).status; }
    await chat.waitFor(`document.querySelectorAll('.proposal:not(.answered)').length === 0`, 20000);
    await s.screenshot('permission-answered');
    check('a click on Yes answers it: the agent goes on and finishes', done === 'completed', { status: done });
    s.ctl('run.interrupt', { run_id: watch.run.id });
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
