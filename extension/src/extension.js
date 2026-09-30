// Overseer VS Code extension: presentation, navigation and editor buffers.
// All durable state (tasks, runs, workspaces, events) lives in overseerd.
const vscode = require('vscode');
const path = require('path');
const { execFile } = require('child_process');
const { DaemonClient, resolveBinary, isProductionInstall } = require('./daemon-client');
const { Model, AgentsProvider, AccountsProvider, ACTIVE, accountName } = require('./views');
const { SearchView } = require('./search-view');
const { OutputPanels } = require('./output-panel');
const { Review } = require('./review');
const { CommandCenter } = require('./command-center');
const { Arrangement } = require('./arrangement');
const { AgentHead } = require('./agent-head');
const { NewTaskPanel } = require('./new-task');
const { PullRequests } = require('./pull-request');
const { Landing } = require('./landing');
const { TaskLauncher } = require('./task-launcher');
const { Steering } = require('./run-actions');
const { PhoneAccess } = require('./phone-access');
const { OverseerWindow, retireFocusMode } = require('./overseer-window');
const { VsCodeChat } = require('./vscode-chat');
const { OverseerChat } = require('./overseer-chat');
const { SwarmControls } = require('./swarm-controls');
const { Continuity } = require('./continuity');
const { AutoUsage } = require('./auto-usage');
const features = require('./features');
const { Voice } = require('./voice');
const { agentsOnScreen, permissionTarget, commandTarget } = require('./on-screen');
const { Notices, agentFromUri } = require('./notices');
const Rollup = require('../media/rollup.js');
const Plain = require('../media/plain-words.js');

let client;
let centerRef;

function gitRoot(dir) {
  return new Promise(resolve => execFile('git', ['rev-parse', '--show-toplevel'], { cwd: dir }, (err, out) => resolve(err ? undefined : out.trim())));
}

// AC-100: the Overseer side bar is the one tree. Overseer offers once to take Explorer's place in the
// primary side bar when VS Code opens (Explorer stays one click away in the activity bar); the answer
// is the overseer.sideBar.openOnStartup setting.
async function offerSideBar(context) {
  const config = vscode.workspace.getConfiguration('overseer');
  const choice = config.get('sideBar.openOnStartup', null);
  if (choice === true) { await vscode.commands.executeCommand('workbench.view.extension.overseer'); return; }
  if (choice === false || context.globalState.get('overseer.sideBarOffered')) return;
  await context.globalState.update('overseer.sideBarOffered', true);
  const pick = await vscode.window.showInformationMessage('Show Overseer in the side bar instead of Explorer when VS Code opens? Explorer stays one click away in the activity bar.', 'Use Overseer', 'Keep Explorer');
  if (pick === 'Use Overseer') {
    await config.update('sideBar.openOnStartup', true, vscode.ConfigurationTarget.Global);
    await vscode.commands.executeCommand('workbench.view.extension.overseer');
  } else if (pick === 'Keep Explorer') await config.update('sideBar.openOnStartup', false, vscode.ConfigurationTarget.Global);
}

