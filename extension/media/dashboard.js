// Overseer editor view (AC-55, AC-58, AC-59; Gate K): the selected agent's chat, the new-agent
// composer (nothing selected) or the agent grid. The agents list is VS Code's side bar (views.js);
// the review opens beside this view when the agent has changes. State survives reloads.
(function () {
  const vscode = acquireVsCodeApi();
  window.overseerApi = vscode; // shared with files.js
  const ui = window.OverseerUI, el = ui.el;
  const post = m => vscode.postMessage(m);
  const saved = vscode.getState() || {};
  let state = { tasks: [], runs: [], workspaces: [], profiles: [], accounts: [], attention: [], pinned: [], gridMax: 6, archived: [] };
  let selected = saved.selected, mode = saved.mode || (saved.selected ? 'chat' : 'composer'), filesOpen = !!saved.filesOpen;
  const persist = () => vscode.setState({ ...(vscode.getState() || {}), selected, mode, filesOpen, chat: chat && chat.state() });

  // ---------- Layout ----------
  // Gate K: the agents list lives in VS Code's side bar; this view is the chat, composer or grid.
  const main = el('main', 'main');
  const chatHost = el('section', 'view-chat');
  const composerHost = el('section', 'view-composer'); composerHost.dataset.auditView = 'composer';
  const gridHost = el('section', 'view-grid'); gridHost.dataset.auditView = 'grid'; gridHost.setAttribute('aria-label', 'Agent grid');
  const filesPanel = el('aside', 'files-panel'); filesPanel.dataset.auditView = 'files'; filesPanel.setAttribute('aria-label', 'Files');
  const filesHead = el('div', 'files-head');
  const filesTitle = el('span', 'files-title ellipsis'); filesTitle.id = 'files-for';
  const filesRefresh = ui.iconButton('refresh', 'Refresh files', { cls: 'sm' }); filesRefresh.id = 'files-refresh';
  const filesClose = ui.iconButton('close', 'Close files', { cls: 'sm' });
  filesHead.append(ui.icon('list-tree', 'sm'), filesTitle, filesRefresh, filesClose);
  const filesNote = el('p', 'files-note'); filesNote.id = 'files-note'; filesNote.setAttribute('role', 'status');
  const filesTree = el('div', 'files-tree'); filesTree.id = 'files'; filesTree.setAttribute('role', 'tree'); filesTree.setAttribute('aria-label', 'Worktree files');
  filesPanel.append(filesHead, filesNote, filesTree);
  main.append(chatHost, composerHost, gridHost);
  const body = el('div', 'dash'); body.append(main, filesPanel);
  document.body.append(body);

  const chat = new window.OverseerChat(chatHost, { post: m => post({ ...m, runId: m.runId || selected, scope: 'chat' }), mode: 'dashboard', onState: persist,
    onFiles: () => setFiles(!filesOpen) });

  function selectRun(runId, { focusChat, fromHost } = {}) {
    if (!runId) return;
    selected = runId; setMode('chat', { quiet: true });
    if (!fromHost) post({ type: 'select', runId });
    window.overseerFiles?.show(selected);
    persist();
    if (focusChat) chat.prompt.focus();
  }

  // ---------- Main area ----------
  function setMode(m, opts = {}) {
    mode = m;
    chatHost.hidden = m !== 'chat'; composerHost.hidden = m !== 'composer'; gridHost.hidden = m !== 'grid';
    document.body.dataset.mode = m;
    if (m === 'composer') { composer.open(opts); }
    if (m === 'grid') { grid.open(); } else grid.close();
    if (m !== 'chat') setFiles(false, true);
    post({ type: 'mode', mode: m, selected });
    persist();
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
        grid.onState(state); composer.onState(state);
        window.overseerFiles?.onState(state, filesOpen ? selected : undefined);
        document.body.dataset.ready = '1';
        break;
      case 'selected': if (m.runId && (m.runId !== selected || mode !== 'chat')) selectRun(m.runId, { fromHost: true }); break;
      case 'mode': setMode(m.mode, m); break;
      case 'run': if (m.channel === 'grid') grid.run(m); else if (m.run.id === selected) chat.setRun(m); break;
      case 'history': if (m.channel === 'grid') grid.history(m); else if (m.root === selected) chat.history(m.events, m.truncated, saved.chat && saved.chat.runId === selected ? saved.chat : undefined); break;
      case 'events': if (m.channel === 'grid') grid.events(m.items); else chat.events(m.items.filter(x => x.root === selected)); break;
      case 'notice': if (m.scope === 'composer') composer.notice(m); else chat.notice(m.message); break;
      case 'raw': chat.raw(m.raw); break;
      case 'changes': if (m.runId === selected) chat.changes(m.changes); break;
      case 'composerData': composer.data(m.data); break;
      case 'mentionFiles': if (m.scope === 'composer') composer.mentionFiles(m); else chat.mentionFiles(m); break;
      case 'measure': post({ type: 'measured', id: m.id, w: window.innerWidth, h: window.innerHeight }); break;
      case 'dashboard': document.body.dataset.dashboard = m.on ? '1' : ''; break;
      case 'focus': if (m.target === 'composer') setMode('composer'); else if (m.target === 'chat') chat.prompt.focus(); break;
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
