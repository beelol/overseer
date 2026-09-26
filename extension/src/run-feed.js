// Streams runs to a webview: each subscribed run's retained history (paged, merged by sequence,
// including its native descendants), then live events batched every 40 ms so bursts of thousands
// of events become a few messages. Shared by run panels, the dashboard chat and grid tiles.
const vscode = require('vscode');
const { ACTIVE } = require('./views');

function runMessage(model, runId) {
  const run = model.run(runId);
  if (!run) return undefined;
  const profile = run.profile_id && model.profile(run.profile_id);
  const task = model.task(run.task_id);
  return { run, profile: profile?.name, provider: profile?.provider, workspace: model.workspace(run.workspace_id), turns: model.state.turns?.[runId] || [], prompt: task?.prompt,
    repo: task?.repo_root, trusted: vscode.workspace.isTrusted, active: ACTIVE.has(run.status),
    followUpSupported: !String(run.capabilities?.follow_up || '').startsWith('unsupported'),
    interruptSupported: !String(run.capabilities?.interrupt || '').startsWith('unsupported'),
    children: model.descendants(runId).map(c => ({ id: c.id, title: c.title, status: c.status, parent: c.parent_run_id, evidence: c.relation_source })) };
}

class RunFeed {
  /** post(msg) sends to the webview; label(runId) names the source of an event. */
  constructor(client, model, post) {
    this.client = client; this.model = model; this.post = post;
    this.roots = new Map(); // root run id -> Set(run ids including descendants)
    this.queue = []; this.flush = undefined;
    this.listener = event => {
      for (const [root, ids] of this.roots) {
        if (!ids.has(event.run_id)) continue;
        if (event.kind === 'child' && event.payload?.child?.id) ids.add(event.payload.child.id);
        this.queue.push({ root, event, label: this.label(event.run_id) });
        if (!this.flush) this.flush = setTimeout(() => { this.flush = undefined; const items = this.queue; this.queue = []; this.post({ type: 'events', items }); }, 40);
      }
    };
    client.on('event', this.listener);
  }

  label(runId) { const run = this.model.run(runId); return !run ? runId : run.parent_run_id ? `child · ${run.title}` : run.harness; }

  /** Subscribes to exactly these root runs (history is sent for newly added ones). */
  async set(rootIds, { history = true, limit } = {}) {
    const want = new Set(rootIds.filter(Boolean));
    for (const id of [...this.roots.keys()]) if (!want.has(id)) this.roots.delete(id);
    const added = [...want].filter(id => !this.roots.has(id));
    for (const id of added) this.roots.set(id, new Set([id, ...this.model.descendants(id).map(d => d.id)]));
    if (history) await Promise.all(added.map(id => this.sendHistory(id, limit)));
  }

  async sendHistory(root, limit) {
    const ids = this.roots.get(root); if (!ids) return;
    const fetchAll = async id => {
      const out = [];
      for (let after = 0, page = 0; page < 20; page++) {
        const list = await this.client.request('events.list', { run_id: id, after, limit: 5000 });
        out.push(...list.events);
        if (list.events.length < 5000) break;
        after = list.events[list.events.length - 1].seq;
      }
      return out;
    };
    let events = (await Promise.all([...ids].map(fetchAll))).flat().sort((a, b) => a.seq - b.seq);
    const truncated = events.some(e => e.kind === 'retention');
    if (limit && events.length > limit) events = events.slice(-limit);
    this.post({ type: 'history', root, events: events.map(e => ({ event: e, label: this.label(e.run_id) })), truncated });
  }

  refreshDescendants() { for (const [root, ids] of this.roots) for (const d of this.model.descendants(root)) ids.add(d.id); }
  dispose() { clearTimeout(this.flush); this.client.off('event', this.listener); }
}

module.exports = { RunFeed, runMessage };
