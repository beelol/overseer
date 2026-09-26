// Overseer VS Code extension: presentation, navigation and editor buffers.
// All durable state (tasks, runs, workspaces, events) lives in overseerd.
const vscode = require('vscode');
const path = require('path');
const { execFile } = require('child_process');
const { DaemonClient, resolveBinary } = require('./daemon-client');
const { Model, AgentsProvider, DirtyProvider, AccountsProvider, ACTIVE } = require('./views');
const { OutputPanels } = require('./output-panel');
const { Review } = require('./review');
const { CommandCenter, COLUMNS } = require('./command-center');
const { NewTaskPanel } = require('./new-task');

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
  const agents = new AgentsProvider(model, context.workspaceState);
  const dirty = new DirtyProvider(model, client);
  const accounts = new AccountsProvider(model);
  const agentsView = vscode.window.createTreeView('overseer.agents', { treeDataProvider: agents, showCollapseAll: true });
  const dirtyView = vscode.window.createTreeView('overseer.dirty', { treeDataProvider: dirty });
  const accountsView = vscode.window.createTreeView('overseer.accounts', { treeDataProvider: accounts });
  const outputs = new OutputPanels(context, client, model);
  const review = new Review(context, client, model, say);
  let selectedRun;
  // With the Overseer view open, reviews go to its review column and run panels to its conversation column.
  const center = new CommandCenter(context, model, { select: runId => selectRun(runId, { preserveFocus: true }), selected: () => selectedRun, client });
  review.reviewColumn = () => center.active ? COLUMNS.review : undefined;
  outputs.column = () => center.active ? COLUMNS.conversation : undefined;
  context.subscriptions.push(vscode.window.registerWebviewPanelSerializer('overseer.center', center));
  const newTaskPanel = new NewTaskPanel(context, client, model, { selectRun: (...a) => selectRun(...a), refreshAccounts: () => refreshAccounts(), column: () => center.active ? COLUMNS.review : undefined });
  context.subscriptions.push(vscode.window.registerWebviewPanelSerializer('overseer.output', outputs),
    agentsView.onDidExpandElement(e => agents.setCollapsed(e.element, false)),
    agentsView.onDidCollapseElement(e => agents.setCollapsed(e.element, true)));
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50);
  status.command = 'overseer.refresh';
  context.subscriptions.push(agentsView, dirtyView, accountsView, status, { dispose: () => client.dispose() });

  const updateStatus = () => {
    const runs = model.state.runs || [];
    const active = runs.filter(r => ACTIVE.has(r.status)).length;
    const waiting = runs.filter(r => r.status === 'waiting_for_user').length;
    status.text = client.connected ? `$(pulse) Overseer ${active} active${waiting ? `, ${waiting} waiting` : ''}` : client.stopped ? '$(circle-slash) Overseer stopped' : '$(debug-disconnect) Overseer disconnected';
    status.tooltip = client.connected ? 'overseerd is running; agents continue when VS Code closes. Use "Overseer: Stop Agents and Daemon" to stop everything.' : client.stopped ? 'Agents and daemon were stopped. Click to start the daemon again.' : 'Reconnecting to overseerd…';
    status.command = client.stopped && !client.connected ? 'overseer.startDaemon' : 'overseer.refresh';
    status.show();
    vscode.commands.executeCommand('setContext', 'overseer.connected', client.connected);
  };
  model.onDidChange(updateStatus);
  client.on('connected', () => { model.refresh(); updateStatus(); });
  client.on('disconnected', () => { model.error = 'daemon connection lost; reconnecting'; model.emitter.fire(); updateStatus(); });
  client.on('stopped', () => { model.error = 'agents and daemon stopped (Overseer: Start Daemon to restart)'; model.emitter.fire(); updateStatus(); });
  client.on('event', event => {
    model.scheduleRefresh();
    if (selectedRun && ['file_activity', 'status', 'turn_done', 'workspace_removed'].includes(event.kind)) setTimeout(() => dirty.refresh(), 300);
    if (event.kind === 'permission') {
      vscode.window.showWarningMessage(`An agent is waiting for permission to use ${event.payload.tool}.`, 'Show').then(choice => { if (choice) outputs.show(event.run_id, { preserveFocus: false }); });
    }
  });
  const dirtyTimer = setInterval(() => { if (dirtyView.visible && selectedRun) dirty.refresh(); }, 1000);
  context.subscriptions.push({ dispose: () => clearInterval(dirtyTimer) });

  const requireTrust = () => {
    if (!vscode.workspace.isTrusted) throw new Error('Overseer only launches or controls agents in a trusted workspace.');
  };
  const guard = fn => async (...args) => {
    try { return await fn(...args); } catch (error) { vscode.window.showErrorMessage(`Overseer: ${error.message}`); say('error: ' + (error.stack || error.message)); }
  };
  const runArg = arg => (typeof arg === 'string' ? arg : arg?.run?.id) || selectedRun;

  async function selectRun(runId, { follow, preserveFocus = false } = {}) {
    selectedRun = runId;
    center.selected(runId);
    context.workspaceState.update('overseer.selectedRun', runId);
    dirty.select(runId);
    // From the Overseer view, keep keyboard focus in the agents column.
    await review.open(runId, { follow, preserveFocus });
    await outputs.show(runId);
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

  async function refreshAccounts() {
    await model.refresh();
    try { const list = await client.request('account.list'); model.accounts = list.accounts; model.providers = list.providers; } catch (e) { say('account.list: ' + e.message); }
    await Promise.all(model.state.profiles.map(async p => {
      try { model.profileStatus.set(p.id, await client.request('profile.status', { id: p.id })); } catch (e) { say(e.message); }
    }));
    accounts.emitter.fire();
  }

  context.subscriptions.push(
    vscode.commands.registerCommand('overseer.newTask', guard(async () => { requireTrust(); await model.refresh(); await newTaskPanel.open(); })),
    vscode.commands.registerCommand('overseer.newTaskQuick', guard(newTask)),
    vscode.commands.registerCommand('overseer.refresh', guard(async () => { await model.refresh(); dirty.refresh(); })),
    vscode.commands.registerCommand('overseer.selectRun', guard(runId => selectRun(runId))),
    vscode.commands.registerCommand('overseer.openReview', guard(arg => review.open(runArg(arg)))),
    vscode.commands.registerCommand('overseer.openEdit', guard(async (runId, rel) => {
      const run = model.run(runId) || (await model.refresh(), model.run(runId));
      if (!run) return;
      return review.revealEdit(model.rootRun(run).id, rel);
    })),
    vscode.commands.registerCommand('overseer.showOutput', guard(arg => outputs.show(runArg(arg), { preserveFocus: false }))),
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
    vscode.commands.registerCommand('overseer.openDirtyDiff', guard((kind, root, file) => review.openDirtyDiff(kind, root, file))),
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
    vscode.commands.registerCommand('overseer.openCenter', guard(async () => { await model.refresh(); await center.open(); if (selectedRun && model.run(selectedRun)) await selectRun(selectedRun); })),
    vscode.commands.registerCommand('overseer.mergeBack', guard(mergeBack)),
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
    const remembered = context.workspaceState.get('overseer.selectedRun');
    if (remembered && model.run(remembered)) {
      selectedRun = remembered; dirty.select(remembered);
      // Show the remembered run as selected in the Agents view without stealing focus.
      const reveal = () => { const node = agents.nodeFor(remembered); if (node) agentsView.reveal(node, { select: true, focus: false, expand: false }).then(undefined, () => {}); };
      if (agentsView.visible) reveal();
      else { const once = agentsView.onDidChangeVisibility(e => { if (e.visible) { once.dispose(); setTimeout(reveal, 200); } }); context.subscriptions.push(once); }
    }
  } catch (error) {
    say('daemon start failed: ' + error.message);
    vscode.window.showErrorMessage(`Overseer could not start its daemon: ${error.message}`);
  }
  return { client, model, review, outputs, selectRun, dirty, agents, agentsView, center, selectedRun: () => selectedRun }; // exported for UI tests
}

function deactivate() { client?.dispose(); }

module.exports = { activate, deactivate };
