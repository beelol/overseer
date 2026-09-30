// Modified for Overseer from Branch Diff (Local) review/browser.js (MIT): adds the
// comparison/base control, Follow (off/following/paused), agent-edit reveal, restores
// the saved scroll anchor once the rows above it have rendered (after reload/restart), and
// per-hunk Accept (mark reviewed) / Reject (restore the base text through the native edit path),
// and (AC-99) a file navigator over the whole worktree. Overseer (AC-264) has two views: Follow shows
// the file the agent is in, live, read-only with its changes marked, and its list is All files (a
// file picked there shows in the same place); Diffs only shows the diffs, and its list is Changed.
import './browser.css';
import { StatisticsWorker } from './statistics-client';
import { EditingClient, replaceText } from './editing-client';
// A harness by name in tooltips (AC-245), never its lowercase id.
const HARNESS_NAME = { claude: 'Claude Code', codex: 'Codex', 'codex-app': 'Codex', opencode: 'OpenCode', 'opencode-serve': 'Local model', generic: 'Program' };

const vscode = acquireVsCodeApi();
const saved = vscode.getState() || {};
const identity = { repository: document.body.dataset.repository, mode: document.body.dataset.mode, target: document.body.dataset.target, runId: document.body.dataset.runId };
// Save repository identity even if the first comparison has not finished yet.
vscode.setState({ ...saved, ...identity });
const diffs = document.getElementById('diffs');
const tree = document.getElementById('tree');
const total = document.getElementById('total');
const notice = document.getElementById('notice');
const layout = document.getElementById('layout');
const filter = document.getElementById('filter');
const rows = new Map();
const closedFiles = new Set(saved.closedFiles || []);
const closedFolders = new Set(saved.closedFolders || []);
let monaco, monacoLoading, disposeMonaco;
const statistics = new StatisticsWorker(__BRANCH_DIFF_STATISTICS_SOURCE__);
const queued = new Set(), requests = new Map(), manual = new Set();
let requestId = 0, clicked, classifying = false, renderFrame, stopped = false;
let snapshot, settings = {}, selected = saved.selected, initialized = false, restoring = true;
let rendering = false, nextSnapshot, pendingJump, hierarchy = saved.hierarchy;
let frame, resizeFrame, persistTimer, progressTimer, pendingState;
// AC-264: 'follow' (the agent's file, All files) or 'diffs' (the diffs, Changed); the host decides.
let view = document.body.dataset.view === 'follow' ? 'follow' : 'diffs';
// Follow's file (AC-264): its path, who put it there ('agent' or 'user') and the newest message's number.
let viewChosenAt = 0;
let followPath = '', followSource = 'agent', followSeq = 0, followEditor, followModel, followDecorations, followFlash;
const openDirs = new Set(saved.openDirs || []);
const dirCache = new Map(); // folder path -> { loading } | { entries } | { error }
let pendingBrowse;
const listTitle = document.getElementById('list-title');
const editing = new EditingClient({ vscode, saved, repository: identity.repository, snapshot: () => snapshot, settings: () => settings, notice: stickyMessage, changed: row => { if (!rows.has(row.entry.id)) release(row); else { if (row.largeWhileEditing && !row.editor?.getModifiedEditor().hasTextFocus()) { row.largeWhileEditing = false; row.renderedRevision = undefined; } ensure(row); updateViewport(); } } });
filter.value = saved.filter || '';
layout.value = saved.layout === 'split' ? 'split' : 'unified';
let navWidth = saved.navWidth || 260;
const setNavWidth = value => {
  navWidth = Math.max(160, Math.min(Math.max(160, innerWidth * 0.55), value));
  document.documentElement.style.setProperty('--nav-width', navWidth + 'px');
  document.getElementById('resize').setAttribute('aria-valuenow', Math.round(navWidth));
};
setNavWidth(navWidth);
const toggleNavigator = document.getElementById('toggle-navigator');
function collapseNavigator(value) {
  document.body.classList.toggle('nav-collapsed', value);
  toggleNavigator.setAttribute('aria-expanded', String(!value));
}
collapseNavigator(!!saved.navCollapsed);
toggleNavigator.addEventListener('click', () => { collapseNavigator(!document.body.classList.contains('nav-collapsed')); persist(); });
function persist() {
  clearTimeout(persistTimer);
  if (!snapshot) return;
  if (document.visibilityState === 'hidden') return;
  // Overseer (AC-264): Follow hides the diffs; the list's state is kept and the diffs' position left as it was.
  if (view === 'follow') {
    pendingState = { ...(pendingState || saved), ...identity, navWidth, navCollapsed: document.body.classList.contains('nav-collapsed'), filter: filter.value, openDirs: [...openDirs] };
    persistTimer = setTimeout(flushState, 100);
    return;
  }
  // Overseer: a hidden or zero-height view clamps scrollTop to 0; keep the last real position.
  if (!diffs.clientHeight) return;
  pendingState = { ...identity, layout: layout.value, closedFiles: [...closedFiles], closedFolders: [...closedFolders],
    navWidth, navCollapsed: document.body.classList.contains('nav-collapsed'), filter: filter.value,
    selected, scrollTop: diffs.scrollTop, anchor: restoreGoal ? { id: restoreGoal.id, offset: restoreGoal.offset } : anchor(), hierarchy,
    openDirs: [...openDirs] };
  persistTimer = setTimeout(flushState, 100);
}
function flushState() {
  clearTimeout(persistTimer);
  if (pendingState) vscode.setState({ ...pendingState, drafts: editing.state() });
}
function node(tag, className, text) {
  const element = document.createElement(tag);
  if (className) element.className = className;
  if (text !== undefined) element.textContent = text;
  return element;
}
function message(text) { notice.textContent = text || ''; notice.hidden = !text; }
// Overseer: notices the user must see (conflicts, refused actions) outlive the next live refresh.
let stickyNotice;
function stickyMessage(text) { stickyNotice = text ? { text, until: Date.now() + 10000 } : undefined; message(text); }
function anchor() {
  const element = [...diffs.children].find(el => el.offsetTop + el.offsetHeight > diffs.scrollTop);
  const row = element && rows.get(element.dataset.id);
  return row && { id: row.entry.id, offset: diffs.scrollTop - row.element.offsetTop };
}
// Overseer: rows start as short placeholders after a reload and later snapshots can release
// rendered rows, so the saved anchor is re-applied as diffs render and relayout until the user
// interacts with the review (or jumps/follows). Until then it is also the position we save.
let restoreGoal = saved.runId === identity.runId && saved.anchor && typeof saved.anchor.id === 'string' ? { id: saved.anchor.id, offset: Number(saved.anchor.offset) || 0 } : undefined;
document.body.dataset.restore = restoreGoal ? 'pending' : 'none';
function pursueRestore() {
  if (!restoreGoal) return;
  const row = rows.get(restoreGoal.id);
  if (!row) return;
  const want = row.element.offsetTop + restoreGoal.offset;
  if (diffs.scrollTop !== want) diffs.scrollTop = want;
  if (Math.abs(diffs.scrollTop - want) < 2 && !snapshot?.cached && row.element.dataset.loadState === 'rendered') document.body.dataset.restore = 'reached';
}
function stopRestore(reason) { if (restoreGoal) { restoreGoal = undefined; document.body.dataset.restore = 'cancelled:' + reason; } }
for (const type of ['wheel', 'touchstart', 'keydown', 'pointerdown']) document.addEventListener(type, () => stopRestore(type), { passive: true, capture: true });
function restoreAnchor(value) {
  const row = value && rows.get(value.id);
  if (row) diffs.scrollTop = row.element.offsetTop + value.offset;
}
// Overseer: live refreshes re-select the file at the diff anchor. Never scroll the navigator
// under a user who is pointing at or scrolling it (items would move under the cursor).
let navigatorTouched = 0;
function navigatorBusy() { return Date.now() - navigatorTouched < 4000; }
function select(id) {
  if (!id || selected === id) return;
  selected = id;
  for (const button of tree.querySelectorAll('[data-id]')) {
    const active = button.dataset.id === id;
    button.classList.toggle('active', active);
    button.setAttribute('aria-selected', String(active));
    if (active && !navigatorBusy()) button.scrollIntoView({ block: 'nearest' });
  }
  persist();
}
function trackScroll() {
  cancelAnimationFrame(frame);
  frame = requestAnimationFrame(() => { updateViewport(); const current = anchor(); if (current) select(current.id); persist(); });
}
diffs.addEventListener('scroll', trackScroll, { passive: true });
function jump(id) {
  stopRestore('jump');
  const row = rows.get(id); if (!row) { pendingJump = id; return; }
  clicked = id; closedFiles.delete(id); row.nearby = true; fold(row); ensure(row);
  diffs.scrollTop = row.element.offsetTop;
  // Overseer (AC-149): following an agent must not take keyboard focus from the chat; the header
  // takes focus only when the review already has it (the user is navigating in it).
  select(id); if (document.hasFocus()) row.header.focus({ preventScroll: true }); updateViewport(); persist();
}
function changedEntries() { return (snapshot?.entries || []).filter(e => !e.browsed); }
function changesOnly() { return view !== 'follow'; }
function countsText(id) {
  const data = id && rows.get(id)?.element.dataset;
  return data && data.additions !== undefined ? [data.additions, data.deletions] : undefined;
}
function fileButton(relPath, entry) {
  const changed = !!entry && !entry.browsed;
  const shown = view === 'follow' ? relPath === followPath : !!entry && entry.id === selected;
  const button = node('button', 'file' + (shown ? ' active' : '') + (changed ? ' changed' : ''));
  if (entry) button.dataset.id = entry.id;
  button.dataset.path = relPath;
  button.title = relPath + (entry?.unsaved ? ' (unsaved)' : '') + (entry?.conflicted ? ' (conflicted)' : '') + (entry?.readOnly ? ' (staged: read-only)' : '');
  button.setAttribute('role', 'treeitem'); button.setAttribute('aria-selected', String(shown));
  button.setAttribute('aria-label', [relPath, changed && entry.status, entry?.unsaved && 'unsaved', entry?.conflicted && 'conflicted'].filter(Boolean).join(', '));
  // Codicon per file, plus markers: conflicted (warning) and unsaved (filled dot).
  const icon = node('span', 'codicon codicon-' + (entry?.conflicted ? 'warning' : 'file')); icon.setAttribute('aria-hidden', 'true');
  button.append(icon, node('span', 'file-name', relPath.split('/').pop()));
  if (entry?.unsaved) { const m = node('span', 'marker codicon codicon-circle-filled'); m.title = 'Unsaved edits'; m.setAttribute('aria-hidden', 'true'); button.append(m); }
  if (changed) {
    const counts = node('span', 'counts'); const c = countsText(entry.id);
    if (c) counts.replaceChildren(node('span', 'additions', '+' + c[0]), node('span', 'deletions', '−' + c[1]));
    button.append(counts, node('span', 'status status-' + entry.status, entry.status));
  }
  if (entry?.conflicted) button.classList.add('conflicted');
  button.addEventListener('click', () => openPath(relPath));
  return button;
}
/**
 * Changed (Diffs only): a file jumps to its diff. All files is Follow's list (AC-264, the owner
 * 2026-09-30): every file of the worktree, each shown in the review's middle, in place of the agent's.
 */
