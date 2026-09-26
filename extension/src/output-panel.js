// Run panel: one agent's chat in its own editor tab (the dashboard shows the same chat inline).
// Streams the run's history and live events, sends follow-ups, stops it and answers permission
// requests. Messages only ever target this panel's run id.
const vscode = require('vscode');
const { RunFeed, runMessage } = require('./run-feed');
const { handleRunMessage, changesFetcher } = require('./run-actions');
const { page, localRoots } = require('./webview-html');

class OutputPanels {
  constructor(context, client, model) {
    this.context = context; this.client = client; this.model = model;
    this.panels = new Map();
    this.changes = changesFetcher(client);
    model.onDidChange(() => { for (const [runId, entry] of this.panels) this.pushRun(runId, entry); });
    client.on('event', event => {
      if (!['file_activity', 'turn_done', 'status'].includes(event.kind)) return;
      for (const [runId, entry] of this.panels) if (entry.feed.roots.get(runId)?.has(event.run_id)) this.pushChanges(runId, entry);
    });
  }

  pushRun(runId, entry) {
    const msg = runMessage(this.model, runId, this.steering);
    if (!msg) return;
    entry.feed.refreshDescendants();
    entry.panel.title = msg.run.title.slice(0, 40) || 'Agent';
    entry.panel.webview.postMessage({ type: 'run', ...msg });
  }

  async pushChanges(runId, entry, force) {
    const run = this.model.run(runId);
    if (!run || run.parent_run_id) return;
    const changes = await this.changes(run.workspace_id, { force });
    if (changes) entry.panel.webview.postMessage({ type: 'changes', runId, changes });
  }

  async show(runId, { preserveFocus = true, viewColumn } = {}) {
    const column = viewColumn || this.column?.();
    const entry = this.panels.get(runId);
    if (entry) { entry.panel.reveal(column, preserveFocus); return; }
    const panel = vscode.window.createWebviewPanel('overseer.output', 'Agent', { viewColumn: column || vscode.ViewColumn.Beside, preserveFocus }, { enableScripts: true, retainContextWhenHidden: true, localResourceRoots: localRoots(this.context.extensionUri) });
    await this.attach(runId, panel);
  }

  async attach(runId, panel) {
    const feed = new RunFeed(this.client, this.model, m => panel.webview.postMessage(m));
    const entry = { panel, feed };
    this.panels.set(runId, entry);
    panel.iconPath = vscode.Uri.joinPath(this.context.extensionUri, 'media', 'overseer.svg');
    panel.onDidDispose(() => { feed.dispose(); if (this.panels.get(runId) === entry) this.panels.delete(runId); });
    panel.webview.onDidReceiveMessage(message => this.receive(runId, message).catch(error => panel.webview.postMessage({ type: 'notice', message: error.message })));
    panel.webview.options = { enableScripts: true, localResourceRoots: localRoots(this.context.extensionUri) };
    panel.webview.html = page(panel.webview, this.context.extensionUri, { title: 'Agent', chat: true, js: ['run-panel.js'], css: ['run-panel.css'],
      body: '<main id="app"></main>', bodyAttrs: `data-run-id="${runId.replace(/[^A-Za-z0-9_-]/g, '')}"` });
  }

  async receive(runId, message) {
    if (message?.type === 'ready') {
      const entry = this.panels.get(runId); if (!entry) return;
      this.pushRun(runId, entry);
      await entry.feed.set([runId]);
      this.pushChanges(runId, entry, true);
      return;
    }
    await handleRunMessage(this, runId, message, m => this.panels.get(runId)?.panel.webview.postMessage(m));
  }

  /** Restores run panels after a window reload or VS Code restart (webview state holds the run id). */
  async deserializeWebviewPanel(panel, state) {
    const runId = state && typeof state.runId === 'string' ? state.runId : undefined;
    try {
      await this.client.waitConnected(20000);
      await this.model.refresh();
    } catch { /* explained below */ }
    const run = runId && this.model.run(runId);
    if (!run || this.panels.has(runId)) {
      if (run) { panel.dispose(); return; }
      panel.webview.options = { enableScripts: false, localResourceRoots: localRoots(this.context.extensionUri) };
      panel.title = 'Agent unavailable';
      panel.webview.html = page(panel.webview, this.context.extensionUri, { title: 'Agent unavailable',
        body: `<div class="empty-state"><span class="codicon codicon-debug-disconnect"></span><div>${this.client.connected ? 'This agent is no longer in Overseer.' : 'Overseer is reconnecting. Reopen the agent from the dashboard.'}</div></div>` });
      return;
    }
    await this.attach(runId, panel);
  }
}

module.exports = { OutputPanels };
