// Side bar views: the agents list (Gate K) and account profiles; plus the shared state model.
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
  // Coalesces bursts without starving: a steady stream of events still refreshes every 120 ms.
  scheduleRefresh() { if (!this.timer) this.timer = setTimeout(() => { this.timer = undefined; this.refresh(); }, 120); }
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

const LOGO_FOR_HARNESS = { claude: 'claudecode', codex: 'codex', 'codex-app': 'codex', opencode: 'opencode' };
const STATUS_TEXT = { queued: 'queued', starting: 'starting', running: 'working', waiting_for_user: 'needs you', completed: 'done', failed: 'failed', interrupted: 'stopped', disconnected: 'disconnected', unknown: 'unknown' };
// Status as a row badge (the row icon is the provider's logo, AC-68).
const STATUS_BADGE = {
  queued: ['○', 'charts.yellow'], starting: ['○', 'charts.blue'], running: ['●', 'charts.blue'], waiting_for_user: ['!', 'charts.orange'],
  completed: ['✓', 'charts.green'], failed: ['✕', 'charts.red'], interrupted: ['■', 'descriptionForeground'], disconnected: ['✕', 'charts.red'], unknown: ['?', 'charts.purple'],
};
const NEEDS_ICON = { Approve: ['shield', 'charts.orange'], Reply: ['comment-discussion', 'charts.orange'], Failed: ['error', 'charts.red'], Review: ['diff', 'charts.blue'] };