function openPath(relPath) {
  if (view === 'follow') { markFollowed(relPath); vscode.postMessage({ type: 'showFile', path: relPath }); return; }
  const entry = snapshot?.entries.find(e => e.path === relPath);
  if (entry) { jump(entry.id); return; }
  pendingBrowse = relPath;
  vscode.postMessage({ type: 'browse', path: relPath });
}
function listDir(dirPath) {
  if (!dirCache.has(dirPath)) { dirCache.set(dirPath, { loading: true }); vscode.postMessage({ type: 'listDir', path: dirPath }); }
  return dirCache.get(dirPath);
}
function renderTree() {
  const only = changesOnly();
  // AC-264: the list follows the view: Changed in Diffs only, All files in Follow.
  listTitle.textContent = only ? 'Changed' : 'All files';
  listTitle.title = only ? 'The files the agent changed; each jumps to its diff.' : 'Every file in the agent\'s worktree; each shows here, in place of the agent\'s file.';
  document.body.dataset.nav = only ? 'changes' : 'all';
  const fragment = document.createDocumentFragment();
  if (only) populateChanges(fragment); else populateAll('', fragment);
  const navigator = document.getElementById('navigator');
  const scrollTop = navigator.scrollTop;
  tree.replaceChildren(fragment); navigator.scrollTop = scrollTop;
}
function populateChanges(fragment) {
  const root = { dirs: new Map(), files: [] };
  const query = filter.value.toLocaleLowerCase();
  for (const entry of changedEntries()) {
    if (!entry.path.toLocaleLowerCase().includes(query)) continue;
    const parts = entry.path.split('/'); parts.pop(); let dir = root;
    for (const part of parts) { if (!dir.dirs.has(part)) dir.dirs.set(part, { dirs: new Map(), files: [] }); dir = dir.dirs.get(part); }
    dir.files.push(entry);
  }
  const populate = (dir, parent, prefix) => {
    for (const [name, child] of dir.dirs) {
      const key = prefix + name + '/';
      const group = node('details', 'folder'); group.open = !closedFolders.has(key) || !!query;
      const summary = node('summary', '', name); summary.title = key; group.append(summary);
      const children = node('div', 'folder-children'); children.setAttribute('role', 'group'); group.append(children);
      group.addEventListener('toggle', () => { if (group.open) closedFolders.delete(key); else closedFolders.add(key); persist(); });
      populate(child, children, key); parent.append(group);
    }
    for (const entry of dir.files) parent.append(fileButton(entry.path, entry));
  };
  populate(root, fragment, '');
}
// All files: the worktree one folder at a time (listed when opened); changed files keep their
// status and counts, and folders holding changes are marked.
function populateAll(dirPath, parent) {
  const listing = listDir(dirPath);
  if (listing.loading) { parent.append(node('div', 'tree-note', 'Loading…')); return; }
  if (listing.error) { parent.append(node('div', 'tree-note', listing.error)); return; }
  const byPath = new Map((snapshot?.entries || []).map(e => [e.path, e]));
  const changedDirs = new Set();
  for (const e of changedEntries()) { const parts = e.path.split('/'); for (let i = 1; i < parts.length; i++) changedDirs.add(parts.slice(0, i).join('/')); }
  const query = filter.value.toLocaleLowerCase();
  const limit = listing.limit || 500;
  let shown = 0;
  for (const item of listing.entries) {
    const rel = dirPath ? dirPath + '/' + item.name : item.name;
    if (query && !item.dir && !item.name.toLocaleLowerCase().includes(query)) continue;
    if (shown++ >= limit) {
      const more = node('button', 'tree-more', `Show more (${listing.entries.length - limit} left)`);
      more.addEventListener('click', () => { listing.limit = limit + 500; renderTree(); });
      parent.append(more); break;
    }
    if (!item.dir) { parent.append(fileButton(rel, byPath.get(rel))); continue; }
    const group = node('details', 'folder'); group.open = openDirs.has(rel);
    const summary = node('summary', changedDirs.has(rel) ? 'has-changes' : '', item.name); summary.title = rel + '/';
    group.append(summary);
    const children = node('div', 'folder-children'); children.setAttribute('role', 'group'); group.append(children);
    group.addEventListener('toggle', () => {
      if (group.open) { openDirs.add(rel); if (!children.childElementCount) populateAll(rel, children); }
      else openDirs.delete(rel);
      persist();
    });
    if (group.open) populateAll(rel, children);
    parent.append(group);
  }
  if (!listing.entries.length && !dirPath) parent.append(node('div', 'tree-note', 'The worktree is empty.'));
}
filter.addEventListener('input', () => { renderTree(); persist(); });
for (const type of ['wheel', 'pointermove', 'pointerdown', 'keydown']) document.getElementById('navigator').addEventListener(type, () => { navigatorTouched = Date.now(); }, { passive: true });
function fold(row) {
  const closed = closedFiles.has(row.entry.id);
  if (row.element.classList.contains('collapsed') === closed) { if (!closed) ensure(row); return; }
  const position = row.element.classList.contains('collapsed') !== closed ? anchor() : undefined;
  if (position?.id === row.entry.id && closed) position.offset = 0;
  row.element.classList.toggle('collapsed', closed);
  row.toggle.setAttribute('aria-expanded', String(!closed));
  if (closed) release(row); else ensure(row);
  restoreAnchor(position);
}
function makeRow(entry) {
  const element = node('article', 'diff-file'); element.dataset.id = entry.id;
  const header = node('header', 'file-header'); header.tabIndex = -1;
  const toggle = node('button', 'fold'); toggle.setAttribute('aria-label', 'Collapse or expand ' + entry.path); toggle.title = 'Collapse or expand'; toggle.setAttribute('aria-expanded', 'true');
  const title = node('a', 'file-path', entry.path); title.setAttribute('role', 'link');
  const status = node('span', 'status'); const unsaved = node('span', 'unsaved');
  const stats = node('span', 'stats', '…');
  const open = node('button', 'open-native', '↗'); open.title = 'Open in native diff (undo, redo, Git gutters)'; open.setAttribute('aria-label', 'Open ' + entry.path + ' in native diff');
  // Overseer (AC-232): Save says what it does; it writes your edits, it does not keep or undo the agent's change.
  const save = node('button', 'save-file', "Save your changes to the agent's copy"); save.disabled = true; save.hidden = true;
  save.title = "Save your changes to the agent's copy (Cmd+S): writes your edits to this file in the agent's worktree. It does not keep or undo the agent's change.";
  // An unchanged file opened from the navigator can be closed again (AC-99).
  const close = node('button', 'close-file'); close.title = 'Close this file (it has no changes)'; close.setAttribute('aria-label', 'Close ' + entry.path);
  const closeIcon = node('span', 'codicon codicon-close'); closeIcon.setAttribute('aria-hidden', 'true'); close.append(closeIcon);
  close.addEventListener('click', () => vscode.postMessage({ type: 'unbrowse', path: row.entry.path }));
  const editStatus = node('span', 'edit-status'); editStatus.setAttribute('role', 'status');
  const host = node('div', 'diff-body'); host.style.height = '220px';
  header.append(toggle, status, title, unsaved, editStatus, stats, save, open, close); element.append(header, host);
  const progress = node('span', 'file-loading'); progress.setAttribute('role', 'status'); progress.hidden = true; header.insertBefore(progress, stats);
  const row = { entry, element, header, toggle, title, status, unsaved, stats, host, progress, open, save, close, editStatus, nearby: false };
  loading(row, true);
  save.addEventListener('click', () => editing.save(row));
  title.addEventListener('click', event => {
    event.preventDefault();
    if (!row.entry.pending && !snapshot.cached) vscode.postMessage({ type: 'openFile', id: row.entry.id, version: snapshot.version });
  });
  toggle.addEventListener('click', () => { if (closedFiles.has(entry.id)) closedFiles.delete(entry.id); else closedFiles.add(entry.id); fold(row); persist(); });
  open.addEventListener('click', () => { if (!row.entry.pending && !snapshot.cached) vscode.postMessage({ type: 'open', id: entry.id, version: snapshot.version }); });
  return row;
}
function loading(row, value) {
  row.element.setAttribute('aria-busy', String(value));
  row.progress.hidden = !value || !row.renderedRevision;
  row.progress.textContent = value ? 'Updating…' : '';
  if (value && !row.editor && !row.renderedRevision && !row.host.querySelector('.loading')) {
    const status = node('div', 'loading'); status.setAttribute('role', 'status');
    const spinner = node('span', 'spinner'); spinner.setAttribute('aria-hidden', 'true');
    status.append(spinner, node('span', '', 'Loading diff…')); row.host.replaceChildren(status);
  }
}
function release(row) {
  queued.delete(row);
  if (!row.editor || !editing.release(row)) return;
  row.viewState = row.editor.saveViewState();
  row.hunks = []; row.hunkDecorations = undefined;
  row.listeners.forEach(l => l.dispose());
  row.editor.dispose(); row.original.dispose(); row.modified.dispose();
  row.editor = undefined; row.body = undefined; row.renderedRevision = undefined; row.save.disabled = true;
  loading(row, true);
}
function relevant(row) { return !stopped && view === 'diffs' && rows.get(row.entry.id) === row && row.nearby && !closedFiles.has(row.entry.id); }
function reviewKey(value) { return JSON.stringify([value.repository, value.mode, value.target, value.description?.headName,
  value.description?.base, value.description?.mergeBase]); }
