// Helpers for scenario-overseer-window.js (AC-264): the owner's other window, attaching to a window
// by its title (a reopened window is a new page), and the readers the screenshots are checked with.
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { delay } = require('./harness');
const { Cdp } = require('./cdp');

const norm = text => { const o = JSON.parse(text); if (o['extensions.autoUpdate'] === false) o['extensions.autoUpdate'] = 'off'; return JSON.stringify(o); };

/** A CDP connection on the workbench page whose title matches (each window is its own page). */
async function attach(s, match, timeout = 60000, skip = new Set()) {
  const probe = await Cdp.connect(s.profile);
  let page;
  const end = Date.now() + timeout;
  while (Date.now() < end && !page) {
    const { targetInfos } = await probe.call('Target.getTargets');
    page = targetInfos.find(t => t.type === 'page' && t.url.includes('workbench') && !skip.has(t.targetId) && match(t.title || ''));
    if (!page) await delay(250);
  }
  if (!page) { const { targetInfos } = await probe.call('Target.getTargets'); probe.close(); throw new Error('no window titled like that: ' + JSON.stringify(targetInfos.filter(t => t.type === 'page').map(t => t.title))); }
  const { sessionId } = await probe.call('Target.attachToTarget', { targetId: page.targetId, flatten: true });
  probe.workbench = sessionId;
  await probe.call('Runtime.enable', {}, sessionId); await probe.call('Page.enable', {}, sessionId);
  await probe.call('Emulation.setFocusEmulationEnabled', { enabled: true }, sessionId).catch(() => {});
  await probe.waitFor(`!!document.querySelector('.monaco-workbench .part.activitybar')`, 60000, 'workbench of ' + page.title);
  probe.title = page.title;
  return probe;
}

/**
 * The owner's other window: File: New Window from the first, then File: Open Folder... there (VS
 * Code's in-window picker). Opened from inside a test window it runs without the test's extensions,
 * which suits it: it stands for any other VS Code window of the owner's.
 */
async function secondWindow(s, main, folder) {
  const { targetInfos: before } = await main.call('Target.getTargets');
  const known = new Set(before.filter(t => t.type === 'page').map(t => t.targetId));
  await main.focusWorkbench();
  await main.key('n', { meta: true, shift: true });
  let c = await attach(s, () => true, 1, known).catch(() => null);
  for (let i = 0; i < 80 && !c; i++) { await delay(250); c = await attach(s, () => true, 1, known).catch(() => null); }
  if (!c) throw new Error('the second window did not open');
  await delay(1500);
  await c.command('File: Open Folder...');
  await c.waitFor(`(() => { const i = document.querySelector('.quick-input-widget input'); return !!i && i === document.activeElement; })()`, 10000, 'folder picker');
  await c.key('a', { meta: true });
  await c.type(folder + '/');
  await delay(800);
  await c.key('Enter');
  c.close();
  await delay(2500);
  return attach(s, t => t.includes(path.basename(folder)), 60000);
}

const layout = c => c.evalWorkbench(`(() => {
  const vis = sel => { const e = document.querySelector(sel); return !!e && e.offsetWidth > 0 && e.offsetHeight > 0 && getComputedStyle(e).display !== 'none' && !e.classList.contains('hidden'); };
  const shown = e => !!e && e.offsetWidth > 0 && e.offsetHeight > 0 && getComputedStyle(e).display !== 'none';
  const groups = [...document.querySelectorAll('.part.editor .editor-group-container')].filter(g => g.offsetParent);
  const aux = document.querySelector('.part.auxiliarybar');
  return { title: document.title, sidebar: vis('.part.sidebar'), sidebarTitle: document.querySelector('.part.sidebar .title-label')?.textContent.trim() || '', panel: vis('.part.panel'),
    auxiliary: vis('.part.auxiliarybar'), auxiliaryWidth: aux && vis('.part.auxiliarybar') ? Math.round(aux.getBoundingClientRect().width) : 0, window: window.innerWidth,
    tabStrips: groups.filter(g => shown(g.querySelector('.tabs-and-actions-container, .tabs-container'))).length,
    breadcrumbs: [...document.querySelectorAll('.part.editor .breadcrumbs-control')].filter(shown).length,
    groups: groups.map(g => ({ width: Math.round(g.getBoundingClientRect().width), share: Math.round(g.getBoundingClientRect().width / groups.reduce((n, x) => n + x.getBoundingClientRect().width, 0) * 100) / 100, tabs: [...g.querySelectorAll('.tab')].map(t => (t.getAttribute('aria-label') || '').split(',')[0]), active: (g.querySelector('.tab.active')?.getAttribute('aria-label') || '').split(',')[0] })) };
})()`);

