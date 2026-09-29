// User controls for one category. Fetch the current generation/revision before every
// versioned transition; Stop is intentionally ID-only so a stale view cannot block it.
const { randomUUID } = require('crypto');

const QUOTA = { measured: 'measured', stale: 'reading stale', unknown: 'usage unknown',
  allowance_unknown: 'allowance unknown', exhausted: 'exhausted', fixture: 'fixture allowance' };

/** The daemon's S0 read-back as the few lines the owner confirms. */
function describeReadback(readback) {
  const pool = readback.account_pool || {};
  const accounts = (pool.accounts || []).map(a => `${a.target} (${QUOTA[a.quota] || a.quota})`).join(', ');
  const allocation = readback.allocation || {};
  const director = readback.director || {};
  return [
    `Accounts: ${accounts || 'none approved'}`,
    `Allocation: up to ${allocation.run_allocation_percent}% of each account's remaining allowance per window, ${allocation.finishing_reserve_percent}% of that kept for finishing`,
    `Ceiling: up to ${readback.ceiling?.workers} workers (of ${readback.ceiling?.agents_max_active} agents)`,
    `Deadline: ${Math.round((readback.deadline_ms || 0) / 60000)} min`,
    `Director: ${director.state === 'qualified' ? 'chosen automatically' : `blocked (${director.reason})`}`,
  ].join('\n');
}

class SwarmControls {
  constructor(client, refresh, confirmStop) {
    this.client = client;
    this.refresh = refresh;
    this.confirmStop = confirmStop;
  }
  async change(method, runId) {
    try {
      const run = await this.client.request('swarm.get', { id: runId });
      return await this.client.request(method, { run_id: runId,
        generation: run.generation, revision: run.revision });
    } finally {
      await this.refresh();
    }
  }
  pause(runId) { return this.change('swarm.pause', runId); }
  resume(runId) { return this.change('swarm.resume', runId); }
  off(runId) { return this.change('swarm.off', runId); }
  async extendDeadline(runId, additionalMs) {
    if (!Number.isSafeInteger(additionalMs) || additionalMs <= 0) throw new Error('Invalid extension duration.');
    try {
      const run = await this.client.request('swarm.get', { id: runId });
      const expected = run.created_ms + run.policy.effective.deadline_ms;
      if (!Number.isSafeInteger(expected)) throw new Error('This Swarm has no valid deadline.');
      return await this.client.request('swarm.deadline.extend', { run_id: runId,
        request_id: randomUUID(), expected_deadline_at_ms: expected, additional_ms: additionalMs });
    } finally {
      await this.refresh();
    }
  }
  /**
   * The normal start (S0): category, objective and repositories, one read-back,
   * one yes. `ui.pickAccounts(readback)` is asked only when no account is
   * approved yet; `ui.confirm(readback, text)` is the owner's single yes.
   * Returns the started run, or `{ status }` when nothing was committed.
   */
  async start({ category, objective, repositories }, ui) {
    const base = { category, objective, repositories };
    let params = base;
    let back = await this.client.request('swarm.start', params);
    if (back.readback.account_pool.needs_account_selection) {
      const targets = await ui.pickAccounts(back.readback);
      if (!targets || !targets.length) return { status: 'cancelled' };
      params = { ...base, allowed_targets: targets };
      back = await this.client.request('swarm.start', params);
    }
    if (back.readback.director.state !== 'qualified') return { status: 'blocked', reason: back.readback.director.reason, readback: back.readback };
    const requestId = randomUUID();
    for (let shown = 0; shown < 2; shown++) {
      if (!await ui.confirm(back.readback, describeReadback(back.readback))) return { status: 'cancelled' };
      const result = await this.client.request('swarm.start', { ...params, request_id: requestId,
        confirm_readback_sha256: back.readback_sha256 });
      if (result.status !== 'readback_changed') {
        await this.refresh();
        return result;
      }
      back = result; // What changed is shown again before anything starts.
    }
    return { status: 'readback_changed' };
  }
  async stop(runId) {
    const run = await this.client.request('swarm.get', { id: runId });
    if (!await this.confirmStop(run)) return undefined;
    const result = await this.client.request('swarm.stop', { run_id: runId });
    await this.refresh();
    return result;
  }
}

module.exports = { SwarmControls, describeReadback };