function valid(job) {
  // A newer snapshot may be waiting for the current DOM batch to yield.
  if (nextSnapshot && (reviewKey(nextSnapshot) !== job.review ||
      !nextSnapshot.entries.some(e => e.id === job.row.entry.id && !e.pending && e.revision === job.revision))) return false;
  return !snapshot.cached && !job.row.entry.pending && relevant(job.row) && job.revision === job.row.entry.revision && job.generation === job.row.generation && job.review === reviewKey(snapshot); }
function priority(row) {
  if (row.entry.id === clicked) return -1e9;
  const start = row.element.offsetTop, end = start + row.element.offsetHeight;
  const top = diffs.scrollTop, bottom = top + diffs.clientHeight;
  if (start <= bottom && end >= top) return Math.max(0, start - top);
  return 1e6 + Math.min(Math.abs(start - bottom), Math.abs(end - top));
}
function ensure(row) {
  if (editing.held(row) || !initialized || !snapshot || snapshot.cached || row.entry.pending || !row.entry.revision || !relevant(row)) return;
  if (row.renderedRevision === row.entry.revision || [...requests.values()].some(job => job.row === row && job.revision === row.entry.revision && job.review === reviewKey(snapshot))) return;
  loading(row, true); queued.add(row);
}
function finish(job) {
  requests.delete(job.id);
  if (job.row.entry.id === clicked) clicked = undefined;
  if (relevant(job.row)) ensure(job.row);
  pump();
}
function pump() {
  if (stopped || !snapshot) return;
  for (const row of queued) if (!relevant(row) || row.entry.pending || row.renderedRevision === row.entry.revision) queued.delete(row);
  for (const job of requests.values()) if (job.body && !valid(job) && job.state !== 'classifying') requests.delete(job.id);
  const candidates = [...queued].sort((a, b) => priority(a) - priority(b));
  for (const row of candidates) {
    if (requests.size >= 4) break;
    queued.delete(row);
    if ([...requests.values()].some(job => job.row === row)) continue;
    const job = { id: ++requestId, row, revision: row.entry.revision, review: reviewKey(snapshot), generation: row.generation, state: 'reading' };
    requests.set(job.id, job); row.element.dataset.loadState = 'reading';
    vscode.postMessage({ type: 'body', id: row.entry.id, version: snapshot.version, revision: row.entry.revision, request: job.id });
  }
  if (!classifying) {
    const job = [...requests.values()].filter(job => job.state === 'ready' && valid(job)).sort((a, b) => priority(a.row) - priority(b.row))[0];
    if (job) prepare(job);
  }
}
async function prepare(job) {
  classifying = true; job.state = 'classifying';
  try {
    const row = job.row, body = job.body;
    const stats = body.problem ? undefined : row.classification?.revision === body.revision ? row.classification :
      await statistics.compute(body.original, body.modified);
    if (!valid(job)) return;
    if (stats) row.classification = { ...stats, revision: body.revision };
    // Yield between visible editor creations. Never build a screen of editors in one task.
    await new Promise(resolve => { renderFrame = requestAnimationFrame(resolve); });
    if (!valid(job)) return;
    const guarded = stats && (stats.reason || stats.additions + stats.deletions > (settings.largeDiffThreshold || 500));
    row.largeWhileEditing = !!guarded && !!row.editor?.getModifiedEditor().hasTextFocus();
    if (guarded && !manual.has(row.entry.id) && !editing.pending(row) && !row.editor?.getModifiedEditor().hasTextFocus()) showCard(row, body, stats);
    else {
      if (!body.problem) await loadMonaco();
      if (valid(job)) applyBody(body);
    }
  } catch (error) {
    if (valid(job)) showProblem(job.row, job.revision, `Diff preview failed: ${error.message || error}. Open in Native Diff to inspect it.`);
  } finally { classifying = false; finish(job); }
}
function showCounts(row, stats) {
  row.stats.title = stats?.reason || "";
  if (row.entry.browsed) {
    row.stats.textContent = ''; delete row.element.dataset.additions; delete row.element.dataset.deletions;
  } else if (!stats || stats.reason) {
    row.stats.textContent = '—'; delete row.element.dataset.additions; delete row.element.dataset.deletions;
  } else {
    row.stats.replaceChildren(node('span', 'additions', '+' + stats.additions), node('span', 'deletions', '−' + stats.deletions));
    row.element.dataset.additions = stats.additions; row.element.dataset.deletions = stats.deletions;
  }
  // The navigator shows the same counts beside the file (AC-99).
  const counts = tree.querySelector(`[data-id="${row.entry.id}"] .counts`);
  const c = countsText(row.entry.id);
  if (counts) counts.replaceChildren(...(c ? [node('span', 'additions', '+' + c[0]), node('span', 'deletions', '−' + c[1])] : []));
}
function showCard(row, body, stats) {
  if (editing.held(row)) return;
  const position = anchor(); release(row);
  const card = node('div', 'large-diff');
  card.append(node('p', '', stats.reason || `${stats.additions + stats.deletions} changed lines. Load this diff to review it.`));
  const load = node('button', 'load-diff', 'Load Diff');
  load.addEventListener('click', () => {
    manual.add(row.entry.id); row.renderedRevision = undefined; clicked = row.entry.id;
    ensure(row); pump();
  });
  card.append(load); row.host.replaceChildren(card); row.host.style.height = '130px';
  row.renderedRevision = body.revision; row.element.dataset.loadState = 'deferred';
  showCounts(row, stats); loading(row, false); restoreAnchor(position); trackScroll();
}
function updateViewport() {
  if (view !== 'diffs') return; // Follow hides the diffs: nothing is near, nothing loads
  pursueRestore();
  const top = diffs.scrollTop - 750, bottom = diffs.scrollTop + diffs.clientHeight + 750;
  for (const row of rows.values()) {
    const start = row.element.offsetTop;
    row.nearby = start + row.element.offsetHeight >= top && start <= bottom;
    if (row.nearby) ensure(row); else release(row);
  }
  pump();
}
function language(file) {
  const extension = '.' + file.split('.').pop().toLowerCase();
  const base = file.split('/').pop();
  return monaco.languages.getLanguages().find(l => l.filenames?.includes(base) || l.extensions?.includes(extension))?.id || 'plaintext';
}
function options(row) {
  const readonly = !row || !editing.enabled(row);
  // A browsed (unchanged) file is plain editable text: the whole file, one column (AC-99).
  const plain = !!row?.entry.browsed;
  return { readOnly: readonly, domReadOnly: readonly, originalEditable: false, renderSideBySide: layout.value === 'split' && !plain,
    useInlineViewWhenSpaceIsLimited: false, renderSideBySideInlineBreakpoint: 0,
    renderOverviewRuler: false, renderMarginRevertIcon: false, renderGutterMenu: false,
    diffAlgorithm: 'advanced', ignoreTrimWhitespace: false, maxComputationTime: 5000,
    hideUnchangedRegions: { enabled: !plain, contextLineCount: 3, minimumLineCount: 8, revealLineCount: 20 },
    minimap: { enabled: false }, scrollBeyondLastLine: false, lineNumbersMinChars: 3,
    automaticLayout: false, links: false, hover: { enabled: false }, contextmenu: false,
    fontFamily: settings.fontFamily, fontSize: settings.fontSize || 14, fontLigatures: settings.fontLigatures || false,
    wordWrap: settings.wordWrap || 'off', scrollbar: { alwaysConsumeMouseWheel: false },
  };
}
function resize(row) {
  if (!row.editor) return;
  const savedAnchor = anchor();
  const height = Math.max(70, Math.min(6000, Math.max(row.editor.getOriginalEditor().getContentHeight(), row.editor.getModifiedEditor().getContentHeight())));
  if (Math.abs(parseFloat(row.host.style.height) - height) > 1) { row.host.style.height = height + 'px'; trackScroll(); }
  row.editor.layout({ width: row.host.clientWidth, height });
  restoreAnchor(savedAnchor);
  pursueRestore();
}
function showProblem(row, revision, problem) {
  if (editing.held(row)) return;
  const position = anchor(); release(row);
  row.original?.dispose(); row.modified?.dispose(); row.original = undefined; row.modified = undefined;
  row.host.style.height = '80px'; row.host.replaceChildren(node('p', 'file-problem', problem));
  showCounts(row); row.renderedRevision = revision; row.element.dataset.loadState = 'unsupported';
  loading(row, false); restoreAnchor(position); trackScroll();
}
function applyBody(body) {
  const row = rows.get(body.id);
  if (!row || body.revision !== row.entry.revision || editing.held(row)) return;
  loading(row, false);
  if (!row.nearby || closedFiles.has(row.entry.id)) return;
  if (body.problem) {
    showProblem(row, body.revision, body.problem); return;
  }
  row.progress.hidden = false; row.progress.textContent = row.editor ? 'Updating…' : 'Loading diff…';
  row.element.setAttribute('aria-busy', 'true');
  const unchanged = row.editor && row.original.getValue() === body.original && row.modified.getValue() === body.modified;
  row.body = body;
  showCounts(row, row.classification);
  if (!row.editor) {
    row.host.replaceChildren();
    const lang = language(row.entry.path);
    const modelUri = side => monaco.Uri.from({ scheme: 'branchdiff-review', path: `/${row.entry.id}/${row.entry.path}`, query: side });
    row.original = monaco.editor.createModel(body.original, lang, modelUri('base'));
    row.modified = monaco.editor.createModel(body.modified, lang, modelUri('working'));
    row.modified.setEOL(body.modified.includes('\r\n') ? monaco.editor.EndOfLineSequence.CRLF : monaco.editor.EndOfLineSequence.LF);
    row.original.updateOptions({ tabSize: Number(settings.tabSize) || 4 }); row.modified.updateOptions({ tabSize: Number(settings.tabSize) || 4 });
    row.editor = monaco.editor.createDiffEditor(row.host, options(row));
    row.editor.setModel({ original: row.original, modified: row.modified });
    row.listeners = [row.editor.getOriginalEditor().onDidContentSizeChange(() => resize(row)),
      row.editor.getModifiedEditor().onDidContentSizeChange(() => resize(row)),
      row.editor.onDidUpdateDiff(() => {
        if (row.renderedRevision === row.entry.revision) loading(row, false);
        resize(row); renderHunks(row);
      }),
      row.editor.getModifiedEditor().onDidLayoutChange(() => placeHunks(row))];
    editing.attach(row, monaco);
  } else {
    row.viewState = row.editor.saveViewState();
    replaceText(row.original, body.original);
    if (!editing.apply(row, body)) return;
  }
  if (row.viewState) row.editor.restoreViewState(row.viewState);
  editing.update(row);
  row.renderedRevision = body.revision; row.element.dataset.revision = body.revision; row.element.dataset.loadState = 'rendered';
  resize(row);
  if (unchanged) loading(row, false);
  if (row.pendingLine && followState === 'following') { const line = row.pendingLine; row.pendingLine = undefined; requestAnimationFrame(() => revealLine(row, line)); }
}
// ------------------------------------------------------------------ Overseer hunk actions
// Accept marks a hunk reviewed (keyed by its content, so a changed hunk is unreviewed again);
// Reject replaces the hunk with the comparison base through the same native edit path as
// typing in the review (a VS Code WorkspaceEdit, undoable in the native editor) and saves.
let reviewedHunks = new Set();
function hunkHash(text) {
  let h1 = 0x811c9dc5, h2 = 0x01000193;
  for (let i = 0; i < text.length; i++) { const c = text.charCodeAt(i); h1 = Math.imul(h1 ^ c, 16777619) >>> 0; h2 = Math.imul(h2 ^ c, 2246822519) >>> 0; }
  return h1.toString(16).padStart(8, '0') + h2.toString(16).padStart(8, '0');
}
function hunkTexts(row, change) {
  const o = row.original.getLinesContent(), m = row.modified.getLinesContent();
  const orig = change.originalEndLineNumber ? o.slice(change.originalStartLineNumber - 1, change.originalEndLineNumber) : [];
  const mod = change.modifiedEndLineNumber ? m.slice(change.modifiedStartLineNumber - 1, change.modifiedEndLineNumber) : [];
  return { orig, mod, key: hunkHash(row.entry.path + '\u0000' + orig.join('\n') + '\u0000' + mod.join('\n')) };
}
function renderHunks(row) {
  const editor = row.editor?.getModifiedEditor();
  if (!editor) return;
  for (const h of row.hunks || []) editor.removeOverlayWidget(h.widget);
  row.hunks = [];
  const changes = row.editor.getLineChanges() || [];
  // Overseer (AC-76): a thin strip above each hunk holds its actions, so they never cover code.
  const tops = [];
  editor.changeViewZones(zones => {
    for (const id of row.hunkZones || []) zones.removeZone(id);
    row.hunkZones = changes.map((change, index) => zones.addZone({ afterLineNumber: change.modifiedEndLineNumber ? change.modifiedStartLineNumber - 1 : change.modifiedStartLineNumber,
      heightInPx: 22, domNode: node('div', 'hunk-strip'), onDomNodeTop: top => { tops[index] = top; const h = row.hunks?.[index]; if (h) { h.zoneTop = top; h.dom.style.top = top + 'px'; } } }));
  });
  const reviewedRanges = [];
  const canEdit = editing.enabled(row);
  changes.forEach((change, index) => {
    const { orig, mod, key } = hunkTexts(row, change);
    const reviewed = reviewedHunks.has(key);
    const dom = node('div', 'hunk-actions' + (reviewed ? ' reviewed' : ''));
    dom.dataset.key = key; dom.dataset.hunk = String(index + 1);
    const where = change.modifiedEndLineNumber ? `lines ${change.modifiedStartLineNumber}–${change.modifiedEndLineNumber}` : `deletion after line ${change.modifiedStartLineNumber}`;
    dom.setAttribute('role', 'group'); dom.setAttribute('aria-label', `Hunk ${index + 1} of ${row.entry.path}, ${where}`);
    const glyph = name => { const g = node('span', 'codicon codicon-' + name); g.setAttribute('aria-hidden', 'true'); return g; };
    if (reviewed) { const badge = node('span', 'hunk-badge'); badge.append(glyph('pass-filled')); badge.title = 'Reviewed'; dom.append(badge); }
    // Overseer (AC-232): keeping or undoing the agent's change is a choice in words, Keep or Undo.
    const accept = node('button', 'hunk-accept'); accept.append(glyph(reviewed ? 'close' : 'check'), node('span', 'hunk-word', reviewed ? 'Unmark' : 'Keep'));
    accept.title = reviewed ? "Kept and marked reviewed. Click to mark it not reviewed (the change stays either way)" : "Keep: the agent's change stays, and this hunk is marked reviewed (nothing is committed or staged)";
    accept.setAttribute('aria-label', reviewed ? `Unmark reviewed hunk ${index + 1}` : `Accept hunk ${index + 1}`);
    accept.addEventListener('click', () => vscode.postMessage({ type: 'hunkReview', reviewed: !reviewed, key, path: row.entry.path, version: snapshot?.version,
      modifiedStart: change.modifiedStartLineNumber, modifiedEnd: change.modifiedEndLineNumber, modified: mod, anchor: row.modified.getLineContent(Math.max(1, Math.min(change.modifiedStartLineNumber || 1, row.modified.getLineCount()))) }));
    const reject = node('button', 'hunk-reject'); reject.append(glyph('discard'), node('span', 'hunk-word', 'Undo'));
    reject.disabled = !canEdit;
    reject.title = canEdit ? "Undo: put back what was there before the agent's change, in the agent's copy (Cmd+Z in the native editor brings it back)" : 'Undo is unavailable: this file cannot be edited in the review (see Open in Native Diff)';
    reject.setAttribute('aria-label', `Reject hunk ${index + 1}`);
    reject.addEventListener('click', () => rejectHunk(row, change, key));
    dom.append(accept, reject);
    const widget = { getId: () => `overseer.hunk.${row.entry.id}.${index}`, getDomNode: () => dom, getPosition: () => null };
    editor.addOverlayWidget(widget);
    row.hunks.push({ widget, dom, change, key, zoneTop: tops[index] });
    if (reviewed && change.modifiedEndLineNumber) reviewedRanges.push({ range: { startLineNumber: change.modifiedStartLineNumber, startColumn: 1, endLineNumber: change.modifiedEndLineNumber, endColumn: 1 }, options: { isWholeLine: true, className: 'hunk-reviewed-line' } });
  });
  if (!row.hunkDecorations) row.hunkDecorations = editor.createDecorationsCollection();
  row.hunkDecorations.set(reviewedRanges);
  row.element.dataset.hunks = String(changes.length);
  row.element.dataset.reviewed = String(row.hunks.filter(h => reviewedHunks.has(h.key)).length);
  placeHunks(row);
}
function placeHunks(row) {
  const editor = row.editor?.getModifiedEditor();
  if (!editor) return;
  for (const h of row.hunks || []) {
    if (h.zoneTop !== undefined) { h.dom.style.top = h.zoneTop + 'px'; continue; }
    const line = Math.max(1, Math.min(row.modified.getLineCount(), h.change.modifiedEndLineNumber ? h.change.modifiedStartLineNumber : h.change.modifiedStartLineNumber + 1));
    h.dom.style.top = Math.max(0, editor.getTopForLineNumber(line) - editor.getScrollTop()) + 'px';
  }
}
function rejectHunk(row, change, key) {
  if (!editing.enabled(row) || editing.held(row)) { message('This hunk cannot be rejected in the review right now. Use Open in Native Diff.'); return; }
  const o = row.original.getLinesContent(), m = row.modified.getLinesContent();
  // Deletion hunks re-insert the base lines after modifiedStart; others replace the modified lines.
  const a = change.modifiedEndLineNumber ? change.modifiedStartLineNumber - 1 : change.modifiedStartLineNumber;
  const b = change.modifiedEndLineNumber ? change.modifiedEndLineNumber : change.modifiedStartLineNumber;
  const seg = change.originalEndLineNumber ? o.slice(change.originalStartLineNumber - 1, change.originalEndLineNumber) : [];
  const next = [...m.slice(0, a), ...seg, ...m.slice(b)].join(row.modified.getEOL());
  editing.hunkOperation(row, key);
  replaceText(row.modified, next);
  editing.save(row);
}
// Theme variables arrive as hex or rgb()/rgba(); Monaco themes take hex. Without this an rgba value
// was dropped and Monaco's own olive inserted-text color showed through (Overseer, AC-101).
function hexColor(value) {
  if (/^#[\da-f]{6}([\da-f]{2})?$/i.test(value)) return value;
  const m = /^rgba?\(\s*(\d+)[,\s]+(\d+)[,\s]+(\d+)(?:[,\s/]+([\d.]+%?))?\s*\)$/i.exec(value);
  if (!m) return undefined;
  const alpha = m[4] === undefined ? 1 : m[4].endsWith('%') ? parseFloat(m[4]) / 100 : parseFloat(m[4]);
  const hex = n => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, '0');
  return '#' + hex(+m[1]) + hex(+m[2]) + hex(+m[3]) + (alpha < 1 ? hex(alpha * 255) : '');
}
function theme() {
  if (!monaco) return;
  const high = document.body.classList.contains('vscode-high-contrast') || document.body.classList.contains('vscode-high-contrast-light');
  const light = document.body.classList.contains('vscode-light') || document.body.classList.contains('vscode-high-contrast-light');
  const css = getComputedStyle(document.body); const colors = {};
  for (const key of ['editor.background', 'editor.foreground', 'editorLineNumber.foreground', 'editor.selectionBackground',
    'diffEditor.insertedTextBackground', 'diffEditor.removedTextBackground', 'diffEditor.insertedLineBackground', 'diffEditor.removedLineBackground']) {
    const value = hexColor(css.getPropertyValue('--vscode-' + key.replaceAll('.', '-')).trim());
    if (value) colors[key] = value;
  }
  monaco.editor.defineTheme('branch-diff', { base: high ? (light ? 'hc-light' : 'hc-black') : (light ? 'vs' : 'vs-dark'), inherit: true, rules: [], colors });
  monaco.editor.setTheme('branch-diff');
}
function updateSettings(value) {
  const thresholdChanged = settings.largeDiffThreshold !== value?.largeDiffThreshold || settings.enableEditing !== value?.enableEditing;
  settings = value || {}; theme();
  if (thresholdChanged) for (const row of rows.values()) { if (row.classification && !manual.has(row.entry.id)) row.renderedRevision = undefined; }

  for (const row of rows.values()) if (row.editor) { row.editor.updateOptions(options(row)); editing.update(row); resize(row); }
  followEditor?.updateOptions(followOptions());
}
// Keep shell assets independent: even delayed Monaco startup cannot block the file list.
async function loadMonaco() {
  if (!monacoLoading) monacoLoading = (async () => {
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    if (stopped) return;
    document.body.dataset.monacoState = 'loading';
    const css = node('link'); css.rel = 'stylesheet'; css.href = document.body.dataset.monacoCss;
    const styled = new Promise((resolve, reject) => { css.onload = resolve; css.onerror = () => reject(new Error('Local editor stylesheet failed to load')); });
    document.head.append(css);
    const module = await import(document.body.dataset.monaco);
    await styled;
    if (stopped) return;
    monaco = module.monaco; disposeMonaco = module.initialize(message); theme();
    document.body.dataset.monacoState = 'ready';
    document.body.dataset.monacoReady = String(performance.now());
  })();
  return monacoLoading;
}
function applySnapshot(next) {
  if (next.version < Math.max(snapshot?.version || 0, nextSnapshot?.version || 0)) return;
  // Coalesce snapshots while yielding to paint. Never interleave two DOM reconciliations.
  nextSnapshot = next;
  if (!rendering) reconcile().catch(error => { message('File list failed to render: ' + error.message); });
}
async function reconcile() {
  rendering = true;
  try {
    while (nextSnapshot && !stopped) {
      const next = nextSnapshot; nextSnapshot = undefined;
      const previousAnchor = anchor();
      if (snapshot && reviewKey(snapshot) !== reviewKey(next)) {
        manual.clear(); queued.clear();
        for (const row of rows.values()) { release(row); row.renderedRevision = undefined; row.classification = undefined; }
      }
      const oldStructure = snapshot?.entries.map(e => [e.id, e.path, e.status, e.unsaved, e.conflicted, !!e.browsed]);
      snapshot = next;
      Object.assign(identity, { repository: next.repository, mode: next.mode, target: next.target, runId: next.overseer?.runId || identity.runId });
      updateSettings(next.settings);
      message(next.error || next.warning || (stickyNotice && Date.now() < stickyNotice.until ? stickyNotice.text : ''));
      const description = next.description;
      if (!next.overseer) document.getElementById('comparison').textContent = description ? `${description.headName || 'HEAD'} → ${description.base}` : 'Branch Diff';
      if (!next.overseer) document.getElementById('comparison').title = description ? `Merge-base ${description.mergeBase} → ${next.mode === 'workingTree' ? 'working tree + unsaved edits' : description.headSha}` : '';
      const changedCount = next.entries.filter(e => !e.browsed).length;
      total.textContent = `${changedCount} ${changedCount === 1 ? 'file' : 'files'}${next.checking ? '…' : ''}`;
      total.title = next.checking ? 'Still looking for changed files' : 'Changed files in this comparison';
      total.dataset.count = changedCount;
      document.body.dataset.checking = String(!!next.checking);
      document.body.dataset.cached = String(!!next.cached);
      document.getElementById('loading-stage').textContent = next.cached ? 'Checking for changes…' : next.checking ? 'Checking files…' : '';
      const ids = new Set(next.entries.map(e => e.id));
      for (const [id, row] of rows) if (!ids.has(id)) { release(row); row.element.remove(); rows.delete(id); }
      diffs.querySelector('.empty')?.remove();
      // Metadata-only validation updates reuse the existing navigator DOM.
      if (JSON.stringify(oldStructure) !== JSON.stringify(next.entries.map(e => [e.id, e.path, e.status, e.unsaved, e.conflicted, !!e.browsed]))) {
        // Files may have come or gone: list the folders shown again (the answers re-render the tree).
        if (oldStructure) for (const [dirPath, listing] of dirCache) if (!listing.loading) vscode.postMessage({ type: 'listDir', path: dirPath });
        renderTree();
      }
      let previous = null, batchStart = performance.now();
      for (let index = 0; index < next.entries.length; index++) {
        const entry = next.entries[index];
        let row = rows.get(entry.id);
        if (!row) { row = makeRow(entry); rows.set(entry.id, row); }
        const wasBrowsed = !!row.entry.browsed;
        row.entry = entry;
        row.element.classList.toggle('browsed', !!entry.browsed); row.close.hidden = !entry.browsed;
        if (row.editor && wasBrowsed !== !!entry.browsed) row.editor.updateOptions(options(row));
        if (row.editor) editing.update(row);
        if (row.title.textContent !== entry.path) row.title.textContent = entry.path;
        if (row.status.textContent !== entry.status) { row.status.textContent = entry.status; row.status.className = 'status status-' + entry.status; }
        const unsaved = entry.unsaved ? 'Unsaved' : '';
        if (row.unsaved.textContent !== unsaved) row.unsaved.textContent = unsaved;
        const disabled = !!entry.pending || !!next.cached;
        if (row.open.disabled !== disabled) row.open.disabled = disabled;
        row.title.setAttribute('aria-disabled', String(disabled));
        row.title.title = disabled ? `Checking ${entry.path}…` : `Open ${entry.path}`;
        if (disabled) row.title.removeAttribute('href'); else row.title.setAttribute('href', '#');
        if (row.element.parentElement !== diffs || row.element.previousElementSibling !== previous) diffs.insertBefore(row.element, previous ? previous.nextSibling : diffs.firstChild);
        previous = row.element;
        fold(row);
        if (index < next.entries.length - 1 && performance.now() - batchStart >= 8) {
          document.body.dataset.hierarchyReady ||= String(performance.now());
          await new Promise(resolve => requestAnimationFrame(resolve));
          if (stopped) return;
          batchStart = performance.now();
        }
      }
      if (!changedCount && !next.entries.length) diffs.append(node('p', 'empty', next.error ? 'Comparison unavailable: ' + next.error : next.checking ? 'Checking files…' : 'No changes for this comparison. Pre-existing dirty work stays listed in the Workspace Dirty view.'));
      if (restoring) {
        diffs.scrollTop = saved.scrollTop || 0; restoreAnchor(saved.anchor); pursueRestore(); restoring = false;
      } else { restoreAnchor(previousAnchor); pursueRestore(); }
      if (pendingJump && rows.has(pendingJump)) { const id = pendingJump; pendingJump = undefined; jump(id); }
      if (pendingBrowse) { const opened = next.entries.find(e => e.path === pendingBrowse && !e.pending); if (opened) { pendingBrowse = undefined; jump(opened.id); } }
      if (pendingReveal) applyReveal(pendingReveal);
      document.body.dataset.version = next.version;
      document.body.dataset.hierarchyReady ||= String(performance.now());
      if (!next.cached && !next.checking && !next.error) {
        const cached = { description: next.description, entries: next.entries.filter(e => !e.browsed).map(e => ({ id: e.id, path: e.path, status: e.status, unsaved: e.unsaved })) };
        hierarchy = cached.entries.length <= 10000 && JSON.stringify(cached).length <= 2 * 1024 * 1024 ? cached : undefined;
      }
      updateViewport(); trackScroll();
    }
  } finally { rendering = false; }
}

