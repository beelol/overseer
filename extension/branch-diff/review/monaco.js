import * as monaco from 'monaco-editor/editor/editor.api.js';
import 'monaco-editor/editor/browser/widget/diffEditor/registrations.contribution.js';
import 'monaco-editor/editor/contrib/find/browser/findController.js';
import 'monaco-editor/editor/contrib/folding/browser/folding.js';
import 'monaco-editor/editor/contrib/readOnlyMessage/browser/contribution.js';
import 'monaco-editor/basic-languages/monaco.contribution.js';

import './monaco.css';

export { monaco };
export function initialize(report) {
  const workers = new Set();
  // Worker source is bundled at build time. No fetch, CDN or runtime connection.
  const workerUrl = URL.createObjectURL(new Blob([__BRANCH_DIFF_WORKER_SOURCE__], { type: 'text/javascript' }));
  self.MonacoEnvironment = { getWorker() {
    return new Promise((resolve, reject) => {
      const worker = new Worker(workerUrl, { name: 'Branch Diff editor' }); workers.add(worker);
      const terminate = worker.terminate.bind(worker);
      worker.terminate = () => { workers.delete(worker); terminate(); };
      const fail = error => { clearTimeout(timer); worker.terminate(); report('Diff worker unavailable: ' + error.message); reject(error); };
      const timer = setTimeout(() => fail(new Error('local worker startup timed out')), 10000);
      worker.addEventListener('error', event => fail(new Error(event.message || 'worker initialization failed')), { once: true });
      worker.onmessage = event => {
        if (event.data?.type !== 'branch-diff-worker-ready') return;
        clearTimeout(timer); worker.onmessage = null; document.body.dataset.workerStatus = 'ready'; resolve(worker);
      };
    });
  } };
  return () => {
    for (const worker of workers) worker.terminate();
    URL.revokeObjectURL(workerUrl);
    delete self.MonacoEnvironment;
  };
}
