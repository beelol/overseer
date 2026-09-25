// Overseer view (command center) — agents column. Tasks from every repository the daemon
// knows (not only the folder open in this window) → top-level runs → native descendants,
// as an accessible tree (role=tree, roving focus, arrow keys) with live status. Selecting a
// run opens its live review and its conversation in the columns to the right.
(function () {
  const vscode = acquireVsCodeApi();
  const saved = vscode.getState() || {};
  const collapsed = new Set(saved.collapsed || []);
  let state = { tasks: [], runs: [], workspaces: [], profiles: [] }, selected = saved.selected, focusId = saved.focus;
  const tree = document.getElementById('tree');
  const ACTIVE = new Set(['queued', 'starting', 'running', 'waiting_for_user']);
  const STATUS = { queued: 'queued', starting: 'starting', running: 'running', waiting_for_user: 'waiting for you', completed: 'completed', failed: 'failed', interrupted: 'interrupted', disconnected: 'disconnected', unknown: 'unknown' };
  const persist = () => vscode.setState({ collapsed: [...collapsed], selected, focus: focusId });
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text !== undefined) e.textContent = text; return e; };

  function children(runId) { return state.runs.filter(r => r.parent_run_id === runId); }
  function render() {
    const byRepo = new Map();
    for (const t of [...state.tasks].sort((a, b) => b.created_ms - a.created_ms)) {
      if (!byRepo.has(t.repo_root)) byRepo.set(t.repo_root, []);
      byRepo.get(t.repo_root).push(t);
    }
    const frag = document.createDocumentFragment();
    const rows = [];
    const add = (id, level, label, opts) => {
      const row = el('div', 'row ' + (opts.cls || ''));
      row.setAttribute('role', 'treeitem'); row.setAttribute('aria-level', String(level));
      row.dataset.id = id; row.dataset.level = level; row.style.setProperty('--level', level - 1);
      if (opts.expandable) row.setAttribute('aria-expanded', String(!collapsed.has(id)));
      if (opts.run) { row.dataset.run = opts.run; row.setAttribute('aria-selected', String(selected === opts.run)); }
      row.tabIndex = -1;
      const twisty = el('span', 'twisty', opts.expandable ? (collapsed.has(id) ? '▸' : '▾') : '');
      twisty.setAttribute('aria-hidden', 'true');
      const dot = el('span', 'dot ' + (opts.status ? 'status-' + opts.status : 'none')); dot.setAttribute('aria-hidden', 'true');
      const main = el('span', 'label', label);
      const desc = el('span', 'desc', opts.desc || '');
      row.append(twisty, dot, main, desc);
      if (opts.badge) row.append(el('span', 'badge ' + (opts.badgeCls || ''), opts.badge));
      row.setAttribute('aria-label', [label, opts.desc, opts.status && STATUS[opts.status], opts.badge].filter(Boolean).join(', '));
      frag.append(row); rows.push(row);
      return !opts.expandable || !collapsed.has(id);
    };
    for (const [repo, tasks] of byRepo) {
      const repoId = 'repo:' + repo;
      const active = tasks.reduce((n, t) => n + state.runs.filter(r => r.task_id === t.id && !r.parent_run_id && ACTIVE.has(r.status)).length, 0);
      const open = add(repoId, 1, repo.split('/').pop(), { expandable: true, cls: 'repo', desc: repo, badge: active ? `${active} active` : '' });
      if (!open) continue;
      for (const task of tasks) {
        const ws = state.workspaces.find(w => w.id === task.workspace_id);
        const roots = state.runs.filter(r => r.task_id === task.id && !r.parent_run_id);
        const taskId = 'task:' + task.id;
        const status = roots.find(r => ACTIVE.has(r.status))?.status || roots[roots.length - 1]?.status;
        if (!add(taskId, 2, task.title, { expandable: roots.length > 0, cls: 'task', status, desc: ws ? (ws.kind === 'current' ? 'current checkout' : ws.branch) + (ws.removed_ms ? ' · removed' : '') : '' })) continue;
        const walk = (run, level) => {
          const kids = children(run.id);
          const profile = state.profiles.find(p => p.id === run.profile_id);
          const label = run.parent_run_id ? run.title : run.harness;
          const desc = run.parent_run_id ? `native child${String(run.relation_confidence || '').startsWith('exact') ? '' : ' (inferred)'}` : [profile?.name, run.model].filter(Boolean).join(' · ');
          const needs = run.attention?.kind === 'permission';
          if (add('run:' + run.id, level, label, { expandable: kids.length > 0, cls: 'run', run: run.id, status: run.status, desc, badge: needs ? 'needs you' : '', badgeCls: needs ? 'attention' : '' })) {
            for (const k of kids) walk(k, level + 1);
          }
        };
        for (const r of roots) walk(r, 3);
      }
    }
    // Live updates re-render the tree; keep keyboard focus on the same row.
    const hadFocus = tree.contains(document.activeElement);
    tree.replaceChildren(frag);
    document.getElementById('empty').hidden = rows.length > 0;
    const focusRow = rows.find(r => r.dataset.id === focusId) || rows.find(r => r.getAttribute('aria-selected') === 'true') || rows[0];
    if (focusRow) { focusRow.tabIndex = 0; if (hadFocus) focusRow.focus({ preventScroll: true }); }
  }
  function rowsList() { return [...tree.querySelectorAll('.row')]; }
  function focusRow(row) {
    if (!row) return;
    for (const r of rowsList()) r.tabIndex = -1;
    row.tabIndex = 0; row.focus(); focusId = row.dataset.id; persist();
  }
  function toggle(row, open) {
    if (!row.hasAttribute('aria-expanded')) return false;
    const isOpen = !collapsed.has(row.dataset.id);
    if (open === isOpen) return false;
    if (isOpen) collapsed.add(row.dataset.id); else collapsed.delete(row.dataset.id);
    focusId = row.dataset.id; persist(); render();
    tree.querySelector(`[data-id="${CSS.escape(focusId)}"]`)?.focus();
    return true;
  }
  function select(row) {
    if (!row.dataset.run) { toggle(row); return; }
    selected = row.dataset.run; persist();
    for (const r of rowsList()) r.setAttribute('aria-selected', String(r.dataset.run === selected));
    vscode.postMessage({ type: 'select', runId: selected });
  }
  tree.addEventListener('click', e => {
    const row = e.target.closest('.row'); if (!row) return;
    focusRow(row);
    if (e.target.classList.contains('twisty')) toggle(row); else select(row);
  });
  tree.addEventListener('keydown', e => {
    const row = e.target.closest('.row'); if (!row) return;
    const list = rowsList(), i = list.indexOf(row);
    if (e.key === 'ArrowDown') { focusRow(list[i + 1]); e.preventDefault(); }
    else if (e.key === 'ArrowUp') { focusRow(list[i - 1]); e.preventDefault(); }
    else if (e.key === 'ArrowRight') { if (!toggle(row, true)) focusRow(list[i + 1]); e.preventDefault(); }
    else if (e.key === 'ArrowLeft') {
      if (!toggle(row, false)) { const level = Number(row.dataset.level); focusRow(list.slice(0, i).reverse().find(r => Number(r.dataset.level) < level)); }
      e.preventDefault();
    } else if (e.key === 'Enter' || e.key === ' ') { select(row); e.preventDefault(); }
    else if (e.key === 'Home') { focusRow(list[0]); e.preventDefault(); }
    else if (e.key === 'End') { focusRow(list[list.length - 1]); e.preventDefault(); }
  });
  document.getElementById('new-task').addEventListener('click', () => vscode.postMessage({ type: 'newTask' }));
  document.getElementById('refresh').addEventListener('click', () => vscode.postMessage({ type: 'refresh' }));
  document.getElementById('collapse').addEventListener('click', () => { for (const r of rowsList()) if (r.hasAttribute('aria-expanded') && r.dataset.level !== '1') collapsed.add(r.dataset.id); persist(); render(); });
  window.addEventListener('message', e => {
    const m = e.data;
    if (m.type === 'state') { state = m.state; if (m.selected) selected = m.selected; render(); document.body.dataset.ready = '1'; }
    else if (m.type === 'selected') { selected = m.runId; persist(); render(); }
  });
  vscode.postMessage({ type: 'ready' });
})();
