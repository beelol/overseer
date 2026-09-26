// Overseer editor view (AC-55, AC-58, AC-59; Gate K): one webview with the selected agent's chat,
// the new-agent composer or the agent grid, plus the files panel. The agents list is the side bar;
// arrangement.js places this view alone or beside the review. Not tied to the folder open in the
// window. Restored after reloads by a webview serializer.
const vscode = require('vscode');
const { RunFeed, runMessage } = require('./run-feed');
const { handleRunMessage, changesFetcher } = require('./run-actions');
const { page, localRoots } = require('./webview-html');
const { ACTIVE } = require('./views');


class CommandCenter {
  constructor(context, model, handlers) {
    this.context = context; this.model = model; this.handlers = handlers; this.client = handlers.client;
    this.changes = changesFetcher(this.client);
    this.mode = 'chat';
    model.onDidChange(() => this.push());
    this.client.on('event', event => {
      if (!this.panel) return;
      if (['file_activity', 'turn_done', 'status'].includes(event.kind) && this.chatRun && this.chatFeed?.roots.get(this.chatRun)?.has(event.run_id)) this.pushChanges();
    });
  }

  get active() { return !!this.panel; }

  /** Opens (or moves) the view into an editor column; the arrangement decides which (Gate K). */
  async open({ column = vscode.ViewColumn.One, preserveFocus = false, reveal = true } = {}) {
    if (this.panel) {
      if (reveal && (this.panel.viewColumn !== column || !this.panel.visible)) this.panel.reveal(column, preserveFocus);
      return this.panel;
    }
    const panel = vscode.window.createWebviewPanel('overseer.center', 'Overseer', { viewColumn: column, preserveFocus }, { enableScripts: true, retainContextWhenHidden: true, localResourceRoots: localRoots(this.context.extensionUri) });
    this.attach(panel);
    return panel;
  }

  attach(panel) {
    this.panel = panel;
    panel.webview.options = { enableScripts: true, localResourceRoots: localRoots(this.context.extensionUri) };
    panel.iconPath = vscode.Uri.joinPath(this.context.extensionUri, 'media', 'overseer.svg');
    panel.webview.html = page(panel.webview, this.context.extensionUri, { title: 'Overseer', chat: true, css: ['dashboard.css'], js: ['composer.js', 'grid.js', 'dashboard.js', 'files.js'] });
    const post = m => panel.webview.postMessage(m);
    this.chatFeed = new RunFeed(this.client, this.model, m => post({ ...m, channel: 'chat' }));
    this.gridFeed = new RunFeed(this.client, this.model, m => post({ ...m, channel: 'grid' }));
    panel.onDidDispose(() => {
      this.chatFeed.dispose(); this.gridFeed.dispose();
      if (this.panel === panel) { this.panel = undefined; this.chatRun = undefined; }
      vscode.commands.executeCommand('setContext', 'overseer.dashboardOpen', false);
    });
    panel.onDidChangeViewState(e => vscode.commands.executeCommand('setContext', 'overseer.dashboardFocus', e.webviewPanel.active));
    vscode.commands.executeCommand('setContext', 'overseer.dashboardOpen', true);
    panel.webview.onDidReceiveMessage(message => this.receive(message).catch(error => {
      if (message?.type === 'start') post({ type: 'notice', scope: 'composer', message: error.message });
      else post({ type: 'notice', message: error.message });
    }));
  }

