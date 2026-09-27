// The extension sends category controls through the daemon's versioned Swarm API.
const assert = require('assert');
const path = require('path');
const { SwarmControls } = require(path.resolve(__dirname, '../../extension/src/swarm-controls.js'));

(async () => {
  let run = { id: 'sw-1', category: 'Backend audit', status: 'running', generation: 3,
    revision: 2, active_worker_processes: 2, job_counts: { total: 4, by_status: { ready: 2, running: 2 } } };
  const calls = [];
  let refreshed = 0;
  let confirmed = false;
  let rejectPause = false;
  const client = { request: async (method, params) => {
    calls.push([method, params]);
    if (method === 'swarm.get') return { ...run };
    if (method === 'swarm.stop') {
      assert.deepEqual(params, { run_id: run.id }, 'Stop must remain safe from a stale view');
      run.status = 'stopped'; return { status: 'stopped' };
    }
    assert.equal(params.run_id, run.id);
    assert.equal(params.generation, run.generation);
    assert.equal(params.revision, run.revision);
    if (method === 'swarm.pause' && rejectPause) throw new Error('stale plan revision');
    if (method === 'swarm.pause') run.status = 'paused';
    else if (method === 'swarm.resume') run.status = 'running';
    else if (method === 'swarm.off') run.status = 'draining';
    else throw new Error(`unexpected ${method}`);
    return { status: run.status };
  } };
  const controls = new SwarmControls(client, () => { refreshed++; }, () => confirmed);

  await controls.pause(run.id);
  assert.equal(run.status, 'paused');
  run.revision = 3; // The director revised the plan after the sidebar snapshot.
  await controls.resume(run.id);
  assert.equal(run.status, 'running');
  assert.equal(calls.findLast(([method]) => method === 'swarm.resume')[1].revision, 3);
  await controls.off(run.id);
  assert.equal(run.status, 'draining');
  await controls.stop(run.id);
  assert.equal(run.status, 'draining', 'cancelled confirmation must not stop a category');
  confirmed = true;
  await controls.stop(run.id);
  assert.equal(run.status, 'stopped');
  assert.equal(refreshed, 4, 'successful controls refresh the category list');
  run.status = 'running';
  rejectPause = true;
  await assert.rejects(controls.pause(run.id), /stale plan revision/);
  assert.equal(run.status, 'running');
  assert.equal(refreshed, 5, 'a stale control reloads current state before the user retries');

  let extensionRefreshed = 0;
  const extensionCalls = [];
  const extensionClient = { request: async (method, params) => {
    extensionCalls.push([method, params]);
    if (method === 'swarm.get') return { id: 'sw-2', created_ms: 1000,
      policy: { effective: { deadline_ms: 3600000 } } };
    if (method === 'swarm.deadline.extend') return { deadline_at_ms: 1000 + 5400000 };
    throw new Error(`unexpected ${method}`);
  } };
  const extensionControls = new SwarmControls(extensionClient,
    () => { extensionRefreshed++; }, () => false);
  const extended = await extensionControls.extendDeadline('sw-2', 1800000);
  assert.equal(extended.deadline_at_ms, 5401000);
  const sent = extensionCalls.find(([method]) => method === 'swarm.deadline.extend')[1];
  assert.equal(sent.run_id, 'sw-2');
  assert.equal(sent.expected_deadline_at_ms, 3601000);
  assert.equal(sent.additional_ms, 1800000);
  assert.match(sent.request_id, /^[0-9a-f-]{36}$/);
  assert.equal(extensionRefreshed, 1);
  console.log('Swarm controls use fresh versions and confirm Stop once');
})().catch(error => { console.error(error); process.exit(1); });
