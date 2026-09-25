// Modified for Overseer from Branch Diff (Local) review/browser.js (MIT): adds the
// comparison/base control, Follow (off/following/paused), agent-edit reveal, restores
// the saved scroll anchor once the rows above it have rendered (after reload/restart), and
// per-hunk Accept (mark reviewed) / Reject (restore the base text through the native edit path).
import './browser.css';
import { StatisticsWorker } from './statistics-client';
import { EditingClient, replaceText } from './editing-client';

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
  // Overseer: a hidden or zero-height view clamps scrollTop to 0; keep the last real position.
  if (document.visibilityState === 'hidden' || !diffs.clientHeight) return;
  pendingState = { ...identity, layout: layout.value, closedFiles: [...closedFiles], closedFolders: [...closedFolders],
    navWidth, navCollapsed: document.body.classList.contains('nav-collapsed'), filter: filter.value,
    selected, scrollTop: diffs.scrollTop, anchor: restoreGoal ? { id: restoreGoal.id, offset: restoreGoal.offset } : anchor(), hierarchy };
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
  select(id); row.header.focus({ preventScroll: true }); updateViewport(); persist();
}
function renderTree() {
  const root = { dirs: new Map(), files: [] };
  const query = filter.value.toLocaleLowerCase();
  for (const entry of snapshot?.entries || []) {
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
    for (const entry of dir.files) {
      const button = node('button', 'file' + (entry.id === selected ? ' active' : ''));
      button.dataset.id = entry.id; button.title = entry.path + (entry.unsaved ? ' (unsaved)' : '');
      button.setAttribute('role', 'treeitem'); button.setAttribute('aria-selected', String(entry.id === selected));
      button.append(node('span', 'file-name', entry.path.split('/').pop()), node('span', 'status status-' + entry.status, entry.status + (entry.unsaved ? ' •' : '')));
      button.addEventListener('click', () => jump(entry.id)); parent.append(button);
    }
  };
  const navigator = document.getElementById('navigator');
  const scrollTop = navigator.scrollTop;
  const fragment = document.createDocumentFragment(); populate(root, fragment, '');
  tree.replaceChildren(fragment); navigator.scrollTop = scrollTop;
}
filter.addEventListener('input', () => { renderTree(); persist(); });
for (const type of ['wheel', 'pointermove', 'pointerdown', 'keydown']) document.getElementById('navigator').addEventListener(type, () => { navigatorTouched = Date.now(); }, { passive: true });
function fold(row) {
  const closed = closedFiles.has(row.entry.id);
  if (row.element.classList.contains('collapsed') === closed) { if (!closed) ensure(row); return; }
  const position = row.element.classList.contains('collapsed') !== closed ? anchor() : undefined;
  if (position?.id === row.entry.id && closed) position.offset = 0;
  row.element.classList.toggle('collapsed', closed);
  row.toggle.textContent = closed ? '▸' : '▾'; row.toggle.setAttribute('aria-expanded', String(!closed));
  if (closed) release(row); else ensure(row);
  restoreAnchor(position);
}
function makeRow(entry) {
  const element = node('article', 'diff-file'); element.dataset.id = entry.id;
  const header = node('header', 'file-header'); header.tabIndex = -1;
  const toggle = node('button', 'fold', '▾'); toggle.setAttribute('aria-label', 'Collapse or expand ' + entry.path); toggle.setAttribute('aria-expanded', 'true');
  const title = node('a', 'file-path', entry.path); title.setAttribute('role', 'link');
  const status = node('span', 'status'); const unsaved = node('span', 'unsaved');
  const stats = node('span', 'stats', '…');
  const open = node('button', 'open-native', 'Open in Native Diff');
  const save = node('button', 'save-file', 'Save'); save.disabled = true;
  const editStatus = node('span', 'edit-status'); editStatus.setAttribute('role', 'status');
  const host = node('div', 'diff-body'); host.style.height = '220px';
  header.append(toggle, status, title, unsaved, editStatus, stats, save, open); element.append(header, host);
  const progress = node('span', 'file-loading'); progress.setAttribute('role', 'status'); progress.hidden = true; header.insertBefore(progress, stats);
  const row = { entry, element, header, toggle, title, status, unsaved, stats, host, progress, open, save, editStatus, nearby: false };
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
function relevant(row) { return !stopped && rows.get(row.entry.id) === row && row.nearby && !closedFiles.has(row.entry.id); }
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
  if (!stats || stats.reason) {
    row.stats.textContent = '—'; delete row.element.dataset.additions; delete row.element.dataset.deletions;
  } else {
    row.stats.replaceChildren(node('span', 'additions', '+' + stats.additions), node('span', 'deletions', '−' + stats.deletions));
    row.element.dataset.additions = stats.additions; row.element.dataset.deletions = stats.deletions;
  }
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
  return { readOnly: readonly, domReadOnly: readonly, originalEditable: false, renderSideBySide: layout.value === 'split',
    useInlineViewWhenSpaceIsLimited: false, renderSideBySideInlineBreakpoint: 0,
    renderOverviewRuler: false, renderMarginRevertIcon: false, renderGutterMenu: false,
    diffAlgorithm: 'advanced', ignoreTrimWhitespace: false, maxComputationTime: 5000,
    hideUnchangedRegions: { enabled: true, contextLineCount: 3, minimumLineCount: 8, revealLineCount: 20 },
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
  const reviewedRanges = [];
  const canEdit = editing.enabled(row);
  changes.forEach((change, index) => {
    const { orig, mod, key } = hunkTexts(row, change);
    const reviewed = reviewedHunks.has(key);
    const dom = node('div', 'hunk-actions' + (reviewed ? ' reviewed' : ''));
    dom.dataset.key = key; dom.dataset.hunk = String(index + 1);
    const where = change.modifiedEndLineNumber ? `lines ${change.modifiedStartLineNumber}–${change.modifiedEndLineNumber}` : `deletion after line ${change.modifiedStartLineNumber}`;
    dom.setAttribute('role', 'group'); dom.setAttribute('aria-label', `Hunk ${index + 1} of ${row.entry.path}, ${where}`);
    if (reviewed) { const badge = node('span', 'hunk-badge', '✓'); badge.title = 'Reviewed'; dom.append(badge); }
    const accept = node('button', 'hunk-accept', reviewed ? '○' : '✓');
    accept.title = reviewed ? 'Unmark: this hunk is reviewed; mark it not reviewed' : 'Accept: keep this change and mark the hunk reviewed (no Git staging)';
    accept.setAttribute('aria-label', reviewed ? `Unmark reviewed hunk ${index + 1}` : `Accept hunk ${index + 1}`);
    accept.addEventListener('click', () => vscode.postMessage({ type: 'hunkReview', reviewed: !reviewed, key, path: row.entry.path, version: snapshot?.version,
      modifiedStart: change.modifiedStartLineNumber, modifiedEnd: change.modifiedEndLineNumber, modified: mod, anchor: row.modified.getLineContent(Math.max(1, Math.min(change.modifiedStartLineNumber || 1, row.modified.getLineCount()))) }));
    const reject = node('button', 'hunk-reject', '↶');
    reject.disabled = !canEdit;
    reject.title = canEdit ? 'Reject: restore this hunk to the comparison base (undo with Cmd+Z in the native editor)' : 'Reject is unavailable: this file cannot be edited in the review (see Open in Native Diff)';
    reject.setAttribute('aria-label', `Reject hunk ${index + 1}`);
    reject.addEventListener('click', () => rejectHunk(row, change, key));
    dom.append(accept, reject);
    const widget = { getId: () => `overseer.hunk.${row.entry.id}.${index}`, getDomNode: () => dom, getPosition: () => null };
    editor.addOverlayWidget(widget);
    row.hunks.push({ widget, dom, change, key });
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
function theme() {
  if (!monaco) return;
  const high = document.body.classList.contains('vscode-high-contrast') || document.body.classList.contains('vscode-high-contrast-light');
  const light = document.body.classList.contains('vscode-light') || document.body.classList.contains('vscode-high-contrast-light');
  const css = getComputedStyle(document.body); const colors = {};
  for (const key of ['editor.background', 'editor.foreground', 'editorLineNumber.foreground', 'editor.selectionBackground',
    'diffEditor.insertedTextBackground', 'diffEditor.removedTextBackground', 'diffEditor.insertedLineBackground', 'diffEditor.removedLineBackground']) {
    const value = css.getPropertyValue('--vscode-' + key.replaceAll('.', '-')).trim();
    if (/^#[\da-f]{6}([\da-f]{2})?$/i.test(value)) colors[key] = value;
  }
  monaco.editor.defineTheme('branch-diff', { base: high ? (light ? 'hc-light' : 'hc-black') : (light ? 'vs' : 'vs-dark'), inherit: true, rules: [], colors });
  monaco.editor.setTheme('branch-diff');
}
function updateSettings(value) {
  const thresholdChanged = settings.largeDiffThreshold !== value?.largeDiffThreshold || settings.enableEditing !== value?.enableEditing;
  settings = value || {}; theme();
  if (thresholdChanged) for (const row of rows.values()) { if (row.classification && !manual.has(row.entry.id)) row.renderedRevision = undefined; }

  for (const row of rows.values()) if (row.editor) { row.editor.updateOptions(options(row)); editing.update(row); resize(row); }
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
      const oldStructure = snapshot?.entries.map(e => [e.id, e.path, e.status, e.unsaved]);
      snapshot = next;
      Object.assign(identity, { repository: next.repository, mode: next.mode, target: next.target, runId: next.overseer?.runId || identity.runId });
      updateSettings(next.settings);
      message(next.error || next.warning || (stickyNotice && Date.now() < stickyNotice.until ? stickyNotice.text : ''));
      const description = next.description;
      if (!next.overseer) document.getElementById('comparison').textContent = description ? `${description.headName || 'HEAD'} → ${description.base}` : 'Branch Diff';
      if (!next.overseer) document.getElementById('comparison').title = description ? `Merge-base ${description.mergeBase} → ${next.mode === 'workingTree' ? 'working tree + unsaved edits' : description.headSha}` : '';
      total.textContent = `${next.entries.length} ${next.checking ? 'files found' : 'changed ' + (next.entries.length === 1 ? 'file' : 'files')}`;
      total.dataset.count = next.entries.length;
      document.body.dataset.checking = String(!!next.checking);
      document.body.dataset.cached = String(!!next.cached);
      document.getElementById('loading-stage').textContent = next.cached ? 'Checking for changes…' : next.checking ? 'Checking files…' : '';
      const ids = new Set(next.entries.map(e => e.id));
      for (const [id, row] of rows) if (!ids.has(id)) { release(row); row.element.remove(); rows.delete(id); }
      diffs.querySelector('.empty')?.remove();
      // Metadata-only validation updates reuse the existing navigator DOM.
      if (JSON.stringify(oldStructure) !== JSON.stringify(next.entries.map(e => [e.id, e.path, e.status, e.unsaved]))) renderTree();
      let previous = null, batchStart = performance.now();
      for (let index = 0; index < next.entries.length; index++) {
        const entry = next.entries[index];
        let row = rows.get(entry.id);
        if (!row) { row = makeRow(entry); rows.set(entry.id, row); }
        row.entry = entry;
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
      if (!next.entries.length) diffs.append(node('p', 'empty', next.error ? 'Comparison unavailable: ' + next.error : next.checking ? 'Checking files…' : 'No changes for this comparison. Pre-existing dirty work stays listed in the Workspace Dirty view.'));
      if (restoring) {
        diffs.scrollTop = saved.scrollTop || 0; restoreAnchor(saved.anchor); pursueRestore(); restoring = false;
      } else { restoreAnchor(previousAnchor); pursueRestore(); }
      if (pendingJump && rows.has(pendingJump)) { const id = pendingJump; pendingJump = undefined; jump(id); }
      if (pendingReveal) applyReveal(pendingReveal);
      document.body.dataset.version = next.version;
      document.body.dataset.hierarchyReady ||= String(performance.now());
      if (!next.cached && !next.checking && !next.error) {
        const cached = { description: next.description, entries: next.entries.map(e => ({ id: e.id, path: e.path, status: e.status, unsaved: e.unsaved })) };
        hierarchy = cached.entries.length <= 10000 && JSON.stringify(cached).length <= 2 * 1024 * 1024 ? cached : undefined;
      }
      updateViewport(); trackScroll();
    }
  } finally { rendering = false; }
}

// ---- Overseer: comparison label, Follow state and reveal of agent edits.
const followBox = document.getElementById('follow');
const resumeButton = document.getElementById('resume');
const followStatus = document.getElementById('follow-state');
let followState = 'off', pendingReveal;
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
  document.getElementById('comparison').textContent = (o.runTitle || 'Run') + (o.harness ? ' · ' + o.harness : '');
  document.getElementById('comparison').title = o.workspacePath || '';
  document.getElementById('workspace-note').textContent = (o.workspaceKind === 'current' ? 'Current checkout: ' : 'Worktree: ') + (o.workspacePath || '');
  followState = o.follow || 'off';
  followBox.checked = followState !== 'off';
  resumeButton.hidden = followState !== 'paused';
  followStatus.textContent = followState === 'paused' ? (/paused/.test(o.followNote || '') ? o.followNote : 'Follow paused by your navigation') : followState === 'following' ? (o.followNote || 'Following agent edits') : '';
}
function userNavigated(reason) {
  if (followState !== 'following') return;
  followState = 'paused'; resumeButton.hidden = false; followStatus.textContent = 'Follow paused by your navigation';
  vscode.postMessage({ type: 'followPause', reason });
}
followBox.addEventListener('change', () => vscode.postMessage({ type: 'follow', enabled: followBox.checked }));
resumeButton.addEventListener('click', () => vscode.postMessage({ type: 'followResume' }));
document.getElementById('base').addEventListener('click', () => vscode.postMessage({ type: 'pickComparison' }));
diffs.addEventListener('wheel', () => userNavigated('scroll'), { passive: true });
diffs.addEventListener('touchstart', () => userNavigated('scroll'), { passive: true });
diffs.addEventListener('mousedown', event => { if (!event.target.closest('button')) userNavigated('pointer'); });
diffs.addEventListener('keydown', event => { if (!['Shift', 'Control', 'Alt', 'Meta'].includes(event.key)) userNavigated('keyboard'); });
tree.addEventListener('click', () => userNavigated('file selection'), true);
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
  if (followState !== 'following' && !value.user) return;
  const id = value.id || snapshot?.entries.find(e => e.path === value.path)?.id;
  if (!id || !rows.has(id)) { pendingReveal = value; return; }
  pendingReveal = undefined;
  const row = rows.get(id);
  if (value.user) { userNavigated('conversation'); jump(id); document.body.dataset.revealed = value.path + ':' + (value.line || ''); if (!revealLine(row, value.line)) row.pendingLine = value.line; return; }
  jump(id);
  followStatus.textContent = 'Following: ' + value.path + (value.line ? ':' + value.line : '') + (value.attribution ? ' (' + value.attribution + ')' : '');
  if (!revealLine(row, value.line)) row.pendingLine = value.line;
}
window.addEventListener('message', event => {
  const value = event.data;
  if (!value || typeof value !== 'object') return;
  if (editing.receive(value)) return;
  if (value.type === 'snapshot') { applyOverseer(value.overseer); applySnapshot(value); }
  else if (value.type === 'overseer') applyOverseer(value.overseer);
  else if (value.type === 'reveal') applyReveal(value);
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
  else if (value.type === 'hunkReviewed') { if (value.reviewed) reviewedHunks.add(value.key); else reviewedHunks.delete(value.key); for (const row of rows.values()) if (row.hunks?.some(h => h.key === value.key)) renderHunks(row); }
  else if (value.type === 'notice') stickyMessage(value.message);
});
layout.addEventListener('change', () => { updateSettings(settings); persist(); });
document.getElementById('refresh').addEventListener('click', () => vscode.postMessage({ type: 'refresh' }));
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
