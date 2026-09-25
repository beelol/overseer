// Overseer VS Code extension: presentation, navigation and editor buffers.
// All durable state (tasks, runs, workspaces, events) lives in overseerd.
const vscode = require('vscode');
const path = require('path');
const { execFile } = require('child_process');
const { DaemonClient, resolveBinary } = require('./daemon-client');
const { Model, AgentsProvider, DirtyProvider, AccountsProvider, ACTIVE } = require('./views');
const { OutputPanels } = require('./output-panel');
const { Review } = require('./review');

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
  const agents = new AgentsProvider(model);
  const dirty = new DirtyProvider(model, client);
  const accounts = new AccountsProvider(model);
  const agentsView = vscode.window.createTreeView('overseer.agents', { treeDataProvider: agents, showCollapseAll: true });
  const dirtyView = vscode.window.createTreeView('overseer.dirty', { treeDataProvider: dirty });
  const accountsView = vscode.window.createTreeView('overseer.accounts', { treeDataProvider: accounts });
  const outputs = new OutputPanels(context, client, model);
  const review = new Review(context, client, model, say);
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50);
  status.command = 'overseer.refresh';
  context.subscriptions.push(agentsView, dirtyView, accountsView, status, { dispose: () => client.dispose() });

  let selectedRun;
  const updateStatus = () => {
    const runs = model.state.runs || [];
    const active = runs.filter(r => ACTIVE.has(r.status)).length;
    const waiting = runs.filter(r => r.status === 'waiting_for_user').length;
    status.text = client.connected ? `$(pulse) Overseer ${active} active${waiting ? `, ${waiting} waiting` : ''}` : '$(debug-disconnect) Overseer disconnected';
    status.tooltip = client.connected ? 'overseerd is running; agents continue when VS Code closes.' : 'Reconnecting to overseerd…';
    status.show();
    vscode.commands.executeCommand('setContext', 'overseer.connected', client.connected);
  };
  model.onDidChange(updateStatus);
  client.on('connected', () => { model.refresh(); updateStatus(); });
  client.on('disconnected', () => { model.error = 'daemon connection lost; reconnecting'; model.emitter.fire(); updateStatus(); });
  client.on('event', event => {
    model.scheduleRefresh();
    if (selectedRun && ['file_activity', 'status', 'turn_done', 'workspace_removed'].includes(event.kind)) setTimeout(() => dirty.refresh(), 300);
    if (event.kind === 'permission') {
      vscode.window.showWarningMessage(`An agent is waiting for permission to use ${event.payload.tool}.`, 'Show').then(choice => { if (choice) outputs.show(event.run_id, { preserveFocus: false }); });
    }
  });
  const dirtyTimer = setInterval(() => { if (dirtyView.visible && selectedRun) dirty.refresh(); }, 2500);
  context.subscriptions.push({ dispose: () => clearInterval(dirtyTimer) });

  const requireTrust = () => {
    if (!vscode.workspace.isTrusted) throw new Error('Overseer only launches or controls agents in a trusted workspace.');
  };
  const guard = fn => async (...args) => {
    try { return await fn(...args); } catch (error) { vscode.window.showErrorMessage(`Overseer: ${error.message}`); say('error: ' + (error.stack || error.message)); }
  };
  const runArg = arg => (typeof arg === 'string' ? arg : arg?.run?.id) || selectedRun;

  async function selectRun(runId, { follow } = {}) {
    selectedRun = runId;
    dirty.select(runId);
    await review.open(runId, { follow });
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
      const profiles = model.state.profiles.filter(p => p.harness === harness);
      const statuses = await Promise.all(profiles.map(p => client.request('profile.status', { id: p.id }).catch(() => undefined)));
      statuses.forEach((s, i) => s && model.profileStatus.set(profiles[i].id, s));
      const pPick = await vscode.window.showQuickPick(profiles.map((p, i) => ({ label: p.name, description: statuses[i]?.logged_in ? 'signed in' : 'not signed in', detail: statuses[i]?.detail, p, ok: statuses[i]?.logged_in })), { title: 'New task: account profile (account login only; no API keys)' });
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
    const prompt = await vscode.window.showInputBox({ title: 'Task prompt', prompt: harness === 'generic' ? 'Optional first line sent to stdin' : 'What should the agent do?', ignoreFocusOut: true });
    if (prompt === undefined || (!prompt && harness !== 'generic')) return;
    const title = (prompt || path.basename(program || 'task')).slice(0, 60);
    const created = await client.request('task.create', { repo, harness, profile_id: profileId, workspace_mode: mode.mode, target_ref: targetRef, model: model_ || undefined,
      prompt, title, program, args, unsaved: unsaved.map(d => path.relative(repo, d.uri.fsPath)) });
    if (created.launch_error) vscode.window.showErrorMessage(`Overseer could not launch ${harness}: ${created.launch_error}`);
    await model.refresh();
    await selectRun(created.run.id, { follow: vscode.workspace.getConfiguration('overseer').get('followNewRuns', true) });
  }

  async function signIn(profileId) {
    requireTrust();
    const cmd = await client.request('profile.login_command', { id: profileId });
    const terminal = vscode.window.createTerminal({ name: `Sign in: ${cmd.profile.name}`, shellPath: cmd.program, shellArgs: cmd.args, env: cmd.env });
    terminal.show();
    const done = vscode.window.onDidCloseTerminal(async t => {
      if (t !== terminal) return;
      done.dispose();
      await refreshAccounts();
    });
    context.subscriptions.push(done);
  }

  async function refreshAccounts() {
    await model.refresh();
    await Promise.all(model.state.profiles.map(async p => {
      try { model.profileStatus.set(p.id, await client.request('profile.status', { id: p.id })); } catch (e) { say(e.message); }
    }));
    accounts.emitter.fire();
  }

  context.subscriptions.push(
    vscode.commands.registerCommand('overseer.newTask', guard(newTask)),
    vscode.commands.registerCommand('overseer.refresh', guard(async () => { await model.refresh(); dirty.refresh(); })),
    vscode.commands.registerCommand('overseer.selectRun', guard(runId => selectRun(runId))),
    vscode.commands.registerCommand('overseer.openReview', guard(arg => review.open(runArg(arg)))),
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
    vscode.commands.registerCommand('overseer.addProfile', guard(async () => {
      const harness = await vscode.window.showQuickPick(['codex', 'claude', 'opencode'], { title: 'Harness for the new account profile' });
      if (!harness) return;
      const name = await vscode.window.showInputBox({ title: 'Profile name', prompt: 'For example: ChatGPT (work)', validateInput: v => v.trim() ? undefined : 'Required' });
      if (!name) return;
      const profile = await client.request('profile.create', { name, harness });
      await model.refresh();
      const choice = await vscode.window.showInformationMessage(`Created ${profile.name}. Sign in now?`, 'Sign In');
      if (choice) await signIn(profile.id);
    })),
    vscode.commands.registerCommand('overseer.signIn', guard(async arg => signIn(arg?.profile?.id || (await vscode.window.showQuickPick(model.state.profiles.map(p => ({ label: p.name, id: p.id }))))?.id))),
    vscode.commands.registerCommand('overseer.signOut', guard(async arg => {
      const p = arg?.profile; if (!p) return;
      const ok = await vscode.window.showWarningMessage(`Sign out ${p.name}? Only this isolated profile is affected.`, { modal: true }, 'Sign Out');
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
    vscode.commands.registerCommand('overseer.showLog', () => log.show()),
    vscode.commands.registerCommand('overseer.restartDaemonConnection', guard(async () => { client.dispose(); client.disposed = false; await client.start(); })),
    vscode.workspace.onDidGrantWorkspaceTrust(() => model.emitter.fire()),
  );

  updateStatus();
  try {
    await client.start();
    await model.refresh();
    refreshAccounts().catch(() => {});
  } catch (error) {
    say('daemon start failed: ' + error.message);
    vscode.window.showErrorMessage(`Overseer could not start its daemon: ${error.message}`);
  }
  return { client, model, review, outputs, selectRun, dirty }; // exported for UI tests
}

function deactivate() { client?.dispose(); }

module.exports = { activate, deactivate };
