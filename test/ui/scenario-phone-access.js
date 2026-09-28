// Packaged-UI scenario for phone access on the Mac (Gate N): AC-116 (the switch in VS Code),
// AC-117 (Pair a Phone: the code and the owner's confirmation), AC-119 (the Devices list, scope,
// revoke) and the Mac's side of AC-129 (the switch for notifications to every phone, and no
// notification about the agent the window is showing). What the daemon sent or held back is read
// from its own record (its "push" events); payloads go to a folder of the scenario
// (OVERSEER_TEST_PUSH_DIR), never to Apple or to a phone.
//
// The real packaged extension and a real daemon, in an isolated profile and OVERSEER_HOME. The
// daemon advertises nothing on the network (OVERSEER_GATEWAY_MDNS=off) and listens on a port of
// its own, so the owner's daemon and the default port are never touched. The phone is the
// reference phone in test/ui/ref-phone.js: it does the real pairing handshake
// (Noise_IKpsk1_25519_ChaChaPoly_SHA256 over a WebSocket) and holds a real session.
//
// The QR code is checked three ways: the modules drawn in the panel (read from the SVG in the
// page) are read back by test/ui/qr-decode.js, which shares no code with the encoder; the
// screenshot of each theme is read by the Mac's own reader (Vision, test/ui/qr-vision.swift); and
// both must give the code shown as text, character for character.
//
// The Copy button is not pressed: it would replace what the owner has on the clipboard.
// Accounts come from the SYNTHETIC account CLI (fixtures/fake-harness/account-cli.js) with an
// empty desktop home, so no real login is read or shown.
// This scenario turns phone access on and off itself and checks it starts off: the suite-wide
// OVERSEER_TEST_PHONE_ACCESS=on (which turns it on before every other scenario) does not apply here.
if (process.env.OVERSEER_TEST_PHONE_ACCESS === 'on') {
  console.log('note: this scenario switches phone access itself; the suite-wide setting on does not apply');
  delete process.env.OVERSEER_TEST_PHONE_ACCESS;
}
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { Session, makeRepo, latestVsix, delay, repoRoot } = require('./harness');
const { auditExpression } = require('./audit');
const { Phone, parseCode, fingerprint, refused, freePort } = require('./ref-phone');
const { decode } = require('./qr-decode');
const { qrFromPath } = require('../../extension/src/phone-text');

const THEMES = ['Overseer Dark', 'Overseer Light', 'Default Dark Modern'];
const PAIRING_WINDOW_MS = 45000;
const CONFIRM_WAIT_MS = 12000;
const SOURCES = ['extension/media/pairing.css', 'extension/media/pairing.js', 'extension/src/phone-access.js', 'extension/src/phone-text.js'];

