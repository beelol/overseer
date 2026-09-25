// One local worker, one computation at a time. Its watchdog also covers startup
// and algorithm code that does not yield to Monaco's internal budget checks.
export class StatisticsWorker {
  constructor(source) { this.url = URL.createObjectURL(new Blob([source], { type: 'text/javascript' })); this.next = 0; }
  compute(original, modified) {
    if (this.pending) throw new Error('A statistics computation is already running.');
    if (this.disposed) return Promise.resolve({ reason: 'Diff classification was closed.' });
    return new Promise(resolve => {
      const id = ++this.next;
      const finish = (result, reset = false) => {
        clearTimeout(this.timer); this.pending = undefined;
        if (reset) { this.worker?.terminate(); this.worker = undefined; }
        resolve(result);
      };
      this.pending = finish;
      this.timer = setTimeout(() => finish({ reason: 'Diff classification timed out after 1 second. Counts are unavailable.' }, true), 1000);
      try {
        if (!this.worker) this.worker = new Worker(this.url, { name: 'Branch Diff statistics' });
        this.worker.onmessage = event => { if (event.data?.id === id) finish(event.data); };
        this.worker.onerror = event => { event.preventDefault(); finish({ reason: 'Diff classification worker failed. Counts are unavailable.' }, true); };
        this.worker.postMessage({ id, original, modified });
      } catch { finish({ reason: 'Diff classification worker could not start. Counts are unavailable.' }, true); }
    });
  }
  dispose() {
    this.disposed = true; this.pending?.({ reason: 'Diff classification was closed.' }, true);
    this.worker?.terminate(); this.worker = undefined; URL.revokeObjectURL(this.url);
  }
}