// ---- Overseer: comparison label, Follow state and reveal of agent edits.
// One icon (AC-74): following, paused by your navigation (click resumes), or manual.
const followButton = document.getElementById('follow');
const followStatus = document.getElementById('follow-state');
let followState = 'off', followNote = '', pendingReveal;
function renderFollow() {
  const on = followState === 'following', paused = followState === 'paused';
  followButton.setAttribute('aria-pressed', String(on));
  followButton.dataset.state = followState;
  const name = on ? 'Following the agent' : paused ? 'Resume Follow' : 'Follow the agent';
  followButton.setAttribute('aria-label', name);
  followButton.title = on ? `Following the agent's edits${followNote ? ' — ' + followNote : ''}\nClick to stay where you are` : paused ? 'Follow paused by your navigation\nClick to resume' : "Manual: your file and scroll stay put\nClick to follow the agent's edits";
  followButton.firstElementChild.className = 'codicon codicon-' + (on ? 'eye' : paused ? 'debug-pause' : 'eye-closed');
}
function applyOverseer(o) {
  if (!o) return;
  if (Array.isArray(o.reviewed)) {
    const next = new Set(o.reviewed);
    const changed = next.size !== reviewedHunks.size || [...next].some(k => !reviewedHunks.has(k));
    reviewedHunks = next;
    if (changed) for (const row of rows.values()) if (row.editor) renderHunks(row);
  }
  const label = document.getElementById('base-label');
  const c = o.comparison || {};
  label.textContent = c.label || 'Comparison';
  document.getElementById('base').title = [c.label, c.base ? 'Base: ' + c.base : 'Base unavailable', c.detail, c.provenance ? 'Provenance: ' + c.provenance : ''].filter(Boolean).join('\n') + '\nClick to choose another comparison.';
  document.getElementById('comparison').textContent = o.runTitle || 'Run';
  const scope = document.getElementById('scope');
  if (scope && o.scope && scope.value !== o.scope) scope.value = o.scope;
  document.body.dataset.scope = o.scope || 'all';
  document.getElementById('comparison').title = [o.runTitle, HARNESS_NAME[o.harness] || o.harness, o.workspacePath].filter(Boolean).join('\n');
  // The full path is in the tooltip and data-workspace; the note shows ~/…/last/two.
  const note = document.getElementById('workspace-note');
  const home = document.body.dataset.home || '';
  let short = o.workspacePath || '';
  if (home && short.startsWith(home + '/')) short = '~' + short.slice(home.length);
  const parts = short.split('/').filter(Boolean);
  if (parts.length > 3) short = (short.startsWith('~') ? '~/…/' : '…/') + parts.slice(-2).join('/');
  // Kept for assistive tech and tests; the visible header stays short (the path is in tooltips).
  note.textContent = o.workspacePath || '';
  note.hidden = true;
  document.getElementById('base').title += `\n${o.workspaceKind === 'current' ? 'Checkout' : 'Worktree'}: ${short}`;
  document.body.dataset.workspace = o.workspacePath || '';
  renderLand(o.land);
  // A switch made here wins over a message the host sent before it heard of it.
  if (o.view && o.view !== view && Date.now() - viewChosenAt > 1500) setView(o.view);
  followState = o.follow || 'off';
  followNote = o.followNote || '';
  followStatus.textContent = followState === 'paused' ? (/paused/.test(o.followNote || '') ? o.followNote : 'Follow paused by your navigation') : followState === 'following' ? (o.followNote || 'Following agent edits') : '';
  renderFollow();
}
// Overseer (AC-243): the chat's Merge buttons in the review's toolbar, for this agent.
const landBox = document.getElementById('land');
// Publish to GitHub (no remote) is in the chat only; the review keeps its toolbar short (AC-54, AC-76).
for (const [id, action] of [['land-merge', 'merge'], ['land-pr', 'openPullRequest'], ['land-cancel', 'cancelMerge'], ['land-cleanup', 'cleanup']]) {
  document.getElementById(id)?.addEventListener('click', () => vscode.postMessage({ type: 'land', action }));
}
function renderLand(l) {
  if (!landBox) return;
  // Narrow reviews show the buttons' icons only; their names stay in the tooltip and accessible name.
  const show = (id, on, text) => { const b = document.getElementById(id); b.hidden = !on; const w = b.querySelector('.land-word'); if (text) w.textContent = text; b.title = w.textContent; b.setAttribute('aria-label', w.textContent); };
  const state = !l || !l.worktree || l.active ? '' : l.conflicts ? 'conflicts' : l.merged ? 'merged' : l.canMerge ? 'ready' : l.text ? 'landed' : '';
  landBox.hidden = !state; landBox.dataset.state = state;
  if (!state) return;
  const words = document.getElementById('land-text');
  words.title = state === 'conflicts' ? `Merge stopped: conflicts in ${l.conflicts.join(', ')}` : l.text || '';
  words.textContent = state === 'conflicts' ? (l.conflicts.length ? `Merge stopped: conflicts in ${l.conflicts.join(', ')}` : 'Merge stopped: conflicts') : state === 'ready' ? (l.landing && l.landing.state === 'pr' ? l.text : '') : l.text;
  show('land-merge', state === 'conflicts' || state === 'ready', state === 'conflicts' ? 'Finish merge' : `Merge into ${l.target || 'main'}`);
  show('land-pr', state === 'ready' && !!l.github);
  show('land-cancel', state === 'conflicts');
  show('land-cleanup', state === 'merged' && !l.removed);
}
function userNavigated(reason) {
  if (followState !== 'following') return;
  followState = 'paused'; followStatus.textContent = 'Follow paused by your navigation'; renderFollow();
  vscode.postMessage({ type: 'followPause', reason });
}
// AC-233, AC-264: Follow or Diffs only, both this review. The switch changes the view at once and
// tells the host, which remembers it for the agent.
for (const seg of document.querySelectorAll('#view-mode .seg')) seg.addEventListener('click', () => {
  if (seg.dataset.view === view) return;
  viewChosenAt = Date.now(); setView(seg.dataset.view); vscode.postMessage({ type: 'setView', view: seg.dataset.view });
});
followButton.addEventListener('click', () => {
  if (followState === 'paused') vscode.postMessage({ type: 'followResume' });
  else vscode.postMessage({ type: 'follow', enabled: followState !== 'following' });
});
document.getElementById('base').addEventListener('click', () => vscode.postMessage({ type: 'pickComparison' }));
document.getElementById('scope').addEventListener('change', event => vscode.postMessage({ type: 'scope', scope: event.target.value }));
diffs.addEventListener('wheel', () => userNavigated('scroll'), { passive: true });
diffs.addEventListener('touchstart', () => userNavigated('scroll'), { passive: true });
diffs.addEventListener('mousedown', event => { if (!event.target.closest('button')) userNavigated('pointer'); });
diffs.addEventListener('keydown', event => { if (!['Shift', 'Control', 'Alt', 'Meta'].includes(event.key)) userNavigated('keyboard'); });
tree.addEventListener('click', () => { if (view === 'diffs') userNavigated('file selection'); }, true);
filter.addEventListener('input', () => userNavigated('filter'));
function revealLine(row, line) {
  if (!row.editor || !line) return false;
  const editor = row.editor.getModifiedEditor();
  const top = row.element.offsetTop + row.header.offsetHeight + editor.getTopForLineNumber(line);
  diffs.scrollTop = Math.max(0, top - diffs.clientHeight / 3);
  row.revealDecorations = editor.deltaDecorations(row.revealDecorations || [], [{ range: { startLineNumber: line, startColumn: 1, endLineNumber: line, endColumn: 1 }, options: { isWholeLine: true, className: 'follow-line' } }]);
  setTimeout(() => { if (row.editor) row.revealDecorations = row.editor.getModifiedEditor().deltaDecorations(row.revealDecorations || [], []); }, 2500);
  return true;
}
function applyReveal(value) {
  // Overseer: `user` reveals (a file edit clicked in the conversation) work without Follow and pause it.
  if (view !== 'diffs' || (followState !== 'following' && !value.user)) return;
  const id = value.id || snapshot?.entries.find(e => e.path === value.path)?.id;
  if (!id || !rows.has(id)) { pendingReveal = value; return; }
  pendingReveal = undefined;
  const row = rows.get(id);
  if (value.user) { userNavigated('conversation'); jump(id); document.body.dataset.revealed = value.path + ':' + (value.line || ''); if (!revealLine(row, value.line)) row.pendingLine = value.line; return; }
  jump(id);
  followStatus.textContent = 'Following: ' + value.path + (value.line ? ':' + value.line : '') + (value.attribution ? ' (' + value.attribution + ')' : '');
  followNote = followStatus.textContent; renderFollow();
  if (!revealLine(row, value.line)) row.pendingLine = value.line;
}
// ---- Overseer (AC-264): Follow, in the review. The middle shows the file the agent is in right now,
// live and read-only, its changes marked (added and changed lines with a bar, a changed line saying
// what it was, a marker where lines were removed), scrolled to the line the agent is at. A file
// picked in All files shows in the same place; Follow goes back to the agent when the agent moves to
// another file, or at once with "Follow the agent".
const followView = document.getElementById('follow-view');
const followEditorHost = document.getElementById('follow-editor');
const followProblem = document.getElementById('follow-problem');
const followAgainButton = document.getElementById('follow-again');
followAgainButton.addEventListener('click', () => vscode.postMessage({ type: 'followAgain' }));
function renderViewSwitch() {
  document.body.dataset.view = view;
  for (const seg of document.querySelectorAll('#view-mode .seg')) {
    const on = seg.dataset.view === view;
    seg.classList.toggle('on', on); seg.setAttribute('aria-pressed', String(on));
  }
}
function setView(next) {
  if (next !== 'follow' && next !== 'diffs') return;
  const was = view; view = next; renderViewSwitch();
  if (was === view) return;
  renderTree();
  if (view === 'diffs') requestAnimationFrame(() => { for (const row of rows.values()) resize(row); updateViewport(); trackScroll(); });
  else followEditor?.layout();
}
renderViewSwitch();
/** Marks the file Follow shows in the list, opening the folders above it (All files lists one folder at a time). */
function markFollowed(relPath) {
  followPath = relPath || '';
  if (view !== 'follow') return;
  const parts = followPath.split('/'); parts.pop();
  let opened = false;
  for (let i = 1; i <= parts.length; i++) { const dir = parts.slice(0, i).join('/'); if (!openDirs.has(dir)) { openDirs.add(dir); opened = true; } }
  if (opened) { renderTree(); persist(); }
  for (const button of tree.querySelectorAll('.file')) {
    const on = button.dataset.path === followPath;
    button.classList.toggle('active', on); button.setAttribute('aria-selected', String(on));
    if (on && !navigatorBusy()) button.scrollIntoView({ block: 'nearest' });
  }
}
function showFollowProblem(text) {
  followProblem.textContent = text; followProblem.hidden = !text;
  followEditorHost.hidden = !!text;
  document.body.dataset.followState = text ? 'problem' : 'shown';
}
function followOptions() {
  return { readOnly: true, domReadOnly: true, automaticLayout: true, minimap: { enabled: false }, scrollBeyondLastLine: false,
    lineNumbersMinChars: 4, lineDecorationsWidth: 12, glyphMargin: false, folding: false, links: false, hover: { enabled: false }, contextmenu: false,
    renderLineHighlight: 'none', renderValidationDecorations: 'off', occurrencesHighlight: 'off', selectionHighlight: false, matchBrackets: 'never',
    fontFamily: settings.fontFamily, fontSize: settings.fontSize || 14, fontLigatures: settings.fontLigatures || false,
    wordWrap: settings.wordWrap || 'off', scrollbar: { alwaysConsumeMouseWheel: true }, tabSize: Number(settings.tabSize) || 4 };
}
function followMarks(marks) {
  const out = [];
  if (!marks || !followModel) return out;
  const lines = followModel.getLineCount();
  const at = line => { const l = Math.max(1, Math.min(lines, line)); const c = followModel.getLineMaxColumn(l); return { startLineNumber: l, startColumn: c, endLineNumber: l, endColumn: c }; };
  const note = (line, content, className) => out.push({ range: at(line), options: { after: { content, inlineClassName: className } } });
  for (const r of marks.added || []) out.push({ range: { startLineNumber: r.start, startColumn: 1, endLineNumber: Math.min(lines, r.end), endColumn: 1 }, options: { isWholeLine: true, className: 'follow-added', linesDecorationsClassName: 'follow-bar-added' } });
  for (const r of marks.changed || []) {
    out.push({ range: { startLineNumber: r.start, startColumn: 1, endLineNumber: Math.min(lines, r.end), endColumn: 1 }, options: { isWholeLine: true, className: 'follow-changed', linesDecorationsClassName: 'follow-bar-changed' } });
    if (r.was) r.was.forEach((text, i) => { if (r.start + i <= lines) note(r.start + i, `was: ${text || '(empty line)'}`, 'follow-was'); });
    else if (r.replaced) note(r.start, `replaced ${r.replaced} line${r.replaced === 1 ? '' : 's'}`, 'follow-was');
  }
  for (const r of marks.removed || []) {
    const line = r.line > 0 ? r.line : 1;
    out.push({ range: at(line), options: { linesDecorationsClassName: 'follow-bar-removed' } });
    note(line, `− ${r.count} line${r.count === 1 ? '' : 's'} removed ${r.line > 0 ? 'below' : 'above'}`, 'follow-removed');
  }
  return out;
}
async function applyFollowFile(value) {
  if (!Number.isSafeInteger(value.seq) || value.seq < followSeq) return;
  followSeq = value.seq;
  followSource = value.source === 'user' ? 'user' : 'agent';
  const path = String(value.path || '');
  document.getElementById('follow-path').textContent = path || 'No file yet';
  document.getElementById('follow-path').title = path;
  const why = followSource === 'user' ? 'You picked this file. Follow goes back to the agent when it moves to another file.' : `The agent is here${value.attribution ? ' · ' + value.attribution : ''}`;
  document.getElementById('follow-why').textContent = why;
  document.getElementById('follow-icon').className = 'codicon codicon-' + (followSource === 'user' ? 'file' : 'eye');
  followAgainButton.hidden = followSource !== 'user';
  followView.dataset.source = followSource;
  document.body.dataset.followPath = path; document.body.dataset.followSource = followSource;
  markFollowed(path);
  if (value.problem || typeof value.text !== 'string') { showFollowProblem(value.problem || 'This file cannot be shown.'); return; }
  await loadMonaco();
  if (stopped || value.seq !== followSeq) return;
  showFollowProblem('');
  if (!followEditor) {
    followEditor = monaco.editor.create(followEditorHost, followOptions());
    followDecorations = followEditor.createDecorationsCollection();
  }
  const uri = monaco.Uri.from({ scheme: 'overseer-follow', path: '/' + path });
  const same = followModel && followModel.uri.toString() === uri.toString();
  if (same) {
    const state = value.reveal ? undefined : followEditor.saveViewState();
    replaceText(followModel, value.text);
    if (state) followEditor.restoreViewState(state);
  } else {
    const old = followModel;
    followModel = monaco.editor.createModel(value.text, language(path), uri);
    followEditor.setModel(followModel);
    old?.dispose();
  }
  followDecorations.set(followMarks(value.marks));
  const line = Math.max(1, Math.min(followModel.getLineCount(), Number(value.line) || 1));
  if (value.reveal || !same) {
    followEditor.revealLineInCenterIfOutsideViewport(line);
    if (!value.line) followEditor.setScrollTop(0);
    if (value.line && followSource === 'agent') {
      clearTimeout(followFlash);
      const flash = followEditor.createDecorationsCollection([{ range: { startLineNumber: line, startColumn: 1, endLineNumber: line, endColumn: 1 }, options: { isWholeLine: true, className: 'follow-line' } }]);
      followFlash = setTimeout(() => flash.clear(), 2500);
    }
  }
  document.body.dataset.followShown = `${path}:${line}:${followModel.getLineCount()}`;
  document.body.dataset.followMarks = JSON.stringify({ added: (value.marks?.added || []).length, changed: (value.marks?.changed || []).length, removed: (value.marks?.removed || []).length });
}
window.addEventListener('message', event => {
  const value = event.data;
  if (!value || typeof value !== 'object') return;
  if (editing.receive(value)) return;
  if (value.type === 'snapshot') { applyOverseer(value.overseer); applySnapshot(value); }
  else if (value.type === 'overseer') applyOverseer(value.overseer);
  else if (value.type === 'reveal') applyReveal(value);
  else if (value.type === 'followFile') applyFollowFile(value).catch(error => showFollowProblem('This file could not be shown: ' + (error.message || error)));
  else if (value.type === 'body' && snapshot) {
    const job = requests.get(value.request);
    if (!job) return;
    if (!valid(job) || value.revision !== job.revision) { finish(job); return; }
    job.body = value; job.state = 'ready'; pump();
  }
  else if (value.type === 'retry') {
    const job = requests.get(value.request);
    if (job) { requests.delete(job.id); setTimeout(() => { ensure(job.row); pump(); }, 150); }
  }
  else if (value.type === 'progress') {
    clearTimeout(progressTimer);
    const stage = document.getElementById('loading-stage');
    if (!value.stage) stage.textContent = snapshot?.cached ? 'Checking for changes…' : snapshot?.checking ? 'Checking files…' : '';
    else if (snapshot) progressTimer = setTimeout(() => { stage.textContent = snapshot?.cached ? 'Checking for changes…' : value.stage; }, 200);
    const empty = diffs.querySelector('.empty');
    if (!snapshot && empty) empty.textContent = value.stage || 'Finding changed files…';
    if (snapshot && value.id) { const row = rows.get(value.id); if (row) { row.generation = (row.generation || 0) + 1; loading(row, true); } }
    if (!value.stage) for (const row of rows.values()) if (row.renderedRevision === row.entry.revision) loading(row, false);
  }
  else if (value.type === 'settings') { updateSettings(value.settings); updateViewport(); }
  else if (value.type === 'dir') { dirCache.set(value.path, value.error ? { error: value.error } : { entries: value.entries || [], limit: dirCache.get(value.path)?.limit }); if (!changesOnly()) renderTree(); }
  else if (value.type === 'hunkReviewed') { if (value.reviewed) reviewedHunks.add(value.key); else reviewedHunks.delete(value.key); for (const row of rows.values()) if (row.hunks?.some(h => h.key === value.key)) renderHunks(row); }
  else if (value.type === 'notice') stickyMessage(value.message);
});
layout.addEventListener('change', () => { updateSettings(settings); persist(); });
document.getElementById('refresh').addEventListener('click', () => vscode.postMessage({ type: 'refresh' }));
document.getElementById('where').addEventListener('click', () => vscode.postMessage({ type: 'whereAmI' }));
const themeObserver = new MutationObserver(theme);
themeObserver.observe(document.body, { attributes: true, attributeFilter: ['class', 'data-vscode-theme-id'] });
const sizeObserver = new ResizeObserver(() => {
  cancelAnimationFrame(resizeFrame);
  resizeFrame = requestAnimationFrame(() => { for (const row of rows.values()) resize(row); updateViewport(); });
});
sizeObserver.observe(diffs);
const handle = document.getElementById('resize');
handle.addEventListener('pointerdown', event => {
  handle.setPointerCapture(event.pointerId);
  const move = e => setNavWidth(e.clientX);
  const stop = () => { handle.removeEventListener('pointermove', move); handle.removeEventListener('pointerup', stop); persist(); };
  handle.addEventListener('pointermove', move); handle.addEventListener('pointerup', stop);
});
handle.addEventListener('keydown', event => {
  if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') { event.preventDefault(); setNavWidth(navWidth + (event.key === 'ArrowLeft' ? -20 : 20)); persist(); }
});
document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden') flushState(); });
window.addEventListener('pagehide', () => {
  flushState(); stopped = true; statistics.dispose(); queued.clear(); requests.clear();
  sizeObserver.disconnect(); themeObserver.disconnect();
  for (const row of rows.values()) release(row);
  followEditor?.dispose(); followModel?.dispose();
  disposeMonaco?.();
  clearTimeout(persistTimer); clearTimeout(progressTimer); cancelAnimationFrame(frame); cancelAnimationFrame(resizeFrame); cancelAnimationFrame(renderFrame);
});
initialized = true; document.body.dataset.rendererState = 'ready';
document.body.dataset.shellReady = String(performance.now());
const cached = saved.hierarchy;
if (saved.repository === identity.repository && saved.mode === identity.mode && (saved.target || '') === identity.target &&
    cached && Array.isArray(cached.entries) && cached.entries.length <= 10000 && JSON.stringify(cached).length <= 2 * 1024 * 1024 &&
    new Set(cached.entries.map(e => e?.id)).size === cached.entries.length &&
    cached.entries.every(e => e && /^[a-f0-9]{64}$/.test(e.id) && typeof e.path === 'string' && e.path.length <= 8192 && typeof e.status === 'string')) {
  applySnapshot({ ...identity, ...cached, version: 0, cached: true, checking: true,
    entries: cached.entries.map(e => ({ id: e.id, path: e.path, status: e.status, unsaved: !!e.unsaved, pending: true })) });
}
vscode.postMessage({ type: 'ready', drafts: editing.state() });
