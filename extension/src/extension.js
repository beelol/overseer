// Overseer VS Code extension: presentation, navigation and editor buffers.
// All durable state (tasks, runs, workspaces, events) lives in overseerd.
const vscode = require('vscode');
const path = require('path');
const { execFile } = require('child_process');
const { DaemonClient, resolveBinary } = require('./daemon-client');
const { Model, AgentsProvider, AccountsProvider, ACTIVE } = require('./views');
const { OutputPanels } = require('./output-panel');
const { Review } = require('./review');
const { CommandCenter } = require('./command-center');
const { Arrangement } = require('./arrangement');
const { NewTaskPanel } = require('./new-task');
const { PullRequests } = require('./pull-request');
const { TaskLauncher } = require('./task-launcher');
const { Steering } = require('./run-actions');
const { Dashboard } = require('./dashboard-mode');

let client;

function gitRoot(dir) {
  return new Promise(resolve => execFile('git', ['rev-parse', '--show-toplevel'], { cwd: dir }, (err, out) => resolve(err ? undefined : out.trim())));
}

async function activate(context) {
  const log = vscode.window.createOutputChannel('Overseer', { log: true });
  context.subscriptions.push(log);
  const say = msg => log.info(msg);
  const binary = resolveBinary(context, vscode.workspace.getConfiguration('overseer').get('daemonPath'));
  client = new DaemonClient(binary, say);
  const model = new Model(client);
  // The side bar's agents list (Gate K): Needs you, then agents by repository.
  const agents = new AgentsProvider(model, context.workspaceState, context.extensionUri, { attention: () => attention(), pinned: () => pinned() });
  const accounts = new AccountsProvider(model, context.extensionUri);
  const agentsView = vscode.window.createTreeView('overseer.agents', { treeDataProvider: agents, showCollapseAll: true, dragAndDropController: agentDrag() });
  const accountsView = vscode.window.createTreeView('overseer.accounts', { treeDataProvider: accounts });
  context.subscriptions.push(vscode.window.registerFileDecorationProvider(agents.decorations));
  const outputs = new OutputPanels(context, client, model);
  const review = new Review(context, client, model, say);
  let selectedRun;
  const launcher = new TaskLauncher(context, client, model, () => refreshAccounts());
  const steering = new Steering(client, model);
  outputs.steering = steering;
  // Needs you (AC-61): waiting for a decision, failed, or finished with changes not yet reviewed.
  const reviewed = new Map(Object.entries(context.workspaceState.get('overseer.reviewed', {})));
  const markReviewed = runId => { if (!runId) return; reviewed.set(runId, Date.now()); context.workspaceState.update('overseer.reviewed', Object.fromEntries([...reviewed].slice(-800))); };
  const changedRuns = new Map(); // run id -> changed files (finished runs)
  const checkChanged = async run => {
    if (!run || changedRuns.has(run.id) || run.parent_run_id) return;
    changedRuns.set(run.id, 0);
    try { const c = await client.request('workspace.changes', { workspace_id: run.workspace_id }); changedRuns.set(run.id, c.files || 0); if (c.files) model.emitter.fire(); } catch { /* removed worktree */ }
  };
  const archivedTasks = () => (model.state.tasks || []).filter(t => t.archived_ms).map(t => t.id);
  function attention() {
    const archived = new Set(archivedTasks());
    const out = [];
    // Only the 40 most recently finished runs are checked for unreviewed changes (large histories stay fast).
    const recent = new Set((model.state.runs || []).filter(r => !r.parent_run_id && r.status === 'completed').sort((a, b) => (b.ended_ms || b.created_ms) - (a.ended_ms || a.created_ms)).slice(0, 40).map(r => r.id));
    for (const r of (model.state.runs || []).filter(r => !r.parent_run_id)) {
      if (archived.has(r.task_id)) continue;
      const seen = reviewed.get(r.id) || 0;
      if (r.status === 'waiting_for_user') out.push({ run_id: r.id, rank: 0, label: r.attention?.kind === 'permission' ? 'Approve' : 'Reply', detail: r.attention?.kind === 'permission' ? `Wants to use ${r.attention.tool}` : 'Waiting for your reply' });
      else if (['failed', 'disconnected'].includes(r.status) && seen < (r.ended_ms || r.created_ms)) out.push({ run_id: r.id, rank: 1, label: 'Failed', detail: r.exit_reason || 'The agent failed' });
      else if (r.status === 'completed' && recent.has(r.id) && seen < (r.ended_ms || r.created_ms) && Date.now() - (r.ended_ms || r.created_ms) < 7 * 86400000) {
        if (!changedRuns.has(r.id)) checkChanged(r);
        const n = changedRuns.get(r.id);
        if (n) out.push({ run_id: r.id, rank: 2, label: 'Review', detail: `${n} file${n === 1 ? '' : 's'} changed` });
      }
    }
    return out.sort((a, b) => a.rank - b.rank);
  }
  const pinned = () => context.workspaceState.get('overseer.pinned', []).filter(id => model.run(id));
  const setPinned = (runId, on) => context.workspaceState.update('overseer.pinned', [...new Set([...pinned().filter(id => id !== runId), ...(on ? [runId] : [])])]);
  const search = async q => { try { return (await client.request('search', { query: q, limit: 200 })).task_ids || []; } catch { return []; } };
  // With the dashboard open, the chat stays inside it and reviews go to the column on its right.
  const center = new CommandCenter(context, model, { select: (runId, opts) => selectRun(runId, opts), selected: () => selectedRun, client, model, launcher, attention, pinned, setPinned, archived: archivedTasks, search, steering,
    // The grid takes the editor area and gives it back as it was (AC-79).
    onMode: async (mode, was) => { if (mode === 'grid') await arrangement.enterGrid(); else if (was === 'grid') await arrangement.leaveGrid(); } });
  const arrangement = new Arrangement({ context, center, review, model, client, log: say });
  outputs.column = () => vscode.ViewColumn.Beside;
  context.subscriptions.push(vscode.window.registerWebviewPanelSerializer('overseer.center', center));
  const dashboard = new Dashboard(context, center, say, {
    arrange: () => (selectedRun && model.run(selectedRun) ? arrangement.show(selectedRun) : arrangement.chatOnly()),
    agentsVisible: () => agentsView.visible });
  const pullRequests = new PullRequests(client, model, say);
  const newTaskPanel = new NewTaskPanel(context, client, model, { selectRun: (...a) => selectRun(...a), launcher, column: () => vscode.ViewColumn.Beside });
  context.subscriptions.push(vscode.window.registerWebviewPanelSerializer('overseer.output', outputs),
    agentsView.onDidExpandElement(e => agents.setCollapsed(e.element, false)),
    agentsView.onDidCollapseElement(e => agents.setCollapsed(e.element, true)));
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50);
  status.command = 'overseer.refresh';
  context.subscriptions.push(agentsView, accountsView, status, { dispose: () => client.dispose() });

  const updateStatus = () => {
    const runs = model.state.runs || [];
    const active = runs.filter(r => ACTIVE.has(r.status)).length;
    const needs = client.connected ? attention().length : 0;
    status.text = client.connected ? `$(eye) Overseer ${active} active${needs ? `  $(bell-dot) ${needs}` : ''}` : client.stopped ? '$(circle-slash) Overseer stopped' : '$(debug-disconnect) Overseer disconnected';
    status.tooltip = client.connected ? `${active} agent${active === 1 ? '' : 's'} running${needs ? ` · ${needs} need${needs === 1 ? 's' : ''} you` : ''}\nAgents keep running when VS Code closes.\nClick to open the dashboard.` : client.stopped ? 'Agents and daemon were stopped. Click to start the daemon again.' : 'Reconnecting to overseerd…';
    status.command = client.stopped && !client.connected ? 'overseer.startDaemon' : 'overseer.openCenter';
    status.show();
    vscode.commands.executeCommand('setContext', 'overseer.connected', client.connected);
  };
  model.onDidChange(updateStatus);
  // The Needs-you count on the Overseer activity icon (AC-70).
  model.onDidChange(() => { const n = client.connected ? attention().length : 0; agentsView.badge = n ? { value: n, tooltip: `${n} need${n === 1 ? 's' : ''} you` } : undefined; });
  model.onDidChange(() => { autoArchive().catch(() => {}); });
  client.on('connected', () => { model.refresh(); updateStatus(); });
  client.on('disconnected', () => { model.error = 'daemon connection lost; reconnecting'; model.emitter.fire(); updateStatus(); });
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
      if (center.panel?.visible) return; // the dashboard's Needs you shows it
      vscode.window.showWarningMessage(`An agent is waiting for permission to use ${event.payload.tool}.`, 'Show').then(choice => { if (!choice) return; if (center.active) { center.open(); selectRun(model.rootRun(model.run(event.run_id) || {})?.id || event.run_id); } else outputs.show(event.run_id, { preserveFocus: false }); });
    }
  });

  const requireTrust = () => {
    if (!vscode.workspace.isTrusted) throw new Error('Overseer only launches or controls agents in a trusted workspace.');
  };
  const guard = fn => async (...args) => {
    try { return await fn(...args); } catch (error) { vscode.window.showErrorMessage(`Overseer: ${error.message}`); say('error: ' + (error.stack || error.message)); }
  };
  const runArg = arg => (typeof arg === 'string' ? arg : arg?.run?.id) || selectedRun;

  /** Shows an agent: its chat, and its review beside it when it has changes (Gate K). */
  async function selectRun(runId, { follow, reveal = true } = {}) {
    const picked = model.run(runId) || (await model.refresh(), model.run(runId));
    if (!picked) return;
    selectedRun = runId;
    // Opening a failed or finished run counts as seeing it (it leaves Needs you).
    if (!ACTIVE.has(picked.status)) markReviewed(model.rootRun(picked)?.id || runId);
    context.workspaceState.update('overseer.selectedRun', runId);
    await arrangement.show(runId, { follow });
    await center.select(runId);
    if (reveal) revealInTree(runId);
    model.emitter.fire();
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
    agentsView.description = filter ? `${filter.taskIds.size} match${filter.taskIds.size === 1 ? '' : 'es'} for “${filter.query}”` : undefined;
    vscode.commands.executeCommand('setContext', 'overseer.agentsFiltered', !!filter);
    // Keep the selected agent in view (and selected) when the list changes shape.
    if (selectedRun && (!filter || filter.taskIds.has(model.run(selectedRun)?.task_id))) setTimeout(() => revealInTree(selectedRun), 150);
  }

  /** Search agents by title, prompt, message text, file, repository, account or status (daemon search). */
  async function searchAgents() {
    const input = vscode.window.createInputBox();
    input.title = 'Search agents';
    input.placeholder = 'Title, message, file, repository, account or status';
    input.value = agents.filter?.query || '';
    let seq = 0, accepted = false, timer;
    const run = async q => {
      const mine = ++seq;
      if (!q) { setAgentFilter(undefined); return; }
      const lower = q.toLowerCase();
      // One tree update with titles and the daemon's matches (messages, files, repository, account, status).
      const local = (model.state.tasks || []).filter(t => (t.title || '').toLowerCase().includes(lower)).map(t => t.id);
      const ids = await search(q);
      if (mine === seq) setAgentFilter({ query: q, taskIds: new Set([...local, ...ids]) });
    };
    // Typing again right after clearing supersedes the clear, so the list updates once.
    input.onDidChangeValue(q => { clearTimeout(timer); timer = setTimeout(() => run(q.trim()), q.trim() ? 25 : 200); });
    input.onDidAccept(() => { accepted = true; input.hide(); });
    input.onDidHide(() => { clearTimeout(timer); if (!accepted) setAgentFilter(undefined); input.dispose(); });
    input.show();
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
    const repoPick = await vscode.window.showQuickPick([...roots.map(r => ({ label: path.basename(r), description: r, root: r })), { label: '$(folder) Choose repository…', browse: true }], { title: 'New task: repository' });
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
    const hPick = await vscode.window.showQuickPick(harnesses.map(h => ({ label: h.harness, description: h.installed ? (h.version || 'installed') : 'not installed', detail: `children: ${h.capabilities.children}`, h })), { title: 'New task: harness' });
    if (!hPick) return;
    const harness = hPick.h.harness;
    if (!hPick.h.installed) throw new Error(`${harness} is not installed.`);
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
      const pPick = await vscode.window.showQuickPick(compatible.map((a, i) => ({ label: a.name, description: `${statuses[i]?.logged_in ? 'signed in' : 'not signed in'}${statuses[i]?.identity?.plan ? ' · ' + statuses[i].identity.plan : ''} · ${a.kind === 'follows-app' ? 'follows the desktop app (can change)' : 'fixed account'}`, detail: statuses[i]?.detail, p: model.profile(a.id) || { id: a.id, name: a.name }, ok: statuses[i]?.logged_in })),
        { title: `New task: account for ${harness} (only compatible accounts; account login only, no API keys)` });
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
    ], { title: 'New task: workspace' });
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
    const prompt = await vscode.window.showInputBox({ title: 'Task prompt', prompt: harness === 'generic' ? 'Optional first line sent to stdin' : 'What should the agent do?', ignoreFocusOut: true });
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
   * Merge back (never automatic): prepare in the worktree (commit its work, merge the target into
   * the run's branch; conflicts go to the same session), show what will land for review, then
   * merge into the target branch in the source checkout only after confirmation.
   */
  async function mergeBack(arg) {
    requireTrust();
    const picked = model.run(runArg(arg));
    if (!picked) return;
    const run = model.rootRun(picked);
    const wsId = run.workspace_id;
    let plan = await client.request('workspace.merge_plan', { workspace_id: wsId });
    if (!plan.ok) { vscode.window.showWarningMessage(`Merge back is unavailable: ${plan.reason}`); return; }
    if (plan.state === 'idle') {
      const detail = [`${plan.branch} → ${plan.target} in ${plan.repo}`,
        plan.worktree_uncommitted.length ? `1. Commit ${plan.worktree_uncommitted.length} uncommitted worktree file(s) to ${plan.branch}.` : '1. The worktree has no uncommitted changes.',
        `2. Merge ${plan.target} into ${plan.branch} inside the worktree. Conflicts go back to ${run.harness} in the same session.`,
        `3. You review exactly what will land, then confirm. Nothing reaches ${plan.target} before that.`,
        ...plan.blockers.map(b => '⚠ ' + b)].join('\n');
      const go = await vscode.window.showInformationMessage(`Merge back ${plan.branch} into ${plan.target}?`, { modal: true, detail }, 'Prepare Merge Back');
      if (go !== 'Prepare Merge Back') return;
      const prep = await client.request('workspace.merge_prepare', { workspace_id: wsId, handoff: true });
      await model.refresh();
      if (prep.state === 'conflicts') {
        const how = prep.handoff?.sent ? `Sent to ${run.harness} as a follow-up in the same session. Run Merge Back again when it finishes.` : `Resolve them in the worktree (${prep.handoff?.why || 'no follow-up possible'}), then run Merge Back again.`;
        vscode.window.showWarningMessage(`Merge back: conflicts in ${prep.files.join(', ')}. ${how}`);
        await outputs.show(run.id, { preserveFocus: false });
        return;
      }
      plan = await client.request('workspace.merge_plan', { workspace_id: wsId });
    }
    if (plan.state === 'resolving' || plan.state === 'resolved') {
      const res = await client.request('workspace.merge_resolved', { workspace_id: wsId });
      if (res.state !== 'ready') { vscode.window.showWarningMessage(`Merge back: conflict markers remain in ${res.remaining.join(', ')}. Resolve them (or ask the agent again), then run Merge Back again.`); return; }
      plan = await client.request('workspace.merge_plan', { workspace_id: wsId });
    }
    // Show the result for review: exactly what lands on the target (merge-base comparison).
    const opts = await client.request('comparison.options', { run_id: run.id, branch: plan.target });
    const landing = opts.options.find(o => o.mode === 'branch_merge_base' && o.branch === plan.target && o.available);
    if (landing) { review.setComparison(run.id, landing); await review.open(run.id); }
    const files = landing ? (await client.request('workspace.diff', { workspace_id: wsId, base: landing.base, status: false })).changes : [];
    if (plan.blockers.length) { vscode.window.showWarningMessage(`Merge back is ready but blocked: ${plan.blockers.join(' ')}`); return; }
    const detail = `The review now shows exactly what lands on ${plan.target} (merge-base comparison), ${files.length} file(s):\n${files.slice(0, 20).map(f => `${f.status} ${f.path}`).join('\n')}${files.length > 20 ? '\n…' : ''}\n\nThe worktree and ${plan.branch} are kept.`;
    const ok = await vscode.window.showWarningMessage(`Merge ${plan.branch} into ${plan.target} in ${path.basename(plan.repo)}?`, { modal: true, detail }, 'Complete Merge Back');
    if (ok !== 'Complete Merge Back') return;
    const done = await client.request('workspace.merge_complete', { workspace_id: wsId });
    await model.refresh();
    vscode.window.showInformationMessage(`Merged ${done.branch} into ${done.target} (${String(done.commit).slice(0, 10)}). The worktree and branch are kept; clean them up when you no longer need them.`);
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

  /** Allow or deny the pending permission of the selected agent, or of the first agent waiting. */
  async function answerPermission(allow) {
    requireTrust();
    const waiting = (model.state.runs || []).filter(r => r.attention?.kind === 'permission');
    const run = waiting.find(r => r.id === selectedRun || model.rootRun(r)?.id === selectedRun) || waiting[0];
    if (!run) { vscode.window.setStatusBarMessage('$(check) No permission request is waiting', 2500); return; }
    await client.request('run.permission', { run_id: run.id, request_id: String(run.attention.request_id), allow });
    model.scheduleRefresh();
  }

  async function refreshAccounts() {
    await model.refresh();
    try { const list = await client.request('account.list'); model.accounts = list.accounts; model.providers = list.providers; } catch (e) { say('account.list: ' + e.message); }
    await Promise.all(model.state.profiles.map(async p => {
      try { model.profileStatus.set(p.id, await client.request('profile.status', { id: p.id })); } catch (e) { say(e.message); }
      try { (model.accountUsage ||= new Map()).set(p.id, await client.request('account.usage', { id: p.id })); } catch { /* older daemon */ }
    }));
    accounts.emitter.fire();
  }

  context.subscriptions.push(
    vscode.commands.registerCommand('overseer.newTask', guard(async () => { requireTrust(); await model.refresh(); await newTaskPanel.open(); })),
    vscode.commands.registerCommand('overseer.newTaskQuick', guard(newTask)),
    vscode.commands.registerCommand('overseer.refresh', guard(async () => { await model.refresh(); })),
    vscode.commands.registerCommand('overseer.selectRun', guard(runId => selectRun(runId))),
    vscode.commands.registerCommand('overseer.openReview', guard(async arg => { const id = runArg(arg); if (!id) return; selectedRun = id; await arrangement.openReview(id); await center.select(id); })),
    vscode.commands.registerCommand('overseer.openEdit', guard(async (runId, rel) => {
      const run = model.run(runId) || (await model.refresh(), model.run(runId));
      if (!run) return;
      return review.revealEdit(model.rootRun(run).id, rel);
    })),
    vscode.commands.registerCommand('overseer.showOutput', guard(arg => outputs.show(runArg(arg), { preserveFocus: false }))),
    vscode.commands.registerCommand('overseer.openAgentToSide', guard(arg => outputs.show(runArg(arg), { preserveFocus: false }))),
    vscode.commands.registerCommand('overseer.followUp', guard(async arg => {
      requireTrust();
      const runId = runArg(arg);
      const text = await vscode.window.showInputBox({ title: 'Follow-up for this run only', ignoreFocusOut: true });
      if (!text) return;
      await client.request('run.follow_up', { run_id: runId, prompt: text });
      await model.refresh();
    })),
    vscode.commands.registerCommand('overseer.interrupt', guard(async arg => { requireTrust(); await client.request('run.interrupt', { run_id: runArg(arg) }); })),
    vscode.commands.registerCommand('overseer.selectComparison', guard(arg => review.pickComparison(runArg(arg)))),
    vscode.commands.registerCommand('overseer.cleanupWorkspace', guard(async arg => {
      requireTrust();
      const run = model.run(runArg(arg));
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
    vscode.commands.registerCommand('overseer.showCapabilities', guard(async () => {
      const list = await client.request('harness.list');
      const doc = await vscode.workspace.openTextDocument({ language: 'markdown', content: '# Harness capabilities (reported by this Overseer build)\n\n' + list.map(h =>
        `## ${h.harness} ${h.version || ''}\n\nExecutable: \`${h.program || 'n/a'}\` (${h.installed ? 'installed' : 'not installed'})\n\n` + Object.entries(h.capabilities).map(([k, v]) => `- **${k}**: ${v}`).join('\n')).join('\n\n') +
        '\n\n## Not integrated\n\n- **Gemini CLI**: not installed on this machine; no adapter yet.\n- **Devin**: skipped — no account-login CLI path without API keys/personal access tokens was available.\n' });
      await vscode.window.showTextDocument(doc, { preview: true });
    })),
    vscode.commands.registerCommand('overseer.stopAll', guard(stopAll)),
    vscode.commands.registerCommand('overseer.testNotification', guard(async () => {
      const { delivered_via: via } = await client.request('daemon.test_notice');
      const native = /^overseer-notifier \(ok\)/.test(via);
      vscode.window.showInformationMessage(native ? 'Sent a test notification from Overseer. If no banner appeared, allow Overseer in System Settings → Notifications.'
        : `Sent a test notification, but not as Overseer: ${via}. Allow Overseer in System Settings → Notifications to get Overseer-branded banners.`);
      return via;
    })),
    // Notification clicks open vscode://beelol.overseer/open-center (AC-52).
    vscode.window.registerUriHandler({ handleUri: uri => { if (uri.path === '/open-center') vscode.commands.executeCommand('overseer.openCenter'); } }),
    vscode.commands.registerCommand('overseer.openCenter', guard(async () => { await model.refresh(); if (selectedRun && model.run(selectedRun)) await selectRun(selectedRun); else { await arrangement.chatOnly(); center.setMode('composer'); } })),
    vscode.commands.registerCommand('overseer.openDashboard', guard(async () => { await model.refresh(); await dashboard.enter(); if (selectedRun && model.run(selectedRun)) await selectRun(selectedRun); })),
    vscode.commands.registerCommand('overseer.exitDashboard', guard(() => dashboard.exit())),
    vscode.commands.registerCommand('overseer.toggleDashboard', guard(async () => { if (dashboard.inDashboard) await dashboard.exit(); else { await model.refresh(); await dashboard.enter(); } })),
    vscode.commands.registerCommand('overseer.openDashboardWindow', guard(() => dashboard.openWindow())),
    vscode.commands.registerCommand('overseer.newAgent', guard(async () => { requireTrust(); await arrangement.chatOnly(); center.setMode('composer'); center.focus('composer'); })),
    vscode.commands.registerCommand('overseer.toggleGrid', guard(async () => { if (center.mode === 'grid') { center.setMode(selectedRun ? 'chat' : 'composer'); } else { await arrangement.enterGrid(); center.setMode('grid'); } })),
    vscode.commands.registerCommand('overseer.searchAgents', guard(searchAgents)),
    vscode.commands.registerCommand('overseer.clearAgentSearch', guard(async () => setAgentFilter(undefined))),
    vscode.commands.registerCommand('overseer.showArchived', guard(async () => { agents.showArchived = true; setAgentFilter(undefined); vscode.commands.executeCommand('setContext', 'overseer.showArchived', true); })),
    vscode.commands.registerCommand('overseer.hideArchived', guard(async () => { agents.showArchived = false; agents.refresh(); vscode.commands.executeCommand('setContext', 'overseer.showArchived', false); })),
    vscode.commands.registerCommand('overseer.archiveAgent', guard(async arg => { const task = agentTask(arg); if (task) { await client.request('task.archive', { task_id: task.id, archived: true }); await model.refresh(); } })),
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
    vscode.commands.registerCommand('overseer.nextNeedsYou', guard(async () => {
      const list = attention();
      if (!list.length) { vscode.window.setStatusBarMessage('$(check) Nothing needs you', 2500); return; }
      // The most urgent item that is not already open (approvals first, then failures, then reviews).
      const next = list.find(a => a.run_id !== selectedRun) || list[0];
      await selectRun(next.run_id); center.focus('chat');
    })),
    vscode.commands.registerCommand('overseer.allowPermission', guard(async () => answerPermission(true))),
    vscode.commands.registerCommand('overseer.denyPermission', guard(async () => answerPermission(false))),
    vscode.commands.registerCommand('overseer.cleanupArchived', guard(cleanupArchived)),
    vscode.commands.registerCommand('overseer.stopSelected', guard(async () => { requireTrust(); const r = selectedRun && model.run(selectedRun); if (r && ACTIVE.has(r.status)) await client.request('run.interrupt', { run_id: model.rootRun(r).id }); })),
    vscode.commands.registerCommand('overseer.mergeBack', guard(mergeBack)),
    vscode.commands.registerCommand('overseer.openPullRequest', guard(arg => pullRequests.open(runArg(arg)))),
    vscode.commands.registerCommand('overseer.startDaemon', guard(async () => { client.disposed = false; await client.start(); await model.refresh(); updateStatus(); })),
    vscode.commands.registerCommand('overseer.showLog', () => log.show()),
    vscode.commands.registerCommand('overseer.restartDaemonConnection', guard(async () => { client.dispose(); client.disposed = false; await client.start(); })),
    vscode.workspace.onDidGrantWorkspaceTrust(() => model.emitter.fire()),
  );

  updateStatus();
  try {
    await client.start();
    await model.refresh();
    refreshAccounts().catch(() => {});
    announceBackgroundAgents().catch(error => say('background notice check: ' + error.message));
    dashboard.startup().catch(error => say('dashboard startup: ' + error.message));
    const remembered = context.workspaceState.get('overseer.selectedRun');
    if (remembered && model.run(remembered)) {
      selectedRun = remembered;
      // Show the remembered run as selected in the Agents view without stealing focus.
      const reveal = () => { const node = agents.nodeFor(remembered); if (node) agentsView.reveal(node, { select: true, focus: false, expand: false }).then(undefined, () => {}); };
      if (agentsView.visible) reveal();
      else { const once = agentsView.onDidChangeVisibility(e => { if (e.visible) { once.dispose(); setTimeout(reveal, 200); } }); context.subscriptions.push(once); }
    }
  } catch (error) {
    say('daemon start failed: ' + error.message);
    vscode.window.showErrorMessage(`Overseer could not start its daemon: ${error.message}`);
  }
  return { client, model, review, outputs, selectRun, agents, agentsView, center, dashboard, arrangement, attention, selectedRun: () => selectedRun }; // exported for UI tests
}

function deactivate() { client?.dispose(); }

module.exports = { activate, deactivate };
