// Sidebar views: recursive agent tree, workspace-dirty layers and account profiles.
const vscode = require('vscode');
const path = require('path');

const STATUS_ICON = {
  queued: ['clock', 'charts.yellow'], starting: ['loading~spin', 'charts.blue'], running: ['sync~spin', 'charts.blue'],
  waiting_for_user: ['bell-dot', 'charts.orange'], completed: ['pass', 'charts.green'], failed: ['error', 'charts.red'],
  interrupted: ['debug-stop', 'charts.orange'], disconnected: ['debug-disconnect', 'charts.red'], unknown: ['question', 'charts.purple'],
};
const ACTIVE = new Set(['queued', 'starting', 'running', 'waiting_for_user']);
const STATUS_LABEL = { waiting_for_user: 'waiting for you' };

function statusIcon(status) {
  const [icon, color] = STATUS_ICON[status] || STATUS_ICON.unknown;
  return new vscode.ThemeIcon(icon, new vscode.ThemeColor(color));
}

class Model {
  constructor(client) {
    this.client = client;
    this.state = { tasks: [], runs: [], workspaces: [], profiles: [], turns: {} };
    this.emitter = new vscode.EventEmitter();
    this.onDidChange = this.emitter.event;
    this.profileStatus = new Map();
  }
  async refresh() {
    try { this.state = await this.client.request('state'); this.error = undefined; }
    catch (error) { this.error = error.message; }
    this.emitter.fire();
  }
  scheduleRefresh() { clearTimeout(this.timer); this.timer = setTimeout(() => this.refresh(), 120); }
  run(id) { return this.state.runs.find(r => r.id === id); }
  task(id) { return this.state.tasks.find(t => t.id === id); }
  workspace(id) { return this.state.workspaces.find(w => w.id === id); }
  profile(id) { return this.state.profiles.find(p => p.id === id); }
  children(runId) { return this.state.runs.filter(r => r.parent_run_id === runId); }
  rootRun(run) { let r = run; const seen = new Set(); while (r?.parent_run_id && !seen.has(r.id)) { seen.add(r.id); r = this.run(r.parent_run_id); } return r; }
  descendants(runId) {
    const out = [], queue = [runId], seen = new Set();
    while (queue.length) { const id = queue.shift(); if (seen.has(id)) continue; seen.add(id); for (const c of this.children(id)) { out.push(c); queue.push(c.id); } }
    return out;
  }
}

