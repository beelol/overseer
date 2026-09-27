// Side bar views: the agents list (Gate K) and account profiles; plus the shared state model.
const vscode = require('vscode');
const path = require('path');

const STATUS_ICON = {
  queued: ['clock', 'charts.yellow'], starting: ['loading~spin', 'charts.blue'], running: ['sync~spin', 'charts.blue'],
  planned: ['clock', 'charts.yellow'], ready: ['clock', 'charts.yellow'], reserved: ['clock', 'charts.blue'],
  launching: ['loading~spin', 'charts.blue'], submitted: ['inbox', 'charts.orange'], blocked: ['warning', 'charts.orange'],
  accepted: ['pass', 'charts.green'], rejected: ['error', 'charts.red'],
  cancel_requested: ['debug-stop', 'charts.orange'], cancelled: ['debug-stop', 'charts.orange'],
  superseded: ['history', 'descriptionForeground'],
  waiting_for_user: ['bell-dot', 'charts.orange'], completed: ['pass', 'charts.green'], failed: ['error', 'charts.red'],
  interrupted: ['debug-stop', 'charts.orange'], disconnected: ['debug-disconnect', 'charts.red'], unknown: ['question', 'charts.purple'],
};
const ACTIVE = new Set(['queued', 'starting', 'running', 'waiting_for_user']);
const STATUS_LABEL = { waiting_for_user: 'waiting for you' };
// Continuity's states (Gate L): waiting for a connection or for memory (still the agent's work), and handed off.
const CONTINUITY = require('../media/continuity-text.js').STATES;
for (const [s, x] of Object.entries(CONTINUITY)) { STATUS_ICON[s] = [x.icon, x.active ? 'charts.orange' : 'descriptionForeground']; if (x.active) ACTIVE.add(s); }

function statusIcon(status) {
  const [icon, color] = STATUS_ICON[status] || STATUS_ICON.unknown;
  return new vscode.ThemeIcon(icon, new vscode.ThemeColor(color));
}

/** The machine's own login reads "Your login" (the harness is named beside it), not "codex (existing login)". */
function accountName(a) { return a && (a.is_system || a.kind === 'follows-app' || / \(existing login\)$/.test(a.name || '')) ? 'Your login' : a?.name; }

class Model {
  constructor(client) {
    this.client = client;
    this.state = { tasks: [], runs: [], workspaces: [], profiles: [], turns: {} };
    this.all = this.state;
    // Tasks Overseer runs for itself (the Talk to Overseer chat, AC-107): kept out of every list.
    this.hidden = new Set();
    this.emitter = new vscode.EventEmitter();
    this.onDidChange = this.emitter.event;
    this.profileStatus = new Map();
    this.swarms = [];
    this.swarmFetchMs = 0;
  }
  async refresh(forceSwarm = false) {
    try {
      this.all = await this.client.request('state'); for (const p of this.all.profiles || []) p.name = accountName(p);
      this.state = this.visible(this.all); this.error = undefined;
      if (forceSwarm || Date.now() - this.swarmFetchMs >= 1000) {
        try {
          const page = await this.client.request('swarm.list', { limit: 20 });
          this.swarms = page.runs || [];
          this.swarmError = undefined;
        } catch (error) {
          // A newer extension can still view ordinary agents on an older daemon.
          this.swarms = [];
          this.swarmError = error.message;
        }
        this.swarmFetchMs = Date.now();
      }
    }
    catch (error) { this.error = error.message; }
    this.emitter.fire();
  }
  // Coalesces bursts without starving: a steady stream of events still refreshes every 120 ms.
  scheduleRefresh() { if (!this.timer) this.timer = setTimeout(() => { this.timer = undefined; this.refresh(); }, 120); }
  hide(taskId) { if (taskId && !this.hidden.has(taskId)) { this.hidden.add(taskId); this.state = this.visible(this.all); this.emitter.fire(); } }
  visible(all) {
    const swarmTasks = new Set((all.runs || []).filter(r => r.swarm_membership && !r.parent_run_id)
      .map(r => r.task_id));
    if (!this.hidden.size && !swarmTasks.size) return all;
    const excluded = new Set([...this.hidden, ...swarmTasks]);
    const runs = (all.runs || []).filter(r => !excluded.has(r.task_id));
    const used = new Set(runs.map(r => r.workspace_id));
    return { ...all, tasks: (all.tasks || []).filter(t => !excluded.has(t.id)), runs, workspaces: (all.workspaces || []).filter(w => used.has(w.id) || !(all.runs || []).some(r => r.workspace_id === w.id)) };
  }
  // Lookups see every run, hidden ones included (the Overseer chat shows its own run).
  run(id) { return this.all.runs.find(r => r.id === id); }
  task(id) { return this.all.tasks.find(t => t.id === id); }
  workspace(id) { return this.all.workspaces.find(w => w.id === id); }
  profile(id) { return this.state.profiles.find(p => p.id === id); }
  children(runId) { return this.all.runs.filter(r => r.parent_run_id === runId); }
  rootRun(run) { let r = run; const seen = new Set(); while (r?.parent_run_id && !seen.has(r.id)) { seen.add(r.id); r = this.run(r.parent_run_id); } return r; }
  descendants(runId) {
    const out = [], queue = [runId], seen = new Set();
    while (queue.length) { const id = queue.shift(); if (seen.has(id)) continue; seen.add(id); for (const c of this.children(id)) { out.push(c); queue.push(c.id); } }
    return out;
  }
}