async function openFile(c, name) {
  await c.focusWorkbench();
  await c.key('p', { meta: true });
  await c.waitFor(`(() => { const i = document.querySelector('.quick-input-widget input'); return !!i && i === document.activeElement; })()`, 5000, 'quick open');
  await c.type(name);
  await c.waitFor(`[...document.querySelectorAll('.quick-input-widget .monaco-list-row')].some(r => (r.getAttribute('aria-label') || '').startsWith(${JSON.stringify(name)}))`, 8000, 'quick open ' + name);
  await c.key('Enter'); await delay(600);
}

/** The rows of File: Open Recent (VS Code's recent list), then closed again. */
async function recent(c) {
  await c.command('File: Open Recent...');
  await delay(800);
  const rows = await c.evalWorkbench(`[...document.querySelectorAll('.quick-input-widget .monaco-list-row')].map(r => r.getAttribute('aria-label') || r.textContent).slice(0, 10)`);
  await c.key('Escape'); await delay(300);
  return rows;
}

async function size(c, width, height) {
  await c.call('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 0, mobile: false }, c.workbench);
  await delay(1200);
}

async function shot(s, c, label) {
  const file = path.join(s.evidence, `${String(++s.shot).padStart(2, '0')}-${label}.png`);
  await c.screenshot(file);
  s.note('screenshot ' + file);
  return file;
}

/** Two windows' screenshots side by side (ImageMagick), as the owner would see them on one screen. */
function sideBySide(s, left, right, label) {
  const file = path.join(s.evidence, `${String(++s.shot).padStart(2, '0')}-${label}.png`);
  const r = cp.spawnSync('magick', [left, right, '-resize', 'x1080', '-bordercolor', '#000000', '-border', '6', '+append', file], { encoding: 'utf8' });
  if (r.status !== 0) s.note('side by side: ' + (r.stderr || r.error?.message));
  else s.note('screenshot ' + file);
  return file;
}

/** The Overseer view (home, Voice Mode or an agent's chat): which it shows. */
async function overseerView(c) {
  const f = await c.webview(`document.body.dataset.ready === '1' && !!document.querySelector('.view-composer') && !!window.__overseer`, 20000);
  return f.eval(`({ mode: document.body.dataset.mode, selected: window.__overseer.selected(), title: document.querySelector('.view-chat #title')?.textContent || '', said: [...document.querySelectorAll('#home-conv .home-msg.from-owner')].map(m => m.textContent.trim()), voice: document.getElementById('voice-state')?.checkVisibility() ? document.getElementById('voice-state').dataset.state : '', back: !!document.querySelector('#back-to-overseer') && !document.querySelector('#back-to-overseer').hidden, width: window.innerWidth })`);
}

/** The extension's own log lines about the layout (its output channel's file in the profile's logs). */
function extensionLog(s, pattern = /one layout/) {
  const out = [];
  const logs = path.join(s.profile, 'logs');
  const walk = d => { for (const e of fs.readdirSync(d, { withFileTypes: true })) { const p = path.join(d, e.name); if (e.isDirectory()) walk(p); else if (e.name === 'Overseer.log') out.push(...fs.readFileSync(p, 'utf8').split('\n').filter(l => pattern.test(l))); } };
  try { walk(logs); } catch {}
  return out;
}

/** Before a screenshot: the review shows its files and home's composer has its choices (repository, agent...). */
async function settled(c, file) {
  await c.webview(`!!document.getElementById('diffs') && document.getElementById('diffs').innerText.includes(${JSON.stringify(file)})`, 20000).catch(() => null);
  await c.webview(`!!document.querySelector('.view-composer') && [...document.querySelectorAll('.view-composer button.chip')].some(b => /\\w/.test(b.textContent))`, 15000).catch(() => null);
  await delay(800);
}

/** The window on the owner's repository, once it is (or is no longer) the Overseer window: a reopened window is a new page. */
async function reopened(s, overseer, timeout = 90000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    const c = await attach(s, t => /ws-repo/.test(t) && !/notes-repo/.test(t), 5000).catch(() => null);
    if (c) {
      const file = await c.evalWorkbench(`globalThis.vscode?.context?.configuration?.()?.workspace?.configPath?.path || ''`).catch(() => null);
      if (file !== null && /\/layouts\//.test(file) === overseer) return c;
      c.close();
    }
    await delay(500);
  }
  throw new Error(`the window did not reopen ${overseer ? 'as the Overseer window' : 'on the folder'}`);
}

module.exports = { reopened, settled, extensionLog, secondWindow, norm, attach, layout, openFile, recent, size, shot, sideBySide, overseerView };
