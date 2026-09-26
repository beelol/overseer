// Overseer view — Files pane (AC-51): the selected run's worktree as a lazily loaded file tree
// (one directory per request, so large repositories stay responsive). Entries changed since the
// task started are marked (A/M/D/R; folders show how many changes they contain). Clicking a file
// opens it in the editor, whatever folder the window has open. Accessible tree with arrow keys.
(function () {
  const vscode = window.overseerApi;
  const box = document.getElementById('files');
  const title = document.getElementById('files-for');
  const note = document.getElementById('files-note');
  const saved = (vscode.getState() || {}).files || {};
  let runId, runs = [], workspaces = [];
  const expanded = new Map(Object.entries(saved.expanded || {}).map(([k, v]) => [k, new Set(v)])); // runId -> Set(dir)
  const cache = new Map(); // dir -> listing, for the current run
  let focusPath, lastLoad = 0;
  const ui = window.OverseerUI;
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text !== undefined) e.textContent = text; return e; };
  const STATUS = { A: 'added', M: 'modified', D: 'deleted', R: 'renamed', T: 'type changed', U: 'conflicted' };
  const persist = () => vscode.setState({ ...(vscode.getState() || {}), files: { expanded: Object.fromEntries([...expanded].map(([k, v]) => [k, [...v]])) } });
  const open = () => { if (!expanded.has(runId)) expanded.set(runId, new Set()); return expanded.get(runId); };

  function request(dir) { vscode.postMessage({ type: 'tree', runId, dir }); }
  function reload() { lastLoad = Date.now(); request(''); for (const d of open()) request(d); }

  function show(id) {
    if (!id || id === runId) return;
    runId = id; cache.clear(); focusPath = undefined; document.body.dataset.filesReady = ''; box.dataset.run = id;
    const run = runs.find(r => r.id === id);
    const ws = run && workspaces.find(w => w.id === run.workspace_id);
    title.textContent = ws ? (ws.kind === 'current' ? 'Current checkout' : ui.basename(ws.branch || ws.path)) : '';
    title.title = ws ? `${ws.branch || ''}\n${ws.path}` : '';
    box.setAttribute('aria-label', `Files in the worktree of ${run?.title || 'the selected run'}`);
    box.replaceChildren(); note.textContent = 'Loading…';
    reload();
  }
  function onState(state, selected) {
    runs = state.runs; workspaces = state.workspaces;
    if (selected && selected !== runId) { show(selected); return; }
    // Live runs change files: refresh open folders at most every 2 s.
    const run = runs.find(r => r.id === runId);
    if (run && ['running', 'starting', 'waiting_for_user'].includes(run.status) && Date.now() - lastLoad > 2000) reload();
  }

  function render() {
    const rows = [];
    const frag = document.createDocumentFragment();
    const walk = (dir, level) => {
      const listing = cache.get(dir);
      if (!listing) { if (dir) { const r = el('div', 'row muted', '…'); r.style.setProperty('--level', level - 1); frag.append(r); } return; }
      for (const e of listing.entries) {
        const row = el('div', 'row file' + (e.deleted ? ' deleted' : '') + (e.status ? ' changed' : ''));
        row.setAttribute('role', 'treeitem'); row.setAttribute('aria-level', String(level)); row.tabIndex = -1;
        row.dataset.path = e.path; row.dataset.level = level; row.style.setProperty('--level', level - 1);
        if (e.dir) { row.dataset.dir = '1'; row.setAttribute('aria-expanded', String(open().has(e.path))); }
        const twisty = el('span', 'twisty'); if (e.dir) twisty.append(ui.icon(open().has(e.path) ? 'chevron-down' : 'chevron-right', 'xs')); twisty.setAttribute('aria-hidden', 'true');
        const icon = el('span', 'ficon'); icon.append(ui.icon(e.dir ? (open().has(e.path) ? 'folder-opened' : 'folder') : 'file', 'sm')); icon.setAttribute('aria-hidden', 'true');
        const name = el('span', 'label', e.name);
        row.append(twisty, icon, name);
        if (e.status) row.append(el('span', 'fstatus st-' + e.status, e.status));
        else if (e.changes_inside) row.append(el('span', 'fcount', String(e.changes_inside)));
        row.setAttribute('aria-label', [e.name, e.dir ? 'folder' : 'file', e.status && STATUS[e.status], e.changes_inside ? `${e.changes_inside} changed inside` : ''].filter(Boolean).join(', '));
        row.title = e.path + (e.status ? ` (${STATUS[e.status] || e.status})` : '');
        frag.append(row); rows.push(row);
        if (e.dir && open().has(e.path)) walk(e.path, level + 1);
      }
      if (listing.truncated) { const r = el('div', 'row muted', `+${listing.total - listing.entries.filter(x => !x.deleted).length} more`); r.title = `${listing.total - listing.entries.filter(x => !x.deleted).length} more entries not shown`; r.style.setProperty('--level', level - 1); frag.append(r); }
    };
    walk('', 1);
    const hadFocus = box.contains(document.activeElement);
    box.replaceChildren(frag);
    const root = cache.get('');
    note.textContent = !root ? 'Loading…' : root.changed_total ? `${root.changed_total} changed` : 'No changes';
    note.title = 'Since the task started';
    const f = rows.find(r => r.dataset.path === focusPath) || rows[0];
    if (f) { f.tabIndex = 0; if (hadFocus) f.focus({ preventScroll: true }); }
    document.body.dataset.filesReady = root ? '1' : '';
  }
  function rowsList() { return [...box.querySelectorAll('.row[role=treeitem]')]; }
  function focus(row) { if (!row) return; for (const r of rowsList()) r.tabIndex = -1; row.tabIndex = 0; row.focus(); focusPath = row.dataset.path; }
  function toggle(row, want) {
    if (!row.dataset.dir) return false;
    const set = open(), isOpen = set.has(row.dataset.path);
    if (want === isOpen) return false;
    if (isOpen) set.delete(row.dataset.path); else { set.add(row.dataset.path); if (!cache.has(row.dataset.path)) request(row.dataset.path); }
    focusPath = row.dataset.path; persist(); render();
    box.querySelector(`[data-path="${CSS.escape(focusPath)}"]`)?.focus();
    return true;
  }
  function activate(row) {
    if (row.dataset.dir) { toggle(row); return; }
    vscode.postMessage({ type: 'openFile', runId, path: row.dataset.path });
  }
  box.addEventListener('click', e => { const row = e.target.closest('.row[role=treeitem]'); if (!row) return; focus(row); activate(row); });
  box.addEventListener('keydown', e => {
    const row = e.target.closest('.row[role=treeitem]'); if (!row) return;
    const list = rowsList(), i = list.indexOf(row);
    if (e.key === 'ArrowDown') focus(list[i + 1]);
    else if (e.key === 'ArrowUp') focus(list[i - 1]);
    else if (e.key === 'ArrowRight') { if (!toggle(row, true)) focus(list[i + 1]); }
    else if (e.key === 'ArrowLeft') { if (!toggle(row, false)) { const lv = Number(row.dataset.level); focus(list.slice(0, i).reverse().find(r => Number(r.dataset.level) < lv)); } }
    else if (e.key === 'Enter' || e.key === ' ') activate(row);
    else return;
    e.preventDefault();
  });
  document.getElementById('files-refresh').addEventListener('click', () => { if (runId) reload(); });
  window.addEventListener('message', e => {
    const m = e.data;
    if (m.type === 'treeData' && m.runId === runId) { cache.set(m.dir, m.data); render(); }
    else if (m.type === 'treeError' && m.runId === runId) { note.textContent = m.message; if (!m.dir) box.replaceChildren(); }
  });
  window.overseerFiles = { show, onState };
})();