const LOGO_FOR_HARNESS = { claude: 'claudecode', codex: 'codex', 'codex-app': 'codex', opencode: 'opencode', 'opencode-serve': 'opencode' };
const STATUS_TEXT = { queued: 'queued', starting: 'starting', running: 'working', waiting_for_user: 'needs you', completed: 'done', failed: 'failed', interrupted: 'stopped', disconnected: 'disconnected', unknown: 'unknown' };
// Status as a row badge (the row icon is the provider's logo, AC-68).
const STATUS_BADGE = {
  queued: ['○', 'charts.yellow'], starting: ['○', 'charts.blue'], running: ['●', 'charts.blue'], waiting_for_user: ['!', 'charts.orange'],
  completed: ['✓', 'charts.green'], failed: ['✕', 'charts.red'], interrupted: ['■', 'descriptionForeground'], disconnected: ['✕', 'charts.red'], unknown: ['?', 'charts.purple'],
};

for (const [s, x] of Object.entries(CONTINUITY)) { STATUS_TEXT[s] = x.text.toLowerCase(); STATUS_BADGE[s] = [x.active ? '☁' : '→', x.active ? 'charts.orange' : 'descriptionForeground']; }

function ago(ms) {
  if (!ms) return '';
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return 'now';
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

function nativeAmount(milli, unit) {
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 3 }).format((milli || 0) / 1000)} ${unit}`;
}

function unconfirmedExitLabel(count) { return `${count} exit${count === 1 ? '' : 's'} unconfirmed`; }

/** The side bar's agents list (AC-67 to AC-71): Needs you, then agents by repository. */
class AgentsProvider {
  constructor(model, memento, extensionUri, handlers = {}) {
    this.model = model; this.memento = memento; this.extensionUri = extensionUri; this.handlers = handlers;
    // Expansion is remembered per workspace (item ids are stable), so reloads keep the tree shape.
    this.collapsed = new Set(memento?.get('overseer.collapsed', []) || []);
    this.expandedSwarms = new Set(memento?.get('overseer.expandedSwarms', []) || []);
    this.filter = undefined; // { query, taskIds: Set }
    this.showArchived = false;
    this.statusFilter = 'all'; // 'all' | 'working' | 'needs' | 'done' | 'failed' (the search field's filters)
    this.swarmStatusFilter = 'all';
    this.emitter = new vscode.EventEmitter();
    this.onDidChangeTreeData = this.emitter.event;
    // Redraw only when something the list shows changed: a redraw between a click's mouse-down and
    // mouse-up loses the click, and busy agents change the model many times a second.
    model.onDidChange(() => { const sig = this.signature(); if (sig === this.shownSignature) return; this.shownSignature = sig; this.emitter.fire(); this.decorationEmitter?.fire(undefined); });
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
  }
  /** What the list shows (relative times in 30-second steps). */
  signature() {
    const st = this.model.state || {};
    return JSON.stringify([Math.floor(Date.now() / 30000), (st.tasks || []).map(t => [t.id, t.title, t.repo_root, t.archived_ms ? 1 : 0]),
      (st.runs || []).map(r => [r.id, r.status, r.parent_run_id, r.attention?.kind, r.harness, r.model, r.profile_id, r.workspace_id, r.title, r.exit_reason, r.ended_ms ? 1 : 0]),
      (st.profiles || []).map(p => [p.id, p.name]), (st.workspaces || []).map(w => [w.id, w.branch, w.kind]),
      (this.handlers.attention?.() || []).map(a => [a.run_id, a.label, a.detail]), this.handlers.pinned?.() || [],
      this.model.swarms.map(s => [s.id, s.status, s.revision, s.active_worker_processes,
        s.job_counts, s.unconfirmed_exit_count, s.benefit?.decision, s.benefit?.reason, s.capacity]),
      (this.continuity?.data?.handoffs || []).length]);
  }
  getTreeItem(node) { return node.item; }
  getParent(node) { return node.parent; }
  refresh() { if (this.indexed) this.indexed.visible.clear(); this.emitter.fire(); }
  setSwarmStatusFilter(status) { this.swarmStatusFilter = status; this.refresh(); }
  setCollapsed(node, collapsed) {
    const id = node?.item?.id; if (!id) return;
    if (id.startsWith('swarm:')) {
      if (collapsed) this.expandedSwarms.delete(id); else this.expandedSwarms.add(id);
      this.memento?.update('overseer.expandedSwarms', [...this.expandedSwarms].slice(-500));
      return;
    }
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
    const key = `${this.showArchived}|${this.statusFilter}|${this.filter ? this.filter.query + ':' + this.filter.taskIds.size : ''}`;
    if (ix.visible.has(key)) return ix.visible.get(key);
    const archived = t => !!t.archived_ms;
    const list = this.model.state.tasks.filter(t => ix.roots.has(t.id))
      // Search looks within the list shown: active agents, or archived ones under Show Archived.
      .filter(t => (!this.filter || this.filter.taskIds.has(t.id)) && (this.showArchived ? archived(t) : !archived(t)) && this.statusMatches(t))
      .map(t => ({ t, at: this.lastActivity(t) })).sort((a, b) => b.at - a.at).map(x => x.t);
    ix.visible.set(key, list);
    return list;
  }
  /** The search field's status filter, on the task's root run. */
  statusMatches(task) {
    if (this.statusFilter === 'all') return true;
    const r = this.rootOf(task); if (!r) return false;
    if (this.statusFilter === 'needs') return (this.handlers.attention?.() || []).some(a => a.run_id === r.id);
    if (this.statusFilter === 'working') return ACTIVE.has(r.status) && r.status !== 'waiting_for_user';
    if (this.statusFilter === 'done') return r.status === 'completed' || r.status === 'interrupted';
    if (this.statusFilter === 'failed') return r.status === 'failed' || r.status === 'disconnected';
    return true;
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
    // A Swarm worker can be opened from its job, but its task is deliberately
    // absent from the ordinary Agents tree. Never construct a node for a row
    // that the tree cannot actually reveal.
    if (!task || !this.visibleTasks().some(visible => visible.id === task.id)) return undefined;
    let node = this.agentNode(task, this.repoNode(task.repo_root));
    for (const run of chain.slice(1)) node = this.childNode(run, node);
    return node;
  }
  getChildren(node) {
    const m = this.model;
    if (!node) {
      if (m.error) return [{ item: Object.assign(new vscode.TreeItem(`Daemon unavailable: ${m.error}`), { iconPath: new vscode.ThemeIcon('warning') }) }];
      const out = [];
      const needs = this.filter || this.showArchived || this.statusFilter !== 'all' ? [] : (this.handlers.attention?.() || []);
      if (needs.length) out.push(this.needsSection(needs));
      if (m.swarms.length && !this.filter && !this.showArchived) out.push(this.swarmsSection());
      const repos = [...new Set(this.visibleTasks().map(t => t.repo_root))];
      for (const repo of repos) out.push(this.repoNode(repo));
      return out;
    }
    if (node.section === 'needs') return node.list.map(a => this.needsRow(a, node)).filter(Boolean);
    if (node.section === 'swarms') return m.swarms.map(run => this.swarmNode(run, node));
    if (node.swarm) return this.swarmPage(node.swarm, undefined, node, true);
    if (node.swarmPage) return this.swarmPage(node.swarmPage.run, node.swarmPage.cursor, node, false);
    if (node.swarmCapacity) return this.swarmCapacityRows(node.swarmCapacity, node);
    if (node.job) return (node.job.worker_runs || []).map(worker => this.swarmWorkerNode(worker, node));
    if (node.repo) return this.visibleTasks().filter(t => t.repo_root === node.repo).map(t => this.agentNode(t, node));
    if (node.earlier) return [];
    if (node.run) return [...this.kidsOf(node.run.id).map(run => this.childNode(run, node)), ...this.earlier(node)];
    return [];
  }
  swarmsSection() {
    const item = new vscode.TreeItem('Swarms', this.expansion('section:swarms'));
    item.id = 'section:swarms';
    item.iconPath = new vscode.ThemeIcon('organization');
    item.description = String(this.model.swarms.length);
    item.contextValue = 'section-swarms';
    return { item, section: 'swarms' };
  }
  swarmNode(run, parent) {
    const counts = run.job_counts?.by_status || {};
    const working = run.active_worker_processes || 0;
    const item = new vscode.TreeItem(run.category, this.expandedSwarms.has('swarm:' + run.id)
      ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.Collapsed);
    item.id = 'swarm:' + run.id;
    item.iconPath = new vscode.ThemeIcon('organization');
    item.description = `${working} working · ${counts.ready || 0} ready · ${counts.blocked || 0} blocked` +
      (run.unconfirmed_exit_count ? ` · ${unconfirmedExitLabel(run.unconfirmed_exit_count)}` : '');
    item.tooltip = `${run.objective}\n${run.status} · ${run.job_counts?.total || 0} jobs\n${item.description}\nProvider usage ${run.capacity?.provider_usage_state || 'unknown'}`;
    item.accessibilityInformation = { label: `${run.category} swarm, ${run.status}, ${item.description}` };
    item.contextValue = 'swarm-run-' + run.status;
    return { item, swarm: run, parent };
  }
  async swarmPage(run, cursor, parent, includeDirector) {
    const page = await this.model.client.request('swarm.jobs', { id: run.id, cursor, limit: 50,
      ...(this.swarmStatusFilter === 'all' ? {} : { status: this.swarmStatusFilter }) });
    const rows = [];
    if (includeDirector) {
      const item = new vscode.TreeItem('Director');
      item.id = 'swarm-director:' + run.id;
      item.iconPath = new vscode.ThemeIcon('account');
      item.description = run.director?.process_status || run.director?.owner_status || 'not started';
      item.contextValue = 'swarm-director';
      rows.push({ item, parent });
      const capacity = new vscode.TreeItem('Capacity', vscode.TreeItemCollapsibleState.Collapsed);
      capacity.id = 'swarm-capacity:' + run.id;
      capacity.iconPath = new vscode.ThemeIcon('pulse');
      const selected = run.capacity?.selected_targets?.length || 0;
      capacity.description = `${selected}${run.capacity?.selected_targets_truncated ? '+' : ''} selected · usage ${run.capacity?.provider_usage_state || 'unknown'}`;
      capacity.tooltip = 'Selected targets and frozen fixture commitments. Provider usage is not reported.';
      capacity.accessibilityInformation = { label: `Swarm capacity, ${capacity.description}` };
      rows.push({ item: capacity, swarmCapacity: run, parent });
      if (run.unconfirmed_exit_count) {
        const exits = new vscode.TreeItem(unconfirmedExitLabel(run.unconfirmed_exit_count));
        exits.id = 'swarm-unconfirmed:' + run.id;
        exits.iconPath = new vscode.ThemeIcon('warning', new vscode.ThemeColor('charts.orange'));
        exits.description = 'Stop requested · reservations held';
        exits.tooltip = 'These swarm processes have not confirmed exit. Their reservations remain held.';
        rows.push({ item: exits, parent });
      }
    }
    for (const job of page.jobs || []) {
      const workerRuns = job.worker_runs || [];
      const active = workerRuns.find(worker => !worker.ended_ms &&
        ['queued', 'starting', 'running', 'waiting_for_user'].includes(worker.status));
      const item = new vscode.TreeItem(job.title, workerRuns.length
        ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None);
      item.id = `swarm-job:${run.id}:${job.id}`;
      item.iconPath = statusIcon(active?.status || job.status);
      item.description = active?.status === 'running' ? 'working' : active?.status || job.status;
      item.tooltip = `${job.acceptance}\n${item.description} · ${job.attempt_count} attempts`;
      item.accessibilityInformation = { label: `${job.title}, ${item.description}, ${job.attempt_count} attempts` };
      item.contextValue = 'swarm-job';
      rows.push({ item, job, parent });
    }
    if (page.next_cursor) {
      const item = new vscode.TreeItem('More jobs…', vscode.TreeItemCollapsibleState.Collapsed);
      item.id = `swarm-page:${run.id}:${page.next_cursor}`;
      item.iconPath = new vscode.ThemeIcon('list-unordered');
      rows.push({ item, swarmPage: { run, cursor: page.next_cursor }, parent });
    }
    return rows;
  }
  swarmCapacityRows(run, parent) {
    const rows = [];
    for (const target of run.capacity?.selected_targets || []) {
      const item = new vscode.TreeItem(`Target: ${target.id}`);
      item.iconPath = this.logo(target.harness);
      const account = target.profile_id ? this.model.profile(target.profile_id)?.name || target.profile_id : 'account unknown';
      item.description = `${target.harness || 'harness unknown'} · ${account} · ${target.attempts} attempt${target.attempts === 1 ? '' : 's'}`;
      item.tooltip = [target.id, target.harness, account, target.model, target.effort].filter(Boolean).join(' · ');
      rows.push({ item, parent });
    }
    if (run.capacity?.selected_targets_truncated) {
      const item = new vscode.TreeItem('More selected targets');
      item.description = 'readout capped at 32';
      rows.push({ item, parent });
    }
    for (const window of run.capacity?.windows || []) {
      const item = new vscode.TreeItem(`Allocation: ${window.pool_id} / ${window.window_id}`);
      item.iconPath = new vscode.ThemeIcon('graph');
      item.description = `${nativeAmount(window.allocation_milli, window.unit)} · finishing reserve ${nativeAmount(window.finishing_reserve_milli, window.unit)}`;
      item.tooltip = `${item.description}\nOutstanding estimate: ${nativeAmount(window.outstanding_estimate_milli, window.unit)}. Fixture commitment, not measured provider usage.`;
      rows.push({ item, parent });
    }
    if (run.capacity?.windows_truncated) {
      const item = new vscode.TreeItem('More allocation windows');
      item.description = 'readout capped at 32';
      rows.push({ item, parent });
    }
    if (run.benefit?.decision) {
      const item = new vscode.TreeItem(`Planning: ${run.benefit.decision}`);
      item.iconPath = new vscode.ThemeIcon('lightbulb');
      item.description = (run.benefit.reason || 'reason unknown').replaceAll('_', ' ');
      item.tooltip = `Latest recorded planning decision. Current admission limit may differ. ${item.description}`;
      rows.push({ item, parent });
    }
    const last = run.capacity?.last_admission;
    if (last) {
      const held = last.status === 'blocked';
      const item = new vscode.TreeItem(held ? 'Last admission held' : 'Last admission: admitted');
      item.iconPath = new vscode.ThemeIcon(held ? 'warning' : 'check');
      item.description = held ? String(last.reason || 'reason unknown').replaceAll('_', ' ') : `${last.job_id} → ${last.target_id}`;
      item.tooltip = `Job ${last.job_id} · target ${last.target_id}\n${item.description}\nRecorded ${new Date(last.observed_ms).toLocaleString()}. Current eligibility may have changed.`;
      rows.push({ item, parent });
    } else {
      const item = new vscode.TreeItem('No admission recorded');
      item.iconPath = new vscode.ThemeIcon('question');
      item.description = 'current limit unknown';
      rows.push({ item, parent });
    }
    const usage = new vscode.TreeItem(`Provider usage ${run.capacity?.provider_usage_state || 'unknown'}`);
    usage.iconPath = new vscode.ThemeIcon('question');
    usage.description = 'current allowance not reported';
    rows.push({ item: usage, parent });
    return rows;
  }
  swarmWorkerNode(worker, parent) {
    const item = new vscode.TreeItem('Worker');
    item.id = `swarm-worker:${worker.overseer_run_id}`;
    item.iconPath = this.logo(worker.harness);
    item.description = worker.status === 'running' ? 'working' : worker.status;
    const profile = worker.profile_id ? this.model.profile(worker.profile_id) : undefined;
    item.tooltip = [worker.harness, profile?.name, worker.model, worker.status].filter(Boolean).join(' · ');
    item.accessibilityInformation = { label: `Worker, ${item.tooltip}` };
    item.contextValue = 'swarm-worker';
    item.command = { command: 'overseer.selectRun', title: 'Open worker', arguments: [worker.overseer_run_id] };
    return { item, worker, parent };
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
    // One row may stand for several agents (Continuity's waiting agents): it brings its own title.
    const item = new vscode.TreeItem(a.title || task?.title || run.title);
    item.id = 'needs:' + run.id;
    // The provider's mark (AC-68), the reason as text, and the same status badge as the agent's row.
    item.iconPath = this.logo(run.harness);
    item.resourceUri = vscode.Uri.from({ scheme: 'overseer-agent', path: '/' + run.id });
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
    const kids = this.kidsOf(run.id).length + (this.continuity?.predecessors(run).length || 0);
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
  /** The agents whose work this one took over (Gate L), folded under it, newest first. */
  earlier(node) {
    if (!node.task || !this.continuity) return [];
    return this.continuity.predecessors(node.run).map(({ run, reason }) => {
      const why = { offline: 'the connection was lost', back_online: 'the connection came back', user: 'moved by you' }[reason] || String(reason).replace(/^provider_unreachable:(.*)$/, (m, p) => `${{ openai: 'OpenAI', anthropic: 'Claude' }[p] || p} could not be reached`);
      const name = { claude: 'Claude Code', codex: 'Codex', 'codex-app': 'Codex', opencode: 'OpenCode', 'opencode-serve': 'Local model' }[run.harness] || run.harness;
      const item = new vscode.TreeItem(`Earlier: ${name}`);
      item.id = 'earlier:' + run.id;
      item.iconPath = this.logo(run.harness === 'opencode-serve' ? 'opencode' : run.harness);
      item.description = `handed off · ${why}`;
      item.tooltip = new vscode.MarkdownString(`**${run.title}**\n\nHanded off: ${why}.\n\n${[name, run.model].filter(Boolean).join(' · ')}`);
      item.accessibilityInformation = { label: `Earlier agent, ${name}, handed off, ${why}` };
      item.contextValue = 'agent-earlier';
      item.command = { command: 'overseer.selectRun', title: 'Open', arguments: [run.id] };
      return { item, run, parent: node, earlier: true };
    });
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

module.exports = { Model, AgentsProvider, AccountsProvider, ACTIVE, statusIcon, ago, accountName };
