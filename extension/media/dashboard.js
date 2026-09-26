// Overseer dashboard webview (AC-48, AC-54..AC-63): the agents rail (needs you, then agents by
// repository), and in the main area the selected agent's chat, the new-agent composer (nothing
// selected) or the agent grid. The files panel opens on the right. State survives reloads.
(function () {
  const vscode = acquireVsCodeApi();
  window.overseerApi = vscode; // shared with files.js
  const ui = window.OverseerUI, el = ui.el;
  const post = m => vscode.postMessage(m);
  const saved = vscode.getState() || {};
  const collapsed = new Set(saved.collapsed || []);
  let state = { tasks: [], runs: [], workspaces: [], profiles: [], accounts: [], attention: [], pinned: [], gridMax: 6, archived: [] };
  let selected = saved.selected, mode = saved.mode || (saved.selected ? 'chat' : 'composer'), filesOpen = !!saved.filesOpen, query = saved.query || '', focusId = saved.focus;
  let showArchived = false, searchHits = null;
  const ACTIVE = new Set(['queued', 'starting', 'running', 'waiting_for_user']);
  const persist = () => vscode.setState({ ...(vscode.getState() || {}), collapsed: [...collapsed], selected, mode, filesOpen, query, focus: focusId, chat: chat && chat.state() });

  // ---------- Layout ----------
  const rail = el('aside', 'rail'); rail.dataset.auditView = 'agents'; rail.setAttribute('aria-label', 'Agents');
  const railHead = el('div', 'rail-head');
  const newBtn = ui.iconButton('add', 'New agent', { action: 'new-agent', shortcut: '⌥⌘N' });
  const gridBtn = ui.iconButton('layout', 'Grid', { action: 'grid', pressed: mode === 'grid', shortcut: '⌥⌘G' });
  const moreBtn = ui.iconButton('ellipsis', 'More', { action: 'rail-more' }); moreBtn.setAttribute('aria-haspopup', 'menu');
  const search = el('label', 'search'); search.append(ui.icon('search', 'sm'));
  const searchInput = el('input'); searchInput.type = 'search'; searchInput.placeholder = 'Search'; searchInput.setAttribute('aria-label', 'Search agents'); searchInput.value = query;
  search.append(searchInput);
  railHead.append(search, newBtn, gridBtn, moreBtn);
  const list = el('div', 'rail-list'); list.setAttribute('role', 'tree'); list.setAttribute('aria-label', 'Agents in all repositories');
  const railFoot = el('div', 'rail-foot');
  const acctBtn = el('button', 'rail-accounts'); acctBtn.type = 'button'; acctBtn.dataset.action = 'accounts'; acctBtn.setAttribute('aria-haspopup', 'menu');
  railFoot.append(acctBtn);
  rail.append(railHead, list, railFoot);

  const main = el('main', 'main');
  const chatHost = el('section', 'view-chat');
  const composerHost = el('section', 'view-composer'); composerHost.dataset.auditView = 'composer';
  const gridHost = el('section', 'view-grid'); gridHost.dataset.auditView = 'grid'; gridHost.setAttribute('aria-label', 'Agent grid');
  const filesPanel = el('aside', 'files-panel'); filesPanel.dataset.auditView = 'files'; filesPanel.setAttribute('aria-label', 'Files');
  filesPanel.innerHTML = '';
  const filesHead = el('div', 'files-head');
  const filesTitle = el('span', 'files-title ellipsis'); filesTitle.id = 'files-for';
  const filesRefresh = ui.iconButton('refresh', 'Refresh files', { cls: 'sm' }); filesRefresh.id = 'files-refresh';
  const filesClose = ui.iconButton('close', 'Close files', { cls: 'sm' });
  filesHead.append(ui.icon('list-tree', 'sm'), filesTitle, filesRefresh, filesClose);
  const filesNote = el('p', 'files-note'); filesNote.id = 'files-note'; filesNote.setAttribute('role', 'status');
  const filesTree = el('div', 'files-tree'); filesTree.id = 'files'; filesTree.setAttribute('role', 'tree'); filesTree.setAttribute('aria-label', 'Worktree files');
  filesPanel.append(filesHead, filesNote, filesTree);
  main.append(chatHost, composerHost, gridHost);
  const body = el('div', 'dash'); body.append(rail, main, filesPanel);
  document.body.append(body);

  const chat = new window.OverseerChat(chatHost, { post: m => post({ ...m, runId: m.runId || selected, scope: 'chat' }), mode: 'dashboard', onState: persist,
    onFiles: () => setFiles(!filesOpen) });

  // ---------- Rail ----------
  const byId = (arr, id) => arr.find(x => x.id === id);
  const runOf = t => state.runs.filter(r => r.task_id === t.id && !r.parent_run_id).sort((a, b) => b.created_ms - a.created_ms)[0];
  const matches = (t, r) => {
    if (!query) return true;
    if (searchHits && searchHits.has(t.id)) return true;
    const q = query.toLowerCase();
    const p = r && byId(state.profiles, r.profile_id);
    return [t.title, t.prompt, t.repo_root, r?.harness, r?.status && ui.statusText(r.status), p?.name, r?.model].some(v => v && String(v).toLowerCase().includes(q));
  };
  let rows = [];
  function addRow(frag, { id, level, cls, label, run, task, status, attention, meta, twisty, title, badge }) {
    const row = el('div', 'row ' + (cls || ''));
    row.setAttribute('role', 'treeitem'); row.setAttribute('aria-level', String(level));
    row.dataset.id = id; row.dataset.level = level; row.style.setProperty('--level', level - 1); row.tabIndex = -1;
    if (run) { row.dataset.run = run; row.setAttribute('aria-selected', String(selected === run && mode === 'chat')); }
    if (task) row.dataset.task = task;
    if (twisty !== undefined) { row.setAttribute('aria-expanded', String(!collapsed.has(id))); }
    const tw = el('span', 'twisty'); if (twisty) tw.append(ui.icon(collapsed.has(id) ? 'chevron-right' : 'chevron-down', 'xs')); tw.setAttribute('aria-hidden', 'true');
    if (level > 1 || twisty) row.append(tw);
    if (status) row.append(ui.status(status, attention));
    const t = el('span', 'title', label); row.append(t);
    if (badge) row.append(badge);
    if (meta) row.append(meta);
    row.title = title || label;
    row.setAttribute('aria-label', [label, status && ui.statusText(status), title && title !== label ? '' : ''].filter(Boolean).join(', '));
    frag.append(row); rows.push(row);
    return !twisty || !collapsed.has(id);
  }
  function runMeta(r) {
    const m = el('span', 'meta');
    const p = r.profile_id && byId(state.profiles, r.profile_id);
    const time = el('span', 'meta-time', ui.ago(r.ended_ms || r.created_ms));
    time.title = new Date(r.ended_ms || r.created_ms).toLocaleString();
    const mark = el('span', 'meta-mark'); mark.append(ui.harnessMark(r.harness, 12));
    mark.title = [ui.HARNESS[r.harness] || r.harness, p?.name, r.model].filter(Boolean).join(' · ');
    m.append(mark, time);
    return m;
  }
  function renderRail() {
    const frag = document.createDocumentFragment(); rows = [];
    const archivedSet = new Set(state.archived || []);
    const tasks = [...state.tasks].sort((a, b) => b.created_ms - a.created_ms);
    // Needs you: waiting for a decision, failed, or finished with changes not yet reviewed.
    const needs = (state.attention || []).filter(a => { const r = byId(state.runs, a.run_id); const t = r && byId(state.tasks, r.task_id); return t && matches(t, r); });
    if (needs.length) {
      addRow(frag, { id: 'sec:needs', level: 1, cls: 'section needs', label: 'Needs you', twisty: true, badge: Object.assign(el('span', 'badge attention', String(needs.length)), { title: `${needs.length} agent${needs.length === 1 ? '' : 's'} need you` }) });
      if (!collapsed.has('sec:needs')) for (const a of needs) {
        const r = byId(state.runs, a.run_id), t = byId(state.tasks, r.task_id);
        const why = el('span', 'meta'); why.append(el('span', 'why-chip', a.label)); why.title = a.detail || a.label;
        addRow(frag, { id: 'needs:' + r.id, level: 2, cls: 'run needs-row', label: t.title, run: r.id, status: r.status, attention: r.attention?.kind, meta: why, title: `${t.title}\n${a.detail || a.label}` });
      }
    }
    const byRepo = new Map();
    for (const t of tasks) {
      if (archivedSet.has(t.id) !== showArchived) continue;
      const r = runOf(t); if (!matches(t, r)) continue;
      if (!byRepo.has(t.repo_root)) byRepo.set(t.repo_root, []);
      byRepo.get(t.repo_root).push(t);
    }
    for (const [repo, list] of byRepo) {
      const active = list.filter(t => { const r = runOf(t); return r && ACTIVE.has(r.status); }).length;
      const add = ui.iconButton('add', `New agent in ${ui.basename(repo)}`, { cls: 'sm repo-add' }); add.dataset.repo = repo; add.tabIndex = -1;
      const meta = el('span', 'meta'); if (active) meta.append(Object.assign(el('span', 'count', String(active)), { title: `${active} active` })); meta.append(add);
      if (!addRow(frag, { id: 'repo:' + repo, level: 1, cls: 'section repo', label: ui.basename(repo), twisty: true, meta, title: repo })) continue;
      for (const t of list) {
        const r = runOf(t);
        const kids = r ? state.runs.filter(x => x.parent_run_id === r.id) : [];
        const meta = r && runMeta(r);
        if (meta && r && !ACTIVE.has(r.status)) {
          const arch = ui.iconButton(showArchived ? 'discard' : 'archive', showArchived ? 'Restore' : 'Archive', { cls: 'sm row-archive' }); arch.dataset.task = t.id; arch.tabIndex = -1;
          meta.prepend(arch);
        }
        const open = addRow(frag, { id: 'task:' + t.id, level: 2, cls: 'run', label: t.title, run: r?.id, task: t.id, status: r?.status || 'unknown', attention: r?.attention?.kind, meta, twisty: kids.length ? true : undefined,
          title: [t.title, ui.firstLine(t.prompt, 200), r && ui.statusText(r.status)].filter(Boolean).join('\n') });
        if (open) {
          const walk = (parent, level) => { for (const c of state.runs.filter(x => x.parent_run_id === parent.id)) {
            const gk = state.runs.filter(x => x.parent_run_id === c.id);
            if (addRow(frag, { id: 'run:' + c.id, level, cls: 'run child', label: c.title, run: c.id, status: c.status, twisty: gk.length ? true : undefined, title: `${c.title}\nSub-agent of ${parent.title}` })) walk(c, level + 1);
          } };
          walk(r, 3);
        }
      }
    }
    if (!rows.filter(r => r.dataset.run).length) {
      const empty = el('div', 'rail-empty');
      if (query) empty.append(ui.icon('search'), el('div', null, 'No matches'));
      else if (showArchived) empty.append(ui.icon('archive'), el('div', null, 'Nothing archived'));
      else { empty.append(ui.icon('hubot'), el('div', null, 'No agents yet')); const b = el('button', 'btn primary sm', 'Start one'); b.type = 'button'; b.addEventListener('click', () => setMode('composer')); empty.append(b); }
      frag.append(empty);
    }
    const hadFocus = list.contains(document.activeElement);
    list.replaceChildren(frag);
    const f = rows.find(r => r.dataset.id === focusId) || rows.find(r => r.getAttribute('aria-selected') === 'true') || rows[0];
    if (f) { f.tabIndex = 0; if (hadFocus) f.focus({ preventScroll: true }); }
    renderAccounts();
    newBtn.setAttribute('aria-pressed', String(mode === 'composer'));
    gridBtn.setAttribute('aria-pressed', String(mode === 'grid'));
  }
  function renderAccounts() {
    const accts = state.accounts || [];
    const signed = accts.filter(a => a.signedIn).length;
    acctBtn.replaceChildren(ui.icon('account', 'sm'), el('span', 'ellipsis', `${signed}/${accts.length} signed in`), ui.icon('chevron-up', 'xs'));
    acctBtn.title = accts.map(a => `${a.name}: ${a.signedIn ? 'signed in' : 'not signed in'}${a.plan ? ' · ' + a.plan : ''}`).join('\n') || 'No accounts';
    acctBtn.setAttribute('aria-label', `Accounts: ${signed} of ${accts.length} signed in`);
  }

  function rowsList() { return [...list.querySelectorAll('.row')]; }
  function focusRow(row) { if (!row) return; for (const r of rowsList()) r.tabIndex = -1; row.tabIndex = 0; row.focus(); focusId = row.dataset.id; persist(); }
  function toggle(row, open) {
    if (!row.hasAttribute('aria-expanded')) return false;
    const isOpen = !collapsed.has(row.dataset.id);
    if (open === isOpen) return false;
    if (isOpen) collapsed.add(row.dataset.id); else collapsed.delete(row.dataset.id);
    focusId = row.dataset.id; persist(); renderRail();
    list.querySelector(`[data-id="${CSS.escape(focusId)}"]`)?.focus();
    return true;
  }
  function selectRun(runId, { focusChat } = {}) {
    if (!runId) return;
    selected = runId; setMode('chat', { quiet: true });
    for (const r of rowsList()) r.setAttribute('aria-selected', String(r.dataset.run === selected));
    post({ type: 'select', runId });
    window.overseerFiles?.show(selected);
    persist();
    if (focusChat) chat.prompt.focus();
  }
  list.addEventListener('click', e => {
    const add = e.target.closest('.repo-add'); if (add) { e.stopPropagation(); setMode('composer', { repo: add.dataset.repo }); return; }
    const arch = e.target.closest('.row-archive'); if (arch) { e.stopPropagation(); post({ type: 'archive', taskId: arch.dataset.task, archived: !showArchived }); return; }
    const row = e.target.closest('.row'); if (!row) return;
    focusRow(row);
    if (e.target.closest('.twisty') || !row.dataset.run) toggle(row); else selectRun(row.dataset.run);
  });
  list.addEventListener('keydown', e => {
    const row = e.target.closest('.row'); if (!row) return;
    const all = rowsList(), i = all.indexOf(row);
    if (e.key === 'ArrowDown') focusRow(all[i + 1]);
    else if (e.key === 'ArrowUp') focusRow(all[i - 1]);
    else if (e.key === 'ArrowRight') { if (!toggle(row, true)) focusRow(all[i + 1]); }
    else if (e.key === 'ArrowLeft') { if (!toggle(row, false)) { const lv = Number(row.dataset.level); focusRow(all.slice(0, i).reverse().find(r => Number(r.dataset.level) < lv)); } }
    else if (e.key === 'Enter' || e.key === ' ') { if (row.dataset.run) selectRun(row.dataset.run, { focusChat: e.key === 'Enter' && e.metaKey }); else toggle(row); }
    else if ((e.key === 'Delete' || e.key === 'Backspace') && row.dataset.task && !e.metaKey) { const r = byId(state.runs, row.dataset.run); if (r && !ACTIVE.has(r.status)) post({ type: 'archive', taskId: row.dataset.task, archived: !showArchived }); }
    else if (e.key === 'Home') focusRow(all[0]);
    else if (e.key === 'End') focusRow(all[all.length - 1]);
    else return;
    e.preventDefault();
  });
  let searchTimer;
  searchInput.addEventListener('input', () => {
    query = searchInput.value.trim(); persist();
    clearTimeout(searchTimer); searchHits = null; renderRail();
    if (query.length >= 2) searchTimer = setTimeout(() => post({ type: 'search', q: query }), 60);
  });
  searchInput.addEventListener('keydown', e => { if (e.key === 'ArrowDown') { focusRow(rowsList()[0]); e.preventDefault(); } else if (e.key === 'Escape') { searchInput.value = ''; query = ''; searchHits = null; renderRail(); } });
  newBtn.addEventListener('click', () => setMode(mode === 'composer' && selected ? 'chat' : 'composer'));
  gridBtn.addEventListener('click', () => setMode(mode === 'grid' ? (selected ? 'chat' : 'composer') : 'grid'));
  moreBtn.addEventListener('click', () => ui.menu(moreBtn, [
    { label: showArchived ? 'Show active agents' : 'Show archived', icon: 'archive', run: () => { showArchived = !showArchived; renderRail(); } },
    { label: 'Clean up archived worktrees…', icon: 'trash', run: () => post({ type: 'cleanupArchived' }) },
    'sep',
    { label: 'Open dashboard in a new window', icon: 'empty-window', run: () => post({ type: 'dashboardWindow' }) },
    document.body.dataset.dashboard ? { label: 'Exit dashboard', icon: 'screen-normal', hint: '⌥⌘O', run: () => post({ type: 'exitDashboard' }) } : { label: 'Dashboard mode', icon: 'screen-full', hint: '⌥⌘O', run: () => post({ type: 'command', command: 'overseer.openDashboard' }) },
    'sep',
    { label: 'Test notification', icon: 'bell', run: () => post({ type: 'command', command: 'overseer.testNotification' }) },
    { label: 'Stop all agents…', icon: 'debug-stop', danger: true, run: () => post({ type: 'command', command: 'overseer.stopAll' }) },
  ], { label: 'Dashboard' }));
  acctBtn.addEventListener('click', () => {
    const items = (state.accounts || []).map(a => ({ label: a.name, logo: ui.providerMark(a.provider, 14), hint: a.signedIn ? (ui.usageText(a.usage) || a.plan || 'signed in') : 'sign in', title: a.signedIn ? `${a.name} · signed in${a.plan ? ' · ' + a.plan : ''}${a.fingerprint ? ' · ' + a.fingerprint : ''}\n${ui.usageDetail(a.usage)}` : `Sign in ${a.name}`,
      run: () => post({ type: 'command', command: a.signedIn ? 'overseer.refreshAccounts' : 'overseer.signIn', args: a.signedIn ? undefined : { profile: { id: a.id, name: a.name } } }) }));
    ui.menu(acctBtn, [{ head: 'Accounts' }, ...items, 'sep', { label: 'Add account…', icon: 'person-add', run: () => post({ type: 'command', command: 'overseer.addProfile' }) },
      { label: 'Manage accounts', icon: 'settings-gear', run: () => post({ type: 'command', command: 'overseer.accounts.focus' }) }], { label: 'Accounts' });
  });

  // ---------- Main area ----------
  function setMode(m, opts = {}) {
    mode = m;
    chatHost.hidden = m !== 'chat'; composerHost.hidden = m !== 'composer'; gridHost.hidden = m !== 'grid';
    document.body.dataset.mode = m;
    if (m === 'composer') { composer.open(opts); }
    if (m === 'grid') { grid.open(); } else grid.close();
    if (m !== 'chat') setFiles(false, true);
    post({ type: 'mode', mode: m, selected });
    persist(); if (!opts.quiet) renderRail();
  }
  function setFiles(open, quiet) {
    filesOpen = open && mode === 'chat' && !!selected;
    filesPanel.hidden = !filesOpen;
    chat.filesBtn.setAttribute('aria-pressed', String(filesOpen));
    if (filesOpen) window.overseerFiles?.show(selected, true);
    if (!quiet) persist();
  }
  filesClose.addEventListener('click', () => setFiles(false));

  // ---------- New-agent composer (AC-59) ----------
  const composer = window.OverseerComposer.create(composerHost, { post, onStarted: runId => { selected = runId; setMode('chat'); } });

  // ---------- Grid (AC-58) ----------
  const grid = window.OverseerGrid.create(gridHost, { post, open: runId => selectRun(runId, { focusChat: true }), getState: () => state });

  // ---------- Messages ----------
  window.addEventListener('message', e => {
    const m = e.data;
    switch (m.type) {
      case 'state':
        state = m.state; if (m.selected && m.selected !== selected && mode !== 'composer') selected = m.selected;
        renderRail(); grid.onState(state); composer.onState(state);
        window.overseerFiles?.onState(state, filesOpen ? selected : undefined);
        document.body.dataset.ready = '1';
        break;
      case 'selected': if (m.runId && (m.runId !== selected || mode !== 'chat')) selectRun(m.runId); break;
      case 'mode': setMode(m.mode, m); break;
      case 'run': if (m.channel === 'grid') grid.run(m); else if (m.run.id === selected) chat.setRun(m); break;
      case 'history': if (m.channel === 'grid') grid.history(m); else if (m.root === selected) chat.history(m.events, m.truncated, saved.chat && saved.chat.runId === selected ? saved.chat : undefined); break;
      case 'events': if (m.channel === 'grid') grid.events(m.items); else chat.events(m.items.filter(x => x.root === selected)); break;
      case 'notice': if (m.scope === 'composer') composer.notice(m); else chat.notice(m.message); break;
      case 'raw': chat.raw(m.raw); break;
      case 'changes': if (m.runId === selected) chat.changes(m.changes); break;
      case 'composerData': composer.data(m.data); break;
      case 'mentionFiles': if (m.scope === 'composer') composer.mentionFiles(m); else chat.mentionFiles(m); break;
      case 'searchHits': if (m.q === query) { searchHits = new Set(m.taskIds); renderRail(); } break;
      case 'measure': post({ type: 'measured', id: m.id, w: window.innerWidth, h: window.innerHeight }); break;
      case 'dashboard': document.body.dataset.dashboard = m.on ? '1' : ''; break;
      case 'focus': if (m.target === 'search') searchInput.focus(); else if (m.target === 'composer') setMode('composer'); else if (m.target === 'chat') chat.prompt.focus(); break;
    }
  });

  // Keyboard: ⌘. stops, Escape leaves the grid; arrows handled per region.
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape' && mode === 'grid' && !e.target.closest('input, textarea')) { setMode(selected ? 'chat' : 'composer'); e.preventDefault(); }
  });

  // Read-only view of the dashboard's state for UI tests.
  window.__overseer = { state: () => state, mode: () => mode, selected: () => selected };
  setMode(mode === 'chat' && !selected ? 'composer' : mode, { quiet: true });
  if (selected && mode === 'chat') post({ type: 'select', runId: selected, restore: true });
  setFiles(filesOpen, true);
  post({ type: 'ready' });
})();
