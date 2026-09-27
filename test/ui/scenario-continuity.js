// Packaged-UI scenario for Continuity (Gate L: AC-83, AC-91 to AC-95, AC-98 in the extension).
// Everything outside VS Code and the daemon is SYNTHETIC and deterministic: the network and the
// machine's memory are files, Ollama is fixtures/fake-harness/ollama-fixture.js with four models
// (no model runs), Codex and Claude Code are fixtures/fake-harness/continuity-harness.js (they fail
// on command), OpenCode is fixtures/fake-harness/opencode-serve-fixture.js. No account, no tokens.
//
// What is shown: the first-use notice once per machine (its Allow flips the daemon's setting); the
// connection in the status bar and the side bar in each state; local models under Local in the
// Agent menu with fit badges that match the daemon's own dry run; offline, the online agents are
// blocked with the reason and one click moves to a local model; a local agent started from the
// composer completes the write check; the transition to local announced in both chats with the
// predecessor folded under its successor; a waiting agent's quiet card with Use a local model now,
// Retry now and Stop, and one Needs-you item; back online, Switch back and Stay; in both Overseer
// themes; the grid with a waiting tile.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const net = require('net');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');
const { auditExpression } = require('./audit');

const freePort = () => new Promise(resolve => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => resolve(p)); }); });
const writeWhole = (file, text) => { fs.writeFileSync(file + '.tmp', text); fs.renameSync(file + '.tmp', file); };
const ONLINE = { system: 'connected', baseline: { by_name: true, by_ip: true }, providers: { openai: true, anthropic: true } };
const OFFLINE = { system: 'none' };
const G = 2 ** 30;

