// Overseer view (command center, AC-48): a full-page layout that does not depend on the native
// sidebar or on the folder open in this window. Column 1: the agents column (every repository
// the daemon knows). Column 2: the selected run's live editable review. Column 3: that run's
// conversation with its event log. Below the agents: the selected run's worktree files (AC-51).
// Restored after reloads by a webview serializer.
const vscode = require('vscode');
const { randomBytes } = require('crypto');

const COLUMNS = { agents: vscode.ViewColumn.One, review: vscode.ViewColumn.Two, conversation: vscode.ViewColumn.Three };

class CommandCenter {
  constructor(context, model, handlers) {
    this.context = context; this.model = model; this.handlers = handlers;
    model.onDidChange(() => this.push());
  }

  get active() { return !!this.panel; }

  async open({ layout = true } = {}) {
    if (this.panel) { this.panel.reveal(COLUMNS.agents); return this.panel; }
    if (layout) await this.layout();
    const panel = vscode.window.createWebviewPanel('overseer.center', 'Overseer', { viewColumn: COLUMNS.agents, preserveFocus: false }, { enableScripts: true, retainContextWhenHidden: true });
    this.attach(panel);
    return panel;
  }

  /** Three columns: agents (narrow), review (wide), conversation. */
  async layout() {
    await vscode.commands.executeCommand('vscode.setEditorLayout', { orientation: 0, groups: [{ size: 0.22 }, { size: 0.48 }, { size: 0.30 }] });
  }

  attach(panel) {
    this.panel = panel;
    const media = vscode.Uri.joinPath(this.context.extensionUri, 'media');
    panel.webview.options = { enableScripts: true, localResourceRoots: [media] };
    panel.iconPath = vscode.Uri.joinPath(this.context.extensionUri, 'media', 'overseer.svg');
    const nonce = randomBytes(18).toString('base64');
    const asset = name => panel.webview.asWebviewUri(vscode.Uri.joinPath(media, name));
    panel.webview.html = `<!doctype html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${panel.webview.cspSource}; script-src 'nonce-${nonce}';">
<link rel="stylesheet" href="${asset('center.css')}"><title>Overseer</title></head>
<body><header class="bar"><h1>Agents</h1><span class="spacer"></span>
<button id="new-task" class="primary" title="Start a new agent task in any repository">New Task</button>
<button id="collapse" class="icon" title="Collapse all tasks" aria-label="Collapse all tasks">⊟</button>
<button id="refresh" class="icon" title="Refresh" aria-label="Refresh">↻</button></header>
<div id="tree" role="tree" aria-label="Agents in all repositories"></div>
<p id="empty" class="empty" hidden>No agent tasks yet. Start one with New Task; it can target any repository, not only the folder open in this window.</p>
<section id="files-section" aria-labelledby="files-h"><header class="bar sub"><h1 id="files-h">Files</h1><span id="files-for" class="desc"></span><span class="spacer"></span>
<button id="files-refresh" class="icon" title="Refresh files" aria-label="Refresh files">↻</button></header>
<p id="files-note" class="note" role="status">Select a run to browse its worktree.</p><div id="files" role="tree" aria-label="Worktree files"></div></section>
<script nonce="${nonce}" src="${asset('center.js')}"></script><script nonce="${nonce}" src="${asset('files.js')}"></script></body></html>`;
    panel.onDidDispose(() => { if (this.panel === panel) this.panel = undefined; });
    panel.webview.onDidReceiveMessage(message => this.receive(message).catch(error => vscode.window.showErrorMessage(`Overseer: ${error.message}`)));
  }

  async receive(message) {
    if (message?.type === 'ready') await this.push();
    else if (message?.type === 'select' && typeof message.runId === 'string') await this.handlers.select(message.runId);
    else if (message?.type === 'newTask') await vscode.commands.executeCommand('overseer.newTask');
    else if (message?.type === 'refresh') await this.model.refresh();
    else if (message?.type === 'tree') await this.tree(String(message.runId || ''), String(message.dir || ''));
    else if (message?.type === 'openFile') await this.openFile(String(message.runId || ''), String(message.path || ''));
  }

  /** One directory of the selected run's worktree (AC-51). */
  async tree(runId, dir) {
    const run = this.model.run(runId);
    try {
      if (!run) throw new Error('Unknown run.');
      const data = await this.handlers.client.request('workspace.tree', { workspace_id: run.workspace_id, dir });
      this.panel?.webview.postMessage({ type: 'treeData', runId, dir, data });
    } catch (error) {
      this.panel?.webview.postMessage({ type: 'treeError', runId, dir, message: `Files unavailable: ${error.message}` });
    }
  }

  /** Opens a worktree file in the editor (review column), independent of the window's folder. */
  async openFile(runId, rel) {
    const run = this.model.run(runId);
    const ws = run && this.model.workspace(run.workspace_id);
    if (!ws || !rel || rel.startsWith('/') || rel.split('/').includes('..')) return;
    const uri = vscode.Uri.joinPath(vscode.Uri.file(ws.path), ...rel.split('/'));
    try { await vscode.workspace.fs.stat(uri); }
    catch { vscode.window.showInformationMessage(`${rel} was deleted in this worktree; open the review to see its change.`); return; }
    await vscode.commands.executeCommand('vscode.open', uri, { viewColumn: COLUMNS.review, preview: true });
  }

  async push() {
    if (!this.panel) return;
    const { tasks, runs, workspaces, profiles } = this.model.state;
    await this.panel.webview.postMessage({ type: 'state', state: { tasks, runs, workspaces, profiles }, selected: this.handlers.selected() });
  }

  selected(runId) { this.panel?.webview.postMessage({ type: 'selected', runId }); }

  async deserializeWebviewPanel(panel) {
    this.attach(panel);
  }
}

module.exports = { CommandCenter, COLUMNS };