class AgentsProvider {
  constructor(model, memento) {
    this.model = model; this.memento = memento;
    // Expansion is remembered per workspace (item ids are stable), so reloads keep the tree shape.
    this.collapsed = new Set(memento?.get('overseer.collapsed', []) || []);
    this.emitter = new vscode.EventEmitter();
    this.onDidChangeTreeData = this.emitter.event;
    model.onDidChange(() => this.emitter.fire());
  }
  getTreeItem(node) { return node.item; }
  getParent(node) { return node.parent; }
  setCollapsed(node, collapsed) {
    const id = node?.item?.id; if (!id) return;
    if (collapsed) this.collapsed.add(id); else this.collapsed.delete(id);
    this.memento?.update('overseer.collapsed', [...this.collapsed].slice(-500));
  }
  expansion(id, expandable = true) {
    if (!expandable) return vscode.TreeItemCollapsibleState.None;
    return this.collapsed.has(id) ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.Expanded;
  }
  /** The tree node for a run, with its parents, for TreeView.reveal. */
  nodeFor(runId) {
    const chain = [];
    for (let r = this.model.run(runId), seen = new Set(); r && !seen.has(r.id); r = r.parent_run_id && this.model.run(r.parent_run_id)) { seen.add(r.id); chain.unshift(r); }
    const task = chain.length && this.model.task(chain[0].task_id);
    if (!task) return undefined;
    let node = this.taskNode(task);
    for (const run of chain) node = this.runNode(run, node);
    return node;
  }
  getChildren(node) {
    const m = this.model;
    if (!node) {
      if (m.error) return [{ item: Object.assign(new vscode.TreeItem(`Daemon unavailable: ${m.error}`), { iconPath: new vscode.ThemeIcon('warning') }) }];
      return [...m.state.tasks].sort((a, b) => b.created_ms - a.created_ms).map(task => this.taskNode(task));
    }
    if (node.task) return m.state.runs.filter(r => r.task_id === node.task.id && !r.parent_run_id).map(run => this.runNode(run, node));
    if (node.run) {
      const kids = m.children(node.run.id).map(run => this.runNode(run, node));
      const caps = node.run.capabilities || {};
      if (!node.run.parent_run_id && typeof caps.children === 'string' && !caps.children.startsWith('supported')) {
        const info = new vscode.TreeItem(`Native children: ${caps.children}`);
        info.iconPath = new vscode.ThemeIcon('info'); info.tooltip = 'Overseer only shows children the harness actually reports. Missing telemetry is shown as unknown, never as zero.';
        kids.push({ item: info, parent: node });
      }
      return kids;
    }
    return [];
  }
  taskNode(task) {
    const ws = this.model.workspace(task.workspace_id);
    // A task whose run is not recorded yet is not expandable: VS Code would otherwise cache it
    // as an empty expanded node and not ask for its children again when the run appears.
    const hasRuns = this.model.state.runs.some(r => r.task_id === task.id);
    const item = new vscode.TreeItem(task.title, this.expansion('task:' + task.id, hasRuns));
    item.id = 'task:' + task.id;
    item.iconPath = new vscode.ThemeIcon(ws?.kind === 'current' ? 'repo' : 'git-branch');
    item.description = ws ? `${ws.kind === 'current' ? 'current checkout' : ws.branch} · ${path.basename(task.repo_root)}${ws.removed_ms ? ' · removed' : ''}` : '';
    item.tooltip = new vscode.MarkdownString(`**${task.title}**\n\nRepository: \`${task.repo_root}\`\n\nWorkspace: \`${ws?.path}\` (${ws?.kind})\n\nFork: ${task.fork_provenance || 'unknown'}`);
    item.contextValue = 'task';
    return { item, task };
  }
  runNode(run, parent) {
    const m = this.model;
    const kids = m.children(run.id).length;
    const caps = run.capabilities || {};
    const expandable = kids || (!run.parent_run_id && typeof caps.children === 'string' && !caps.children.startsWith('supported'));
    const label = run.parent_run_id ? run.title : `${run.harness}`;
    const item = new vscode.TreeItem(label, this.expansion('run:' + run.id, !!expandable));
    item.id = 'run:' + run.id;
    item.iconPath = statusIcon(run.status);
    const profile = run.profile_id ? m.profile(run.profile_id) : undefined;
    const status = STATUS_LABEL[run.status] || run.status;
    item.description = run.parent_run_id
      ? `${status} · native child${run.relation_confidence?.startsWith('exact') ? '' : ' (inferred)'}`
      : `${status}${profile ? ' · ' + profile.name : ''}${run.model ? ' · ' + run.model : ''}`;
    const ws = m.workspace(run.workspace_id);
    const lines = [`**${run.title}**`, '', `Run \`${run.id}\` · ${run.harness} ${run.harness_version || ''}`, `Status: ${status}${run.exit_reason ? ` — ${run.exit_reason}` : ''}`,
      `Workspace: \`${ws?.path}\`${run.parent_run_id ? ' (shared with parent)' : ''}`];
    if (run.native_id) lines.push(`Native id: \`${run.native_id}\``);
    if (run.relation_source) lines.push(`Relationship evidence: ${run.relation_source} (${run.relation_confidence})`);
    if (run.attention) lines.push(`**Needs you:** ${run.attention.tool} permission request`);
    item.tooltip = new vscode.MarkdownString(lines.join('\n\n'));
    item.contextValue = run.parent_run_id ? 'run-child' : (ACTIVE.has(run.status) ? 'run-root-active' : 'run-root');
    item.command = { command: 'overseer.selectRun', title: 'Select', arguments: [run.id] };
    return { item, run, parent };
  }
}