  async receive(m) {
    if (!m || typeof m !== 'object') return;
    const post = x => this.panel?.webview.postMessage(x);
    switch (m.type) {
      case 'ready': await this.push(); if (this.inDashboard) post({ type: 'dashboard', on: true }); return;
      case 'measured': { const done = this.measuring?.get(m.id); if (done) { this.measuring.delete(m.id); done({ w: Number(m.w), h: Number(m.h) }); } return; }
      case 'select': if (typeof m.runId === 'string') { if (m.restore) await this.showChat(m.runId); else await this.handlers.select(m.runId); } return;
      case 'mode': { const was = this.mode; this.mode = m.mode; if (was !== m.mode) await this.handlers.onMode?.(m.mode, was); return; }
      case 'focusComposer': post({ type: 'mode', mode: 'composer' }); return;
      case 'gridSubscribe': {
        const ids = (m.runIds || []).filter(id => this.model.run(id));
        // Metadata first: the tile needs its root run id before history arrives.
        for (const id of ids) { const msg = runMessage(this.model, id, this.handlers.steering); if (msg) post({ type: 'run', channel: 'grid', ...msg }); }
        await this.gridFeed.set(ids, { limit: 400 });
        return;
      }
      case 'tree': return this.tree(String(m.runId || ''), String(m.dir || ''));
      case 'openFile': return this.openFile(String(m.runId || ''), String(m.path || ''));
      case 'composerData': post({ type: 'composerData', data: await this.handlers.launcher.data() }); return;
      case 'composerDefaults': await this.handlers.launcher.saveDefaults(m.defaults || {}); return;
      case 'composerBrowse': { const repo = await this.handlers.launcher.browse(); if (repo) post({ type: 'notice', scope: 'composer', kind: 'repo', repo }); return; }
      case 'composerBranches': post({ type: 'notice', scope: 'composer', kind: 'branches', branches: await this.handlers.launcher.branches(m.repo) }); return;
      case 'composerModel': {
        const model = await vscode.window.showInputBox({ title: 'Model', prompt: 'Any model name this harness accepts. Leave empty for the default.', value: m.current || '' });
        if (model !== undefined) post({ type: 'notice', scope: 'composer', kind: 'model', model: model.trim() });
        return;
      }
      case 'start': {
        const runId = await this.handlers.launcher.start(m.form || {});
        if (!runId) { post({ type: 'notice', scope: 'composer', message: 'Not started.' }); return; }
        post({ type: 'notice', scope: 'composer', kind: 'started', runId });
        await this.handlers.select(runId, { follow: vscode.workspace.getConfiguration('overseer').get('followNewRuns', true) });
        return;
      }
      case 'mentionFiles': await handleRunMessage(this.handlers, this.chatRun, { ...m, scope: m.scope || 'composer' }, x => post(x)); return;
      case 'pin': await this.handlers.setPinned(m.runId, !!m.on); await this.push(); return;
      case 'archive': await this.client.request('task.archive', { task_id: String(m.taskId), archived: m.archived !== false }); await this.model.refresh(); return;
      case 'search': post({ type: 'searchHits', q: m.q, taskIds: await this.handlers.search(String(m.q || '')) }); return;
      case 'command': await vscode.commands.executeCommand(String(m.command), ...(m.args !== undefined ? [m.args] : [])); return;
      case 'openExternal': { const url = String(m.url || ''); if (/^https?:\/\//i.test(url)) await vscode.env.openExternal(vscode.Uri.parse(url)); return; }
      case 'exitDashboard': await vscode.commands.executeCommand('overseer.exitDashboard'); return;
      case 'dashboardWindow': await vscode.commands.executeCommand('overseer.openDashboardWindow'); return;
      case 'cleanupArchived': await vscode.commands.executeCommand('overseer.cleanupArchived'); return;
      default: {
        const runId = typeof m.runId === 'string' ? m.runId : this.chatRun;
        if (!runId) return;
        await handleRunMessage(this.handlers, runId, m, x => post(x));
      }
    }
  }

  /** Shows a run's chat in the dashboard: its metadata, history and live events. */
  async showChat(runId) {
    if (!this.panel || !this.model.run(runId)) return;
    this.chatRun = runId;
    const msg = runMessage(this.model, runId, this.handlers.steering);
    this.panel.webview.postMessage({ type: 'run', channel: 'chat', ...msg });
    await this.chatFeed.set([runId]);
    this.pushChanges(true);
  }

  async pushChanges(force) {
    const run = this.chatRun && this.model.run(this.chatRun);
    if (!run || run.parent_run_id) return;
    const changes = await this.changes(run.workspace_id, { force });
    if (changes) this.panel?.webview.postMessage({ type: 'changes', runId: run.id, changes });
  }

  /** One directory of the selected run's worktree (AC-51). */
  async tree(runId, dir) {
    const run = this.model.run(runId);
    try {
      if (!run) throw new Error('Unknown run.');
      const data = await this.client.request('workspace.tree', { workspace_id: run.workspace_id, dir });
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
    await vscode.commands.executeCommand('vscode.open', uri, { viewColumn: vscode.ViewColumn.Beside, preview: false });
  }

  async push() {
    if (!this.panel) return;
    const { tasks, runs, workspaces, profiles } = this.model.state;
    const state = { tasks, runs, workspaces, profiles, accounts: this.handlers.launcher.accounts(), attention: this.handlers.attention(), pinned: this.handlers.pinned(),
      gridMax: Math.max(1, Math.min(9, vscode.workspace.getConfiguration('overseer').get('grid.maxTiles', 6))), archived: this.handlers.archived() };
    await this.panel.webview.postMessage({ type: 'state', state, selected: this.handlers.selected() });
    if (this.chatRun) { const msg = runMessage(this.model, this.chatRun, this.handlers.steering); if (msg) { this.chatFeed.refreshDescendants(); this.panel.webview.postMessage({ type: 'run', channel: 'chat', ...msg }); } }
    for (const id of this.gridFeed?.roots.keys() || []) { const msg = runMessage(this.model, id, this.handlers.steering); if (msg) this.panel.webview.postMessage({ type: 'run', channel: 'grid', ...msg }); }
  }

  /** Shows an agent's chat (host-driven selection from the side bar, commands or keys). */
  async select(runId) {
    if (!this.panel) return;
    this.panel.webview.postMessage({ type: 'selected', runId });
    await this.showChat(runId);
  }
  selected(runId) { this.panel?.webview.postMessage({ type: 'selected', runId }); }
  setDashboard(on) { this.inDashboard = on; this.panel?.webview.postMessage({ type: 'dashboard', on }); }

  /** The dashboard webview's size (dashboard mode uses it to see which parts were open). */
  measure() {
    if (!this.panel) return Promise.resolve(undefined);
    const id = Math.random().toString(36).slice(2);
    return new Promise(resolve => {
      const timer = setTimeout(() => { this.measuring?.delete(id); resolve(undefined); }, 1500);
      (this.measuring ||= new Map()).set(id, size => { clearTimeout(timer); resolve(size); });
      this.panel.webview.postMessage({ type: 'measure', id });
    });
  }
  setMode(mode) { this.panel?.webview.postMessage({ type: 'mode', mode }); }
  focus(target) { this.panel?.webview.postMessage({ type: 'focus', target }); }

  async deserializeWebviewPanel(panel) {
    this.attach(panel);
  }
}

module.exports = { CommandCenter, ACTIVE };