(async () => {
  const s = new Session('continuity');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const fx = name => path.join(repoRoot, 'fixtures/fake-harness', name);
  const netFile = path.join(s.root, 'net.json'), memFile = path.join(s.root, 'memory.json'), control = path.join(s.root, 'harness.json'), harnessLog = path.join(s.root, 'harness.log');
  writeWhole(netFile, JSON.stringify(ONLINE));
  writeWhole(memFile, JSON.stringify({ total: 128 * G, available: 115 * G, pressure: 'normal' }));
  writeWhole(control, JSON.stringify({ codex: 'ok', claude: 'ok' }));
  const sys = path.join(s.root, 'system-home'); fs.mkdirSync(path.join(sys, '.codex'), { recursive: true });
  // The synthetic Ollama, on its own port, with four models.
  const port = await freePort();
  const ollama = cp.spawn(fx('ollama-fixture.js'), ['serve'], { env: { ...process.env, OLLAMA_HOST: `127.0.0.1:${port}`, OLLAMA_FIXTURE_MODELS: path.join(repoRoot, 'fixtures/continuity/ui-models.json'), OLLAMA_FIXTURE_LOAD_MS: '200' }, stdio: 'ignore' });
  const behave = (codex, claude) => writeWhole(control, JSON.stringify({ codex, claude }));
  const network = v => writeWhole(netFile, JSON.stringify(v));
  const settingsFile = path.join(s.profile, 'User/settings.json');
  const setTheme = async theme => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = theme; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(2500); };
  try {
    const repo = makeRepo(path.join(s.root, 'continuity-repo'), { dirty: false });
    s.settings({ 'workbench.colorTheme': 'Overseer Dark' });
    s.install(process.env.CONTINUITY_VSIX || latestVsix());
    const env = {
      OVERSEER_CODEX_PATH: fx('continuity-harness.js'), OVERSEER_CLAUDE_PATH: fx('continuity-harness.js'), OVERSEER_OPENCODE_PATH: fx('opencode-serve-fixture.js'),
      OVERSEER_OLLAMA_URL: `http://127.0.0.1:${port}`, OVERSEER_OLLAMA_CANDIDATES: fx('ollama-fixture.js'),
      OVERSEER_TEST_NET: netFile, OVERSEER_TEST_MEMORY: memFile, OVERSEER_TEST_SYSTEM_HOME: sys,
      OVERSEER_HARNESS_ENV_PASSTHROUGH: 'CONTINUITY_FIXTURE,CONTINUITY_LOG', CONTINUITY_FIXTURE: control, CONTINUITY_LOG: harnessLog,
      OVERSEER_TEST_CONTINUITY_TICK_MS: '400', OVERSEER_TEST_PROBE_MS: '400', OVERSEER_TEST_PROBE_IDLE_MS: '400', OVERSEER_TEST_WATCH_MS: '100',
      OVERSEER_TEST_RETRY_BASE_MS: '4000', OVERSEER_TEST_RETRY_CAP_MS: '16000', OVERSEER_TEST_STALL_MS: '60000',
    };
    s.launch(repo, env);
    let cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');
    const state = id => s.ctl('state').runs.find(r => r.id === id);
    const waitStatus = async (id, re, ms = 30000) => { for (let t = 0; t < ms; t += 250) { const st = state(id)?.status; if (re.test(st || '')) return st; await delay(250); } return state(id)?.status; };
    const statusItems = () => cdp.evalWorkbench(`[...document.querySelectorAll('.statusbar-item')].filter(e => e.offsetParent).map(e => ({ text: e.textContent.trim(), aria: e.querySelector('[aria-label]')?.getAttribute('aria-label') || e.getAttribute('aria-label') || '' }))`);
    const connectionItem = async () => (await statusItems()).find(i => /^Connection:/.test(i.aria));
    const sideMessage = () => cdp.evalWorkbench(`[...document.querySelectorAll('.pane')].filter(p => /^Agents/.test(p.querySelector('.pane-header')?.textContent.trim() || '')).map(p => p.querySelector('.message')?.textContent.trim() || '')[0] ?? null`);
    const waitConn = async (re, ms = 20000) => { for (let t = 0; t < ms; t += 300) { const c = await connectionItem(); if (c && re.test(c.aria)) return c; await delay(300); } return connectionItem(); };
    /** Opens the new-agent composer: the palette command, or the Agents view's own New Agent action when a webview keeps the keys. */
    const newAgent = async () => {
      await cdp.command('Overseer: New Agent'); await delay(800);
      const shown = await cdp.webview(`document.body.dataset.mode === 'composer'`, 5000).then(() => true, () => false);
      if (!shown) {
        await s.openOverseerView();
        const at = await cdp.evalWorkbench(`(() => { const a = [...document.querySelectorAll('.pane-header .actions .action-label')].find(a => /^New Agent/.test(a.getAttribute('aria-label') || '')); if (!a) return null; const r = a.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
        if (at) { await cdp.click(at.x, at.y); await delay(800); }
      }
      const d = await s.editorView(`document.body.dataset.mode === 'composer'`);
      await d.waitFor(`!document.querySelector('[data-chip="repo"]').textContent.includes('Loading')`, 20000);
      return d;
    };

    // ---- Online: the status bar has the connection, the side bar says nothing ----
    const online = await waitConn(/Online/);
    check('the status bar shows the connection: online, quietly', !!online && /Connection: Online/.test(online.aria) && !/Online/.test(online.text), online);
    await cdp.command('Overseer: Open Overseer View');
    let dash = await s.editorView();
    await dash.waitFor(`document.body.dataset.mode === 'composer' && !!document.querySelector('.view-composer:not([hidden]) #task') && !document.querySelector('[data-chip="repo"]').textContent.includes('Loading')`, 20000);
    await s.openOverseerView();
    check('the side bar says nothing about the connection while online', (await sideMessage()) === '' || (await sideMessage()) === null, await sideMessage());

    // ---- The first-use notice (AC-98): once, with Allow ----
    const notice = await dash.waitFor(`(() => { const n = document.querySelector('[data-continuity="notice"]'); return n && n.offsetParent ? { title: n.querySelector('.cont-title')?.textContent, switches: [...n.querySelectorAll('.cont-switch')].map(r => r.dataset.setting + ':' + r.querySelector('.state').textContent) } : null; })()`, 15000);
    check('the first time, one notice above the composer says what Continuity does and shows downloads and install as off', notice && notice.title === 'Continuity is on' && notice.switches.join(',') === 'allowModelDownloads:off,allowOllamaInstall:off', notice);
    // Compact where it sits (AC-54): one line and Got it; the explanation and the settings unfold from it.
    const compact = await dash.eval(`(() => { const n = document.querySelector('[data-continuity="notice"]'); const d = n && n.querySelector('details'); return d ? { open: d.open, text: n.innerText.replace(/\\s+/g, ' ').trim() } : null; })()`);
    check('until opened it is one line, Continuity is on · Got it, within the composer\'s text budget', compact && !compact.open && compact.text.length <= 30, compact);
    { const at = await s.webviewPoint(dash, '[data-continuity="notice"] summary'); await cdp.click(at.x, at.y); await delay(600); }
    const unfolded = await dash.eval(`(() => { const n = document.querySelector('[data-continuity="notice"]'); const d = n.querySelector('details'); const b = n.querySelector('[data-continuity="allow:allowModelDownloads"]'); return { open: d.open, allowVisible: !!(b && b.offsetParent), line: n.querySelector('.cont-line')?.textContent.slice(0, 40) }; })()`);
    check('opened, it says what may happen and its Allow is one click', unfolded && unfolded.open && unfolded.allowVisible && /^If the connection drops/.test(unfolded.line || ''), unfolded);
    await s.screenshot('notice-dark');
    { const at = await s.webviewPoint(dash, '[data-continuity="allow:allowModelDownloads"]'); await cdp.click(at.x, at.y); await delay(1200); }
    // Downloads first allowed: the one-time offer to keep the best-fitting model ready, declined.
    const offerToast = await cdp.waitFor(`[...document.querySelectorAll('.notification-toast, .notification-list-item')].map(t => t.innerText).find(t => /Keep qwen3-coder:30b ready for offline/.test(t)) || null`, 15000).catch(() => null);
    const notNow = await cdp.evalWorkbench(`(() => { const b = [...document.querySelectorAll('.notification-toast .monaco-button, .notification-list-item .monaco-button')].find(b => b.textContent.trim() === 'Not now'); if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
    if (notNow) { await cdp.click(notNow.x, notNow.y); await delay(1200); }
    const afterOffer = { prefetch: s.ctl('settings.get').settings.prefetch, again: s.ctl('continuity.prefetch_offer').show };
    check('when downloads are first allowed, Overseer offers once to keep the best-fitting model ready, naming it and its size; Not now leaves prefetch off', /Keep qwen3-coder:30b ready for offline\? \(already installed\)/.test(offerToast || '') && !!notNow && afterOffer.prefetch === false && afterOffer.again === false, { offerToast, afterOffer });
    const allowed = s.ctl('settings.get').settings.allowModelDownloads;
    const switched = await dash.eval(`document.querySelector('[data-continuity="notice"] .cont-switch[data-setting="allowModelDownloads"] .state')?.textContent`);
    check('Allow downloads flips the setting in the daemon, and the notice shows it', allowed === true && switched === 'allowed', { allowed, switched });
    { const at = await s.webviewPoint(dash, '[data-continuity="dismiss_notice"]'); await cdp.click(at.x, at.y); await delay(1200); }
    const gone = await dash.eval(`!document.querySelector('[data-continuity="notice"]')`);
    check('Got it dismisses the notice, and the daemon records it', gone && s.ctl('continuity.notice').show === false, { gone, notice: s.ctl('continuity.notice') });
    s.ctl('settings.set', { values: { allowModelDownloads: false } });

    // ---- Local models in the Agent menu (AC-94), badges as the daemon's dry run says ----
    /** Opens the Agent menu, lists it, and picks the entry `label` under the heading `head` (or closes it). */
    const pickMenu = async (head, label) => {
      const at = await s.webviewPoint(dash, '[data-chip="agent"]'); await cdp.click(at.x, at.y); await delay(400);
      await dash.waitFor(`!!document.querySelector('.menu')`, 5000);
      const items = await dash.eval(`[...document.querySelectorAll('.menu .menu-item, .menu .menu-head')].map(e => ({ head: e.classList.contains('menu-head') ? e.textContent : undefined, label: e.querySelector('.menu-label')?.textContent, hint: e.querySelector('.menu-hint')?.textContent, disabled: e.disabled, title: e.title }))`).catch(() => null);
      if (head) { await dash.eval(`(() => { let under = ''; for (const e of document.querySelectorAll('.menu .menu-item, .menu .menu-head')) { if (e.classList.contains('menu-head')) { under = e.textContent; continue; } if (under.startsWith(${JSON.stringify(head)}) && e.querySelector('.menu-label')?.textContent === ${JSON.stringify(label)}) { e.click(); return true; } } return false; })()`); await delay(400); }
      else { await cdp.key('Escape'); await delay(200); }
      return items;
    };
    const menu = await pickMenu();
    const local = menu && menu.filter(i => i.label && /qwen|Best fit/.test(i.label));
    const dry = s.ctl('local.models');
    const fits = tag => dry.models.find(m => m.tag === tag);
    check('the Agent menu lists local models under Local with a fit badge each', menu && menu.some(i => /^Local models · Ollama/.test(i.head || '')) && local.some(i => i.label === 'qwen3-coder:30b' && i.hint === 'fits at 64k') && local.some(i => i.label === 'qwen3.5:122b' && i.disabled && /too big/.test(i.hint)) && local.some(i => i.label === 'qwen2.5-coder:14b' && /fits at 32k · failed its check/.test(i.hint)), local);
    check("the badges match the daemon's dry run", fits('qwen3-coder:30b').fit.context === 65536 && fits('qwen3.5:122b').fit.status === 'too_big' && fits('qwen2.5-coder:14b').verified === 'failed' && local.find(i => i.label === 'Best fit · qwen3-coder:30b'), { dry: dry.pick });
    { const at = await s.webviewPoint(dash, '[data-chip="agent"]'); await cdp.click(at.x, at.y); await delay(400); await dash.waitFor(`!!document.querySelector('.menu')`, 5000); }
    await s.screenshot('agent-menu-local-dark');
    await cdp.key('Escape'); await delay(200);

    // ---- Offline (AC-83, AC-95): every view says so, none says online ----
    await pickMenu('Codex', 'Your login');
    network(OFFLINE);
    const off = await waitConn(/Offline/);
    let side; for (let i = 0; i < 30 && !/offline/.test(side || ''); i++) { await delay(300); side = await sideMessage(); }
    const banner = await dash.waitFor(`(() => { const b = document.querySelector('[data-continuity="banner"]'); return b && !b.hidden ? b.textContent : null; })()`, 10000);
    const note = await dash.waitFor(`(() => { const n = document.querySelector('.view-composer .composer-note'); return n && /Offline/.test(n.textContent) ? { text: n.textContent, fix: n.querySelector('.fix')?.textContent, disabled: document.getElementById('start').disabled } : null; })()`, 10000);
    check('offline: the status bar says Offline with the reason', off && /Connection: Offline/.test(off.aria) && /Offline/.test(off.text), off);
    check('offline: the side bar says so under Agents', /^Overseer is offline: no network \(system\)\./.test(side || ''), side);
    check('offline: the composer says so and blocks an online agent with the reason and a one-click move to a local model', /Overseer is offline/.test(banner || '') && note && /Offline: no network \(system\)\. Codex cannot be reached\./.test(note.text) && note.fix === 'Use a local model (qwen3-coder:30b)' && note.disabled, { banner, note });
    check('no view says online while offline', ![off.text, off.aria.replace('Connection: Offline', ''), side, banner, note.text].some(t => /\bonline\b/i.test(t || '')), { texts: [off.text, side, banner, note.text] });
    await s.screenshot('offline-composer-dark');
    { const at = await s.webviewPoint(dash, '.view-composer .composer-note .fix'); await cdp.click(at.x, at.y); await delay(500); }
    const chips = await dash.eval(`({ agent: document.querySelector('[data-chip="agent"]').getAttribute('aria-label'), model: document.querySelector('[data-chip="model"]').getAttribute('aria-label') })`);
    check('one click moves the composer to the best local model', /Local model/.test(chips.agent) && /Best fit · qwen3-coder:30b/.test(chips.model), chips);

    // ---- A local agent started from the composer, offline (AC-94) ----
    const before = s.ctl('state').runs.length;
    { const at = await s.webviewPoint(dash, '#task'); await cdp.click(at.x, at.y); await delay(200); await cdp.type('write local.txt hello from the composer; say done'); await delay(200); await cdp.key('Enter'); }
    let localRun; for (let i = 0; i < 60 && !localRun; i++) { await delay(300); localRun = s.ctl('state').runs.find((r, j) => j >= before && !r.parent_run_id && r.harness === 'opencode-serve'); }
    // Ask first is the default: the local agent asks before it writes, as any agent would, and is allowed.
    let asked = false;
    if (localRun) for (let i = 0; i < 100; i++) { const r = state(localRun.id); if (/completed|failed/.test(r?.status || '')) break; if (r?.status === 'waiting_for_user' && r.attention?.request_id) { asked = true; s.ctl('run.permission', { run_id: localRun.id, request_id: r.attention.request_id, allow: true }); } await delay(300); }
    const localDone = localRun && await waitStatus(localRun.id, /completed|failed/);
    const ws = localRun && s.ctl('state').workspaces.find(w => w.id === localRun.workspace_id);
    const wrote = ws && fs.existsSync(path.join(ws.path, 'local.txt')) ? fs.readFileSync(path.join(ws.path, 'local.txt'), 'utf8') : null;
    const modelNote = await dash.waitFor(`[...document.querySelectorAll('#conv .cont-note')].map(n => n.textContent).find(t => /^Local model/.test(t)) || null`, 20000).catch(() => null);
    check('a local agent started from the composer asks before it writes, then completes the write check with the network fixture off', localDone === 'completed' && asked && wrote === 'hello from the composer\n' && state(localRun.id).profile_id === 'local-ollama' && state(localRun.id).model === 'ollama/qwen3-coder:30b-64k', { status: localDone, asked, wrote, model: localRun && state(localRun.id).model });
    check('its chat names the local model, its context and its size', /Local model qwen3-coder:30b at a 64k context · 24.3 GiB\./.test(modelNote || ''), modelNote);
    await s.screenshot('local-run-dark');

    // ---- Transition to local (AC-91): both chats, the side bar folds the predecessor ----
    behave('network', 'ok');
    const moved = s.ctl('task.create', { repo, harness: 'codex', title: 'Rename the helpers', prompt: 'write moved.txt done by the local model; say done' });
    let successor; for (let i = 0; i < 100 && !successor; i++) { await delay(300); successor = s.ctl('continuity.handoffs').handoffs.find(h => h.predecessor === moved.run.id)?.successor; }
    await waitStatus(successor, /completed|failed/);
    await s.selectAgent('Rename the helpers');
    dash = await s.editorView(`document.getElementById('title')?.textContent === 'Rename the helpers'`);
    const opened = await dash.waitFor(`[...document.querySelectorAll('#conv .cont-note')].map(n => n.textContent)`, 15000);
    const rows = await s.agentRows();
    const folded = rows.find(r => /^Earlier: Codex/.test(r.label || ''));
    if (!folded) { await s.clickAgentRow('Rename the helpers', { twisty: true }); }
    const rows2 = await s.agentRows();
    const earlier = rows2.find(r => /^Earlier: Codex/.test(r.label || ''));
    check('the successor is the task\'s agent; its chat says where it continues from', state(moved.run.id)?.status === 'handed_off' && opened.some(t => /^Continued from "Rename the helpers" after the connection was lost at \d\d:\d\d\./.test(t)) && opened.some(t => /^This agent continues the work of another\./.test(t)), opened);
    check('the side bar folds the handed-off Codex under the agent that took over, with the reason', !!earlier && /handed off · the connection was lost/.test(earlier.description || ''), earlier || rows2);
    const sideOffline = await sideMessage();
    check('offline with agents: the side bar line stays', /^Overseer is offline: no network \(system\)\./.test(sideOffline || ''), sideOffline);
    await s.screenshot('transition-successor-dark');
    await s.clickAgentRow('Earlier: Codex');
    dash = await s.editorView(`/^Rename the helpers/.test(document.getElementById('title')?.textContent || '') && [...document.querySelectorAll('#conv .cont-note')].some(n => /Transitioning/.test(n.textContent))`);
    const told = await dash.eval(`[...document.querySelectorAll('#conv .cont-note')].map(n => n.textContent)`);
    const status = await dash.eval(`document.getElementById('status')?.getAttribute('aria-label')`);
    check("the predecessor's chat announces the transition in the owner's words, and reads handed off, not failed", told.some(t => t === 'Transitioning to qwen3-coder:30b (local, Ollama) because you\'ve disconnected. Work continues in the same worktree.') && told.some(t => /^The work continues in another agent\.Open it$/.test(t)) && status === 'Handed off', { told, status });
    await s.screenshot('transition-predecessor-dark');

    // ---- Waiting (AC-92): Continuity off, the quiet card and one Needs-you item ----
    s.ctl('settings.set', { values: { enabled: false } });
    const waiting = s.ctl('task.create', { repo, harness: 'codex', title: 'Update the docs', prompt: 'write docs.txt later; say done' });
    await waitStatus(waiting.run.id, /waiting_for_connection/);
    await s.selectAgent('Update the docs');
    dash = await s.editorView(`document.getElementById('title')?.textContent === 'Update the docs'`);
    const card = await dash.waitFor(`(() => { const c = document.querySelector('#conv [data-continuity-card="waiting"]'); return c && c.querySelectorAll('button').length >= 3 ? { title: c.querySelector('.cont-title')?.textContent, line: c.querySelector('.cont-line')?.textContent, foot: c.querySelector('.cont-foot')?.textContent, actions: [...c.querySelectorAll('button')].map(b => b.textContent), status: document.getElementById('status')?.getAttribute('aria-label'), footer: [...document.querySelectorAll('#conv .turn-foot')].filter(f => !f.hidden).map(f => f.textContent) } : null; })()`, 15000);
    const needs = (await s.agentRows()).find(r => /agent waiting for a connection/.test(r.label || ''));
    const bar = await connectionItem();
    const errors = await dash.eval(`document.querySelectorAll('#conv .error-block').length`);
    check('a waiting agent shows one quiet card: what is kept, the next check, Use a local model now, Retry now and Stop; its status is waiting, not failed', card && card.title === 'Waiting for a connection' && /Your message is kept\./.test(card.line) && /Next check (in \d+ s|now) · waiting \d+ s · gives up after 36 hours/.test(card.foot) && card.actions.join('|') === 'Use a local model now|Retry now|Stop' && card.status === 'Waiting for a connection' && card.footer.length === 0 && errors === 1, { ...card, errors });
    check('one Needs-you item counts the waiting agents, and the status bar counts them too', !!needs && needs.label === '1 agent waiting for a connection' && /1 waiting/.test(bar.text), { needs, bar });
    await s.screenshot('waiting-dark');
    // The grid shows the waiting agent as a tile, not as a failure.
    await cdp.command('Overseer: Toggle Grid'); await delay(1500);
    dash = await s.editorView(`document.body.dataset.mode === 'grid'`);
    const tile = await dash.waitFor(`(() => { const t = [...document.querySelectorAll('.tile')].find(t => t.querySelector('.tile-title')?.textContent === 'Update the docs'); return t ? { status: t.querySelector('.status')?.getAttribute('aria-label'), card: !!t.querySelector('[data-continuity-card="waiting"]'), needs: t.classList.contains('needs') } : null; })()`, 15000);
    check('the grid keeps the waiting agent as a tile with a cloud, its card compact, and no failure', tile && tile.status === 'Waiting for a connection' && tile.card && !tile.needs, tile);
    await s.screenshot('grid-waiting-dark');
    await cdp.command('Overseer: Toggle Grid'); await delay(1000);
    await s.selectAgent('Update the docs');
    dash = await s.editorView(`document.getElementById('title')?.textContent === 'Update the docs' && !!document.querySelector('#conv [data-continuity-card="waiting"]')`);
    { const at = await s.webviewPoint(dash, '[data-continuity="handoff:local"]'); await cdp.click(at.x, at.y); }
    let taken; for (let i = 0; i < 60 && !taken; i++) { await delay(300); taken = s.ctl('continuity.handoffs').handoffs.find(h => h.predecessor === waiting.run.id); }
    const takenDone = taken && await waitStatus(taken.successor, /completed|failed/);
    const shownNext = await dash.waitFor(`document.getElementById('title')?.textContent === 'Update the docs' && [...document.querySelectorAll('#conv .cont-note')].some(n => /^Continued from/.test(n.textContent))`, 20000).then(() => true, () => false);
    check('Use a local model now makes the handoff and shows the agent that took over', taken && taken.reason === 'user' && takenDone === 'completed' && shownNext, { taken, takenDone, shownNext });
    s.ctl('settings.set', { values: { enabled: true } });

    // ---- Back online (AC-93): the way back is offered once; Switch back resumes the first agent ----
    behave('ok', 'ok');
    network(ONLINE);
    await waitConn(/Online/);
    await s.selectAgent('Rename the helpers');
    dash = await s.editorView(`document.getElementById('title')?.textContent === 'Rename the helpers'`);
    const back = await dash.waitFor(`(() => { const c = document.querySelector('#conv [data-continuity-card="back"]'); return c ? { title: c.querySelector('.cont-title')?.textContent, line: c.querySelector('.cont-line')?.textContent, actions: [...c.querySelectorAll('button')].map(b => b.textContent) } : null; })()`, 20000);
    check('back online, the local agent offers Switch back and Stay', back && back.title === 'Back online' && back.line === 'This agent can continue with Codex, in the same worktree.' && back.actions.join('|') === 'Switch back to Codex|Stay here', back);
    check('the side bar and the status bar say nothing about being offline any more', !(/offline/i.test((await sideMessage()) || '')) && /Connection: Online/.test((await connectionItem()).aria), { side: await sideMessage() });
    await s.screenshot('back-online-dark');
    { const at = await s.webviewPoint(dash, '[data-continuity="handoff:back"]'); await cdp.click(at.x, at.y); }
    let backRun; for (let i = 0; i < 60 && !backRun; i++) { await delay(300); backRun = s.ctl('continuity.handoffs').handoffs.find(h => h.predecessor === successor)?.successor; }
    const backDone = backRun && await waitStatus(backRun, /completed|failed/);
    const first = state(moved.run.id), resumed = backRun && state(backRun);
    const starts = fs.readFileSync(harnessLog, 'utf8').trim().split('\n').map(l => JSON.parse(l)).filter(x => x.who === 'codex');
    const resume = starts[starts.length - 1];
    check('Switch back continues in Codex, in the same worktree, in its own session', backDone === 'completed' && resumed.harness === 'codex' && resumed.workspace_id === first.workspace_id && resumed.native_id === first.native_id && resume.args[0] === 'exec' && resume.args[1] === 'resume' && resume.args[2] === first.native_id, { backDone, session: first.native_id, args: resume?.args?.slice(0, 3) });
    await s.screenshot('switched-back-dark');
    // Back online, a new agent is the online one again (the last local start does not become the default).
    dash = await newAgent();
    await delay(600);
    const defaultAgent = await dash.eval(`document.querySelector('[data-chip="agent"]').getAttribute('aria-label')`);
    check('back online, a new agent defaults to the online agent again, not the local one', /Codex/.test(defaultAgent || ''), defaultAgent);

    // ---- The light theme: the same states ----
    await setTheme('Overseer Light');
    dash = await newAgent();
    await pickMenu('Codex', 'Your login');
    network(OFFLINE);
    await waitConn(/Offline/);
    await dash.waitFor(`(() => { const n = document.querySelector('.view-composer .composer-note'); return n && /Offline/.test(n.textContent); })()`, 10000);
    await s.screenshot('offline-composer-light');
    { const at = await s.webviewPoint(dash, '[data-chip="agent"]'); await cdp.click(at.x, at.y); await delay(400); await dash.waitFor(`!!document.querySelector('.menu')`, 5000); }
    await s.screenshot('agent-menu-local-light');
    await cdp.key('Escape'); await delay(200);
    s.ctl('settings.set', { values: { enabled: false } });
    behave('network', 'ok');
    const waiting2 = s.ctl('task.create', { repo, harness: 'codex', title: 'Tidy the tests', prompt: 'write tidy.txt later; say done' });
    await waitStatus(waiting2.run.id, /waiting_for_connection/);
    await s.selectAgent('Tidy the tests');
    dash = await s.editorView(`document.getElementById('title')?.textContent === 'Tidy the tests' && !!document.querySelector('#conv [data-continuity-card="waiting"]')`);
    await s.screenshot('waiting-light');
    await s.selectAgent('Rename the helpers');
    dash = await s.editorView(`/Rename the helpers/.test(document.getElementById('title')?.textContent || '')`);
    await s.screenshot('transition-light');
    // A stock theme, and the text budget: the side bar with its offline line, the chat with a waiting card.
    await setTheme('Default Dark Modern');
    await s.selectAgent('Tidy the tests');
    dash = await s.editorView(`document.getElementById('title')?.textContent === 'Tidy the tests' && !!document.querySelector('#conv [data-continuity-card="waiting"]')`);
    await s.screenshot('waiting-default-dark');
    const chatAudit = await dash.eval(auditExpression({ root: '.view-chat' }));
    const sideAudit = await cdp.evalWorkbench(auditExpression({ root: '.sidebar', exclude: ['.pane:has(.pane-header[aria-label*="Accounts"])', '.pane:has(.pane-header[aria-label*="Search"])'], nativeHover: true }));
    const sideLine = (await sideMessage()) || '';
    const waitingRow = ((await s.agentRows()).find(r => /waiting for a connection/.test(r.label || '')) || {}).label || '';
    result.textBudget = { chat: { chars: chatAudit.chars, longRuns: chatAudit.longRuns, overflow: chatAudit.overflow.length, unnamed: chatAudit.unnamed.length }, agents: { chars: sideAudit.chars, overflow: sideAudit.overflow.length, offlineLine: sideLine.length, waitingRow: waitingRow.length, rows: (await s.agentRows()).length } };
    check('offline, no view overflows sideways, no unbroken run over 80 characters outside code, every icon-only control has a name', chatAudit.overflow.length === 0 && chatAudit.longRuns.length === 0 && chatAudit.unnamed.length === 0 && sideAudit.overflow.length === 0, result.textBudget);
    check('the text budget: the chat with a waiting card stays under Gate J\'s chat budget (1050 characters); what Continuity adds to the side bar is one line under 80 characters and one row under 40', chatAudit.chars <= 1050 && sideLine.length > 0 && sideLine.length <= 80 && waitingRow.length > 0 && waitingRow.length <= 40, result.textBudget);
    s.ctl('settings.set', { values: { enabled: true } });
    s.ctl('run.interrupt', { run_id: waiting2.run.id });
    network(ONLINE); behave('ok', 'ok');

    // ---- The notice is shown once per machine: not after a reload ----
    await cdp.command('Developer: Reload Window'); await delay(6000);
    cdp = await s.connect(); s.cdp = cdp;
    dash = await s.editorView();
    dash = await newAgent();
    await delay(1500);
    const again = await dash.eval(`!!document.querySelector('[data-continuity="notice"]')`);
    check('after a reload the notice is not shown again', again === false, { again });
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    ollama.kill();
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    process.exit(failed ? 1 : 0);
  }
})();