/** The same rule as scenario-theme.js: theme tokens, never a colour of our own. */
function lint() {
  const hits = [];
  for (const f of SOURCES) {
    fs.readFileSync(path.join(repoRoot, f), 'utf8').split('\n').forEach((line, i) => {
      if (/^\s*(\/\/|\*|\/\*)/.test(line)) return;
      for (const m of line.matchAll(/#[0-9a-fA-F]{3,8}\b|rgba?\(|hsla?\(|\b(?:white|black)\b(?=\s*[;}])/g)) hits.push(`${f}:${i + 1}: ${m[0]}`);
    });
  }
  const css = fs.readFileSync(path.join(repoRoot, 'extension/media/pairing.css'), 'utf8');
  const vars = [...css.matchAll(/var\((--[a-z0-9-]+)/g)].map(m => m[1]);
  const tokens = fs.readFileSync(path.join(repoRoot, 'extension/media/tokens.css'), 'utf8');
  const foreign = [...new Set(vars)].filter(v => !tokens.includes(v + ':') && !/^--qr-/.test(v));
  return { files: SOURCES.length, hits, foreign };
}

(async () => {
  const s = new Session('phone-access');
  const result = { checks: [] };
  const check = (name, ok, detail) => { result.checks.push({ name, ok: !!ok, detail }); s.note(`${ok ? 'PASS' : 'FAIL'} ${name}`, detail); };
  const phones = [];
  let vision;
  try {
    const l = lint();
    check('the pairing panel and phone access sources use theme tokens only (no colours of their own)', l.hits.length === 0 && l.foreign.length === 0, l);

    // The Mac's own QR reader, built for this run (skipped, and said so, where there is no Swift).
    const tool = path.join(s.root, 'qr-vision');
    const built = cp.spawnSync('swiftc', ['-O', path.join(__dirname, 'qr-vision.swift'), '-o', tool], { encoding: 'utf8' });
    if (built.status === 0) vision = file => { const r = cp.spawnSync(tool, [file], { encoding: 'utf8' }); try { return JSON.parse(r.stdout).codes || []; } catch { return []; } };
    else s.note('no Swift compiler: screenshots are not read by the Mac\'s QR reader', (built.stderr || '').slice(0, 200));

    const port = await freePort();
    const cli = path.join(repoRoot, 'fixtures/fake-harness/account-cli.js');
    const sys = path.join(s.root, 'desktop-home');
    const pushed = path.join(s.root, 'pushed');
    fs.mkdirSync(sys, { recursive: true });
    fs.mkdirSync(pushed, { recursive: true });
    const repo = makeRepo(path.join(s.root, 'phone-demo'), { dirty: false });
    const settingsFile = path.join(s.profile, 'User/settings.json');
    s.settings({ 'workbench.colorTheme': THEMES[0], 'window.dialogStyle': 'custom', 'window.menuStyle': 'custom', 'window.titleBarStyle': 'custom' });
    s.install(latestVsix());
    s.launch(repo, { OVERSEER_GATEWAY_MDNS: 'off', OVERSEER_GATEWAY_PORT: String(port), OVERSEER_TEST_PAIRING_WINDOW_MS: String(PAIRING_WINDOW_MS), OVERSEER_TEST_CONFIRM_WAIT_MS: String(CONFIRM_WAIT_MS),
      OVERSEER_TEST_PUSH_DIR: pushed, OVERSEER_CODEX_PATH: cli, OVERSEER_CLAUDE_PATH: cli, OVERSEER_OPENCODE_PATH: '/nonexistent/opencode', OVERSEER_TEST_SYSTEM_HOME: sys, OVERSEER_HARNESS_ENV_PASSTHROUGH: 'OVERSEER_TEST_SYSTEM_HOME' });
    const cdp = await s.connect();
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar');

    // ---------------------------------------------------------------- helpers
    const status = () => s.ctl('gateway.status');
    const devices = () => s.ctl('gateway.devices').devices.filter(d => !d.revoked_ms);
    const events = kind => s.ctl('events.list', { limit: 5000 }).events.filter(e => e.kind === kind);
    const bar = () => cdp.evalWorkbench(`(() => { const e = [...document.querySelectorAll('.statusbar-item')].find(e => /phone/i.test(e.textContent)); if (!e) return null; const a = e.querySelector('a') || e; const r = e.getBoundingClientRect(); const cs = getComputedStyle(e);
      const other = [...document.querySelectorAll('.statusbar-item')].find(o => /Overseer \\d+ active/.test(o.textContent));
      return { text: e.textContent.trim(), aria: e.getAttribute('aria-label') || a.getAttribute('aria-label'), icon: (e.querySelector('.codicon')?.className.match(/codicon-([a-z-]+)/) || [])[1], x: r.left + r.width / 2, y: r.top + r.height / 2,
        background: cs.backgroundColor, color: getComputedStyle(a).color, otherColor: other ? getComputedStyle(other.querySelector('a') || other).color : null, otherBackground: other ? getComputedStyle(other).backgroundColor : null,
        afterOverseer: other ? r.left > other.getBoundingClientRect().left : null, classes: e.className }; })()`);
    const barSays = async (re, ms = 15000) => { for (let t = 0; t < ms; t += 200) { const b = await bar(); if (b && re.test(b.text)) return b; await delay(200); } return bar(); };
    const quietMessage = (re, ms = 10000) => cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].map(e => e.textContent.trim()).find(t => ${re}.test(t)) || null`, ms).catch(() => null);
    const toasts = () => cdp.evalWorkbench(`[...document.querySelectorAll('.notification-toast')].map(t => t.innerText)`);
    const dialogState = () => cdp.evalWorkbench(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d) return null; return { text: d.innerText, message: d.querySelector('.dialog-message-text')?.innerText || '', detail: d.querySelector('.dialog-message-detail')?.innerText || '', buttons: [...d.querySelectorAll('.monaco-button')].map(b => b.textContent.trim()) }; })()`);
    const waitDialog = (re, ms = 20000) => cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d || !${re}.test(d.innerText)) return null; return { text: d.innerText, message: d.querySelector('.dialog-message-text')?.innerText || '', detail: d.querySelector('.dialog-message-detail')?.innerText || '', buttons: [...d.querySelectorAll('.monaco-button')].map(b => b.textContent.trim()) }; })()`, ms, 'dialog ' + re);
    const press = async button => {
      const b = await cdp.waitFor(`(() => { const d = document.querySelector('.monaco-dialog-box'); if (!d) return null; const b = [...d.querySelectorAll('.monaco-button')].find(b => b.textContent.trim() === ${JSON.stringify(button)}); if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`, 20000, 'button ' + button);
      await cdp.click(b.x, b.y);
      await cdp.waitFor(`!document.querySelector('.monaco-dialog-box')`, 10000, 'dialog closed');
      await delay(400);
    };
    const panel = () => cdp.webview(`!!document.querySelector('[data-audit-view="pairing"]') && document.body.dataset.ready === '1'`, 30000);
    const panelState = frame => frame.eval(`(() => { const $ = id => document.getElementById(id); const svg = document.querySelector('#qr svg'); const vis = e => !!e && e.checkVisibility();
      const cs = sel => { const e = document.querySelector(sel); return e ? getComputedStyle(e).fill : null; };
      return { phase: document.querySelector('main').dataset.phase, title: document.querySelector('h1').textContent, mac: $('mac').textContent, code: $('code').textContent, groups: [...document.querySelectorAll('#code .group')].map(g => g.textContent),
        left: $('left').textContent, steps: [...document.querySelectorAll('.steps li')].map(li => li.textContent), d: svg?.querySelector('path')?.getAttribute('d') || '', side: Number(svg?.dataset.side || 0), quiet: Number(svg?.dataset.quiet || 0),
        qr: (() => { const q = $('qr').getBoundingClientRect(); return { w: Math.round(q.width), h: Math.round(q.height), shown: vis($('qr')) && !!svg }; })(), paper: cs('.qr-paper'), ink: cs('.qr-ink'),
        copy: vis($('copy')) ? { text: $('copy').textContent.trim(), name: $('copy').getAttribute('aria-label') || $('copy').textContent.trim() } : null,
        over: vis($('over')) ? $('over-text').textContent : '', newCode: vis($('new')) ? $('new').textContent.trim() : '', done: vis($('done')) ? $('done').textContent.trim() : '', selectable: getComputedStyle($('code')).userSelect }; })()`);
    const panelPhase = async (frame, phase, ms = 20000) => { await frame.waitFor(`document.querySelector('main').dataset.phase === ${JSON.stringify(phase)}`, ms); return panelState(frame); };
    const clickIn = async (frame, selector) => { const p = await s.webviewPoint(frame, selector); await cdp.click(p.x, p.y); await delay(300); };
    const luminance = rgb => { const m = String(rgb).match(/[\d.]+/g)?.map(Number) || [0, 0, 0]; const f = v => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(m[0]) + 0.7152 * f(m[1]) + 0.0722 * f(m[2]); };
    const setTheme = async theme => { const cur = JSON.parse(fs.readFileSync(settingsFile, 'utf8')); cur['workbench.colorTheme'] = theme; fs.writeFileSync(settingsFile, JSON.stringify(cur, null, 2)); await delay(2200); };
    const slug = t => t.replace(/\s+/g, '-').toLowerCase();
    const pane = () => cdp.evalWorkbench(`(() => { const pane = [...document.querySelectorAll('.pane')].find(p => /^Devices/.test(p.querySelector('.pane-header')?.textContent.trim() || '')); if (!pane) return null;
      const titles = [...document.querySelectorAll('.pane-header')].filter(h => h.closest('.part.sidebar, .part.auxiliarybar')).map(h => (h.querySelector('.title')?.textContent || h.textContent).trim());
      return { header: pane.querySelector('.pane-header h3, .pane-header .title')?.textContent.trim(), description: pane.querySelector('.pane-header .description')?.textContent.trim() || '', expanded: pane.querySelector('.pane-header').getAttribute('aria-expanded'), titles,
        welcome: (() => { const w = pane.querySelector('.welcome-view'); return w && w.offsetParent ? { text: w.innerText.trim(), buttons: [...w.querySelectorAll('.monaco-button')].map(b => b.textContent.trim()) } : null; })(),
        titleActions: [...pane.querySelectorAll('.pane-header .actions .action-label')].map(a => a.getAttribute('aria-label')).filter(Boolean),
        rows: [...pane.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).map(r => ({ label: r.querySelector('.label-name')?.textContent.trim(), description: r.querySelector('.label-description')?.textContent.trim() || '', aria: r.getAttribute('aria-label'), level: Number(r.getAttribute('aria-level')), expanded: r.getAttribute('aria-expanded'),
          icon: (r.querySelector('.custom-view-tree-node-item-icon')?.className.match(/codicon-([a-z-]+)/) || [])[1] || '', iconColor: (() => { const i = r.querySelector('.custom-view-tree-node-item-icon'); return i ? getComputedStyle(i).color : ''; })(),
          actions: [...r.querySelectorAll('.actions .action-label')].map(a => ({ name: a.getAttribute('aria-label'), icon: (a.className.match(/codicon-([a-z-]+)/) || [])[1] })) })) }; })()`);
    const rowPoint = label => cdp.waitFor(`(() => { const pane = [...document.querySelectorAll('.pane')].find(p => /^Devices/.test(p.querySelector('.pane-header')?.textContent.trim() || '')); const r = pane && [...pane.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent && (r.querySelector('.label-name')?.textContent || '').trim() === ${JSON.stringify(label)}); if (!r) return null; const b = r.getBoundingClientRect(); return { x: b.left + 70, y: b.top + b.height / 2 }; })()`, 15000, 'device ' + label);
    const waitRows = async (test, ms = 15000) => { let p; for (let t = 0; t < ms; t += 250) { p = await pane(); if (p && test(p)) return p; await delay(250); } return p; };
    const contextMenu = async (label, entry) => {
      const pt = await rowPoint(label);
      await cdp.click(pt.x, pt.y); await delay(300);
      await cdp.key('F10', { shift: true });
      await cdp.waitFor(`!!document.querySelector('.monaco-menu .action-item')`, 10000, 'context menu');
      const entries = await cdp.evalWorkbench(`[...document.querySelectorAll('.monaco-menu .action-item .action-label')].map(e => e.textContent.trim()).filter(Boolean)`);
      if (!entry) { await cdp.key('Escape'); await delay(300); return entries; }
      for (let i = 0; i < 12; i++) {
        const focused = await cdp.evalWorkbench(`(document.querySelector('.monaco-menu .action-item.focused')?.textContent || '').trim()`);
        if (focused.startsWith(entry)) break;
        await cdp.key('ArrowDown'); await delay(120);
      }
      await cdp.key('Enter'); await delay(600);
      return entries;
    };
    const phone = (name, platform) => { const p = new Phone({ name, platform }); phones.push(p); return p; };
    // Fixture agents (a shell that waits, then ends) and the daemon's record of what it told the phones.
    const agent = (title, seconds) => s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', `sleep ${seconds}; echo done`], prompt: '', title, workspace_mode: 'worktree' }).run.id;
    const told = async (run, name, ms = 40000) => { for (let t = 0; t < ms; t += 300) { const e = events('push').find(e => e.run_id === run && e.payload.name === name); if (e) return e.payload; await delay(300); } return null; };
    const payloads = () => fs.readdirSync(pushed).map(f => JSON.parse(fs.readFileSync(path.join(pushed, f), 'utf8')));
    /** What the extension wrote to its log (the Overseer output channel). */
    const logText = () => { const out = []; const walk = dir => { for (const e of fs.readdirSync(dir, { withFileTypes: true })) { const f = path.join(dir, e.name); if (e.isDirectory()) walk(f); else if (/Overseer\.log$/.test(e.name)) out.push(fs.readFileSync(f, 'utf8')); } }; try { walk(path.join(s.profile, 'logs')); } catch {} return out.join('\n'); };
    const logged = async (text, ms = 15000) => { for (let t = 0; t < ms; t += 300) { if (logText().includes(text)) return true; await delay(300); } return false; };
    /** Folds a side bar view by its header, so the view under test has room. */
    const fold = async name => {
      const h = await cdp.evalWorkbench(`(() => { const h = [...document.querySelectorAll('.part.sidebar .pane-header')].find(h => (h.querySelector('.title')?.textContent || h.textContent).trim().startsWith(${JSON.stringify(name)})); if (!h || h.getAttribute('aria-expanded') !== 'true') return null; const r = h.getBoundingClientRect(); return { x: r.left + 60, y: r.top + r.height / 2 }; })()`);
      if (h) { await cdp.click(h.x, h.y); await delay(500); }
    };
    const readCode = state => { const rows = qrFromPath(state.d, state.side, state.quiet).rows; return decode(rows); };

    // ---------------------------------------------------------------- 1. off, and nothing listens
    const off = await barSays(/Phone access off/);
    const st0 = status();
    check('1. the status bar says "Phone access off", with a phone icon and an accessible name', !!off && off.text === 'Phone access off' && off.icon === 'device-mobile' && /Phone access off/.test(off.aria || ''), off);
    check('1. off is quiet: the item has no colour of its own (same colours as the Overseer item beside it)', !!off && off.color === off.otherColor && off.background === off.otherBackground && off.afterOverseer === true && !/(warning|error|prominent)-kind/.test(off.classes), off);
    check('1. phone access is off in the daemon and nothing listens on its port', st0.enabled === false && st0.port === null && await refused(port), { enabled: st0.enabled, port: st0.port, configured: st0.settings.port, refused: await refused(port) });
    await s.screenshot('status-bar-off');

    // The status bar item opens the actions that make sense now.
    await cdp.click(off.x, off.y);
    await cdp.waitQuickTitle('Phone access: off');
    const menuOff = (await cdp.quickInputState()).rows;
    await s.screenshot('status-bar-menu-off');
    await cdp.key('Escape'); await delay(400);
    check('1. clicking it offers Turn On, Pair a Phone, Show Devices and the notifications switch (not Turn Off)', menuOff.some(r => /Turn On Phone Access/.test(r)) && menuOff.some(r => /Pair a Phone/.test(r)) && menuOff.some(r => /Show Devices/.test(r)) && menuOff.some(r => /Turn Off Notifications to Phones/.test(r)) && !menuOff.some(r => /Turn Off Phone Access/.test(r)), menuOff);

    // ---------------------------------------------------------------- 2. turn it on
    await cdp.command('Overseer: Turn On Phone Access');
    const on = await barSays(/Phone access on/);
    const st1 = status();
    check('2. Turn On Phone Access from the command palette: the status bar says "Phone access on"', !!on && on.text === 'Phone access on' && /Phone access on, no phone connected/.test(on.aria || ''), on);
    check('2. the daemon says phone access is on, and its port listens', st1.enabled === true && st1.port === port && !(await refused(port)), { enabled: st1.enabled, port: st1.port });
    check('2. on has no colour of its own either', on.color === on.otherColor && on.background === on.otherBackground, on);
    await s.screenshot('status-bar-on');
    await cdp.click(on.x, on.y);
    await cdp.waitQuickTitle('Phone access: on');
    const menuOn = (await cdp.quickInputState()).rows;
    await cdp.key('Escape'); await delay(400);
    check('2. now the menu offers Turn Off, and no longer Turn On', menuOn.some(r => /Turn Off Phone Access/.test(r)) && !menuOn.some(r => /Turn On Phone Access/.test(r)) && menuOn.some(r => /Pair a Phone/.test(r)), menuOn);

    // ---------------------------------------------------------------- 3. Pair a Phone: the code
    await cdp.command('Overseer: Pair a Phone');
    let frame = await panel();
    const first = await panelPhase(frame, 'open');
    const opened = Date.now();
    const parsed = parseCode(first.code);
    check('3. Pair a Phone shows the code as text: it starts with OVSR1-, names this Mac\'s key and port', first.code.startsWith('OVSR1-') && parsed.port === port && fingerprint(parsed.gatewayPublic) === st1.fingerprint && parsed.addresses.includes('127.0.0.1'), { code: first.code, port: parsed.port, addresses: parsed.addresses, key: fingerprint(parsed.gatewayPublic) });
    check('3. the text is the code, character for character, in groups for the eye, and can be selected', first.groups.join('') === first.code && first.groups[0] === 'OVSR1-' && first.groups.slice(1, -1).every(g => g.length === 4) && first.selectable === 'all', { groups: first.groups.length, selectable: first.selectable });
    let read; try { read = readCode(first); } catch (error) { read = { error: error.message }; }
    check('3. the QR code drawn in the panel reads back as exactly the code shown as text', read.text === first.code, { version: read.version, level: read.level, mask: read.mask, modules: first.side - 2 * first.quiet, quiet: first.quiet, error: read.error });
    check('3. the panel shows the Mac\'s name, three numbered steps, the time left and a Copy button', first.mac === `With ${st1.name}` && first.steps.length === 3 && /^Works once, for 0:4\d more\.$/.test(first.left) && first.copy?.text === 'Copy' && first.qr.shown && first.qr.w >= 200 && first.qr.w === first.qr.h, { mac: first.mac, steps: first.steps, left: first.left, copy: first.copy, qr: first.qr });
    await delay(2100);
    const later = await panelState(frame);
    check('3. the time left counts down', later.left !== first.left && later.left < first.left, { first: first.left, later: later.left });
    const pairingNow = status().pairing;
    check('3. the daemon has pairing open, for this code only', pairingNow?.open === true && pairingNow.waiting.length === 0, pairingNow);

    fs.writeFileSync(path.join(s.evidence, 'qr.json'), JSON.stringify({ note: 'A pairing code of the scenario\'s own daemon; it worked once, for 45 seconds.', code: first.code, read, modules: qrFromPath(first.d, first.side, first.quiet).rows.map(r => r.map(v => (v ? '#' : '.')).join('')) }, null, 2));
    const themed = {};
    for (const theme of THEMES) {
      await setTheme(theme);
      const st = await panelState(frame);
      const file = await s.screenshot(`pair-a-phone-${slug(theme)}`);
      const audit = await frame.eval(auditExpression({ root: '[data-audit-view="pairing"]' }));
      const seen = vision ? vision(file) : null;
      themed[theme] = { paper: st.paper, ink: st.ink, contrast: Number(((Math.max(luminance(st.paper), luminance(st.ink)) + 0.05) / (Math.min(luminance(st.paper), luminance(st.ink)) + 0.05)).toFixed(2)), darkOnLight: luminance(st.ink) < luminance(st.paper), audit: { chars: audit.chars, overflow: audit.overflow, unnamed: audit.unnamed, longRuns: audit.longRuns }, camera: seen };
    }
    result.themes = themed;
    check('3. in Overseer Dark, Overseer Light and a stock theme the code is dark on light, from the theme\'s own colours', Object.values(themed).every(t => t.darkOnLight && t.contrast >= 7), Object.fromEntries(Object.entries(themed).map(([k, v]) => [k, { paper: v.paper, ink: v.ink, contrast: v.contrast }])));
    check('3. in each theme: nothing overflows, no long unbroken text, every icon-only control is named', Object.values(themed).every(t => t.audit.overflow.length === 0 && t.audit.unnamed.length === 0 && t.audit.longRuns.length === 0), Object.fromEntries(Object.entries(themed).map(([k, v]) => [k, v.audit])));
    if (vision) check('3. the Mac\'s own QR reader reads the screenshot of each theme as exactly the code', Object.values(themed).every(t => t.camera.length === 1 && t.camera[0] === first.code), Object.fromEntries(Object.entries(themed).map(([k, v]) => [k, v.camera.map(c => c === first.code)])));
    else result.skipped = ['the Mac\'s QR reader (no Swift compiler)'];
    await setTheme(THEMES[0]);

    // The code works for its time only (45 seconds in this scenario; two minutes for the owner).
    const expired = await panelPhase(frame, 'over', PAIRING_WINDOW_MS + 5000);
    const took = Date.now() - opened;
    check('3. when its time is over the panel says the code has expired and offers "New code"', expired.over === 'This code has expired.' && expired.newCode === 'New code' && !expired.qr.shown && expired.code === '' && took >= PAIRING_WINDOW_MS - 1500, { over: expired.over, newCode: expired.newCode, afterMs: took });
    await s.screenshot('pair-a-phone-expired');
    const late = phone('Late Phone', 'ios');
    const lateAnswer = await late.pair(first.code, { wait: 6000 }).then(() => 'paired', e => e.message);
    check('3. an expired code pairs nothing', lateAnswer !== 'paired' && devices().length === 0 && !(await dialogState()), { lateAnswer });

    // ---------------------------------------------------------------- 4 and 5. a phone asks; the owner pairs it
    await clickIn(frame, '#new');
    const second = await panelPhase(frame, 'open');
    check('4. New code shows another code, and its QR code reads back as that code', second.code !== first.code && second.code.startsWith('OVSR1-') && readCode(second).text === second.code, { changed: second.code !== first.code });
    const iphone = phone("Bilal's iPhone", 'ios');
    const asking = iphone.pair(second.code);
    asking.catch(() => {});
    const question = await waitDialog('/Pair "Bilal\'s iPhone"\\?/');
    const key = fingerprint(iphone.keys.public).replace(/(.{4})(?=.)/g, '$1 ');
    check('4. the Mac asks: Pair "Bilal\'s iPhone"? with the platform, the address and the phone\'s key', /^Pair "Bilal's iPhone"\?$/.test(question.message.trim()) && question.detail.includes('iPhone · 127.0.0.1') && question.detail.includes(`Key ${key}`), question);
    check('4. the choices are Pair and Don\'t Pair', JSON.stringify([...question.buttons].sort()) === JSON.stringify(["Don't Pair", 'Pair']), question.buttons);
    const waiting = await panelPhase(frame, 'waiting');
    check('4. the panel says the phone is asking, and no longer shows the used code', /"Bilal's iPhone" is asking to pair/.test(waiting.over) && !waiting.qr.shown && waiting.code === '', waiting.over);
    check('4. nothing is paired before the owner answers', devices().length === 0 && status().pairing?.waiting?.[0]?.name === "Bilal's iPhone", status().pairing);
    await s.screenshot('pair-confirmation');
    await press('Pair');
    const hello = await Promise.race([asking, delay(10000).then(() => ({ error: 'no answer' }))]).catch(e => ({ error: e.message }));
    iphone.keepAlive(8000);
    check('5. after Pair the phone\'s handshake completes: it has its device id and full control', !!hello.device && hello.scope === 'full' && hello.gateway === st1.name && hello.fingerprint === st1.fingerprint, hello);
    const pairedPanel = await panelPhase(frame, 'paired');
    check('5. the panel says it is paired, with Done and Pair Another Phone', pairedPanel.over === 'Paired with "Bilal\'s iPhone".' && pairedPanel.done === 'Done' && pairedPanel.newCode === 'Pair Another Phone', pairedPanel);
    const one = await barSays(/^1 phone$/);
    check('5. the status bar says "1 phone"', one?.text === '1 phone' && /Phone access on, 1 phone connected/.test(one.aria || ''), one);
    await cdp.command('Overseer: Show Devices');
    await fold('Accounts');
    let list = await waitRows(p => p.rows.some(r => r.label === "Bilal's iPhone"));
    check('5. Devices is a view of the Overseer side bar, after Accounts', JSON.stringify(list?.titles.map(t => t.replace(/\s+/g, ' ')).filter(t => /^(Agents|Accounts|Devices)/.test(t)).map(t => t.split(' ')[0])) === JSON.stringify(['Agents', 'Accounts', 'Devices']), list?.titles);
    const row = list?.rows.find(r => r.label === "Bilal's iPhone");
    check('5. the Devices view lists the phone as connected, with its scope and a phone icon', !!row && row.description === 'connected · Full control' && row.icon === 'device-mobile' && /Bilal's iPhone, iPhone, connected, Full control/.test(row.aria || ''), row);
    // Its details: what AC-119 lists (platform, paired time, last seen, address) and its key.
    const pt = await rowPoint("Bilal's iPhone");
    await cdp.click(pt.x, pt.y); await delay(300);
    await cdp.key('ArrowRight'); await delay(600);
    list = await waitRows(p => p.rows.some(r => r.level === 2));
    const details = Object.fromEntries((list?.rows || []).filter(r => r.level === 2).map(r => [r.label, r.description]));
    check('5. opened, the row shows the phone, its scope, when it was paired, when it was last seen, its address and its key', /^iPhone/.test(details.Phone || '') && details.Scope === 'Full control' && !!details.Paired && details['Last seen'] === 'connected now' && details.Address === '127.0.0.1' && details.Key === key, details);
    const selected = (await pane()).rows.find(r => r.label === "Bilal's iPhone");
    check('5. the row\'s icons are named: Rename…, Make Watch Only, Revoke Device…', JSON.stringify(selected.actions.map(a => a.name)) === JSON.stringify(['Rename…', 'Make Watch Only', 'Revoke Device…']), selected.actions);
    await s.screenshot('devices-row-actions');
    // At rest (not selected, not under the pointer, where a row gives room to its icons) the row
    // shows its whole description.
    const detail = await rowPoint('Address');
    await cdp.click(detail.x, detail.y); await delay(200);
    await cdp.move(1200, 700); await delay(500);
    const whole = await cdp.evalWorkbench(`(() => { const pane = [...document.querySelectorAll('.pane')].find(p => /^Devices/.test(p.querySelector('.pane-header')?.textContent.trim() || '')); const r = [...pane.querySelectorAll('.monaco-list-row')].find(r => r.offsetParent && (r.querySelector('.label-name')?.textContent || '').trim() === "Bilal's iPhone"); const d = r.querySelector('.label-description');
      // Cut short when any box around the text is narrower than the text it holds.
      const cut = []; for (let e = d; e && e !== r; e = e.parentElement) if (e.scrollWidth > e.clientWidth + 1) cut.push(e.className);
      const right = d.getBoundingClientRect().right, edge = r.getBoundingClientRect().right;
      return { text: d.textContent.trim(), cut, right: Math.round(right), edge: Math.round(edge), icons: [...r.querySelectorAll('.actions .action-label')].filter(a => a.offsetParent).length, rows: [...pane.querySelectorAll('.monaco-list-row')].filter(r => r.offsetParent).length }; })()`);
    check('5. the whole list fits the side bar: name, connected, scope and the six details', whole.text === 'connected · Full control' && whole.cut.length === 0 && whole.right <= whole.edge && whole.rows === 7, whole);
    await s.screenshot('devices-connected');
    for (const theme of THEMES.slice(1)) { await setTheme(theme); await s.screenshot(`devices-${slug(theme)}`); }
    await setTheme(THEMES[0]);

    // ---------------------------------------------------------------- 6. a phone the owner does not pair
    await clickIn(frame, '#new');
    const third = await panelPhase(frame, 'open');
    const stranger = phone('Unknown Pixel', 'android');
    const refusedPairing = stranger.pair(third.code).then(() => 'paired', e => e.message);
    const q2 = await waitDialog('/Pair "Unknown Pixel"\\?/');
    await s.screenshot('pair-confirmation-second-phone');
    await press("Don't Pair");
    const answer2 = await refusedPairing;
    const declined = await panelPhase(frame, 'over');
    check('6. Don\'t Pair: the second phone gets no answer, and nothing is added', answer2 !== 'paired' && q2.detail.includes('Android phone · 127.0.0.1') && devices().length === 1 && !(await pane()).rows.some(r => r.label === 'Unknown Pixel'), { answer2, devices: devices().map(d => d.name) });
    check('6. the panel says the phone was not paired and offers a new code', declined.over === '"Unknown Pixel" was not paired.' && declined.newCode === 'New code', declined.over);
    await s.screenshot('pair-declined');

    // One answer wins: the owner answers somewhere else (another window, the terminal) while this question is open.
    await clickIn(frame, '#new');
    const fourth = await panelPhase(frame, 'open');
    const tablet = phone('Spare Phone', 'android');
    const elsewhere = tablet.pair(fourth.code);
    elsewhere.catch(() => {});
    await waitDialog('/Pair "Spare Phone"\\?/');
    const request = status().pairing.waiting[0].request;
    s.ctl('gateway.pair_confirm', { request, accept: true });
    const hello3 = await Promise.race([elsewhere, delay(10000).then(() => ({ error: 'no answer' }))]).catch(e => ({ error: e.message }));
    tablet.keepAlive(8000);
    await barSays(/^2 phones$/);
    await press('Pair');
    const quiet1 = await quietMessage('/no longer waiting to pair/');
    const errors1 = (await toasts()).filter(t => /Overseer:|no phone is waiting/i.test(t));
    check('6. answered elsewhere first: this window\'s answer is taken quietly (no error), and the phone is paired once', !!hello3.device && !!quiet1 && errors1.length === 0 && devices().filter(d => d.name === 'Spare Phone').length === 1 && devices().length === 2, { quiet1, errors1, devices: devices().map(d => d.name) });
    const two = await barSays(/^2 phones$/);
    check('6. the status bar says "2 phones"', two?.text === '2 phones', two);

    // A phone nobody answers: it stops waiting (12 seconds in this scenario; a minute for the owner).
    const panelNow = await panelPhase(frame, 'paired');
    await clickIn(frame, '#new');
    const fifth = await panelPhase(frame, 'open');
    const slow = phone('Slow Phone', 'ios');
    const gaveUp = slow.pair(fifth.code).then(() => 'paired', e => e.message);
    await waitDialog('/Pair "Slow Phone"\\?/');
    const quiet2 = await quietMessage('/"Slow Phone" stopped waiting to pair/', CONFIRM_WAIT_MS + 8000);
    const timedOut = await panelPhase(frame, 'over');
    const answer5 = await gaveUp;
    await s.screenshot('pair-timed-out');
    await press('Pair');
    const quiet3 = await quietMessage('/"Slow Phone" is no longer waiting to pair/');
    const errors2 = (await toasts()).filter(t => /Overseer:|no phone is waiting/i.test(t));
    check('6. a question nobody answers: the phone stops waiting, the owner is told quietly, and a late Pair pairs nothing', answer5 !== 'paired' && !!quiet2 && !!quiet3 && errors2.length === 0 && /"Slow Phone" was not paired\./.test(timedOut.over) && devices().length === 2 && !!panelNow, { answer5, quiet2, quiet3, over: timedOut.over, errors2 });

    // Closing the panel takes the code back.
    await clickIn(frame, '#new');
    await panelPhase(frame, 'open');
    const before = events('pairing_closed').length;
    await cdp.command('View: Close Editor');
    await delay(1200);
    const cancelled = events('pairing_closed').slice(before);
    check('closing the panel cancels pairing in the daemon', status().pairing === null && cancelled.some(e => e.payload.reason === 'cancelled on the Mac'), { pairing: status().pairing, cancelled: cancelled.map(e => e.payload.reason) });

    // ---------------------------------------------------------------- 7. watch only, rename, revoke
    const menu = await contextMenu("Bilal's iPhone");
    check('7. the row\'s menu offers Make Watch Only, Rename… and Revoke Device…', ['Make Watch Only', 'Rename…', 'Revoke Device…'].every(e => menu.includes(e)) && !menu.includes('Give Full Control'), menu);
    await contextMenu("Bilal's iPhone", 'Make Watch Only');
    list = await waitRows(p => p.rows.some(r => r.label === "Bilal's iPhone" && /Watch only/.test(r.description)));
    const watch = list.rows.find(r => r.label === "Bilal's iPhone");
    const refusal = await iphone.ask('run.interrupt', { run_id: 'none' }, 'scenario-watch-only-1');
    const readable = await iphone.ask('state', {});
    check('7. Make Watch Only: the row says Watch only, the daemon agrees, and the phone may watch but not control', watch?.description === 'connected · Watch only' && devices().find(d => d.name === "Bilal's iPhone")?.scope === 'watch' && refusal.error?.code === 'watch_only' && !!readable.result,
      { row: watch?.description, refusal: refusal.error, canRead: !!readable.result, actions: watch?.actions.map(a => a.name) });
    const watchMenu = await contextMenu("Bilal's iPhone");
    check('7. a watch-only phone offers Give Full Control instead', watchMenu.includes('Give Full Control') && !watchMenu.includes('Make Watch Only'), watchMenu);
    await s.screenshot('devices-watch-only');

    await contextMenu("Bilal's iPhone", 'Rename');
    await cdp.waitQuickTitle('Rename device');
    await cdp.key('a', { meta: true });
    await cdp.type('Work iPhone');
    await cdp.key('Enter');
    list = await waitRows(p => p.rows.some(r => r.label === 'Work iPhone'));
    check('7. Rename…: the list and the daemon show the new name', list.rows.some(r => r.label === 'Work iPhone') && !list.rows.some(r => r.label === "Bilal's iPhone") && devices().some(d => d.name === 'Work iPhone' && d.id === hello.device), list.rows.map(r => r.label));

    // ---------------------------------------------------------------- notifications (the Mac's side of AC-129)
    // The phone turns its own notifications on (its side of the switches).
    const own = await iphone.ask('device.notifications', { enabled: true });
    // The agent this window shows is not announced to a phone: the owner is looking at it.
    const watched = agent('Watched agent', 10);
    await s.selectRun(watched, { settle: 1200 });
    const looking = await logged(`looking at ${watched}`);
    const other = agent('Agent in the background', 1);
    const [aboutWatched, aboutOther] = [await told(watched, 'Work iPhone'), await told(other, 'Work iPhone')];
    await s.screenshot('looking-at-an-agent');
    check('the window tells the daemon which agent it shows, and no phone is notified about that agent', own.result?.enabled === true && looking && aboutWatched?.outcome === 'not_sent' && /looking at this agent on the Mac/.test(aboutWatched.why) && !payloads().some(p => p.payload.overseer.run_id === watched),
      { looking, aboutWatched });
    check('an agent the window does not show is announced to the phone', aboutOther?.outcome === 'sent' && payloads().some(p => p.payload.overseer.run_id === other && p.payload.overseer.kind === 'finished'), aboutOther);
    // Another agent is shown: the daemon is told, and the first one is no longer held back.
    await s.selectRun(other, { settle: 1200 });
    const moved = await logged(`looking at ${other}`);
    check('showing another agent tells the daemon so', moved, { moved });

    // What a phone does shows in the agent's chat with the phone's name; the daemon's record of
    // its notifications does not.
    const stopped = s.ctl('task.create', { repo, harness: 'generic', program: '/bin/sh', args: ['-c', 'echo started; sleep 60'], prompt: '', title: 'Agent stopped from a phone', workspace_mode: 'worktree' }).run.id;
    await s.selectRun(stopped, { settle: 1200 });
    const stop = await tablet.ask('run.interrupt', { run_id: stopped }, 'scenario-stop-from-phone-1');
    for (let t = 0; t < 15000 && !/interrupted|failed|completed/.test(s.ctl('state').runs.find(r => r.id === stopped)?.status || ''); t += 300) await delay(300);
    const chat = await s.editorView(`[...document.querySelectorAll('[data-audit-view="chat"] .sys')].some(e => /from Spare Phone/.test(e.textContent))`, 20000).then(f => f.eval(`({ sys: [...document.querySelectorAll('[data-audit-view="chat"] .sys')].map(e => e.textContent.trim()) })`), e => ({ error: e.message }));
    const commands = events('remote_command').filter(e => e.run_id === stopped).map(e => ({ source: e.source, method: e.payload.method }));
    await s.screenshot('stopped-from-a-phone');
    check('a command from a phone shows in the agent\'s chat with the phone\'s name, and the daemon records the phone as its source', !stop.error && (chat.sys || []).includes('Stopped from Spare Phone') && commands.some(c => c.source === 'phone:Spare Phone' && c.method === 'run.interrupt') && !(chat.sys || []).some(t => /push|remote/i.test(t)), { stop: stop.error || 'ok', chat, commands });

    // The Mac's switch for notifications to every phone.
    await cdp.command('Overseer: Turn Off Notifications to Phones');
    const quietOff = await quietMessage('/Notifications to phones are off/');
    const notifyOff = status().settings.notifications;
    const silent = agent('Agent while notifications are off', 1);
    const aboutSilent = await told(silent, 'Work iPhone');
    const barOff = await bar();
    await cdp.click(barOff.x, barOff.y);
    await cdp.waitQuickTitle('Phone access: on');
    const menuNotify = (await cdp.quickInputState()).rows;
    await s.screenshot('notifications-off-menu');
    await cdp.key('Escape'); await delay(400);
    await cdp.command('Overseer: Turn On Notifications to Phones');
    await quietMessage('/Notifications to phones are on/');
    check('notifications to every phone are switched on the Mac: off in the daemon, offered as Turn On, then on again', notifyOff === false && !!quietOff && menuNotify.some(r => /Turn On Notifications to Phones/.test(r)) && !menuNotify.some(r => /Turn Off Notifications to Phones/.test(r)) && status().settings.notifications === true, { notifyOff, menuNotify, after: status().settings.notifications });
    check('with the Mac\'s switch off nothing is sent: the daemon\'s record says why, and no payload was written', aboutSilent?.outcome === 'not_sent' && /off on the Mac/.test(aboutSilent.why) && !payloads().some(p => p.payload.overseer.run_id === silent), aboutSilent);
    const loud = agent('Agent after notifications are on again', 1);
    const aboutLoud = await told(loud, 'Work iPhone');
    check('on again, the phone is told again', aboutLoud?.outcome === 'sent' && payloads().some(p => p.payload.overseer.run_id === loud), aboutLoud);
    result.pushes = events('push').map(e => ({ run: e.run_id, ...e.payload }));

    // Turning phone access off with phones connected asks first; paired phones come back by themselves.
    await cdp.command('Overseer: Turn Off Phone Access');
    const sure = await waitDialog('/Turn off phone access\\?/');
    await s.screenshot('turn-off-asks');
    await press('Turn Off');
    const ended = [await iphone.endsWithin(3000), await tablet.endsWithin(3000)];
    const offAgain = await barSays(/Phone access off/);
    check('turning phone access off with phones connected asks first and names them; they are told, then disconnected', /2 phones will be disconnected: /.test(sure.detail) && /Work iPhone/.test(sure.detail) && /Spare Phone/.test(sure.detail) && ended.every(Boolean) && iphone.notices().includes('off') && tablet.notices().includes('off') && offAgain?.text === 'Phone access off' && await refused(port),
      { detail: sure.detail, ended, notices: [iphone.notices(), tablet.notices()], bar: offAgain?.text });
    await cdp.command('Overseer: Turn On Phone Access');
    await barSays(/Phone access on/);
    const again = await iphone.session().catch(e => ({ error: e.message }));
    iphone.keepAlive(8000);
    const back = await barSays(/^1 phone$/);
    check('on again: the paired phone connects by itself, with no pairing and no question', again.device === hello.device && again.scope === 'watch' && back?.text === '1 phone' && !(await dialogState()), { again, bar: back?.text });

    // The daemon restarts (the extension starts it again): phone access is still on, the phones
    // are still paired, and a phone connects again by itself. The status bar and the list rebuild.
    s.ctl('daemon.shutdown');
    const dropped = await iphone.endsWithin(5000);
    let restarted = false;
    for (let t = 0; t < 30000 && !restarted; t += 500) { await delay(500); try { restarted = events('daemon_started').length >= 2 && status().enabled === true; } catch { /* starting */ } }
    const afterRestart = await barSays(/^Phone access on$/, 20000);
    list = await waitRows(p => p.rows.some(r => r.label === 'Work iPhone' && /^last seen/.test(r.description)), 20000);
    const resting = list.rows.filter(r => r.level === 1).map(r => [r.label, r.description]);
    const helloAgain = await iphone.session().catch(e => ({ error: e.message }));
    iphone.keepAlive(8000);
    const afterReconnect = await barSays(/^1 phone$/);
    check('after the daemon restarts: phone access is on again at the same port, the list shows both phones as last seen, and a phone connects by itself', dropped && restarted && status().port === port && afterRestart?.text === 'Phone access on' && resting.length === 2 && resting.every(r => /^last seen just now · /.test(r[1])) && helloAgain.device === hello.device && afterReconnect?.text === '1 phone',
      { dropped, restarted, bar: [afterRestart?.text, afterReconnect?.text], resting, helloAgain });
    await s.screenshot('after-daemon-restart');
    // The window reloads (the extension starts again): the same picture, from the daemon.
    await cdp.command('Developer: Reload Window'); await delay(7000);
    await cdp.waitFor(`[...document.querySelectorAll('.statusbar-item')].some(e => /Overseer \\d+ active/.test(e.textContent))`, 60000, 'status bar after reload');
    const afterReload = await barSays(/^1 phone$/, 20000);
    await s.openOverseerView();
    list = await waitRows(p => p.rows.some(r => r.label === 'Work iPhone' && /^connected/.test(r.description)), 20000);
    check('after the window reloads: the status bar and the Devices view say the same again', afterReload?.text === '1 phone' && list.rows.some(r => r.label === 'Work iPhone' && r.description === 'connected · Watch only') && list.rows.some(r => r.label === 'Spare Phone' && /^last seen .* · Full control$/.test(r.description)) && !(await dialogState()),
      { bar: afterReload?.text, rows: list.rows.filter(r => r.level === 1).map(r => [r.label, r.description]) });
    await s.screenshot('after-window-reload');

    // Revoke asks, names the device, ends its session, and the phone leaves the list.
    await cdp.command('Overseer: Show Devices');
    await waitRows(p => p.rows.some(r => r.label === 'Work iPhone' && /^connected/.test(r.description)));
    await contextMenu('Work iPhone', 'Revoke Device');
    const revoke = await waitDialog('/Revoke "Work iPhone"\\?/');
    await s.screenshot('revoke-asks');
    await press('Revoke');
    const gone = await iphone.endsWithin(3000);
    const noticed = iphone.notices().includes('revoked');
    list = await waitRows(p => !p.rows.some(r => r.label === 'Work iPhone'));
    // Its key never serves again. The daemon may leave the handshake unanswered, or answer it only
    // to say "revoked" and close: either way the session ends and nothing can be requested.
    const answered = await iphone.session({ wait: 4000 }).then(() => true, () => false);
    const serves = answered ? await iphone.serves(3000) : false;
    const endedAgain = await iphone.endsWithin(4000);
    check('7. Revoke… asks and names the device; its session ends, it leaves the list, and its key gets no session again', /^Revoke "Work iPhone"\?$/.test(revoke.message.trim()) && revoke.buttons.includes('Revoke') && gone && noticed && !list.rows.some(r => r.label === 'Work iPhone') && !serves && endedAgain && devices().length === 1,
      { revoke: revoke.detail, gone, noticed, afterRevoke: { handshake: answered ? 'answered' : 'no answer', serves, ended: endedAgain, notices: iphone.notices() }, rows: list.rows.map(r => r.label) });
    // The other phone, from the command palette.
    await cdp.command('Overseer: Revoke Device');
    await cdp.pick('Revoke device', 'Spare Phone');
    await waitDialog('/Revoke "Spare Phone"\\?/');
    await press('Revoke');
    list = await waitRows(p => p.rows.length === 0 && !!p.welcome);
    const none = await barSays(/^Phone access on$/);
    check('7. with every phone revoked the list is empty: one sentence and Pair a Phone', !!list?.welcome && /^No phone is paired\. Pair one to watch and answer your agents from it\./.test(list.welcome.text) && list.welcome.buttons.includes('Pair a Phone') && devices().length === 0, list?.welcome);
    check('7. the status bar says phone access is on and no phone is connected', none?.text === 'Phone access on' && /no phone connected/.test(none.aria || ''), none);
    await s.screenshot('devices-empty');

    // The view and the status bar rebuild from the daemon: a change made outside VS Code shows without a refresh.
    const viaCtl = s.ctl('gateway.pair_start');
    const outside = phone('Paired Outside', 'android');
    const outsidePairing = outside.pair(viaCtl.code);
    outsidePairing.catch(() => {});
    await waitDialog('/Pair "Paired Outside"\\?/');
    s.ctl('gateway.pair_confirm', { request: status().pairing.waiting[0].request, accept: true });
    await outsidePairing.catch(() => {});
    outside.keepAlive(8000);
    await press("Don't Pair");
    list = await waitRows(p => p.rows.some(r => r.label === 'Paired Outside'));
    const outsideBar = await barSays(/^1 phone$/);
    s.ctl('gateway.device_revoke', { id: devices()[0].id });
    list = await waitRows(p => p.rows.length === 0);
    check('a phone paired and revoked outside VS Code appears in, and leaves, the list and the status bar without a refresh', outsideBar?.text === '1 phone' && list.rows.length === 0 && (await barSays(/^Phone access on$/))?.text === 'Phone access on', { bar: outsideBar?.text });

    // ---------------------------------------------------------------- 8. turn it off
    await cdp.command('Overseer: Turn Off Phone Access');
    const last = await barSays(/Phone access off/);
    const st9 = status();
    check('8. Turn Off Phone Access (no phone connected: no question): the status bar says off and the port refuses', last?.text === 'Phone access off' && !(await dialogState()) && st9.enabled === false && st9.port === null && await refused(port), { bar: last?.text, enabled: st9.enabled });
    await s.screenshot('status-bar-off-again');
    const states = events('gateway_state').map(e => e.payload.state);
    // On and off by the owner, on again by the owner, on again when the daemon restarted, off by the owner.
    check('every change was the daemon\'s: on, off, on, on (the daemon restarted), off', JSON.stringify(states) === JSON.stringify(['on', 'off', 'on', 'on', 'off']), states);
    const shown = [...(await toasts()), ...result.checks.flatMap(c => [c.detail?.text, c.detail?.over]).filter(Boolean)];
    check('nothing on screen says "gateway"', !shown.some(t => /gateway/i.test(t)) && !/gateway/i.test(JSON.stringify([menuOff, menuOn, menuNotify, first.steps, question, sure, revoke])), shown.filter(t => /gateway/i.test(t)));
  } catch (error) {
    s.note('ERROR ' + (error.stack || error.message)); result.error = error.message;
    try { await s.screenshot('error'); } catch {}
  } finally {
    for (const p of phones) p.close();
    s.writeLog();
    fs.writeFileSync(path.join(s.evidence, 'result.json'), JSON.stringify(result, null, 2));
    if (!process.env.KEEP_OPEN) { await s.quit(); s.stopDaemon(); }
    const failed = result.error || result.checks.some(c => !c.ok);
    console.log(failed ? 'SCENARIO FAILED' : 'SCENARIO PASSED', s.root);
    console.log(`${result.checks.filter(c => c.ok).length} of ${result.checks.length} checks passed`);
    process.exit(failed ? 1 : 0);
  }
})();