function ago(ms) {
  if (!ms) return '';
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return 'now';
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

/** The side bar's agents list (AC-67 to AC-71): Needs you, then agents by repository. */
class AgentsProvider {
  constructor(model, memento, extensionUri, handlers = {}) {
    this.model = model; this.memento = memento; this.extensionUri = extensionUri; this.handlers = handlers;
    // Expansion is remembered per workspace (item ids are stable), so reloads keep the tree shape.
    this.collapsed = new Set(memento?.get('overseer.collapsed', []) || []);
    this.filter = undefined; // { query, taskIds: Set }
    this.showArchived = false;
    this.emitter = new vscode.EventEmitter();
    this.onDidChangeTreeData = this.emitter.event;
    model.onDidChange(() => this.emitter.fire());
    // Status badges and colors for agent rows (resourceUri overseer-agent:/<run id>).
    this.decorationEmitter = new vscode.EventEmitter();
    this.decorations = {
      onDidChangeFileDecorations: this.decorationEmitter.event,
      provideFileDecoration: uri => {
        if (uri.scheme !== 'overseer-agent') return undefined;
        const run = this.model.run(uri.path.replace(/^\//, ''));
        if (!run) return undefined;
        const [badge, color] = STATUS_BADGE[run.status] || STATUS_BADGE.unknown;
        return { badge, color: ['failed', 'disconnected', 'waiting_for_user'].includes(run.status) ? new vscode.ThemeColor(color) : undefined, tooltip: STATUS_TEXT[run.status] || run.status, propagate: false };
      },
    };
    model.onDidChange(() => this.decorationEmitter.fire(undefined));
  }
  getTreeItem(node) { return node.item; }
  getParent(node) { return node.parent; }
  refresh() { if (this.indexed) this.indexed.visible.clear(); this.emitter.fire(); }
  setCollapsed(node, collapsed) {
    const id = node?.item?.id; if (!id) return;
    if (collapsed) this.collapsed.add(id); else this.collapsed.delete(id);
    this.memento?.update('overseer.collapsed', [...this.collapsed].slice(-500));
  }
  expansion(id, expandable = true) {
    if (!expandable) return vscode.TreeItemCollapsibleState.None;
    return this.collapsed.has(id) ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.Expanded;
  }
  logo(harness) {
    const name = LOGO_FOR_HARNESS[harness];
    if (!name || !this.extensionUri) return new vscode.ThemeIcon(harness === 'generic' ? 'terminal' : 'hubot');
    return { light: vscode.Uri.joinPath(this.extensionUri, 'media', 'logos', `${name}-light.svg`), dark: vscode.Uri.joinPath(this.extensionUri, 'media', 'logos', `${name}-dark.svg`) };
  }
  /** One pass over the state per snapshot: each task's newest top-level run, each run's children. */
  index() {
    const st = this.model.state;
    if (this.indexed?.state === st) return this.indexed;
    const roots = new Map(), kids = new Map();
    for (const r of st.runs) {
      if (r.parent_run_id) { if (!kids.has(r.parent_run_id)) kids.set(r.parent_run_id, []); kids.get(r.parent_run_id).push(r); continue; }
      const cur = roots.get(r.task_id);
      if (!cur || r.created_ms > cur.created_ms) roots.set(r.task_id, r);
    }
    this.indexed = { state: st, roots, kids, visible: new Map() };
    return this.indexed;
  }
  /** The newest top-level run of a task: the agent a row stands for. */
  rootOf(task) { return this.index().roots.get(task.id); }
  kidsOf(runId) { return this.index().kids.get(runId) || []; }
  visibleTasks() {
    const ix = this.index();
    const key = `${this.showArchived}|${this.filter ? this.filter.query + ':' + this.filter.taskIds.size : ''}`;
    if (ix.visible.has(key)) return ix.visible.get(key);
    const archived = t => !!t.archived_ms;
    const list = this.model.state.tasks.filter(t => ix.roots.has(t.id))
      .filter(t => (this.filter ? this.filter.taskIds.has(t.id) : this.showArchived ? archived(t) : !archived(t)))
      .map(t => ({ t, at: this.lastActivity(t) })).sort((a, b) => b.at - a.at).map(x => x.t);
    ix.visible.set(key, list);
    return list;
  }
  lastActivity(task) {
    const r = this.rootOf(task);
    return Math.max(task.created_ms || 0, r?.ended_ms || 0, r?.created_ms || 0, ACTIVE.has(r?.status) ? Date.now() : 0);
  }
  /** The tree node for a run, with its parents, for TreeView.reveal. */
  nodeFor(runId) {
    const chain = [];
    for (let r = this.model.run(runId), seen = new Set(); r && !seen.has(r.id); r = r.parent_run_id && this.model.run(r.parent_run_id)) { seen.add(r.id); chain.unshift(r); }
    const task = chain.length && this.model.task(chain[0].task_id);
    if (!task) return undefined;
    let node = this.agentNode(task, this.repoNode(task.repo_root));
    for (const run of chain.slice(1)) node = this.childNode(run, node);
    return node;
  }
  getChildren(node) {
    const m = this.model;
    if (!node) {
      if (m.error) return [{ item: Object.assign(new vscode.TreeItem(`Daemon unavailable: ${m.error}`), { iconPath: new vscode.ThemeIcon('warning') }) }];
      const out = [];
      const needs = this.filter || this.showArchived ? [] : (this.handlers.attention?.() || []);
      if (needs.length) out.push(this.needsSection(needs));
      const repos = [...new Set(this.visibleTasks().map(t => t.repo_root))];
      for (const repo of repos) out.push(this.repoNode(repo));
      return out;
    }
    if (node.section === 'needs') return node.list.map(a => this.needsRow(a, node)).filter(Boolean);
    if (node.repo) return this.visibleTasks().filter(t => t.repo_root === node.repo).map(t => this.agentNode(t, node));
    if (node.run) return this.kidsOf(node.run.id).map(run => this.childNode(run, node));
    return [];
  }
  needsSection(list) {
    const item = new vscode.TreeItem('Needs you', this.expansion('section:needs'));
    item.id = 'section:needs';
    item.iconPath = new vscode.ThemeIcon('bell-dot', new vscode.ThemeColor('charts.orange'));
    item.description = String(list.length);
    item.accessibilityInformation = { label: `Needs you, ${list.length}` };
    item.contextValue = 'section-needs';
    return { item, section: 'needs', list };
  }
  needsRow(a, parent) {
    const run = this.model.run(a.run_id); const task = run && this.model.task(run.task_id);
    if (!run) return undefined;
    const item = new vscode.TreeItem(task?.title || run.title);
    item.id = 'needs:' + run.id;
    const [icon, color] = NEEDS_ICON[a.label] || ['bell', 'charts.orange'];
    item.iconPath = new vscode.ThemeIcon(icon, new vscode.ThemeColor(color));
    item.description = a.label;
    item.tooltip = `${task?.title || run.title}\n${a.detail}`;
    item.accessibilityInformation = { label: `${task?.title || run.title}, ${a.label}: ${a.detail}` };
    item.contextValue = 'needs';
    item.command = { command: 'overseer.selectRun', title: 'Open', arguments: [run.id] };
    return { item, run, parent, needs: a };
  }
  repoNode(repo) {
    const tasks = this.visibleTasks().filter(t => t.repo_root === repo);
    const active = tasks.filter(t => ACTIVE.has(this.rootOf(t)?.status)).length;
    const item = new vscode.TreeItem(path.basename(repo), this.expansion('repo:' + repo));
    item.id = 'repo:' + repo;
    item.iconPath = new vscode.ThemeIcon('repo');
    item.description = active ? String(active) : '';
    item.tooltip = repo;
    item.accessibilityInformation = { label: `${path.basename(repo)}, ${tasks.length} agent${tasks.length === 1 ? '' : 's'}${active ? `, ${active} active` : ''}` };
    item.contextValue = 'repo';
    return { item, repo };
  }
  agentNode(task, parent) {
    const run = this.rootOf(task);
    const m = this.model;
    const kids = this.kidsOf(run.id).length;
    const item = new vscode.TreeItem(task.title, this.expansion('agent:' + task.id, kids > 0));
    item.id = 'agent:' + task.id;
    item.iconPath = this.logo(run.harness);
    item.resourceUri = vscode.Uri.from({ scheme: 'overseer-agent', path: '/' + run.id });
    // Working agents show their badge (●); finished ones say how long ago.
    item.description = ACTIVE.has(run.status) ? '' : ago(run.ended_ms || run.created_ms);
    const profile = run.profile_id ? m.profile(run.profile_id) : undefined;
    const ws = m.workspace(run.workspace_id);
    const status = STATUS_TEXT[run.status] || run.status;
    item.tooltip = new vscode.MarkdownString([`**${task.title}**`, `${status}${run.exit_reason && !ACTIVE.has(run.status) ? ` — ${run.exit_reason}` : ''}`,
      [run.harness, profile?.name, run.model].filter(Boolean).join(' · '), ws ? `${ws.kind === 'current' ? 'current checkout' : ws.branch} · ${path.basename(task.repo_root)}` : ''].filter(Boolean).join('\n\n'));
    item.accessibilityInformation = { label: `${task.title}, ${status}, ${run.harness}${profile ? ', ' + profile.name : ''}` };
    const pinned = (this.handlers.pinned?.() || []).includes(run.id);
    item.contextValue = `agent-${ACTIVE.has(run.status) ? 'active' : 'done'}${task.archived_ms ? '-archived' : ''}${pinned ? '-pinned' : ''}`;
    item.command = { command: 'overseer.selectRun', title: 'Open', arguments: [run.id] };
    return { item, run, task, parent };
  }
  childNode(run, parent) {
    const kids = this.kidsOf(run.id).length;
    const item = new vscode.TreeItem(run.title, this.expansion('run:' + run.id, kids > 0));
    item.id = 'run:' + run.id;
    item.iconPath = this.logo(run.harness);
    item.resourceUri = vscode.Uri.from({ scheme: 'overseer-agent', path: '/' + run.id });
    item.description = ACTIVE.has(run.status) ? '' : ago(run.ended_ms || run.created_ms);
    const status = STATUS_TEXT[run.status] || run.status;
    item.tooltip = new vscode.MarkdownString(`**${run.title}**\n\n${status} · native child${run.relation_confidence?.startsWith('exact') ? '' : ' (inferred)'}`);
    item.accessibilityInformation = { label: `${run.title}, ${status}, native child` };
    item.contextValue = 'agent-child';
    item.command = { command: 'overseer.selectRun', title: 'Open', arguments: [run.id] };
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

const LOGO = { openai: 'openai', anthropic: 'claude', local: 'opencode', devin: undefined };
const SHORT = { openai: 'ChatGPT', anthropic: 'Claude', local: 'OpenCode' };

class AccountsProvider {
  constructor(model, extensionUri) {
    this.model = model; this.extensionUri = extensionUri;
    this.emitter = new vscode.EventEmitter();
    this.onDidChangeTreeData = this.emitter.event;
    model.onDidChange(() => this.emitter.fire());
  }
  getTreeItem(node) { return node.item; }
  /** Provider logo (AC-65) for native views; codicon fallback. */
  logo(provider, fallback) {
    const name = LOGO[provider];
    if (!name || !this.extensionUri) return new vscode.ThemeIcon(fallback);
    return { light: vscode.Uri.joinPath(this.extensionUri, 'media', 'logos', `${name}-light.svg`), dark: vscode.Uri.joinPath(this.extensionUri, 'media', 'logos', `${name}-dark.svg`) };
  }
  /** Accounts grouped by provider (docs/rfcs/account-governance.md). */
  getChildren(node) {
    const accounts = this.model.accounts || this.model.state.profiles.map(p => ({ id: p.id, name: p.name, provider: { codex: 'openai', claude: 'anthropic', opencode: 'local' }[p.harness], kind: p.is_system ? 'follows-app' : 'fixed', harnesses: [p.harness] }));
    const providers = this.model.providers || [{ id: 'openai', label: 'OpenAI / ChatGPT', available: true }, { id: 'anthropic', label: 'Anthropic / Claude', available: true }, { id: 'local', label: 'OpenCode (local models)', available: true }];
    if (!node) {
      // Providers without an account login (Devin) are listed in Add Account, not here.
      return providers.filter(pr => pr.available).map(pr => {
        const mine = accounts.filter(a => a.provider === pr.id);
        const item = new vscode.TreeItem(SHORT[pr.id] || pr.label, mine.length ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.None);
        item.id = 'provider:' + pr.id;
        item.iconPath = pr.available ? this.logo(pr.id, 'account') : new vscode.ThemeIcon('circle-slash');
        item.description = pr.available ? '' : 'unavailable';
        item.tooltip = pr.available ? `${pr.label}\nSign-in: ${pr.sign_in || 'provider flow'}. Account login only; no API keys.` : pr.why;
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
        else if (st.logged_in) detail = [st.identity?.plan, (st.identity?.account_fingerprint || st.identity?.fingerprint || '').slice(0, 8)].filter(Boolean).join(' · ') || 'signed in';
        else detail = 'signed out';
      }
      const usage = this.model.accountUsage?.get(a.id);
      const near = usage?.reported ? (usage.windows || []).filter(w => w.used >= 0.8).sort((x, y) => y.used - x.used)[0] : undefined;
      item.description = `${detail}${a.kind === 'follows-app' ? ' · desktop' : ''}${near ? ` · ${Math.round(near.used * 100)}% of ${near.label}` : ''}`;
      item.iconPath = st?.logged_in ? this.logo(a.provider, 'account') : new vscode.ThemeIcon('circle-slash');
      item.accessibilityInformation = { label: `${a.name}, ${st?.logged_in ? 'signed in' : 'not signed in'}${detail && st?.logged_in ? ', ' + detail : ''}${a.kind === 'follows-app' ? ', follows the desktop app' : ''}` };
      item.tooltip = new vscode.MarkdownString(`**${a.name}** — ${node.provider.label}\n\n${a.kind === 'follows-app' ? `Follows ${a.follows}. It changes when that app switches accounts; Overseer never signs it out.` : `Fixed account with its own credential folder: \`${p.home || ''}\`. The desktop app switching accounts does not change it.`}\n\nUsable by: ${(a.harnesses || []).join(', ')}${a.last_used_ms ? `\n\nLast used ${new Date(a.last_used_ms).toLocaleString()}` : ''}\n\n${usage?.reported ? `Usage (${usage.source}): ${(usage.windows || []).map(w => `${w.label} ${Math.round(w.used * 100)}%${w.resets_at_ms ? `, resets ${new Date(w.resets_at_ms).toLocaleString()}` : ''}`).join('; ')}` : 'Usage: not reported by this harness yet'}${st ? '\n\n```\n' + (st.detail || '') + '\n```' : ''}`);
      // The sign-in state picks the menu: Sign In for a signed-out account, Sign Out only for a signed-in one.
      item.contextValue = `${a.kind === 'follows-app' ? 'profile-system' : 'profile-isolated'}-${st?.logged_in ? 'signedin' : 'signedout'}`;
      return { item, profile: p, account: a };
    });
  }
}

module.exports = { Model, AgentsProvider, DirtyProvider, AccountsProvider, ACTIVE, statusIcon, ago };
