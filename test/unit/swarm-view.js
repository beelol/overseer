// Swarm summaries share the Agents tree without eagerly loading a large job backlog.
const assert = require('assert');
const path = require('path');
const Module = require('module');

class EventEmitter {
  constructor() { this.listeners = []; this.event = fn => { this.listeners.push(fn); return { dispose() {} }; }; }
  fire() { for (const fn of this.listeners) fn(); }
}
class TreeItem { constructor(label, collapsibleState) { this.label = label; this.collapsibleState = collapsibleState; } }
class ThemeIcon { constructor(id) { this.id = id; } }
class ThemeColor { constructor(id) { this.id = id; } }
const vscode = { EventEmitter, TreeItem, ThemeIcon, ThemeColor,
  TreeItemCollapsibleState: { None: 0, Collapsed: 1, Expanded: 2 },
  Uri: { from: value => value, joinPath: (...parts) => ({ parts }) },
  MarkdownString: class { constructor(value) { this.value = value; } } };
const originalLoad = Module._load;
Module._load = function (name, ...args) { return name === 'vscode' ? vscode : originalLoad.call(this, name, ...args); };
const { Model, AgentsProvider } = require(path.resolve(__dirname, '../../extension/src/views.js'));
Module._load = originalLoad;

(async () => {
  const calls = [];
  const run = { id: 'sw-large', category: 'Backend audit', objective: 'Audit Atlas', status: 'running',
    created_ms: 1, updated_ms: 2, revision: 1, allowed_targets: ['profile-a'], policy: {},
    job_counts: { total: 100, by_status: { running: 32, ready: 56, submitted: 8, blocked: 4 } },
    active_worker_processes: 32, registered_attempts: 32, director: { owner_status: 'active' },
    availability: null, benefit: null, unconfirmed_exit_count: 0 };
  const jobs = Array.from({ length: 50 }, (_, n) => ({ id: `j${String(n).padStart(3, '0')}`,
    title: `Check route ${n}`, status: n < 32 ? 'running' : 'ready', attempt_count: n < 32 ? 1 : 0,
    plan_revision: 1, acceptance: 'Evidence', deps: [], resource_claims: [], run_id: run.id,
    deadline_at_ms: null, stop_reason: null }));
  let swarmRuns = [run];
  const client = { request: async (method, params) => {
    calls.push([method, params]);
    if (method === 'state') return { tasks: [], runs: [], workspaces: [], profiles: [], turns: {} };
    if (method === 'swarm.list') return { runs: swarmRuns, next_cursor: null };
    if (method === 'swarm.jobs' && params.status === 'blocked') return { jobs: Array.from({ length: 4 }, (_, n) =>
      ({ ...jobs[n], id: `blocked-${n}`, status: 'blocked' })), next_cursor: null };
    if (method === 'swarm.jobs') return params.cursor
      ? { jobs: jobs.map((job, n) => ({ ...job, id: `j${String(n + 50).padStart(3, '0')}` })), next_cursor: null }
      : { jobs, next_cursor: 'j049' };
    throw new Error(`unexpected ${method}`);
  } };
  const model = new Model(client);
  await model.refresh();
  const provider = new AgentsProvider(model, { get: () => [], update: () => {} });
  const section = provider.getChildren().find(node => node.section === 'swarms');
  assert(section, 'active Swarms section should appear in the existing Agents tree');
  assert.equal(section.item.label, 'Swarms');
  assert.equal(calls.filter(([method]) => method === 'swarm.jobs').length, 0,
    'opening the sidebar must not load any job rows');
  const [swarm] = provider.getChildren(section);
  assert.equal(swarm.item.label, 'Backend audit');
  assert.equal(swarm.item.collapsibleState, vscode.TreeItemCollapsibleState.Collapsed,
    'a large category should show its aggregate first');
  assert.match(swarm.item.description, /32 working/);
  assert.match(swarm.item.description, /56 ready/);
  assert.match(swarm.item.description, /4 blocked/);
  assert.match(swarm.item.tooltip, /4 blocked/,
    'narrow sidebars must still expose the full aggregate on hover');
  const firstPage = await provider.getChildren(swarm);
  assert.equal(firstPage.length, 52, 'director, 50 job rows and one next-page control');
  assert.equal(firstPage[0].item.label, 'Director');
  assert.equal(firstPage[1].item.iconPath.id, 'sync~spin');
  assert.equal(firstPage[33].item.iconPath.id, 'clock');
  assert.equal(calls.filter(([method]) => method === 'swarm.jobs').length, 1);
  assert.equal(calls.find(([method]) => method === 'swarm.jobs')[1].limit, 50);
  const secondPage = await provider.getChildren(firstPage[51]);
  assert.equal(secondPage.length, 50);
  assert.equal(secondPage[49].job.id, 'j099');
  assert.equal(calls.filter(([method]) => method === 'swarm.jobs').length, 2);
  provider.setSwarmStatusFilter('blocked');
  const blocked = await provider.getChildren(swarm);
  assert.equal(blocked.length, 5, 'director plus four blocked jobs');
  assert(blocked.slice(1).every(row => row.job.status === 'blocked'));
  assert.equal(calls.findLast(([method]) => method === 'swarm.jobs')[1].status, 'blocked');
  swarmRuns = [];
  await model.refresh(true);
  assert(!provider.getChildren().some(node => node.section === 'swarms'),
    'explicit refresh should remove a run that no longer appears in the daemon list');
  console.log('Swarm summaries stay compact; jobs load only when a run expands');
})().catch(error => { console.error(error); process.exit(1); });