async function activate(context) {
  const log = vscode.window.createOutputChannel('Overseer', { log: true });
  context.subscriptions.push(log);
  const say = msg => log.info(msg);
  const settings = vscode.workspace.getConfiguration('overseer');
  const binary = resolveBinary(context, settings.get('daemonPath'));
  // Production (the owner's installed extension) only ever uses the standard daemon (AC-212).
  const production = isProductionInstall(context.extensionPath);
  // A dev profile (scripts/dev code) pins this window to one dev daemon (AC-209); production ignores it.
  const pinSocket = settings.get('daemonSocket'), pinInstance = settings.get('devInstance');
  const pin = !production && pinSocket ? { socket: pinSocket, instance: pinInstance } : null;
  if (production && (pinSocket || pinInstance)) say('ignoring overseer.daemonSocket and overseer.devInstance: this is the installed extension');
  say(`${production ? 'installed in the standard extensions folder: production' : 'loaded from an isolated extensions folder'}; daemon ${binary}${pin ? `; pinned to ${pin.instance} at ${pin.socket}` : ''}`);
  client = new DaemonClient(binary, say, { production, pin });
  const devLabel = pin ? ` ${pin.instance || 'dev'}` : '';
  const model = new Model(client);
  const autoUsage = new AutoUsage(client, context);
  const swarmControls = new SwarmControls(client, () => model.refresh(true), run =>
    vscode.window.showWarningMessage(`Stop ${run.category} swarm?`, { modal: true,
      detail: 'Queued jobs will be cancelled and active workers asked to stop. Unconfirmed exits remain visible.' }, 'Stop Swarm')
      .then(choice => choice === 'Stop Swarm'));
  // The side bar's agents list (Gate K): Needs you, then agents by repository.
  let voiceTargeted = () => new Set(); // set once Voice Mode is up (below)
  const agents = new AgentsProvider(model, context.workspaceState, context.extensionUri, { attention: () => attention(), pinned: () => pinned(), voiceTargeted: () => voiceTargeted(), reviewed: () => reviewed });
  const accounts = new AccountsProvider(model, context.extensionUri);
  const agentsView = vscode.window.createTreeView('overseer.agents', { treeDataProvider: agents, showCollapseAll: true, dragAndDropController: agentDrag() });
  // The search field above the Agents list (AC-112): typing filters the list through the daemon's search.
  const searchView = new SearchView(context.extensionUri, { onQuery: q => runAgentSearch(q), onFilter: kind => setStatusFilter(kind), onFilterMenu: () => filterMenu() });
  /** The filter icon's menu (a native pick: a menu inside the short search pane would be clipped). */
  async function filterMenu() {
    const now = agents.showArchived ? 'archived' : agents.statusFilter;
    const items = [['all', 'All', 'list-flat'], ['working', 'Working', 'sync'], ['needs', 'Needs you', 'bell'], ['review', 'To review', 'sparkle'], ['done', 'Done', 'check'], ['failed', 'Failed', 'error'], ['archived', 'Archived', 'archive']]
      .map(([value, label, icon]) => ({ value, label: `$(${icon}) ${label}`, description: value === now ? '✓' : '' }));
    const picked = await vscode.window.showQuickPick(items, { title: 'Show agents', placeHolder: 'Filter the Agents list' });
    if (!picked) return;
    setStatusFilter(picked.value); searchView.setFilter(picked.value);
  }
  /** The search field's filters: All, Working, Needs you, Done, Failed, Archived. */
  function setStatusFilter(kind) {
    agents.showArchived = kind === 'archived';
    agents.statusFilter = ['working', 'needs', 'review', 'done', 'failed'].includes(kind) ? kind : 'all';
    vscode.commands.executeCommand('setContext', 'overseer.showArchived', agents.showArchived);
    setAgentFilter(agents.filter);
  }
  context.subscriptions.push(vscode.window.registerWebviewViewProvider('overseer.search', searchView, { webviewOptions: { retainContextWhenHidden: true } }));
  const accountsView = vscode.window.createTreeView('overseer.accounts', { treeDataProvider: accounts });
  context.subscriptions.push(vscode.window.registerFileDecorationProvider(agents.decorations));
  const outputs = new OutputPanels(context, client, model);
  const review = new Review(context, client, model, say);
  // Opening an agent's review clears its "to review" mark (AC-254), however it was opened.
  const openReview = review.open.bind(review);
  review.open = async (runId, opts) => { const r = await openReview(runId, opts); const root = model.run(runId) && model.rootRun(model.run(runId)); if (root && !ACTIVE.has(root.status)) markReviewed(root.id); return r; };
  let selectedRun;
  const launcher = new TaskLauncher(context, client, model, () => refreshAccounts());
  const steering = new Steering(client, model);
  outputs.steering = steering;
  // Continuity (Gate L): the connection, its settings, and the agents that wait for it.
  const continuity = new Continuity(context, client, model, { say, views: () => { try { return { center: centerRef, outputs, agentsView, newTaskPanel }; } catch { return { center: centerRef, outputs, agentsView }; } } });
  launcher.continuity = () => continuity.snapshot(); steering.continuity = continuity; agents.continuity = continuity;
  // Reviewed marks (AC-254): when each agent's review was last opened (or it was merged), for the
  // owner across windows. An agent at its end with no mark since it ended is "to review".
  const reviewed = new Map([...Object.entries(context.workspaceState.get('overseer.reviewed', {})), ...Object.entries(context.globalState.get('overseer.reviewed', {}))]);
  const markReviewed = runId => {
    if (!runId) return;
    const run = model.run(runId);
    if (run && reviewed.get(runId) >= (run.ended_ms || run.created_ms || 0) && !ACTIVE.has(run.status)) return;
    reviewed.set(runId, Date.now());
    context.globalState.update('overseer.reviewed', Object.fromEntries([...reviewed].slice(-2000)));
    model.emitter.fire();
  };
  const archivedTasks = () => (model.state.tasks || []).filter(t => t.archived_ms).map(t => t.id);
  // Needs you (AC-61, AC-246): what waits for the owner's answer, counted as the TUI and the phone
  // count it (media/rollup.js); failed and finished agents carry the "to review" mark instead (AC-254).
  function attention() {
    const out = Rollup.needsYou(model.state);
    const waiting = continuity.attention(); if (waiting) out.push(waiting);
    return out.sort((a, b) => a.rank - b.rank);
  }
  /** The rollup by state (AC-255): the side bar and the grid show these same counts. */
  const rollup = () => Rollup.counts(model.state, reviewed);
  /** Agents at their end not reviewed yet, failed first, then the newest (⌥⌘J visits them after Needs you). */
  const toReview = () => Rollup.agents(model.state).filter(r => Rollup.unreviewed(r, reviewed)).sort((a, b) => (Rollup.FAILED.has(b.status) - Rollup.FAILED.has(a.status)) || ((b.ended_ms || b.created_ms) - (a.ended_ms || a.created_ms)));
  const pinned = () => context.workspaceState.get('overseer.pinned', []).filter(id => model.run(id));
  const setPinned = (runId, on) => context.workspaceState.update('overseer.pinned', [...new Set([...pinned().filter(id => id !== runId), ...(on ? [runId] : [])])]);
  const search = async q => { try { return (await client.request('search', { query: q, limit: 200 })).task_ids || []; } catch { return []; } };
  const center = new CommandCenter(context, model, { select: (runId, opts) => selectRun(runId, opts), selected: () => selectedRun, client, model, launcher, attention, rollup, pinned, setPinned, archived: archivedTasks, search, steering,
    // A card, a request's stage or a Needs-you item opens its agent, or its finished work (AC-226).
    openAgent: (runId, opts) => openAgent(runId, opts),
    // The grid takes the editor area and gives it back as it was (AC-79).
    onMode: async (mode, was) => { if (mode === 'grid') await arrangement.enterGrid(); else if (was === 'grid') await arrangement.leaveGrid(); },
    // No empty grid (AC-113): when its last tile goes, the grid gives way to the home composer.
    track: runId => arrangement.track(runId), untrack: () => arrangement.untrack(),
    gridEmpty: () => goHome(`The grid is empty: no agent is working or pinned.${rollupNote()} Start one here.`),
    // AC-257: from Overseer's conversation back to the agent it left.
    backToAgent: runId => backToAgent(runId) });
  centerRef = center;
  const arrangement = new Arrangement({ context, center, review, model, client, outputs, log: say });
  // The agent's head (AC-233): its worktree in this window, Follow or Diffs only. A file opened from
  // the Worktree view while only the chat is shown brings the head in first.
  const head = new AgentHead({ context, client, model, review, log: say, handlers: { ensureShown: runId => arrangement.openReview(runId) } });
  review.head = head;
  outputs.column = () => vscode.ViewColumn.Beside;
  context.subscriptions.push(vscode.window.registerWebviewPanelSerializer('overseer.center', center));
  // AC-258: VS Code's own chat view is closed the first time Overseer's view is on screen here.
  const vsChat = new VsCodeChat(center, say);
  // It runs as Overseer's view comes on screen (part of the action that showed it), not later, when
  // the owner may already be typing somewhere else.
  const checkVsChat = e => {
    if (vsChat.done) return;
    if (![...e.opened, ...e.changed].some(t => t.isActive && t.input?.viewType?.endsWith('overseer.center'))) return;
    setTimeout(() => { if (center.panel?.visible) vsChat.check().catch(error => say('vscode chat: ' + error.message)); }, 300);
  };
  context.subscriptions.push(vscode.window.tabGroups.onDidChangeTabs(checkVsChat));
  // The agent the Overseer window shows first (AC-264): the selected one, else the most recent working one, else the most recent.
  const focusedAgent = () => {
    if (selectedRun && model.run(selectedRun)) return (model.rootRun(model.run(selectedRun)) || model.run(selectedRun)).id;
    const archived = new Set(archivedTasks());
    // Overseer's own run is no agent of the list (AC-182).
    const overseer = r => r.id === model.state.overseer?.run_id || (model.state.oversight || {})[r.id]?.role === 'overseer';
    const roots = (model.state.runs || []).filter(r => !r.parent_run_id && !archived.has(r.task_id) && !overseer(r)).sort((a, b) => (ACTIVE.has(b.status) - ACTIVE.has(a.status)) || b.created_ms - a.created_ms);
    return roots[0]?.id;
  };
  // AC-264: the one Overseer layout, this window reopened as the Overseer window (Workspace, ⌥⌘⇧O).
  const overseerWindow = new OverseerWindow({ context, center, arrangement, log: say,
    handlers: { focusedAgent, select: (id, opts) => { selectedRun = id; return selectRun(id, opts); }, backToOverseer: () => backToOverseer() } });
  // In the Overseer window Overseer's views take the whole editor area (the owner's layout is their folder window's).
  arrangement.takeover = overseerWindow.active;
  // AC-104: an agent dragged from the side bar onto the grid. VS Code's editor drop opens the agent's
  // chat editor (AC-71) in the grid's group, or in a group split off beside it; while the grid is shown
  // that editor (and a group the drop created) is closed again and the agent is placed on the grid's
  // edge on that side instead.
  outputs.intercept = async (runId, panel, uri) => {
    if (center.mode !== 'grid' || !center.panel) return false;
    const gridColumn = center.panel.viewColumn, column = panel.viewColumn;
    let layout; try { layout = await vscode.commands.executeCommand('vscode.getEditorLayout'); } catch { layout = undefined; }
    const vertical = layout && layout.orientation === 1;
    const edge = column === gridColumn ? 'right' : column > gridColumn ? (vertical ? 'bottom' : 'right') : (vertical ? 'top' : 'left');
    setTimeout(async () => {
      for (const group of vscode.window.tabGroups.all) {
        const tabs = group.tabs.filter(t => t.input instanceof vscode.TabInputCustom && t.input.uri.toString() === uri.toString());
        if (!tabs.length) continue;
        const alone = group.tabs.length === tabs.length && group.viewColumn !== gridColumn;
        await vscode.window.tabGroups.close(alone ? group : tabs).then(undefined, () => {});
      }
      center.panel?.reveal(gridColumn, false);
      center.panel?.webview.postMessage({ type: 'gridPlace', runId, edge });
    }, 0);
    return true;
  };
  let needsQueue = Promise.resolve();
  let atOverseer = false; // the last ⌥⌘J went to Overseer's open proposal: the next one moves on
  const nextNeedsYou = async () => {
    // Needs you first; then the agents at their end still to review (AC-254), failed first.
    const list = [...attention(), ...toReview().map(r => ({ run_id: r.id, rank: 5 }))];
    if (!list.length) { vscode.window.setStatusBarMessage('$(check) Nothing needs you', 2500); return; }
    // The most urgent item that is not already open (approvals first, then failures, then reviews).
    const next = list.find(a => (a.overseer ? !atOverseer : a.run_id !== selectedRun)) || list[0];
    atOverseer = !!next.overseer;
    // Overseer's open proposal (AC-242): its conversation, where the proposal waits for a yes.
    if (next.overseer) { await vscode.commands.executeCommand('overseer.talk'); return; }
    await selectRun(next.run_id); center.focus('chat');
  };
  // AC-106: "Where am I": every Overseer view open in this window (the Overseer view as chat, grid
  // or composer, the review, chats taken out into editor groups, New Task, run output), in editor
  // group order, and a jump to the one picked.
  const whereAmI = async () => {
    const title = id => (id && model.run(id)?.title) || '';
    const panels = [center.panel, ...review.manager.panels.values(), ...[...outputs.panels.values()].map(e => e.panel), newTaskPanel.panel].filter(Boolean);
    const describe = (vt, tab) => {
      if (/overseer\.center$/.test(vt)) {
        if (center.mode === 'grid') return { icon: 'layout', kind: 'Grid', what: arrangement.tracked ? `tracking ${title(arrangement.tracked)}` : 'agents side by side' };
        if (center.mode === 'composer') return { icon: 'add', kind: 'New agent', what: 'the composer' };
        return { icon: 'comment-discussion', kind: 'Chat', what: title(selectedRun) || 'no agent selected' };
      }
      if (/overseer\.review$/.test(vt)) return { icon: 'diff-multiple', kind: 'Review', what: tab.label.replace(/^Review: /, '') };
      if (/overseer\.chatEditor$/.test(vt)) return { icon: 'comment', kind: 'Chat, taken out', what: tab.label.replace(/\.overseer-chat$/, '') };
      if (/overseer\.newTask$/.test(vt)) return { icon: 'new-file', kind: 'Full form', what: 'a new agent with every option' };
      if (/overseer\.output$/.test(vt)) return { icon: 'output', kind: 'Output', what: tab.label };
      return undefined;
    };
    const items = [];
    for (const group of vscode.window.tabGroups.all) for (const tab of group.tabs) {
      const vt = tab.input?.viewType || '';
      const d = describe(vt, tab);
      if (!d) continue;
      const here = group.isActive && tab.isActive;
      items.push({ label: `$(${d.icon}) ${d.kind}`, description: d.what, detail: `Editor group ${group.viewColumn}${here ? ' · you are here' : ''}`, tab, group,
        panel: panels.find(p => p.viewColumn === group.viewColumn && p.title === tab.label) });
    }
    if (!items.length) { vscode.window.showInformationMessage('No Overseer view is open in this window.'); return; }
    const pick = await vscode.window.showQuickPick(items, { title: 'Where am I', placeHolder: 'Overseer views open in this window: pick one to go there', matchOnDescription: true });
    if (!pick) return;
    if (pick.panel) pick.panel.reveal(pick.group.viewColumn, false);
    else if (pick.tab.input instanceof vscode.TabInputCustom) await vscode.commands.executeCommand('vscode.openWith', pick.tab.input.uri, pick.tab.input.viewType, { viewColumn: pick.group.viewColumn, preserveFocus: false });
  };
  // AC-107, AC-227: Talk to Overseer is home, the one view for talking to Overseer (typed or, with
  // Voice Mode on, spoken). Nothing docks below any more; this keeps Overseer's own run hidden and
  // relays what is typed in its chat, and carries out the actions the daemon asks the UI for.
  const overseerChat = new OverseerChat({ context, client, model, outputs, setPinned: (id, on) => { setPinned(id, on); agents.refresh(); center.push(); }, log: say });
  const pullRequests = new PullRequests(client, model, say);
  // AC-232, AC-243: the Merge button, its one confirmation, Cancel merge, and the local merge for a
  // repository with no GitHub remote. Chats read it through the model (run-feed.js), the review through its host.
  const landing = new Landing(client, model, { pickAgent: (...a) => pickAgent(...a), hasWorktree: r => hasWorktree(r), log: say,
    notice: (runId, text) => { outputs.notice?.(runId, text); center.notice?.(runId, text); }, onMerged: runId => markReviewed(runId) });
  model.landing = landing; pullRequests.landing = landing;
  landing.onDidChange(runId => { model.emitter.fire(); review.landingChanged?.(runId); });
  const newTaskPanel = new NewTaskPanel(context, client, model, { selectRun: (...a) => selectRun(...a), launcher, column: () => vscode.ViewColumn.Beside });
  // Voice Mode (Gate R): the voice view, its status bar item and toasts; the daemon listens.
  const voice = new Voice(context, client, { selectRun: (...a) => selectRun(...a), view: () => center.panel, showHome: () => goHome() });
  // The voice mark on targeted agents (side bar and grid) and home's voice strip follow it.
  voiceTargeted = () => voice.targeted;
  center.voiceSource = voice;
  voice.onChange(what => { if (what === 'targets') agents.refresh(); center.pushVoice(); });
  // Agents started by voice take the composer's remembered choices and this window's workspace
  // trust (AC-168): sent to the daemon on connect and whenever the composer remembers new ones.
  const sendStartDefaults = () => {
    const d = launcher.defaults();
    client.request('voice.set', { start_defaults: { harness: d.harness || '', profile_id: d.harness === 'generic' ? '' : d.account || '', model: d.model || '', workspace_mode: d.mode === 'current' ? 'current' : 'worktree', trusted: vscode.workspace.isTrusted } }).catch(() => {});
  };
  client.on('connected', sendStartDefaults);
  const rememberDefaults = launcher.saveDefaults.bind(launcher);
  launcher.saveDefaults = async d => { const r = await rememberDefaults(d); sendStartDefaults(); return r; };
  context.subscriptions.push(vscode.workspace.onDidGrantWorkspaceTrust(sendStartDefaults));
  // An agent dragged from the side bar into the editor opens its chat there (AC-71): a read-only
  // virtual file per agent (overseer-chat:/<run id>/<title>.overseer-chat) shown by a custom editor.
  context.subscriptions.push(
    vscode.workspace.registerFileSystemProvider('overseer-chat', chatFileSystem(), { isReadonly: true, isCaseSensitive: true }),
    vscode.window.registerCustomEditorProvider('overseer.chatEditor', outputs.chatEditorProvider(), { webviewOptions: { retainContextWhenHidden: true }, supportsMultipleEditorsPerDocument: false }));
  context.subscriptions.push(vscode.window.registerWebviewPanelSerializer('overseer.output', outputs),
    agentsView.onDidExpandElement(e => agents.setCollapsed(e.element, false)),
    agentsView.onDidCollapseElement(e => agents.setCollapsed(e.element, true)));
  // AC-264: one button for the Overseer layout, beside Overseer's own item.
  const workspaceButton = vscode.window.createStatusBarItem('overseer.workspace', vscode.StatusBarAlignment.Left, 49);
  workspaceButton.name = 'Overseer Workspace';
  const updateWorkspaceButton = () => {
    const on = overseerWindow.active;
    workspaceButton.text = on ? '$(layout-sidebar-left-off) Close Workspace' : '$(layout) Workspace';
    workspaceButton.tooltip = on ? 'Go back to your own layout of this folder (⌥⌘⇧O)' : 'Open the Overseer layout: your agents on the left, the review in the middle, Overseer on the right (⌥⌘⇧O)';
    workspaceButton.command = on ? 'overseer.closeWorkspace' : 'overseer.openWorkspace';
    workspaceButton.show();
  };
  updateWorkspaceButton();
  context.subscriptions.push(workspaceButton);
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50);
  status.command = 'overseer.refresh';
  context.subscriptions.push(agentsView, accountsView, status, { dispose: () => client.dispose() });

  const updateStatus = () => {
    const runs = model.state.runs || [];
    const active = runs.filter(r => ACTIVE.has(r.status)).length;
    const needs = client.connected ? attention().length : 0;
    status.text = client.connected ? `$(overseer-mark) Overseer${devLabel} ${active} active${needs ? `  $(bell-dot) ${needs}` : ''}` : client.stopped ? '$(circle-slash) Overseer stopped' : '$(debug-disconnect) Overseer disconnected';
    status.tooltip = client.connected ? `${active} agent${active === 1 ? '' : 's'} running${needs ? ` · ${needs} need${needs === 1 ? 's' : ''} you` : ''}\nAgents keep running when VS Code closes.\nClick to open the Overseer view.` : client.stopped ? 'Agents and daemon were stopped. Click to start the daemon again.' : 'Reconnecting to overseerd…';
    status.command = client.stopped && !client.connected ? 'overseer.startDaemon' : 'overseer.openCenter';
    if (pin && !client.connected && !client.refusal) { status.text = `$(debug-disconnect) Overseer${devLabel} not running`; status.tooltip = client.notRunning(); }
    if (client.refusal) { status.text = '$(error) Overseer refused a dev daemon'; status.tooltip = client.refusal; status.command = 'overseer.showLog'; }
    status.show();
    vscode.commands.executeCommand('setContext', 'overseer.connected', client.connected);
  };
  model.onDidChange(updateStatus);
  // The Needs-you count on the Overseer activity icon (AC-70).
  model.onDidChange(() => { const n = client.connected ? attention().length : 0; agentsView.badge = n ? { value: n, tooltip: `${n} need${n === 1 ? 's' : ''} you` } : undefined; });
  // The Agents view's own header says what is left to review (AC-254); a review open on screen as
  // its agent finishes counts as reviewed.
  model.onDidChange(() => {
    for (const [session, panel] of review.manager?.panels || []) {
      const run = session.overseer?.runId && model.run(session.overseer.runId);
      if (run && panel.visible && Rollup.unreviewed(run, reviewed)) markReviewed(run.id);
    }
    agentsView.description = client.connected ? Rollup.reviewText(rollup()) : '';
  });
  model.onDidChange(() => { autoArchive().catch(() => {}); });
  client.on('connected', () => { model.refresh(); updateStatus(); });
  client.on('disconnected', () => { model.error = 'daemon connection lost; reconnecting'; model.emitter.fire(); updateStatus(); });
  client.on('unreachable', message => { model.error = message; model.emitter.fire(); updateStatus(); vscode.window.showWarningMessage(`Overseer: ${message}`); });
  client.on('refused', message => { model.error = message; model.emitter.fire(); updateStatus(); });
  client.on('stopped', () => { model.error = 'agents and daemon stopped (Overseer: Start Daemon to restart)'; model.emitter.fire(); updateStatus(); });
  // Accounts created, removed or signed out elsewhere (overseerd ctl, another window) show up here too.
  let accountsTimer;
  const accountsSoon = () => { clearTimeout(accountsTimer); accountsTimer = setTimeout(() => refreshAccounts().catch(() => {}), 400); };
  let accountsSeen = Date.now();
  context.subscriptions.push(vscode.window.onDidChangeWindowState(w => { if (w.focused && Date.now() - accountsSeen > 30000) { accountsSeen = Date.now(); accountsSoon(); } }));
  // Output and tool events do not change the daemon's state summary; streaming skips the refresh.
  const STREAM_ONLY = new Set(['output', 'tool', 'tool_result']);
  client.on('event', event => {
    if (!STREAM_ONLY.has(event.kind)) model.scheduleRefresh();
    if (event.kind === 'profile') accountsSoon();
    if (event.kind === 'permission') {
      if (event.payload?.auto_allowed) return; // the daemon allowed its own tool (Overseer's reads): nothing waits
      // The toast names the agent (AC-240); it stays quiet only when that agent is on screen.
      const run = model.run(event.run_id); const root = (run && model.rootRun(run)) || run;
      if (root && onScreen().includes(root.id)) return;
      const who = root ? `“${agentTitle(root)}”` : 'An agent';
      const what = event.payload?.kind === 'question' ? 'has a question for you' : `is waiting for permission to use ${event.payload?.tool || 'a tool'}`;
      vscode.window.showWarningMessage(`${who} ${what}.`, 'Show').then(choice => { if (!choice) return; if (center.active) { center.open(); selectRun(root?.id || event.run_id); } else outputs.show(event.run_id, { preserveFocus: false }); });
    }
  });

  const requireTrust = () => {
    if (!vscode.workspace.isTrusted) throw new Error('Overseer only launches or controls agents in a trusted workspace.');
  };
  const guard = fn => async (...args) => {
    try { return await fn(...args); } catch (error) { vscode.window.showErrorMessage(`Overseer: ${Plain.plain(error.message, 300)}`); say('error: ' + (error.stack || error.message)); }
  };
  const runArg = arg => (typeof arg === 'string' ? arg : arg?.run?.id) || selectedRun;
  // Unfinished features stay hidden until their setting is on (AC-204); a command run anyway says so.
  const whenOn = (feature, fn) => async (...args) => {
    if (!features.enabled(vscode, feature)) { vscode.window.showInformationMessage(features.offMessage(feature)); return undefined; }
    return fn(...args);
  };
  async function swarmId(arg, statuses) {
    if (arg?.swarm?.id) return arg.swarm.id;
    if (typeof arg === 'string') return arg;
    await model.refresh(true);
    const choices = model.swarms.filter(run => statuses.includes(run.status))
      .map(run => ({ id: run.id, label: run.category, description: run.status }));
    if (!choices.length) { vscode.window.showInformationMessage('No Swarm run is available for this action.'); return undefined; }
    return (await vscode.window.showQuickPick(choices, { title: 'Choose a Swarm' }))?.id;
  }

  const gridHasAgents = () => (model.state.runs || []).some(r => !r.parent_run_id && ACTIVE.has(r.status)) || pinned().length > 0;
  /** The rollup (AC-255) for a one-line note where the grid would be: " Of the rest: 6 to review · 3 reviewed." */
  const rollupNote = () => { const t = Rollup.text({ ...rollup(), working: 0, needs: 0 }); return t ? ` Of the rest: ${t}.` : ''; };
  /** The home view: the composer alone in the middle, with an optional one-line note. In the Overseer
   *  window home is the right panel: the review stays in the middle (AC-264). */
  async function goHome(note) {
    if (overseerWindow.active && center.panel && arrangement.current === 'split') await center.open({ column: center.panel.viewColumn });
    else await arrangement.chatOnly();
    center.setMode('composer');
    if (note) center.panel?.webview.postMessage({ type: 'notice', scope: 'composer', kind: 'info', message: note });
  }

  /** Opens an agent from a card, a request's stage or Needs you (AC-226, AC-227): its chat, with
   *  its review beside it when it has changes; `work` asks for the finished work (the review). */
  async function openAgent(runId, { work, from } = {}) {
    const run = model.run(runId) || (await model.refresh(), model.run(runId));
    if (!run) return;
    const root = model.rootRun(run) || run;
    // From Overseer's conversation (AC-257): the agent's head opens beside it and the conversation
    // keeps its tab; the agent's chat is one click away (the side bar, ⌥⌘U after it). A Needs-you
    // item is there to be answered, which takes the agent's chat, as before.
    const fromConversation = center.mode === 'composer' && from !== 'needs';
    if (work && !ACTIVE.has(root.status)) { await showWork(root.id, { keepConversation: fromConversation }); return; }
    if (fromConversation) {
      try {
        await selectRun(root.id, { keepConversation: true, force: true }); await head.focus(root.id);
        // The conversation's way back to the agent beside it.
        center.headAgent({ runId: root.id, title: agentTitle(root) });
      }
      catch (error) { say('open agent: ' + (error.stack || error.message)); throw error; }
      return;
    }
    await selectRun(root.id);
    center.focus('chat');
  }

  /** The finished work: the review when the agent changed files, else its chat. */
  async function showWork(runId, { keepConversation = false } = {}) {
    const run = model.run(runId);
    if (!run) return;
    let files = 0;
    try { files = (await client.request('workspace.changes', { workspace_id: run.workspace_id })).files || 0; } catch { /* removed worktree */ }
    if (files) { selectedRun = runId; head.select(runId).catch(() => {}); await arrangement.openReview(runId); if (keepConversation) await head.focus(runId); else await showInCenter(runId); }
    else await selectRun(runId, { keepConversation, force: keepConversation });
  }

  // Overseer moves the owner around VS Code (AC-226): Look actions the daemon carried out and asks
  // the UI to show. Only the window the owner is in acts (the last one focused).
  context.subscriptions.push(vscode.window.onDidChangeWindowState(w => { if (w.focused) context.globalState.update('overseer.lastWindow', vscode.env.sessionId); }));
  if (vscode.window.state.focused) context.globalState.update('overseer.lastWindow', vscode.env.sessionId);
  const ownerIsHere = () => vscode.window.state.focused || [undefined, vscode.env.sessionId].includes(context.globalState.get('overseer.lastWindow'));
  async function lookAction(event) {
    const p = event.payload || {}, runId = event.run_id;
    if (!runId || !ownerIsHere()) return;
    say(`overseer: ${p.action} ${runId}`);
    if (p.action === 'focus') { await selectRun(runId); center.focus('chat'); return; }
    if (p.action === 'open_review') { selectedRun = runId; await arrangement.openReview(runId); await showInCenter(runId); return; }
    if (p.action === 'show_work') { await showWork(runId); return; }
    if (p.action === 'open_file' && p.path) {
      await vscode.window.showTextDocument(vscode.Uri.file(p.path), { preview: false, viewColumn: vscode.ViewColumn.Beside });
      return;
    }
    if (p.action === 'open_worktree' && p.path) {
      // The agent's worktree, in place (no new window): its files to pick from, then the file opens.
      const run = model.run(runId);
      const files = await new Promise(resolve => execFile('git', ['ls-files', '-co', '--exclude-standard'], { cwd: p.path, maxBuffer: 16 * 1024 * 1024 }, (err, out) => resolve(err ? [] : out.split('\n').filter(Boolean).slice(0, 5000))));
      const pick = await vscode.window.showQuickPick(files.map(f => ({ label: path.basename(f), description: path.dirname(f) === '.' ? '' : path.dirname(f), rel: f })), { title: `${run?.title || 'Agent'}: its worktree`, placeHolder: p.path, matchOnDescription: true });
      if (pick) await vscode.window.showTextDocument(vscode.Uri.file(path.join(p.path, pick.rel)), { preview: false, viewColumn: vscode.ViewColumn.Beside });
    }
  }
  // When a request starts a single agent, the view slides aside and shows it working (AC-226),
  // unless the owner turned "Show the agent I start" off.
  async function startedByRequest(event) {
    const p = event.payload || {};
    if (!p.proposal || !event.run_id || !ownerIsHere()) return;
    if (!vscode.workspace.getConfiguration('overseer').get('showStartedAgent', true)) return;
    if (!center.panel || center.mode !== 'composer') return;
    let starts = 0;
    try { starts = ((await client.request('overseer.card', { id: p.proposal })).actions || []).filter(a => a.action === 'start').length; } catch { return; }
    if (starts !== 1) return;
    for (let i = 0; i < 20 && !model.run(event.run_id); i++) { await model.refresh(); if (!model.run(event.run_id)) await new Promise(r => setTimeout(r, 150)); }
    if (!model.run(event.run_id)) return;
    center.setAside(true, event.run_id);
    await selectRun(event.run_id);
  }
  client.on('event', event => {
    if (event?.kind !== 'overseer_action') return;
    const action = event.payload?.action;
    if (['focus', 'open_review', 'open_file', 'open_worktree', 'show_work'].includes(action)) lookAction(event).catch(e => say('overseer look: ' + e.message));
    if (action === 'start') startedByRequest(event).catch(e => say('overseer start: ' + e.message));
  });

  /** The Overseer view shows the agent's chat; in the workspace it keeps Overseer's conversation (AC-250). */
  async function showInCenter(runId) {
    // (Telling the view which agent is selected would switch it to that agent's chat.)
    await center.select(runId);
  }

  /** Shows an agent: its chat, and its review beside it when it has changes (Gate K). */
  async function selectRun(runId, { follow, reveal = true, keepConversation = false, force } = {}) {
    const picked = model.run(runId) || (await model.refresh(), model.run(runId));
    if (!picked) return;
    selectedRun = runId;
    // Opening an agent at its end opens its review when it has changes (Gate K): it is reviewed (AC-254).
    if (!ACTIVE.has(picked.status)) markReviewed(model.rootRun(picked)?.id || runId);
    context.workspaceState.update('overseer.selectedRun', runId);
    // The Worktree view shows the selected agent's worktree (AC-233).
    head.select(model.rootRun(picked)?.id || runId).catch(error => say('head: ' + error.message));
    await arrangement.show(runId, { follow, force });
    say(`selected ${runId} (${arrangement.current}${keepConversation ? ', beside the conversation' : ''})`);
    // Opened from Overseer's conversation, the conversation keeps its tab (AC-257).
    if (!keepConversation) await showInCenter(runId);
    if (reveal) revealInTree(runId);
    model.emitter.fire();
  }

  // AC-257: Overseer's conversation and the agent's head are one action apart. Going to the
  // conversation leaves the agent's head as it is (its files, cursor and scroll stay open beside);
  // going back focuses the head again and, when the Overseer tab showed the agent's chat, that chat.
  let leftAgent; // { runId, chat }: the agent the owner left for the conversation
  async function backToOverseer() {
    const run = (head.runId && model.run(head.runId)) || (selectedRun && model.run(selectedRun));
    const root = run && (model.rootRun(run) || run);
    if (root) leftAgent = { runId: root.id, chat: center.mode === 'chat' && center.chatRun && (model.rootRun(model.run(center.chatRun) || {}) || {}).id === root.id };
    await center.open({ column: center.panel?.viewColumn || vscode.ViewColumn.One });
    if (center.aside) center.setAside(false);
    center.setMode('composer');
    center.focus('composer');
    if (root) center.headAgent({ runId: root.id, title: agentTitle(root) });
    say(`back to Overseer's conversation${root ? ` (from ${root.id})` : ''}`);
  }
  async function backToAgent(runId) {
    const id = runId || leftAgent?.runId || head.runId || selectedRun;
    const run = id && model.run(id);
    if (!run) { vscode.window.showInformationMessage('No agent to go back to: open one from the side bar or the conversation.'); return; }
    const root = model.rootRun(run) || run;
    // In the Overseer window the right panel is the agent's chat again (AC-264).
    const chat = (leftAgent?.runId === root.id && leftAgent.chat) || overseerWindow.active;
    leftAgent = undefined;
    if (chat) { selectedRun = root.id; await center.select(root.id); }
    // Its head is still open: straight back there. Closed meanwhile: opened again where it was left.
    if (!(await head.focus(root.id))) {
      await selectRun(root.id, { keepConversation: !chat, force: true });
      await head.focus(root.id);
    }
    say(`back to the agent ${root.id}`);
  }
  /** One key for both ways (⌥⌘U): in the conversation, back to the agent; anywhere else, to the conversation. */
  async function switchAgentOverseer() {
    const tab = vscode.window.tabGroups.activeTabGroup.activeTab;
    const inConversation = !!tab?.input?.viewType?.endsWith('overseer.center') && center.mode === 'composer';
    if (inConversation) await backToAgent(); else await backToOverseer();
  }

  /** Marks the agent in the side bar without taking focus. */
  function revealInTree(runId) {
    const node = agents.nodeFor(runId);
    if (node && agentsView.visible) agentsView.reveal(node, { select: true, focus: false, expand: false }).then(undefined, () => {});
  }

  /** Dragging an agent row into the editor area opens its chat there (AC-71). */
  function agentDrag() {
    return {
      dragMimeTypes: ['text/uri-list', 'application/vnd.code.tree.overseer.agents'],
      dropMimeTypes: [],
      handleDrag(source, data) {
        const runs = source.map(n => n.run?.id).filter(Boolean);
        if (!runs.length) return;
        data.set('text/uri-list', new vscode.DataTransferItem(runs.map(id => chatUri(id).toString()).join('\r\n')));
      },
    };
  }

  /** The task an agent row (or run id) stands for. */
  function agentTask(arg) {
    // From a key press in the side bar there is no argument: use the focused row.
    const node = arg || agentsView.selection?.[0];
    if (node?.task) return node.task;
    const run = model.run(typeof node === 'string' ? node : node?.run?.id || runArg(node));
    return run && model.task((model.rootRun(run) || run).task_id);
  }

  /** Filters the side bar's agents list (AC-69). */
  function setAgentFilter(filter) {
    agents.filter = filter;
    // VS Code sends the first tree change of each 200 ms window at once and holds the rest:
    // refresh the rows first, then the message.
    agents.refresh();
    // The match count sits beside the view title (not the debounced tree message).
    // Matches in the list shown (active agents, or archived ones under Show Archived).
    // How many agents the list shows, said in the search field while a search or a filter is on.
    const narrowed = filter || agents.statusFilter !== 'all' || agents.showArchived;
    const shown = narrowed ? agents.visibleTasks().length : 0;
    agentsView.description = undefined;
    searchView.setCount(narrowed ? `${shown} ${filter ? `match${shown === 1 ? '' : 'es'}` : `agent${shown === 1 ? '' : 's'}`}` : '');
    vscode.commands.executeCommand('setContext', 'overseer.agentsFiltered', !!filter);
    // Keep the selected agent in view (and selected) when the list changes shape.
    if (selectedRun && (!filter || filter.taskIds.has(model.run(selectedRun)?.task_id))) setTimeout(() => revealInTree(selectedRun), 150);
  }

  /** Search agents by title, prompt, message text, file, repository, account or status (daemon search). */
  let searchSeq = 0;
  /** Filters the Agents list: titles at once plus the daemon's matches (messages, files, repository, account, status). */
  async function runAgentSearch(q) {
    const mine = ++searchSeq;
    if (!q) { setAgentFilter(undefined); return; }
    const lower = q.toLowerCase();
    const local = (model.state.tasks || []).filter(t => (t.title || '').toLowerCase().includes(lower)).map(t => t.id);
    const ids = await search(q);
    if (mine === searchSeq) setAgentFilter({ query: q, taskIds: new Set([...local, ...ids]) });
  }
  /** Search Agents (⌥⌘F, command palette): puts the cursor in the side bar's search field. */
  async function searchAgents() { await searchView.focus(); }

  /** Read-only, empty files: the custom editor shows the chat instead of their content. */
  function chatFileSystem() {
    const emitter = new vscode.EventEmitter();
    const deny = () => { throw vscode.FileSystemError.NoPermissions('Overseer chats are read-only'); };
    return {
      onDidChangeFile: emitter.event,
      watch: () => new vscode.Disposable(() => {}),
      stat: uri => ({ type: uri.path.endsWith('.overseer-chat') ? vscode.FileType.File : vscode.FileType.Directory, ctime: 0, mtime: 0, size: 0, permissions: vscode.FilePermission.Readonly }),
      readDirectory: () => [], readFile: () => new Uint8Array(), createDirectory: deny, writeFile: deny, delete: deny, rename: deny,
    };
  }

  /** A virtual document for an agent's chat: dropping an agent in the editor opens it (AC-71). */
  function chatUri(runId) {
    const run = model.run(runId);
    const title = (run?.title || 'agent').replace(/[\\/:*?"<>|]/g, ' ').slice(0, 60);
    return vscode.Uri.from({ scheme: 'overseer-chat', path: `/${runId}/${title}.overseer-chat` });
  }

  async function newTask() {
    requireTrust();
    await model.refresh();
    // Repository
    const folders = vscode.workspace.workspaceFolders || [];
    const roots = [...new Set((await Promise.all(folders.map(f => gitRoot(f.uri.fsPath)))).filter(Boolean))];
    const repoPick = await vscode.window.showQuickPick([...roots.map(r => ({ label: path.basename(r), description: r, root: r })), { label: '$(folder) Choose repository…', browse: true }], { title: 'New agent: repository' });
    if (!repoPick) return;
    let repo = repoPick.root;
    if (repoPick.browse) {
      const uri = await vscode.window.showOpenDialog({ canSelectFolders: true, canSelectFiles: false, title: 'Repository for the agent' });
      if (!uri) return;
      repo = await gitRoot(uri[0].fsPath);
      if (!repo) throw new Error('That folder is not in a Git repository.');
    }
    // Harness
    const harnesses = await client.request('harness.list');
    // Harnesses by name (AC-245), never their ids.
    const NAME = { claude: 'Claude Code', codex: 'Codex', 'codex-app': 'Codex app-server', opencode: 'OpenCode', generic: 'Program' };
    const hPick = await vscode.window.showQuickPick(harnesses.map(h => ({ label: NAME[h.harness] || Plain.harness(h.harness), description: h.installed ? (h.version || 'installed') : 'not installed', detail: `Sub-agents: ${String(h.capabilities.children || 'unknown').replace(/_/g, ' ')}`, h })), { title: 'New agent: harness' });
    if (!hPick) return;
    const harness = hPick.h.harness;
    if (!hPick.h.installed) throw new Error(`${hPick.label} is not installed.`);
    // Account profile
    let profileId, program, args = [];
    if (harness === 'generic') {
      program = await vscode.window.showInputBox({ title: 'Executable path', prompt: 'Absolute path of the program to run (no shell).', validateInput: v => path.isAbsolute(v) ? undefined : 'Use an absolute path' });
      if (!program) return;
      const argText = await vscode.window.showInputBox({ title: 'Arguments (JSON array of strings)', value: '[]', validateInput: v => { try { const a = JSON.parse(v); return Array.isArray(a) && a.every(x => typeof x === 'string') ? undefined : 'Must be a JSON array of strings'; } catch { return 'Must be a JSON array of strings'; } } });
      if (argText === undefined) return;
      args = JSON.parse(argText);
    } else {
      // Only accounts whose provider this harness accepts (docs/rfcs/account-governance.md).
      await refreshAccounts();
      const compatible = (model.accounts || []).filter(a => (a.harnesses || []).includes(harness));
      const statuses = compatible.map(a => model.profileStatus.get(a.id));
      const pPick = await vscode.window.showQuickPick(compatible.map((a, i) => ({ label: a.name, description: `${statuses[i]?.logged_in ? 'signed in' : 'not signed in'}${a.account?.email ? ' · ' + a.account.short : statuses[i]?.identity?.plan ? ' · ' + statuses[i].identity.plan : ''} · ${a.kind === 'follows-app' ? 'follows the desktop app (can change)' : 'fixed account'}`, detail: statuses[i]?.detail, p: model.profile(a.id) || { id: a.id, name: a.name }, ok: statuses[i]?.logged_in })),
        { title: `New agent: account for ${hPick.label} (only compatible accounts; account login only, no API keys)` });
      if (!pPick) return;
      if (!pPick.ok) {
        const choice = await vscode.window.showWarningMessage(`${pPick.p.name} is not signed in.`, 'Sign In', 'Launch anyway');
        if (choice === 'Sign In') { await signIn(pPick.p.id); return; }
        if (choice !== 'Launch anyway') return;
      }
      profileId = pPick.p.id;
    }
    // Workspace mode
    const mode = await vscode.window.showQuickPick([
      { label: '$(git-branch) New worktree', description: 'recommended', detail: 'Isolated branch and worktree; your checkout is not touched.', mode: 'worktree' },
      { label: '$(repo) Current checkout', detail: 'Works directly in your checkout. Existing staged, unstaged, untracked and unsaved work is recorded and preserved.', mode: 'current' },
    ], { title: 'New agent: workspace' });
    if (!mode) return;
    let targetRef;
    if (mode.mode === 'worktree') {
      const info = await client.request('repo.inspect', { path: repo });
      const refPick = await vscode.window.showQuickPick([{ label: `HEAD (${info.branch || 'detached'})`, ref: '' }, ...info.branches.map(b => ({ label: b, ref: b }))], { title: 'Start the worktree from…' });
      if (!refPick) return;
      targetRef = refPick.ref || undefined;
    }
    const unsaved = vscode.workspace.textDocuments.filter(d => d.isDirty && d.uri.scheme === 'file' && !path.relative(repo, d.uri.fsPath).startsWith('..'));
    if (mode.mode === 'current' && unsaved.length) {
      const choice = await vscode.window.showWarningMessage(`${unsaved.length} unsaved editor(s) in this checkout. They stay as labeled unsaved drafts; the agent only sees files on disk.`, { modal: true }, 'Continue');
      if (choice !== 'Continue') return;
    }
    const model_ = harness === 'generic' ? '' : await vscode.window.showInputBox({ title: 'Model (optional)', prompt: 'Leave empty for the harness default.' });
    if (model_ === undefined) return;
    let approvalPolicy;
    if (harness === 'codex-app') {
      const pick = await vscode.window.showQuickPick([
        { label: 'on-request', detail: 'The model asks when it needs to leave the sandbox (recommended).' },
        { label: 'untrusted', detail: 'Ask before running anything that is not a known read-only command.' },
        { label: 'never', detail: 'Never ask; sandbox limits apply.' },
      ], { title: 'Codex approval policy (requests appear in the run panel; never auto-approved)' });
      if (!pick) return;
      approvalPolicy = pick.label;
    }
    const prompt = await vscode.window.showInputBox({ title: 'What should the agent do?', prompt: harness === 'generic' ? 'Optional first line sent to stdin' : 'What should the agent do?', ignoreFocusOut: true });
    if (prompt === undefined || (!prompt && harness !== 'generic')) return;
    const title = (prompt || path.basename(program || 'task')).slice(0, 60);
    const created = await client.request('task.create', { repo, harness, profile_id: profileId, workspace_mode: mode.mode, target_ref: targetRef, model: model_ || undefined,
      prompt, title, program, args, approval_policy: approvalPolicy, unsaved: unsaved.map(d => path.relative(repo, d.uri.fsPath)) });
    if (created.launch_error) vscode.window.showErrorMessage(`Overseer could not launch ${harness}: ${created.launch_error}`);
    await model.refresh();
    await selectRun(created.run.id, { follow: vscode.workspace.getConfiguration('overseer').get('followNewRuns', true) });
  }

  async function signIn(profileId) {
    requireTrust();
    const profile = model.profile(profileId);
    let device = false;
    if (profile?.harness === 'codex') {
      const how = await vscode.window.showQuickPick([
        { label: '$(globe) Sign in with ChatGPT in the browser', detail: 'Opens the ChatGPT sign-in page on this Mac.', device: false },
        { label: '$(device-mobile) Sign in with a device code', detail: 'Shows a code to enter on another browser or device (needs device-code sign-in enabled in ChatGPT security settings).', device: true },
      ], { title: `Sign in ${profile.name}` });
      if (!how) return;
      device = how.device;
    }
    const cmd = await client.request('profile.login_command', { id: profileId, device });
    const terminal = vscode.window.createTerminal({ name: `Sign in: ${cmd.profile.name}`, shellPath: cmd.program, shellArgs: cmd.args, env: cmd.env });
    terminal.show();
    const done = vscode.window.onDidCloseTerminal(async t => {
      if (t !== terminal) return;
      done.dispose();
      await refreshAccounts();
    });
    context.subscriptions.push(done);
  }

  /**
   * Merge (never automatic, AC-243): one confirmation that lists the files that land (the untracked
   * ones apart), then the merge; conflicts stop in the worktree, where the agent combines them and
   * the chat offers Finish merge or Cancel merge. See landing.js.
   */
  async function mergeBack(arg) {
    requireTrust();
    return landing.merge(arg);
  }

  /** Interrupts every active agent and stops the daemon, after confirmation. Nothing respawns it. */
  async function stopAll() {
    await model.refresh();
    const active = (model.state.runs || []).filter(r => !r.parent_run_id && ACTIVE.has(r.status));
    const detail = active.length
      ? `These agents will be interrupted:\n${active.map(r => `• ${r.harness}: ${r.title} (${r.status.replace(/_/g, ' ')})`).join('\n')}\n\nWorktrees and history are kept. Other VS Code windows will not restart the daemon.`
      : 'No agents are running. The daemon stops; worktrees and history are kept.';
    const ok = await vscode.window.showWarningMessage(active.length ? `Stop ${active.length} running agent${active.length === 1 ? '' : 's'} and the Overseer daemon?` : 'Stop the Overseer daemon?', { modal: true, detail }, 'Stop Agents and Daemon');
    if (ok !== 'Stop Agents and Daemon') return;
    client.stopped = true;
    let result;
    try { result = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: 'Stopping Overseer agents…' }, () => client.request('daemon.stop_all')); }
    catch (error) { client.stopped = false; throw error; }
    say('stop_all: ' + JSON.stringify(result));
    const left = result.remaining?.length ? ` ${result.remaining.length} could not be stopped; check Activity Monitor.` : '';
    vscode.window.showInformationMessage(`Overseer stopped ${result.stopped.length} agent${result.stopped.length === 1 ? '' : 's'} and its daemon.${left}`);
  }

  /** On reopening VS Code: say which agents kept running while it was closed (once per notice). */
  async function announceBackgroundAgents() {
    const { notice } = await client.request('daemon.last_notice');
    const seen = context.globalState.get('overseer.noticeSeen', 0);
    if (!notice || notice.seq <= seen) return;
    await context.globalState.update('overseer.noticeSeen', notice.seq);
    const active = (model.state.runs || []).filter(r => !r.parent_run_id && ACTIVE.has(r.status));
    const names = (notice.payload?.runs || []).map(r => `${r.harness}: ${r.title}`).join('; ');
    const choice = await vscode.window.showInformationMessage(`Overseer agents kept running while VS Code was closed: ${names}. ${active.length} still active.`, 'Show Agents', ...(active.length ? ['Stop Agents and Daemon'] : []));
    if (choice === 'Show Agents') await vscode.commands.executeCommand('overseer.agents.focus');
    else if (choice) await stopAll();
  }

  /** Archives finished tasks older than overseer.history.autoArchiveDays (AC-63); never deletes anything. */
  let lastAutoArchive = 0;
  async function autoArchive() {
    const days = vscode.workspace.getConfiguration('overseer').get('history.autoArchiveDays', 14);
    if (!days || Date.now() - lastAutoArchive < 3600000 || !client.connected) return;
    lastAutoArchive = Date.now();
    const cutoff = Date.now() - days * 86400000;
    for (const t of model.state.tasks || []) {
      if (t.archived_ms) continue;
      const runs = (model.state.runs || []).filter(r => r.task_id === t.id);
      if (!runs.length || runs.some(r => ACTIVE.has(r.status))) continue;
      const last = Math.max(...runs.map(r => r.ended_ms || r.created_ms));
      if (last < cutoff) await client.request('task.archive', { task_id: t.id, archived: true }).catch(error => say('auto-archive: ' + error.message));
    }
  }

  /** Removes the worktrees of archived tasks; branches are kept; uncommitted work needs its own confirmation. */
  async function cleanupArchived() {
    requireTrust();
    await model.refresh();
    const archived = new Set(archivedTasks());
    const plans = [];
    for (const t of (model.state.tasks || []).filter(t => archived.has(t.id))) {
      const ws = model.workspace(t.workspace_id);
      if (!ws || ws.kind !== 'worktree' || ws.removed_ms) continue;
      const plan = await client.request('workspace.cleanup_plan', { workspace_id: ws.id }).catch(e => { say('cleanup plan: ' + e.message); return undefined; });
      if (!plan || !plan.removable) continue;
      const d = plan.dirty || {};
      const dirty = [...(d.staged || []), ...(d.unstaged || []), ...(d.untracked || []), ...(d.conflicted || [])].length;
      plans.push({ task: t, ws, dirty });
    }
    if (!plans.length) { vscode.window.showInformationMessage('No archived worktrees to clean up.', { modal: true }); return; }
    const clean = plans.filter(p => !p.dirty), dirty = plans.filter(p => p.dirty);
    const detail = `Branches are kept, so committed work stays in Git.\n\n${clean.length ? `Clean (${clean.length}):\n${clean.slice(0, 15).map(p => '• ' + p.task.title).join('\n')}` : ''}${dirty.length ? `\n\nWith uncommitted work (${dirty.length}), kept unless you choose to discard it:\n${dirty.slice(0, 15).map(p => `• ${p.task.title} (${p.dirty} file${p.dirty === 1 ? '' : 's'})`).join('\n')}` : ''}`;
    const actions = [...(clean.length ? [`Remove ${clean.length} Clean`] : []), ...(dirty.length ? ['Remove All, Discarding Uncommitted Work'] : [])];
    const choice = await vscode.window.showWarningMessage(`Clean up worktrees of ${plans.length} archived agent${plans.length === 1 ? '' : 's'}?`, { modal: true, detail }, ...actions);
    if (!choice) return;
    let targets = clean;
    if (choice.startsWith('Remove All')) {
      const sure = await vscode.window.showWarningMessage(`Discard uncommitted work in ${dirty.length} worktree${dirty.length === 1 ? '' : 's'}?`, { modal: true, detail: 'This cannot be undone. Branches and committed work are kept.' }, 'Discard and Remove');
      if (sure !== 'Discard and Remove') return;
      targets = plans;
    }
    let removed = 0;
    for (const p of targets) { try { await client.request('workspace.cleanup', { workspace_id: p.ws.id, discard_dirty: p.dirty > 0 }); removed++; } catch (error) { say('cleanup: ' + error.message); } }
    await model.refresh();
    vscode.window.setStatusBarMessage(`$(trash) Removed ${removed} worktree${removed === 1 ? '' : 's'}; branches kept`, 4000);
  }

  /** A notification's click (AC-52, AC-240): the Overseer view, or that agent. */
  async function openUri(uri) {
    if (uri.path === '/open-center') return vscode.commands.executeCommand('overseer.openCenter');
    const runId = agentFromUri(uri);
    if (!runId) return undefined;
    await model.refresh();
    if (!model.run(runId)) { vscode.window.showInformationMessage('That agent is no longer in Overseer.'); return undefined; }
    await selectRun(runId); center.focus('chat');
    return runId;
  }

  /** The agents whose chat or review is on screen in this window (AC-242). */
  const onScreen = () => agentsOnScreen({ model, center, outputs, review });
  const agentTitle = run => { const root = model.rootRun(run) || run; return model.task(root.task_id)?.title || root.title || 'an agent'; };
  const repoName = run => path.basename(model.task((model.rootRun(run) || run).task_id)?.repo_root || '');

  /**
   * ⌥⌘Y / ⌥⌘⌫ (AC-242): answer the request of the agent on screen. With none on screen (or two),
   * show which agent asks for which tool first, and answer only the one picked.
   */
  async function answerPermission(allow) {
    requireTrust();
    const verb = allow ? 'Allow' : 'Deny';
    const target = permissionTarget(model.state.runs, onScreen(), r => (model.rootRun(r) || r).id);
    if (target.none) { vscode.window.setStatusBarMessage('$(check) No permission request is waiting', 2500); return; }
    let run = target.run;
    if (!run) {
      const pick = await vscode.window.showQuickPick(target.choose.map(r => ({ label: `$(bell-dot) ${agentTitle(r)}`, description: `wants to use ${r.attention.tool || 'a tool'}`, detail: repoName(r) || undefined, run: r })),
        { title: `${verb} which request?`, placeHolder: `${target.choose.length === 1 ? 'This request is' : 'These requests are'} not on screen: pick the one to ${verb.toLowerCase()}`, matchOnDescription: true });
      if (!pick) return;
      run = pick.run;
    }
    await client.request('run.permission', { run_id: run.id, request_id: String(run.attention.request_id), allow });
    vscode.window.setStatusBarMessage(`$(${allow ? 'check' : 'circle-slash'}) ${allow ? 'Allowed' : 'Denied'} ${agentTitle(run)}: ${run.attention.tool || 'its request'}`, 4000);
    model.scheduleRefresh();
  }

  /**
   * The agent a palette command acts on (AC-242): the one it was given (a side bar row, a run id),
   * else the one on screen, else the owner picks; never an empty id. `fits` limits the choice.
   */
  async function pickAgent(arg, { title, fits = () => true, none }) {
    const target = commandTarget(arg, { runs: model.state.runs, onScreen: onScreen(), fits, selected: selectedRun && (model.rootRun(model.run(selectedRun) || {}) || {}).id });
    if (target.id) return target.id;
    if (!target.choose.length) { vscode.window.showInformationMessage(none); return undefined; }
    const pick = await vscode.window.showQuickPick(target.choose.map(r => ({ label: `$(${{ waiting_for_user: 'bell-dot', running: 'sync', starting: 'sync', queued: 'clock', completed: 'check', failed: 'error', interrupted: 'circle-slash' }[r.status] || 'circle'}) ${agentTitle(r)}`,
      description: [repoName(r), r.status.replace(/_/g, ' ')].filter(Boolean).join(' · '), id: r.id })), { title, placeHolder: 'No agent is on screen: pick one', matchOnDescription: true });
    return pick?.id;
  }
  const hasWorktree = r => { const ws = model.workspace(r.workspace_id); return !!ws && ws.kind === 'worktree' && !ws.removed_ms; };
  const takesFollowUps = r => !String(r.capabilities?.follow_up || '').startsWith('unsupported');

  async function refreshAccounts() {
    await model.refresh();
    try { const list = await client.request('account.list'); model.accounts = list.accounts.map(a => ({ ...a, name: accountName(a) })); model.providers = list.providers; } catch (e) { say('account.list: ' + e.message); }
    await Promise.all(model.state.profiles.map(async p => {
      try { model.profileStatus.set(p.id, await client.request('profile.status', { id: p.id })); } catch (e) { say(e.message); }
      try { (model.accountUsage ||= new Map()).set(p.id, await client.request('account.usage', { id: p.id })); } catch { /* older daemon */ }
    }));
    accounts.emitter.fire();
  }

  context.subscriptions.push(
    vscode.commands.registerCommand('overseer.newTask', guard(async () => { requireTrust(); await model.refresh(); await newTaskPanel.open(); })),
    vscode.commands.registerCommand('overseer.newTaskQuick', guard(newTask)),
    vscode.commands.registerCommand('overseer.refresh', guard(async () => { await model.refresh(true); })),
    vscode.commands.registerCommand('overseer.filterSwarmJobs', guard(whenOn('swarm', async () => {
      const choices = [['all', 'All jobs'], ['ready', 'Ready'], ['running', 'Running'],
        ['submitted', 'Awaiting review'], ['blocked', 'Blocked'], ['accepted', 'Accepted'], ['failed', 'Failed']]
        .map(([value, label]) => ({ value, label, description: agents.swarmStatusFilter === value ? '✓' : '' }));
      const picked = await vscode.window.showQuickPick(choices, { title: 'Show Swarm jobs' });
      if (picked) agents.setSwarmStatusFilter(picked.value);
    }))),
    vscode.commands.registerCommand('overseer.startSwarm', guard(whenOn('swarm', async () => {
      requireTrust();
      const folders = vscode.workspace.workspaceFolders || [];
      const roots = [...new Set((await Promise.all(folders.map(f => gitRoot(f.uri.fsPath)))).filter(Boolean))];
      if (!roots.length) throw new Error('Open a Git repository to start a swarm.');
      const repo = roots.length === 1 ? roots[0] : (await vscode.window.showQuickPick(
        roots.map(r => ({ label: path.basename(r), description: r, root: r })), { title: 'Start swarm: repository' }))?.root;
      if (!repo) return;
      const category = await vscode.window.showInputBox({ title: 'Start swarm: category', placeHolder: 'Backend security' });
      if (!category) return;
      const objective = await vscode.window.showInputBox({ title: `Start swarm: ${category}`, placeHolder: 'What should this swarm achieve?' });
      if (!objective) return;
      const result = await swarmControls.start({ category, objective, repositories: [repo] }, {
        pickAccounts: async () => {
          const profiles = await client.request('profile.list');
          const picked = await vscode.window.showQuickPick(profiles.map(p => ({ label: accountName(p), description: p.account?.short || p.harness, id: p.id })),
            { title: `Accounts ${category} may use (asked once)`, canPickMany: true });
          return picked?.map(p => p.id);
        },
        confirm: async (readback, text) => (await vscode.window.showInformationMessage(readback.summary,
          { modal: true, detail: text }, 'Start swarm')) === 'Start swarm'
      });
      if (result.status === 'started') vscode.window.showInformationMessage(`${category} swarm started: ${result.run.start?.summary || ''}`);
      else if (result.status === 'blocked') vscode.window.showWarningMessage(`The ${category} swarm cannot start: ${result.reason}.`);
      else if (result.status === 'needs_account_selection') vscode.window.showWarningMessage('Choose at least one account for this category.');
      else if (result.status === 'readback_changed') vscode.window.showWarningMessage('The accounts or limits changed twice while confirming; nothing was started.');
    }))),
    vscode.commands.registerCommand('overseer.pauseSwarm', guard(whenOn('swarm', async arg => {
      requireTrust(); const id = await swarmId(arg, ['planning', 'running']);
      if (id) await swarmControls.pause(id);
    }))),
    vscode.commands.registerCommand('overseer.resumeSwarm', guard(whenOn('swarm', async arg => {
      requireTrust(); const id = await swarmId(arg, ['paused']);
      if (id) await swarmControls.resume(id);
    }))),
    vscode.commands.registerCommand('overseer.stopSwarm', guard(whenOn('swarm', async arg => {
      requireTrust(); const id = await swarmId(arg, ['planning', 'running', 'paused', 'stalled', 'draining', 'stopping']);
      if (id) await swarmControls.stop(id);
    }))),
    vscode.commands.registerCommand('overseer.turnSwarmOff', guard(whenOn('swarm', async arg => {
      requireTrust(); const id = await swarmId(arg, ['planning', 'running', 'paused', 'stalled']);
      if (id) await swarmControls.off(id);
    }))),
    vscode.commands.registerCommand('overseer.extendSwarmDeadline', guard(whenOn('swarm', async arg => {
      requireTrust(); const id = await swarmId(arg, ['planning', 'running', 'paused', 'stalled', 'draining']);
      if (!id) return;
      const choice = await vscode.window.showQuickPick([
        { label: '30 minutes', additionalMs: 30 * 60 * 1000 },
        { label: '1 hour', additionalMs: 60 * 60 * 1000 },
        { label: '2 hours', additionalMs: 2 * 60 * 60 * 1000 }
      ], { title: 'Extend Swarm deadline', placeHolder: 'Choose how much time to add' });
      if (!choice) return;
      const result = await swarmControls.extendDeadline(id, choice.additionalMs);
      vscode.window.showInformationMessage(`Swarm deadline extended until ${new Date(result.deadline_at_ms).toLocaleString()}. Account allocation is unchanged.`);
    }))),
    vscode.commands.registerCommand('overseer.selectRun', guard(runId => selectRun(runId))),
    // The agent's head (AC-233) and the way between it and Overseer's conversation (AC-257).
    vscode.commands.registerCommand('overseer.head.toggleMode', guard(() => head.toggleMode(head.runId || selectedRun))),
    vscode.commands.registerCommand('overseer.head.follow', guard(() => head.setMode(head.runId || selectedRun, 'follow'))),
    vscode.commands.registerCommand('overseer.head.diffsOnly', guard(() => head.setMode(head.runId || selectedRun, 'diffs'))),
    vscode.commands.registerCommand('overseer.head.openFile', guard(rel => head.openFile(head.runId, typeof rel === 'string' ? rel : rel?.rel, { fromTree: true }))),
    vscode.commands.registerCommand('overseer.head.refresh', guard(() => head.refresh())),
    vscode.commands.registerCommand('overseer.head.catchUp', guard(() => head.catchUp())),
    vscode.commands.registerCommand('overseer.backToOverseer', guard(() => backToOverseer())),
    vscode.commands.registerCommand('overseer.backToAgent', guard(() => backToAgent())),
    vscode.commands.registerCommand('overseer.switchAgentOverseer', guard(() => switchAgentOverseer())),
    vscode.commands.registerCommand('overseer.openReview', guard(async arg => { const id = runArg(arg); if (!id) return; selectedRun = id; await arrangement.openReview(id); await showInCenter(id); })),
    vscode.commands.registerCommand('overseer.openEdit', guard(async (runId, rel) => {
      const run = model.run(runId) || (await model.refresh(), model.run(runId));
      if (!run) return;
      return review.revealEdit(model.rootRun(run).id, rel);
    })),
    vscode.commands.registerCommand('overseer.showOutput', guard(arg => outputs.show(runArg(arg), { preserveFocus: false }))),
    vscode.commands.registerCommand('overseer.openAgentToSide', guard(arg => outputs.show(runArg(arg), { preserveFocus: false }))),
    vscode.commands.registerCommand('overseer.followUp', guard(async arg => {
      requireTrust();
      const runId = await pickAgent(arg, { title: 'Send a follow-up to which agent?', fits: takesFollowUps, none: 'No agent takes a follow-up.' });
      if (!runId) return;
      const text = await vscode.window.showInputBox({ title: `Follow-up for ${agentTitle(model.run(runId) || {})} only`, ignoreFocusOut: true });
      if (!text) return;
      await client.request('run.follow_up', { run_id: runId, prompt: text });
      await model.refresh();
    })),
    vscode.commands.registerCommand('overseer.interrupt', guard(async arg => { requireTrust(); const id = await pickAgent(arg, { title: 'Stop which agent?', fits: r => ACTIVE.has(r.status), none: 'No agent is running.' }); if (id) await client.request('run.interrupt', { run_id: id }); })),
    vscode.commands.registerCommand('overseer.selectComparison', guard(arg => review.pickComparison(runArg(arg)))),
    vscode.commands.registerCommand('overseer.cleanupWorkspace', guard(async arg => {
      requireTrust();
      const run = model.run(await pickAgent(arg, { title: 'Clean up which agent\'s worktree?', fits: r => hasWorktree(r) && !ACTIVE.has(r.status), none: 'No finished agent has a worktree to clean up.' }));
      if (!run) return;
      const plan = await client.request('workspace.cleanup_plan', { workspace_id: run.workspace_id });
      if (!plan.removable) { vscode.window.showInformationMessage(`Not removing: ${plan.reason}.`); return; }
      const d = plan.dirty || {};
      const files = [...(d.staged || []).map(f => 'staged ' + f.path), ...(d.unstaged || []).map(f => 'unstaged ' + f.path), ...(d.untracked || []).map(f => 'untracked ' + f), ...(d.conflicted || []).map(f => 'conflicted ' + f)];
      const detail = `${plan.workspace.path}\nBranch ${plan.workspace.branch} is kept.\n${files.length ? 'Uncommitted work that will be LOST:\n' + files.slice(0, 30).join('\n') : 'No uncommitted work.'}`;
      const confirm = await vscode.window.showWarningMessage('Remove this worktree?', { modal: true, detail }, files.length ? 'Discard and Remove' : 'Remove');
      if (!confirm) return;
      await client.request('workspace.cleanup', { workspace_id: run.workspace_id, discard_dirty: files.length > 0 });
      await model.refresh();
    })),
    vscode.commands.registerCommand('overseer.addProfile', guard(async arg => {
      await refreshAccounts();
      let provider = arg?.provider;
      if (!provider) {
        const pick = await vscode.window.showQuickPick((model.providers || []).map(p => ({ label: p.label, description: p.available ? (p.harnesses || []).join(', ') : 'unavailable', detail: p.available ? `Sign-in: ${p.sign_in}` : p.why, p })), { title: 'Add account: provider (account login only; no API keys)' });
        if (!pick) return;
        provider = pick.p;
      }
      if (!provider.available || provider.id === 'devin') { vscode.window.showInformationMessage(`${provider.label} is not available: ${provider.why || 'no account login'}`); return; }
      const name = await vscode.window.showInputBox({ title: `Name for the ${provider.label} account`, prompt: 'For example: ChatGPT (work)', validateInput: v => v.trim() ? undefined : 'Required' });
      if (!name) return;
      const created = await client.request('account.create', { provider: provider.id, name });
      await refreshAccounts();
      if (provider.id === 'local') { vscode.window.showInformationMessage(`Created ${name}. OpenCode uses local providers; configure them in its folder: ${created.account.home}`); return; }
      const choice = await vscode.window.showInformationMessage(`Created ${created.account.name}. Sign in now?`, 'Sign In');
      if (choice) await signIn(created.account.id);
    })),
    vscode.commands.registerCommand('overseer.removeAccount', guard(async arg => {
      const p = arg?.profile; if (!p) return;
      const ok = await vscode.window.showWarningMessage(`Remove the account ${p.name}?`, { modal: true, detail: 'Its own credential folder is deleted. Runs keep their history. No other account, and no desktop-app login, is affected.' }, 'Remove Account');
      if (ok !== 'Remove Account') return;
      await client.request('account.remove', { id: p.id });
      await refreshAccounts();
    })),
    vscode.commands.registerCommand('overseer.signIn', guard(async arg => signIn(arg?.profile?.id || (await vscode.window.showQuickPick(model.state.profiles.map(p => ({ label: p.name, id: p.id }))))?.id))),
    vscode.commands.registerCommand('overseer.signOut', guard(async arg => {
      const p = arg?.profile; if (!p) return;
      const ok = await vscode.window.showWarningMessage(`Sign out ${p.name}? Only this account is affected.`, { modal: true }, 'Sign Out');
      if (!ok) return;
      await client.request('profile.logout', { id: p.id });
      await refreshAccounts();
    })),
    vscode.commands.registerCommand('overseer.renameProfile', guard(async arg => {
      const p = arg?.profile; if (!p) return;
      const name = await vscode.window.showInputBox({ title: 'Rename profile', value: p.name });
      if (!name) return;
      await client.request('profile.rename', { id: p.id, name });
      await model.refresh();
    })),
    vscode.commands.registerCommand('overseer.refreshAccounts', guard(refreshAccounts)),
    vscode.workspace.onDidChangeConfiguration(e => {
      if (e.affectsConfiguration('overseer.experimental')) model.refresh(true);
    }),
    vscode.commands.registerCommand('overseer.autoUsage', guard(() => autoUsage.show())),
    vscode.commands.registerCommand('overseer.showCapabilities', guard(async () => {
      const list = await client.request('harness.list');
      const doc = await vscode.workspace.openTextDocument({ language: 'markdown', content: '# Harness capabilities (reported by this Overseer build)\n\n' + list.map(h =>
        `## ${h.harness} ${h.version || ''}\n\nExecutable: \`${h.program || 'n/a'}\` (${h.installed ? 'installed' : 'not installed'})\n\n` + Object.entries(h.capabilities).map(([k, v]) => `- **${k}**: ${v}`).join('\n')).join('\n\n') +
        '\n\n## Not integrated\n\n- **Gemini CLI**: not installed on this machine; no adapter yet.\n- **Devin**: skipped — no account-login CLI path without API keys/personal access tokens was available.\n' });
      await vscode.window.showTextDocument(doc, { preview: true });
    })),
    vscode.commands.registerCommand('overseer.stopAll', guard(stopAll)),
    vscode.commands.registerCommand('overseer.audioMode', guard(async () => {
      const audio = await client.request('audio.get');
      const trackName = { reactor: 'Reactor signals', system: 'System voice', commander: 'Private Commander' }[audio.track] || audio.track;
      const choice = await vscode.window.showQuickPick([
        { label: audio.enabled ? 'Turn Audio Mode off' : 'Turn Audio Mode on', action: 'toggle' },
        { label: 'Choose audio track…', action: 'track' },
        { label: 'Choose system voice…', action: 'voice' },
        { label: 'Import private Commander pack…', action: 'import' },
        { label: 'Preview cue…', action: 'preview' },
      ], { title: 'Overseer Audio Mode', placeHolder: `${trackName} · ${audio.enabled ? 'On' : 'Off'}` });
      if (!choice) return;
      async function importCommander() {
        const folders = await vscode.window.showOpenDialog({ title: 'Select your private Commander pack folder', canSelectFolders: true, canSelectFiles: false, canSelectMany: false });
        if (!folders?.length) return false;
        await client.request('audio.import_commander', { path: folders[0].fsPath });
        return true;
      }
      if (choice.action === 'toggle') {
        const result = await client.request('audio.set', { enabled: !audio.enabled });
        vscode.window.showInformationMessage(`Overseer Audio Mode ${result.enabled ? 'on' : 'off'}.`);
      } else if (choice.action === 'track') {
        const chosen = await vscode.window.showQuickPick([
          { label: 'Reactor signals', detail: 'Twelve short original synth cues', track: 'reactor' },
          { label: 'System voice', detail: 'Speech generated on this Mac', track: 'system' },
          { label: 'Private Commander', detail: audio.commander_imported ? 'Uses your local imported folder' : 'Choose a private folder first', track: 'commander' },
        ], { title: 'Choose audio track' });
        if (!chosen) return;
        if (chosen.track === 'commander' && !audio.commander_imported && !await importCommander()) return;
        await client.request('audio.set', { track: chosen.track });
        vscode.window.setStatusBarMessage(`Overseer audio: ${chosen.label}`, 3000);
      } else if (choice.action === 'voice') {
        const voices = await client.request('audio.voices');
        const chosen = await vscode.window.showQuickPick([
          { label: 'System default', voice: '' },
          ...voices.map(item => ({ label: item.name, description: item.locale, voice: item.name })),
        ], { title: 'Choose system voice', matchOnDescription: true });
        if (chosen) await client.request('audio.set', { track: 'system', voice: chosen.voice });
      } else if (choice.action === 'import') {
        if (await importCommander()) vscode.window.showInformationMessage('Private Commander pack is ready. The files stay in your selected folder.');
      } else {
        const core = new Set(audio.default_keys);
        const cues = audio.manifest.filter(item => audio.track === 'reactor' || core.has(item.key));
        const cue = await vscode.window.showQuickPick(cues.map(item => ({ label: item.label, description: audio.track === 'reactor' ? `${item.duration.toFixed(2)}s` : undefined, detail: item.meaning, key: item.key })), { title: `Preview ${trackName} cue` });
        if (cue) await client.request('audio.preview', { key: cue.key });
      }
    })),
    vscode.commands.registerCommand('overseer.testNotification', guard(async () => {
      const { delivered_via: via } = await client.request('daemon.test_notice');
      const native = /^overseer-notifier \(ok\)/.test(via);
      vscode.window.showInformationMessage(native ? 'Sent a test notification from Overseer. If no banner appeared, allow Overseer in System Settings → Notifications.'
        : `Sent a test notification, but not as Overseer: ${via}. Allow Overseer in System Settings → Notifications to get Overseer-branded banners.`);
      return via;
    })),
    // Notification clicks open vscode://beelol.overseer/open-center (AC-52), or an agent's own
    // open-agent?run=<id> (AC-240): that agent's chat.
    vscode.window.registerUriHandler({ handleUri: uri => openUri(uri) }),
    vscode.commands.registerCommand('overseer.openCenter', guard(async () => { await model.refresh(); if (selectedRun && model.run(selectedRun)) await selectRun(selectedRun); else { await arrangement.chatOnly(); center.setMode('composer'); } })),
    // AC-264: the Overseer layout in one step; again (or Close Workspace) the owner's own layout.
    vscode.commands.registerCommand('overseer.openWorkspace', guard(async () => { await model.refresh(); await overseerWindow.toggle(); })),
    vscode.commands.registerCommand('overseer.closeWorkspace', guard(async () => { if (overseerWindow.active) await overseerWindow.leave(); })),
    // AC-251: the review (Follow) in its own window, to put on another screen.
    vscode.commands.registerCommand('overseer.popOutReview', guard(async arg => {
      await model.refresh();
      const id = (typeof arg === 'string' && arg) || arg?.run?.id || focusedAgent();
      if (!id) { vscode.window.showInformationMessage('No agent to follow yet: start one first.'); return; }
      selectedRun = (model.rootRun(model.run(id)) || model.run(id)).id;
      if (await arrangement.popOut(selectedRun, { follow: ACTIVE.has(model.run(selectedRun)?.status) || undefined })) return;
      // VS Code could not float the review: said once, with the version (AC-251).
      const told = context.globalState.get('overseer.popOutUnavailable');
      if (told === vscode.version) return;
      await context.globalState.update('overseer.popOutUnavailable', vscode.version);
      vscode.window.showInformationMessage(`VS Code ${vscode.version} could not move the review into its own window. Drag the review's tab out of the window, or use View: Move Editor into New Window.`);
    })),
    vscode.commands.registerCommand('overseer.returnReview', guard(() => arrangement.popIn())),
    // New Agent starts one directly (AC-236): home's box takes the task for a new agent this time.
    vscode.commands.registerCommand('overseer.newAgent', guard(async () => { requireTrust(); await arrangement.chatOnly(); center.setMode('composer'); center.composerTarget('agent'); center.focus('composer'); })),
    vscode.commands.registerCommand('overseer.whereAmI', guard(() => whereAmI())),
    // Talk to Overseer (AC-227): home, with the composer's target Overseer.
    vscode.commands.registerCommand('overseer.talk', guard(async () => { await arrangement.chatOnly(); center.setMode('composer'); center.panel?.webview.postMessage({ type: 'askOverseer', text: '' }); })),
    vscode.commands.registerCommand('overseer.voice.toggle', guard(() => voice.toggle())),
    vscode.commands.registerCommand('overseer.voice.open', guard(() => voice.open())),
    vscode.commands.registerCommand('overseer.voice.mute', guard(() => voice.mute())),
    vscode.commands.registerCommand('overseer.voice.talkTo', guard(() => voice.talkTo())),
    vscode.commands.registerCommand('overseer.voice.cancel', guard(() => voice.cancel())),
    vscode.commands.registerCommand('overseer.voice.yes', guard(() => voice.answer(true))),
    vscode.commands.registerCommand('overseer.voice.no', guard(() => voice.answer(false))),
    vscode.commands.registerCommand('overseer.voice.simulate', guard(() => voice.simulate())),
    vscode.commands.registerCommand('overseer.resetGridLayout', guard(() => center.panel?.webview.postMessage({ type: 'gridReset' }))),
    vscode.commands.registerCommand('overseer.toggleGrid', guard(async () => {
      if (center.mode === 'grid') { center.setMode(selectedRun ? 'chat' : 'composer'); return; }
      // The grid opens only with something to show (AC-113); otherwise home, with a one-line note.
      if (!gridHasAgents()) { await goHome(`No agent is working or pinned yet, so the grid has nothing to show.${rollupNote()} Start one here.`); return; }
      await arrangement.enterGrid(); center.setMode('grid');
    })),
    vscode.commands.registerCommand('overseer.searchAgents', guard(searchAgents)),
    vscode.commands.registerCommand('overseer.filterAgents', guard(filterMenu)),
    vscode.commands.registerCommand('overseer.clearAgentSearch', guard(async () => { searchSeq++; setAgentFilter(undefined); searchView.clear(); })),
    vscode.commands.registerCommand('overseer.showArchived', guard(async () => { searchView.setFilter('archived'); setStatusFilter('archived'); vscode.commands.executeCommand('setContext', 'overseer.showArchived', true); })),
    vscode.commands.registerCommand('overseer.hideArchived', guard(async () => { searchView.setFilter('all'); setStatusFilter('all'); vscode.commands.executeCommand('setContext', 'overseer.showArchived', false); })),
    // Delete on an archived row (Show Archived) restores it, as it did in the Gate J rail.
    vscode.commands.registerCommand('overseer.archiveAgent', guard(async arg => { const task = agentTask(arg); if (task) { await client.request('task.archive', { task_id: task.id, archived: !task.archived_ms }); await model.refresh(); } })),
    vscode.commands.registerCommand('overseer.restoreAgent', guard(async arg => { const task = agentTask(arg); if (task) { await client.request('task.archive', { task_id: task.id, archived: false }); await model.refresh(); } })),
    vscode.commands.registerCommand('overseer.pinAgent', guard(async arg => { const id = runArg(arg); if (id) { await setPinned(id, true); agents.refresh(); center.push(); } })),
    vscode.commands.registerCommand('overseer.unpinAgent', guard(async arg => { const id = runArg(arg); if (id) { await setPinned(id, false); agents.refresh(); center.push(); } })),
    vscode.commands.registerCommand('overseer.switchAgent', guard(async () => {
      await model.refresh();
      const roots = (model.state.runs || []).filter(r => !r.parent_run_id).sort((a, b) => (ACTIVE.has(b.status) - ACTIVE.has(a.status)) || b.created_ms - a.created_ms);
      const pick = await vscode.window.showQuickPick(roots.map(r => { const t = model.task(r.task_id); const p = r.profile_id && model.profile(r.profile_id);
        return { label: `$(${{ waiting_for_user: 'bell-dot', running: 'sync', starting: 'sync', queued: 'clock', completed: 'check', failed: 'error', interrupted: 'circle-slash' }[r.status] || 'circle'}) ${t?.title || r.title}`,
          description: [path.basename(t?.repo_root || ''), p?.name].filter(Boolean).join(' · '), detail: undefined, run: r }; }), { title: 'Switch to agent', matchOnDescription: true, placeHolder: 'Search agents' });
      if (pick) { await selectRun(pick.run.id); center.focus('chat'); }
    })),
    // Presses queue up (AC-149): each one picks after the previous one has opened its agent, so two
    // quick presses never land on the same agent.
    vscode.commands.registerCommand('overseer.nextNeedsYou', guard(() => (needsQueue = needsQueue.then(nextNeedsYou, nextNeedsYou)))),
    vscode.commands.registerCommand('overseer.allowPermission', guard(async () => answerPermission(true))),
    vscode.commands.registerCommand('overseer.denyPermission', guard(async () => answerPermission(false))),
    vscode.commands.registerCommand('overseer.cleanupArchived', guard(cleanupArchived)),
    vscode.commands.registerCommand('overseer.stopSelected', guard(async () => { requireTrust(); const id = await pickAgent(undefined, { title: 'Stop which agent?', fits: r => ACTIVE.has(r.status), none: 'No agent is running.' }); if (id) await client.request('run.interrupt', { run_id: id }); })),
    vscode.commands.registerCommand('overseer.mergeBack', guard(mergeBack)),
    // Internal (the chat's and the review's buttons): cancel a merge stopped on conflicts; publish a repository with no remote.
    vscode.commands.registerCommand('overseer.cancelMerge', guard(async arg => { requireTrust(); const id = runArg(arg); if (id) await landing.cancel(id); })),
    vscode.commands.registerCommand('overseer.publishToGitHub', guard(async arg => { requireTrust(); const id = runArg(arg); if (id) await landing.publish(id); })),
    vscode.commands.registerCommand('overseer.openPullRequest', guard(async arg => { const id = await pickAgent(arg, { title: 'Open a pull request for which agent?', fits: hasWorktree, none: 'No agent has a worktree to open a pull request from.' }); if (id) await pullRequests.open(id); })),
    vscode.commands.registerCommand('overseer.startDaemon', guard(async () => { client.disposed = false; await client.start(); await model.refresh(); updateStatus(); })),
    vscode.commands.registerCommand('overseer.showLog', () => log.show()),
    vscode.commands.registerCommand('overseer.restartDaemonConnection', guard(async () => { client.dispose(); client.disposed = false; await client.start(); })),
    vscode.workspace.onDidGrantWorkspaceTrust(() => model.emitter.fire()),
  );

  // Mac notifications outside VS Code (AC-240): this window's focus and the chosen kinds.
  new Notices(context, client, { say });

  // Phone access (Gate N): its own status bar item, the Devices view and pairing.
  const phoneAccess = new PhoneAccess(context, client, { say, guard, requireTrust, looking: { model, center, outputs, selected: () => selectedRun } });

  updateStatus();
  try {
    await client.start();
    await model.refresh();
    refreshAccounts().catch(() => {});
    announceBackgroundAgents().catch(error => say('background notice check: ' + error.message));
    retireFocusMode(context, say).catch(error => say('focus mode: ' + error.message));
    overseerWindow.startup().catch(error => say('overseer window: ' + (error.stack || error.message)));
    // The first launch offers the Overseer layout, then (another time) the side bar.
    setTimeout(() => overseerWindow.offer().catch(error => say('layout offer: ' + error.message)).then(() => offerSideBar(context)).catch(error => say('side bar offer: ' + error.message)), 3000);
    const remembered = context.workspaceState.get('overseer.selectedRun');
    // Reopen where the user left off (AC-80) when VS Code did not restore the Overseer editor itself.
    if (remembered && model.run(remembered) && context.workspaceState.get('overseer.editorOpen', false)) {
      setTimeout(() => { if (!center.active && !overseerWindow.active) selectRun(remembered).catch(error => say('reopen: ' + error.message)); }, 2500);
    }
    if (remembered && model.run(remembered)) {
      selectedRun = remembered;
      // Show the remembered run as selected in the Agents view without stealing focus.
      const reveal = () => { const node = agents.nodeFor(remembered); if (node) agentsView.reveal(node, { select: true, focus: false, expand: false }).then(undefined, () => {}); };
      if (agentsView.visible) reveal();
      else { const once = agentsView.onDidChangeVisibility(e => { if (e.visible) { once.dispose(); setTimeout(reveal, 200); } }); context.subscriptions.push(once); }
    }
  } catch (error) {
    say('daemon start failed: ' + error.message);
    // A pinned dev window says it once ('unreachable') and keeps waiting for its instance.
    if (pin && !client.refusal) { updateStatus(); client.reconnectLater(); } else vscode.window.showErrorMessage(`Overseer could not start its daemon: ${Plain.plain(error.message, 300)}`);
  }
  return { client, model, review, outputs, selectRun, agents, agentsView, center, overseerWindow, arrangement, attention, phoneAccess, voice, openUri, selectedRun: () => selectedRun }; // exported for UI tests
}

function deactivate() { if (centerRef) centerRef.shuttingDown = true; client?.dispose(); }

module.exports = { activate, deactivate };
