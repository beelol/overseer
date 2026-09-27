// User controls for one category. Fetch the current generation/revision before every
// versioned transition; Stop is intentionally ID-only so a stale view cannot block it.
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
  async stop(runId) {
    const run = await this.client.request('swarm.get', { id: runId });
    if (!await this.confirmStop(run)) return undefined;
    const result = await this.client.request('swarm.stop', { run_id: runId });
    await this.refresh();
    return result;
  }
}

module.exports = { SwarmControls };
