// Overseer editor view (AC-55, AC-58, AC-59; Gate K): the selected agent's chat, the new-agent
// composer (nothing selected) or the agent grid. The agents list is VS Code's side bar (views.js);
// the review opens beside this view when the agent has changes. State survives reloads.
(function () {
  const vscode = acquireVsCodeApi();
  window.overseerApi = vscode; // UI tests post messages as the dashboard does
  const ui = window.OverseerUI, el = ui.el;
  const post = m => vscode.postMessage(m);
  const saved = vscode.getState() || {};
  let state = { tasks: [], runs: [], workspaces: [], profiles: [], accounts: [], attention: [], pinned: [], gridMax: 6, archived: [] };
  let selected = saved.selected, mode = saved.mode || (saved.selected ? 'chat' : 'composer');
  const persist = () => vscode.setState({ ...(vscode.getState() || {}), selected, mode, chat: chat && chat.state() });

  // ---------- Layout ----------
  // Gate K: the agents list lives in VS Code's side bar; this view is the chat, composer or grid.
  const main = el('main', 'main');
  const chatHost = el('section', 'view-chat');
  const composerHost = el('section', 'view-composer'); composerHost.dataset.auditView = 'composer';
  const gridHost = el('section', 'view-grid'); gridHost.dataset.auditView = 'grid'; gridHost.setAttribute('aria-label', 'Agent grid');
  // AC-100: an agent's files live in the review (AC-99); the chat has no Files pane of its own.
  main.append(chatHost, composerHost, gridHost);
  const body = el('div', 'dash'); body.append(main);
  document.body.append(body);

  const chat = new window.OverseerChat(chatHost, { post: m => post({ ...m, runId: m.runId || selected, scope: 'chat' }), mode: 'dashboard', onState: persist });

  function selectRun(runId, { focusChat, fromHost } = {}) {
    if (!runId) return;
    selected = runId; setMode('chat', { quiet: true });
    if (!fromHost) post({ type: 'select', runId });
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
    post({ type: 'mode', mode: m, selected });
    persist();
  }

  // ---------- New-agent composer (AC-59) and home's conversation with Overseer (AC-182) ----------
  const agents = () => state.runs.filter(r => !r.parent_run_id).sort((a, b) => (ACTIVE_STATUS.has(b.status) - ACTIVE_STATUS.has(a.status)) || (b.created_ms - a.created_ms)).map(r => ({ id: r.id, title: r.title, status: r.status, harness: r.harness }));
  const ACTIVE_STATUS = new Set(['queued', 'starting', 'running', 'waiting_for_user', 'waiting_for_connection', 'waiting_for_memory']);
  const composer = window.OverseerComposer.create(composerHost, { post, agents, onStarted: runId => { selected = runId; if (!(vscode.getState() || {}).stayHome) setMode('chat'); } });
  const home = window.OverseerHome.create(composerHost, { post, startWith: text => composer.startWith(text) });

  // ---------- Grid (AC-58) ----------
  const grid = window.OverseerGrid.create(gridHost, { post, open: runId => selectRun(runId, { focusChat: true }), getState: () => state,
    loadLayout: () => saved.gridLayout, saveLayout: layout => vscode.setState({ ...(vscode.getState() || {}), gridLayout: layout }) });

  // ---------- Messages ----------
  window.addEventListener('message', e => {
    const m = e.data;
    switch (m.type) {
      case 'state':
        state = m.state; if (m.selected && m.selected !== selected && mode !== 'composer') selected = m.selected;
        grid.onState(state); composer.onState(state);
        document.body.dataset.ready = '1';
        break;
      case 'selected': if (m.runId && (m.runId !== selected || mode !== 'chat')) selectRun(m.runId, { fromHost: true }); break;
      case 'mode': setMode(m.mode, m); break;
      case 'gridPlace': grid.place(m.runId, m.edge); break;
      case 'gridReset': grid.reset(); break;
      case 'tracked': grid.setTracked(m.runId); break;
      case 'run': if (m.channel === 'grid') grid.run(m); else if (m.run.id === selected) chat.setRun(m); break;
      case 'history': if (m.channel === 'grid') grid.history(m); else if (m.root === selected) chat.history(m.events, m.truncated, saved.chat && saved.chat.runId === selected ? saved.chat : undefined); break;
      case 'events': if (m.channel === 'grid') grid.events(m.items); else chat.events(m.items.filter(x => x.root === selected)); break;
      case 'notice': if (m.scope === 'composer') composer.notice(m); else chat.notice(m.message); break;
      case 'raw': chat.raw(m.raw); break;
      case 'changes': if (m.runId === selected) chat.changes(m.changes); break;
      case 'composerData': composer.data(m.data); break;
      case 'overseer': home.session(m.session); break;
      case 'overseerNotice': if (m.id) home.proposalStatus(m.id, m.message); else composer.notice({ message: m.message }); break;
      case 'askOverseer': setMode('composer'); composer.askOverseer(m.text || ''); break;
      case 'mentionFiles': if (m.scope === 'composer') composer.mentionFiles(m); else chat.mentionFiles(m); break;
      case 'measure': post({ type: 'measured', id: m.id, w: window.innerWidth, h: window.innerHeight }); break;
      case 'dashboard': document.body.dataset.dashboard = m.on ? '1' : ''; break;
      case 'focus': if (m.target === 'composer') setMode('composer'); else if (m.target === 'chat') chat.prompt.focus(); break;
    }
  });

  // Keyboard: ⌘. stops, Escape leaves the grid; arrows handled per region.
  document.addEventListener('keydown', e => {
    // Escape while tracking returns to the grid alone (AC-105); otherwise it leaves the grid.
    if (e.key === 'Escape' && mode === 'grid' && !e.target.closest('input, textarea')) { if (grid.tracked()) post({ type: 'untrack' }); else setMode(selected ? 'chat' : 'composer'); e.preventDefault(); }
  });

  // Read-only view of the dashboard's state for UI tests.
  window.__overseer = { state: () => state, mode: () => mode, selected: () => selected, home: () => home.current };
  setMode(mode === 'chat' && !selected ? 'composer' : mode, { quiet: true });
  if (selected && mode === 'chat') post({ type: 'select', runId: selected, restore: true });
  post({ type: 'ready' });
})();