class DirtyProvider {
  constructor(model, client) {
    this.model = model; this.client = client;
    this.emitter = new vscode.EventEmitter();
    this.onDidChangeTreeData = this.emitter.event;
    this.status = undefined;
  }
  select(runId) { this.runId = runId; this.refresh(); }
  async refresh() {
    const run = this.runId && this.model.run(this.runId);
    if (!run) { this.status = undefined; this.emitter.fire(); return; }
    try { this.status = await this.client.request('workspace.status', { workspace_id: run.workspace_id }); this.error = undefined; }
    catch (error) { this.error = error.message; }
    const fingerprint = JSON.stringify([this.status, this.error, this.drafts()]);
    if (fingerprint !== this.last) { this.last = fingerprint; this.emitter.fire(); }
  }
  workspace() { const run = this.runId && this.model.run(this.runId); return run && this.model.workspace(run.workspace_id); }
  drafts() {
    const ws = this.workspace(); if (!ws) return [];
    return vscode.workspace.textDocuments.filter(d => d.isDirty && d.uri.scheme === 'file' && !path.relative(ws.path, d.uri.fsPath).startsWith('..'))
      .map(d => path.relative(ws.path, d.uri.fsPath));
  }
  getTreeItem(node) { return node.item; }
  getChildren(node) {
    const ws = this.workspace();
    if (!ws) return [{ item: new vscode.TreeItem('Select an agent run to see its workspace.') }];
    if (this.error) return [{ item: new vscode.TreeItem('Status unavailable: ' + this.error) }];
    const st = this.status || { staged: [], unstaged: [], untracked: [], conflicted: [] };
    if (!node) {
      const group = (key, label, files, icon) => {
        const item = new vscode.TreeItem(`${label} (${files.length})`, files.length ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.None);
        item.iconPath = new vscode.ThemeIcon(icon); item.contextValue = 'dirty-group';
        return { item, key, files };
      };
      const header = new vscode.TreeItem(`${ws.kind === 'current' ? 'Current checkout' : 'Worktree'} · ${st.branch || 'detached'}`);
      header.description = ws.path; header.tooltip = ws.path; header.iconPath = new vscode.ThemeIcon('folder-opened');
      return [{ item: header },
        group('staged', 'Staged (HEAD → index)', st.staged, 'diff-added'),
        group('unstaged', 'Unstaged (index → working tree)', st.unstaged, 'diff-modified'),
        group('untracked', 'Untracked', st.untracked.map(p => ({ path: p, status: '?' })), 'diff-ignored'),
        group('conflicted', 'Conflicted', st.conflicted.map(p => ({ path: p, status: 'U' })), 'warning'),
        group('drafts', 'Unsaved drafts', this.drafts().map(p => ({ path: p, status: '•' })), 'circle-filled')];
    }
    return (node.files || []).map(f => {
      const item = new vscode.TreeItem(path.basename(f.path));
      item.description = `${path.dirname(f.path) === '.' ? '' : path.dirname(f.path) + ' · '}${f.status}${f.old_path ? ' ← ' + f.old_path : ''}`;
      item.resourceUri = vscode.Uri.file(path.join(ws.path, f.path));
      item.command = { command: 'overseer.openDirtyDiff', title: 'Open Diff', arguments: [node.key, ws.path, f] };
      return { item };
    });
  }
}

