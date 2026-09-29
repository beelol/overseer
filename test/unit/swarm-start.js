// The normal Swarm start (S0): one read-back, one yes, a one-time account choice only when needed.
const assert = require('assert');
const path = require('path');
const { SwarmControls, describeReadback } = require(path.resolve(__dirname, '../../extension/src/swarm-controls.js'));

const readback = (overrides = {}) => ({
  summary: 'Auto · up to 4 workers · 60 min',
  account_pool: { targets: ['fixture-local', 'system-claude'], needs_account_selection: false,
    accounts: [{ target: 'fixture-local', quota: 'fixture' }, { target: 'system-claude', quota: 'unknown' }] },
  allocation: { run_allocation_percent: 10, finishing_reserve_percent: 20 },
  ceiling: { workers: 4, agents_max_active: 5 }, deadline_ms: 3600000,
  director: { state: 'qualified' }, ...overrides });

(async () => {
  assert.equal(describeReadback(readback()), [
    'Accounts: fixture-local (fixture allowance), system-claude (usage unknown)',
    "Allocation: up to 10% of each account's remaining allowance per window, 20% of that kept for finishing",
    'Ceiling: up to 4 workers (of 5 agents)',
    'Deadline: 60 min',
    'Director: chosen automatically'].join('\n'));

  // Approved accounts: no account question, one confirmation, the yes carries the digest.
  let calls = [];
  let refreshed = 0;
  const client = { request: async (method, params) => {
    calls.push([method, params]);
    assert.equal(method, 'swarm.start');
    if (!params.confirm_readback_sha256) return { status: 'readback', readback: readback(), readback_sha256: 'a'.repeat(64) };
    return { status: 'started', run: { id: 'sw-1', start: { summary: 'Auto · up to 4 workers · 60 min' } } };
  } };
  let asked = 0;
  let confirmed = [];
  const ui = { pickAccounts: async () => { asked++; return ['system-claude']; },
    confirm: async (back, text) => { confirmed.push(text); return true; } };
  const controls = new SwarmControls(client, () => { refreshed++; }, () => false);
  const input = { category: 'Backend security', objective: 'Audit tenant isolation', repositories: ['/repo'] };
  const started = await controls.start(input, ui);
  assert.equal(started.status, 'started');
  assert.equal(asked, 0, 'inherited accounts are not asked for');
  assert.equal(confirmed.length, 1, 'one confirmation, no per-worker question');
  assert.equal(calls.length, 2);
  assert.equal(calls[1][1].confirm_readback_sha256, 'a'.repeat(64));
  assert.match(calls[1][1].request_id, /^[0-9a-f-]{36}$/);
  assert.equal(refreshed, 1);

  // No approved accounts: the one-time selection, then the read-back of that selection.
  calls = []; confirmed = [];
  const empty = readback({ account_pool: { targets: [], accounts: [], needs_account_selection: true } });
  const selecting = { request: async (method, params) => {
    calls.push([method, params]);
    if (!params.allowed_targets) return { status: 'readback', readback: empty, readback_sha256: 'b'.repeat(64) };
    if (!params.confirm_readback_sha256) return { status: 'readback', readback: readback(), readback_sha256: 'c'.repeat(64) };
    return { status: 'started', run: { id: 'sw-2' } };
  } };
  assert.equal((await new SwarmControls(selecting, () => {}, () => false).start(input, ui)).status, 'started');
  assert.equal(asked, 1);
  assert.deepEqual(calls[2][1].allowed_targets, ['system-claude']);
  assert.equal(calls[2][1].confirm_readback_sha256, 'c'.repeat(64));

  // Declining commits nothing; a blocked director is reported without asking for a yes.
  calls = [];
  const declined = await controls.start(input, { ...ui, confirm: async () => false });
  assert.equal(declined.status, 'cancelled');
  assert.ok(calls.every(([, params]) => !params.confirm_readback_sha256));
  const blockedClient = { request: async () => ({ status: 'readback', readback_sha256: 'd'.repeat(64),
    readback: readback({ director: { state: 'blocked', reason: 'no_qualified_director' } }) }) };
  let prompted = false;
  const blocked = await new SwarmControls(blockedClient, () => {}, () => false)
    .start(input, { ...ui, confirm: async () => { prompted = true; return true; } });
  assert.deepEqual([blocked.status, blocked.reason, prompted], ['blocked', 'no_qualified_director', false]);

  // A read-back that changed before the yes is shown again with the same request id.
  calls = []; confirmed = [];
  let digest = 'e';
  const changing = { request: async (method, params) => {
    calls.push([method, params]);
    if (!params.confirm_readback_sha256) return { status: 'readback', readback: readback(), readback_sha256: digest.repeat(64) };
    if (params.confirm_readback_sha256 === 'e'.repeat(64)) { digest = 'f';
      return { status: 'readback_changed', readback: readback({ summary: 'changed' }), readback_sha256: 'f'.repeat(64) }; }
    return { status: 'started', run: { id: 'sw-3' } };
  } };
  assert.equal((await new SwarmControls(changing, () => {}, () => false).start(input, ui)).status, 'started');
  assert.equal(confirmed.length, 2, 'the changed read-back is confirmed again');
  assert.equal(calls[1][1].request_id, calls[2][1].request_id);
  console.log('Swarm start: one read-back, one yes, accounts asked once');
})().catch(error => { console.error(error); process.exit(1); });
