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
  constructor(model) {
    this.model = model;
    this.emitter = new vscode.EventEmitter();
    this.onDidChangeTreeData = this.emitter.event;
    model.onDidChange(() => this.emitter.fire());
  }
  getTreeItem(node) { return node.item; }
  getParent(node) { return node.parent; }
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
    const item = new vscode.TreeItem(task.title, vscode.TreeItemCollapsibleState.Expanded);
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
    const item = new vscode.TreeItem(label, expandable ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.None);
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
  getChildren(node) {
    if (node) return [];
    return this.model.state.profiles.map(p => {
      const st = this.model.profileStatus.get(p.id);
      const item = new vscode.TreeItem(p.name);
      item.id = 'profile:' + p.id;
      let detail = 'status not checked';
      if (st) {
        if (!st.installed) detail = 'harness not installed';
        else if (st.logged_in) detail = `signed in${st.identity?.plan ? ` (${st.identity.plan})` : ''}${st.identity?.account_fingerprint ? ` · account ${st.identity.account_fingerprint.slice(0, 8)}` : st.identity?.fingerprint ? ` · id ${st.identity.fingerprint.slice(0, 8)}` : ''}`;
        else detail = 'not signed in';
      }
      item.description = `${p.harness} · ${detail}`;
      item.iconPath = new vscode.ThemeIcon(st?.logged_in ? 'account' : 'circle-slash');
      item.tooltip = new vscode.MarkdownString(`**${p.name}** (${p.harness})\n\n${p.is_system ? 'Uses the harness\'s existing login location. Overseer never logs this profile out.' : `Isolated credential home: \`${p.home}\``}\n\n${st ? '```\n' + (st.detail || '') + '\n```' : ''}`);
      item.contextValue = p.is_system ? 'profile-system' : 'profile-isolated';
      return { item, profile: p };
    });
  }
}

module.exports = { Model, AgentsProvider, DirtyProvider, AccountsProvider, ACTIVE, statusIcon };
