// Overseer view (command center, AC-48): a full-page layout that does not depend on the native
// sidebar or on the folder open in this window. Column 1: the agents column (every repository
// the daemon knows). Column 2: the selected run's live editable review. Column 3: that run's
// conversation with its event log. Restored after reloads by a webview serializer.
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
<script nonce="${nonce}" src="${asset('center.js')}"></script></body></html>`;
    panel.onDidDispose(() => { if (this.panel === panel) this.panel = undefined; });
    panel.webview.onDidReceiveMessage(message => this.receive(message).catch(error => vscode.window.showErrorMessage(`Overseer: ${error.message}`)));
  }

  async receive(message) {
    if (message?.type === 'ready') await this.push();
    else if (message?.type === 'select' && typeof message.runId === 'string') await this.handlers.select(message.runId);
    else if (message?.type === 'newTask') await vscode.commands.executeCommand('overseer.newTask');
    else if (message?.type === 'refresh') await this.model.refresh();
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
