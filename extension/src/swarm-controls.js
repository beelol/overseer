// User controls for one category. Fetch the current generation/revision before every
// versioned transition; Stop is intentionally ID-only so a stale view cannot block it.
const { randomUUID } = require('crypto');
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
  async stop(runId) {
    const run = await this.client.request('swarm.get', { id: runId });
    if (!await this.confirmStop(run)) return undefined;
    const result = await this.client.request('swarm.stop', { run_id: runId });
    await this.refresh();
    return result;
  }
}

module.exports = { SwarmControls };