class AccountsProvider {
  constructor(model) {
    this.model = model;
    this.emitter = new vscode.EventEmitter();
    this.onDidChangeTreeData = this.emitter.event;
    model.onDidChange(() => this.emitter.fire());
  }
  getTreeItem(node) { return node.item; }
  /** Accounts grouped by provider (docs/rfcs/account-governance.md). */
  getChildren(node) {
    const accounts = this.model.accounts || this.model.state.profiles.map(p => ({ id: p.id, name: p.name, provider: { codex: 'openai', claude: 'anthropic', opencode: 'local' }[p.harness], kind: p.is_system ? 'follows-app' : 'fixed', harnesses: [p.harness] }));
    const providers = this.model.providers || [{ id: 'openai', label: 'OpenAI / ChatGPT', available: true }, { id: 'anthropic', label: 'Anthropic / Claude', available: true }, { id: 'local', label: 'OpenCode (local models)', available: true }];
    if (!node) {
      return providers.map(pr => {
        const mine = accounts.filter(a => a.provider === pr.id);
        const item = new vscode.TreeItem(pr.label, mine.length ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.None);
        item.id = 'provider:' + pr.id;
        item.iconPath = new vscode.ThemeIcon(pr.id === 'openai' ? 'hubot' : pr.id === 'anthropic' ? 'sparkle' : pr.id === 'local' ? 'server' : 'circle-slash');
        item.description = pr.available ? (pr.harnesses?.length ? pr.harnesses.join(', ') : '') : `unavailable: ${pr.why}`;
        item.tooltip = pr.available ? `Sign-in: ${pr.sign_in || 'provider flow'}. Account login only; no API keys.` : pr.why;
        item.contextValue = pr.available && pr.id !== 'local' ? 'provider' : 'provider-unavailable';
        return { item, provider: pr };
      });
    }
    if (!node.provider) return [];
    return accounts.filter(a => a.provider === node.provider.id).map(a => {
      const p = this.model.profile(a.id) || { id: a.id, name: a.name, harness: node.provider.id, is_system: a.kind === 'follows-app' };
      const st = this.model.profileStatus.get(a.id);
      const item = new vscode.TreeItem(a.name);
      item.id = 'profile:' + a.id;
      let detail = 'status not checked';
      if (st) {
        if (!st.installed) detail = 'harness not installed';
        else if (st.logged_in) detail = `signed in${st.identity?.plan ? ` · ${st.identity.plan}` : ''}${st.identity?.account_fingerprint ? ` · ${st.identity.account_fingerprint.slice(0, 8)}` : st.identity?.fingerprint ? ` · ${st.identity.fingerprint.slice(0, 8)}` : ''}`;
        else detail = 'not signed in';
      }
      item.description = `${detail}${a.kind === 'follows-app' ? ' · follows app' : ''}`;
      item.iconPath = new vscode.ThemeIcon(st?.logged_in ? (a.kind === 'follows-app' ? 'link' : 'account') : 'circle-slash', st?.logged_in ? new vscode.ThemeColor('charts.green') : undefined);
      item.tooltip = new vscode.MarkdownString(`**${a.name}** — ${node.provider.label}\n\n${a.kind === 'follows-app' ? `Follows ${a.follows}. It changes when that app switches accounts; Overseer never signs it out.` : `Fixed account with its own credential folder: \`${p.home || ''}\`. The desktop app switching accounts does not change it.`}\n\nUsable by: ${(a.harnesses || []).join(', ')}${a.last_used_ms ? `\n\nLast used ${new Date(a.last_used_ms).toLocaleString()}` : ''}${st ? '\n\n```\n' + (st.detail || '') + '\n```' : ''}`);
      // The sign-in state picks the menu: Sign In for a signed-out account, Sign Out only for a signed-in one.
      item.contextValue = `${a.kind === 'follows-app' ? 'profile-system' : 'profile-isolated'}-${st?.logged_in ? 'signedin' : 'signedout'}`;
      return { item, profile: p, account: a };
    });
  }
}

module.exports = { Model, AgentsProvider, DirtyProvider, AccountsProvider, ACTIVE, statusIcon };
