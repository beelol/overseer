// AC-245: no internal words on any surface. Every packaged-UI scenario reads the owner-facing text
// of its window at each screenshot (the harness calls `collect`): the side bar's rows, the status
// bar, toasts, dialogs, quick picks and hovers, and in every Overseer webview the text and the
// tooltips and accessible names of its controls. What an agent or the owner wrote (messages,
// Markdown, code, diffs, typed input) is their content and is left out. Any listed pattern fails
// the scenario (the harness sets its exit code) and is written to its evidence as plain-words.json.

/** The words and shapes that must never reach the owner, each with why. */
const PATTERNS = [
  ['an internal id', /(?<![\w/.-])(?:r|p|sh|w)-[0-9a-f]{8,}\b/],
  ['a snake_case state', /\b(?:waiting_for_(?:user|connection|memory)|not_for_overseer|already_answered|merge_back|pull_request|ask_first|every_turn|rate_limit|tool_use|tool_result|turn_done|picked_up|not_sent|partly_sent|handed_off|cancel_requested|answered_by|workspace_id|run_id)\b/],
  ['a tool name', /\bmcp__[A-Za-z0-9_]+/],
  ['a lowercase harness id', /(?<![\w./@~:-])(?:claude|codex|opencode)(?:-app|-serve)?(?![\w./@-])/],
  ['a confidence tag', /\btool-input\b|\((?:exact|inferred)[\w-]*\)/],
  ['who answered and where', /\b(?:owner|overseer|agent|ctl)\s·\s(?:vscode|voice|phone|tui|ctl|settle|mcp)\b/],
  ['a raw error', /\bos error \d+|Connection refused|ECONNREFUSED|ENOENT|\bfatal: |\bHTTP [1-5]\d\d\b|\[rate_limit\]|API Error|panicked at|Caused by:|turn reported failure|error sending request|\bstack backtrace\b/],
];

/** Which patterns a text shows, with the matched words. */
function leaks(text) {
  const out = [];
  for (const [why, re] of PATTERNS) { const m = re.exec(String(text || '')); if (m) out.push({ why, match: m[0] }); }
  return out;
}

// Content, not chrome: what agents and the owner wrote, code, diffs and typed input.
const CONTENT = ['.msg', '.md', 'pre', 'code', '.codeblock', '.home-text', '.home-voice-heard', '.card-text', '.card-detail', '.proposal-list', '.card-row-agent', '.tile-body', '.tile-perm-text', '.perm-card', '.tool', '.tool-detail', '.cont-fold', '#log', '#rawout', '.raw-out', '.details-panel',
  '.diff', '.hunk', '.file-list', '.files', '.review-body', '.monaco-editor', '.xterm', '.terminal', 'input', 'textarea', '[contenteditable]', '.chat-title', '.tile-title', '.agent-mention', '.repo-picker', '.req-stage', '.home-progress', '#voice-heard', '.voice-said', '.voice-words', '.transcript', '[data-content]'].join(', ');

/** The owner-facing text of an Overseer webview (evaluated inside the frame). */
const WEBVIEW = `(() => {
  const skip = e => !!e.closest(${JSON.stringify(CONTENT)}) || !!e.closest('[hidden], [aria-hidden="true"]');
  const out = [];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n; (n = walker.nextNode());) { const p = n.parentElement; const t = n.textContent.trim(); if (t && p && !skip(p) && p.checkVisibility()) out.push(t); }
  for (const e of document.querySelectorAll('[title], [aria-label], [placeholder]')) { if (e.closest(${JSON.stringify(CONTENT)}) && !e.matches('input, textarea')) continue; if (e.closest('[hidden]')) continue; for (const a of ['title', 'aria-label', 'placeholder']) { const v = e.getAttribute(a); if (v) out.push(v); } }
  return out;
})()`;

/** The owner-facing text of the workbench's Overseer parts (evaluated in the workbench). */
const WORKBENCH = `(() => {
  const out = [];
  const pane = [...document.querySelectorAll('.pane')].filter(p => /^(Agents|Accounts|Devices)/.test(p.querySelector('.pane-header')?.textContent.trim() || ''));
  for (const p of pane) {
    out.push(p.querySelector('.pane-header')?.innerText || '');
    for (const r of p.querySelectorAll('.monaco-list-row')) { if (!r.offsetParent) continue; out.push(r.querySelector('.label-name')?.textContent || '', r.querySelector('.label-description')?.textContent || '', r.getAttribute('aria-label') || ''); }
  }
  for (const e of document.querySelectorAll('.statusbar-item')) { const t = (e.getAttribute('aria-label') || e.textContent || ''); if (/Overseer|agent|Voice|Phone/i.test(t)) out.push(t); }
  for (const e of document.querySelectorAll('.notifications-toasts .notification-list-item, .notifications-center .notification-list-item')) if (/Overseer|agent|merge|Merge/i.test(e.innerText)) out.push(e.innerText);
  for (const e of document.querySelectorAll('.monaco-dialog-box')) out.push(e.innerText);
  const qi = document.querySelector('.quick-input-widget');
  if (qi && qi.offsetParent && /Overseer|agent|Agent/.test(qi.innerText)) for (const e of qi.querySelectorAll('.quick-input-title, .monaco-list-row')) out.push(e.innerText);
  for (const e of document.querySelectorAll('.activitybar .action-label')) { const t = e.getAttribute('aria-label') || ''; if (/Overseer/.test(t)) out.push(t); }
  return out.filter(Boolean);
})()`;

/**
 * Reads the window's owner-facing text now; returns [{ where, text, leaks }] for texts that leak.
 * `owned` are words the owner or an agent wrote (agent titles, prompts): taken out before the check.
 */
async function collect(cdp, owned = []) {
  const found = [];
  const mine = [...new Set(owned.filter(w => w && w.length > 2))].sort((a, b) => b.length - a.length);
  const strip = t => mine.reduce((x, w) => x.split(w).join('«»'), String(t));
  const add = (where, texts) => { for (const t of texts || []) { const l = leaks(strip(t)); if (l.length) found.push({ where, text: String(t).slice(0, 300), leaks: l }); } };
  try { add('workbench', await cdp.evalWorkbench(WORKBENCH)); } catch { /* the window is going */ }
  try {
    const frames = await cdp.webviews(`!document.querySelector('.monaco-workbench') && !!document.querySelector('link[href*="tokens.css"], #diffs')`);
    for (const f of frames) { try { add(`webview ${await f.eval('document.title')}`, await f.eval(WEBVIEW)); } catch { /* a frame that went away */ } }
  } catch { /* no webviews */ }
  return found;
}

module.exports = { PATTERNS, leaks, collect, WEBVIEW, WORKBENCH, CONTENT };
