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
    availability: { state: 'blocked', reason: 'no_allowed_target',
      observed_ms: Date.now() - 1000, expires_ms: Date.now() + 60000,
      allowance_windows: [{ pool_id: 'fixture-pool', window_id: 'week', unit: 'points',
        remaining_milli: 500, previous_remaining_milli: 100000, change_milli: -99500,
        observed_usable_milli: 500, confidence: 'exact' }],
      allowance_window_count: 1, allowance_windows_truncated: false },
    benefit: { decision: 'serial', reason: 'finishing_unaffordable', max_parallel_workers: 1 },
    capacity: { provider_usage_state: 'unknown', source: 'fixture_admission',
      last_admission: { job_id: 'j032', target_id: 'fixture-a', status: 'blocked',
        reason: 'growth_wave_full', observed_ms: 123 },
      selected_targets: [{ id: 'fixture-a', harness: 'generic', profile_id: 'profile-a', attempts: 1 }],
      windows: [{ pool_id: 'fixture-pool', window_id: 'week', unit: 'points',
        allocation_milli: 6000, finishing_reserve_milli: 1200,
        outstanding_estimate_milli: 1000 }] }, unconfirmed_exit_count: 0 };
  const jobs = Array.from({ length: 50 }, (_, n) => ({ id: `j${String(n).padStart(3, '0')}`,
    title: `Check route ${n}`, status: n < 32 ? 'reserved' : 'ready', attempt_count: n < 32 ? 1 : 0,
    plan_revision: 1, acceptance: 'Evidence', deps: [], resource_claims: [], run_id: run.id,
    deadline_at_ms: null, stop_reason: null,
    worker_runs: n === 0 ? [{ attempt_id: 'a0', overseer_run_id: 'worker-0',
      status: 'running', harness: 'generic', profile_id: null, model: null,
      workspace_id: 'ws-0', ended_ms: null }] : [] }));
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
  assert.match(swarm.item.tooltip, /usage unknown/i);
  const firstPage = await provider.getChildren(swarm);
  assert.equal(firstPage.length, 53, 'director, capacity, 50 job rows and one next-page control');
  assert.equal(firstPage[0].item.label, 'Director');
  assert.equal(firstPage[1].item.label, 'Capacity');
  assert.match(firstPage[1].item.description, /usage unknown/i);
  const capacity = provider.getChildren(firstPage[1]);
  assert(capacity.some(row => /fixture-a/.test(row.item.label)), 'show selected target');
  assert(capacity.some(row => /week/.test(row.item.label) && /finishing reserve/i.test(row.item.description)),
    'show frozen allocation and finishing reserve');
  assert(capacity.some(row => /serial/i.test(row.item.label) && /finishing unaffordable/i.test(row.item.description)),
    'show the recorded planning decision and reason');
  assert(capacity.some(row => /eligibility.*blocked/i.test(row.item.label) &&
    /no allowed target/i.test(row.item.description)),
    'show the recorded blocked eligibility reason');
  assert(capacity.some(row => /fixture-pool.*week/i.test(row.item.label) &&
    /0\.5 points.*down 99\.5 points/i.test(row.item.description) &&
    /exact/.test(row.item.tooltip) && /Last observed allowance/.test(row.item.accessibilityInformation.label)),
    'show the observed native-unit allowance drop without implying live usage');
  assert(capacity.some(row => /usage unknown/i.test(row.item.label)),
    'fixture estimates must not be presented as measured provider usage');
  assert(capacity.some(row => /last admission held/i.test(row.item.label) &&
    /growth wave full/i.test(row.item.description) && /j032/.test(row.item.tooltip)),
    'show the durable held admission as a past observation with its job and reason');
  assert(!capacity.some(row => /current limit/i.test(row.item.label)),
    'a past admission result cannot be mislabeled as the current constraint');
  run.availability.expires_ms = Date.now() - 1;
  const staleCapacity = provider.getChildren(firstPage[1]);
  assert(staleCapacity.some(row => /eligibility.*expired/i.test(row.item.label) &&
    /no allowed target/i.test(row.item.description)),
    'an expired eligibility observation must not be presented as current');
  assert(staleCapacity.some(row => /week.*expired/i.test(row.item.label)),
    'an expired allowance observation must be marked expired');
  run.availability.allowance_windows[0].remaining_milli = null;
  run.availability.allowance_windows[0].change_milli = null;
  run.availability.allowance_windows[0].confidence = 'unknown';
  const unknownCapacity = provider.getChildren(firstPage[1]);
  assert(unknownCapacity.some(row => /fixture-pool.*week/i.test(row.item.label) &&
    /unknown/.test(row.item.description) && !/0 points/.test(row.item.description)),
    'unknown allowance must not be displayed as zero');
  const beforeObservation = provider.signature();
  run.availability.observed_ms += 1000;
  assert.notEqual(provider.signature(), beforeObservation,
    'a newer availability observation must redraw the Agents tree even without a job change');
  run.benefit = { decision: 'parallel', reason: 'beneficial', max_parallel_workers: 3 };
  run.policy = { effective: { max_workers: 8 } };
  const scaledCapacity = provider.getChildren(firstPage[1]);
  assert(scaledCapacity.some(row => /planning: parallel/i.test(row.item.label) &&
    /3 of 8/.test(row.item.description) && /beneficial/.test(row.item.description)),
    'show why a beneficial batch is scaled below the worker ceiling');
  assert.equal(firstPage[2].item.iconPath.id, 'sync~spin');
  assert.equal(firstPage[2].item.description, 'working', 'a live worker must not be shown as merely reserved');
  assert.equal(firstPage[2].item.collapsibleState, vscode.TreeItemCollapsibleState.Collapsed);
  const workerRows = provider.getChildren(firstPage[2]);
  assert.equal(workerRows.length, 1);
  assert.equal(workerRows[0].item.command.command, 'overseer.selectRun');
  assert.deepEqual(workerRows[0].item.command.arguments, ['worker-0']);
  assert.equal(firstPage[34].item.iconPath.id, 'clock');
  assert.equal(calls.filter(([method]) => method === 'swarm.jobs').length, 1);
  assert.equal(calls.find(([method]) => method === 'swarm.jobs')[1].limit, 50);
  const secondPage = await provider.getChildren(firstPage[52]);
  assert.equal(secondPage.length, 50);
  assert.equal(secondPage[49].job.id, 'j099');
  assert.equal(calls.filter(([method]) => method === 'swarm.jobs').length, 2);
  provider.setSwarmStatusFilter('blocked');
  const blocked = await provider.getChildren(swarm);
  assert.equal(blocked.length, 6, 'director, capacity and four blocked jobs');
  assert(blocked.slice(2).every(row => row.job.status === 'blocked'));
  assert.equal(calls.findLast(([method]) => method === 'swarm.jobs')[1].status, 'blocked');
  run.status = 'stopping';
  run.unconfirmed_exit_count = 3;
  await model.refresh(true);
  const stoppingSection = provider.getChildren().find(node => node.section === 'swarms');
  const [stoppingSwarm] = provider.getChildren(stoppingSection);
  assert.match(stoppingSwarm.item.description, /3 exits unconfirmed/,
    'unconfirmed exits must not be conflated with active worker processes');
  const stoppingRows = await provider.getChildren(stoppingSwarm);
  assert(stoppingRows.some(row => row.item.label === '3 exits unconfirmed'),
    'the run details should show exit uncertainty separately');
  swarmRuns = [];
  await model.refresh(true);
  assert(!provider.getChildren().some(node => node.section === 'swarms'),
    'explicit refresh should remove a run that no longer appears in the daemon list');

  const stateWithWorker = { tasks: [
    { id: 'swarm-task', title: 'Fixture worker', repo_root: '/repo', created_ms: 1 },
    { id: 'ordinary-task', title: 'Ordinary agent', repo_root: '/repo', created_ms: 2 },
  ], runs: [
    { id: 'worker-0', task_id: 'swarm-task', status: 'running', created_ms: 1,
      workspace_id: 'worker-ws', swarm_membership: { role: 'worker', run_id: 'sw-large', job_id: 'j000' } },
    { id: 'ordinary-0', task_id: 'ordinary-task', status: 'running', created_ms: 2,
      workspace_id: 'ordinary-ws' },
  ], workspaces: [
    { id: 'worker-ws', repo_root: '/repo', path: '/repo/workers/0' },
    { id: 'ordinary-ws', repo_root: '/repo', path: '/repo/ordinary' },
  ], profiles: [], turns: {} };
  const isolated = new Model({ request: async method => method === 'state' ? stateWithWorker : { runs: [] } });
  await isolated.refresh(true);
  const ordinary = new AgentsProvider(isolated, { get: () => [], update: () => {} });
  assert.deepEqual(ordinary.visibleTasks().map(task => task.id), ['ordinary-task'],
    'Swarm workers should appear under their category, not as duplicate ordinary agents');
  assert.equal(isolated.run('worker-0').id, 'worker-0', 'drilldown still needs the worker run');
  assert.equal(ordinary.nodeFor('worker-0'), undefined,
    'selecting a nested Swarm worker must not try to reveal a hidden ordinary-agent row');
  assert.equal(isolated.workspace('worker-ws').path, '/repo/workers/0',
    'drilldown still needs the worker workspace');
  console.log('Swarm summaries stay compact; jobs load only when a run expands');
})().catch(error => { console.error(error); process.exit(1); });
