// AC-264, phase 1: two prototypes of the one Overseer layout, for the owner to choose by seeing.
// Both give: the agents list in the side bar, the agent's review wide in the middle, and one
// Overseer panel on the right (home, Voice Mode, or the picked agent's chat, with a way back).
// Neither writes a user setting.
//
//   A. The Overseer panel is a view in VS Code's secondary side bar (which has no editor tab row),
//      widened by VS Code's own resize command. The review stays in the editor area, with its one
//      tab row: without settings VS Code cannot hide it.
//   B. The window reopens on an Overseer-owned workspace file (in the extension's global storage,
//      never in a repository) holding the owner's folder, whose own settings hide the tab rows and
//      breadcrumbs, as Focus Mode's look does, for that window only. Running it again reopens the
//      owner's folder, where VS Code restores that window as it was.
//
// This is prototype code: phase 2 builds the chosen one properly and removes the other.
const vscode = require('vscode');
const path = require('path');
const crypto = require('crypto');
const { snapshotTabs, straysToClose } = require('./layout');

const LOOK = { 'workbench.editor.showTabs': 'none', 'breadcrumbs.enabled': false, 'workbench.editor.editorActionsLocation': 'hidden' };
const DEFAULT_TITLE = '${dirty}${activeEditorShort}${separator}${rootName}${separator}${profileName}${separator}${appName}';
const settle = ms => new Promise(r => setTimeout(r, ms));

/** A webview view dressed as the webview panel CommandCenter expects (A: the view is its home). */
function viewAsPanel(view) {
  const state = new vscode.EventEmitter();
  const panel = {
    sideView: true,
    get webview() { return view.webview; },
    get visible() { return view.visible; },
    get active() { return view.visible; },
    viewColumn: undefined,
    title: 'Overseer',
    reveal: (_column, preserveFocus) => view.show(!!preserveFocus),
    onDidDispose: view.onDidDispose,
    onDidChangeViewState: state.event,
    dispose: () => {},
  };
  view.onDidChangeVisibility(() => state.fire({ webviewPanel: panel }));
  return panel;
}

class OneLayout {
  constructor({ context, center, arrangement, model, log, focusedAgent, select, backToOverseer }) {
    Object.assign(this, { context, center, arrangement, model, log, focusedAgent, select, backToOverseer });
  }

  // ------------------------------------------------------------ B: the Overseer window

  /**
   * Where Overseer's workspace files live: its global storage, as a file: URI. (globalStorageUri is a
   * vscode-userdata: URI; a window opened on a workspace file there is a "virtual workspace" to VS
   * Code, which then disables every extension without virtual-workspace support, Overseer included.)
   */
  layoutsDir() { return vscode.Uri.file(path.join(this.context.globalStorageUri.fsPath, 'layouts')); }

  /** This window is open on one of Overseer's own workspace files. */
  isOverseerWindow() {
    const file = vscode.workspace.workspaceFile;
    return !!file && file.scheme === 'file' && file.fsPath.startsWith(this.layoutsDir().fsPath + path.sep);
  }

  async toggleB() { return this.isOverseerWindow() ? this.leaveB() : this.openB(); }

  /** Reopens this window on Overseer's workspace file for its folders (written first when new). */
  async openB() {
    const from = vscode.workspace.workspaceFile || (vscode.workspace.workspaceFolders || [])[0]?.uri;
    const folders = (vscode.workspace.workspaceFolders || []).map(f => f.uri);
    const name = folders.length ? (vscode.workspace.name || path.basename(folders[0].fsPath)).replace(/ \(Workspace\)$/, '') : 'Overseer';
    const key = crypto.createHash('sha1').update(from ? from.toString() : 'empty-window').digest('hex').slice(0, 12);
    const dir = vscode.Uri.joinPath(this.layoutsDir(), key);
    const file = vscode.Uri.joinPath(dir, `${name}.code-workspace`);
    // The window's title keeps the owner's own form, with the folder's name where VS Code would say "<name> (Workspace)".
    const title = (vscode.workspace.getConfiguration('window').inspect('title')?.globalValue || DEFAULT_TITLE).replace(/\$\{rootName(Short)?\}/g, name.replace(/\$/g, ''));
    const content = {
      folders: folders.map(f => ({ path: f.fsPath })),
      settings: { ...LOOK, 'window.title': title },
      // Where Overseer takes the window back to.
      overseer: { from: from ? from.toString() : null, kind: vscode.workspace.workspaceFile ? 'workspace' : from ? 'folder' : 'empty' },
    };
    await vscode.workspace.fs.createDirectory(dir);
    await vscode.workspace.fs.writeFile(file, Buffer.from(JSON.stringify(content, null, 2) + '\n'));
    const dirty = vscode.workspace.textDocuments.filter(d => d.isDirty).map(d => d.uri.toString());
    this.log(`one layout B: reopening on ${file.fsPath} (from ${content.overseer.from}; unsaved: ${dirty.length ? dirty.join(', ') : 'none'})`);
    await this.context.globalState.update('overseer.oneLayout.opened', { file: file.toString(), at: Date.now(), dirty });
    await vscode.commands.executeCommand('vscode.openFolder', file, { forceReuseWindow: true, noRecentEntry: true });
  }

  /** Back to the owner's own folder (or workspace), where VS Code restores that window. */
  async leaveB() {
    const file = vscode.workspace.workspaceFile;
    let from;
    try { from = JSON.parse(Buffer.from(await vscode.workspace.fs.readFile(file)).toString('utf8')).overseer?.from; } catch (error) { this.log('one layout B: ' + error.message); }
    this.log(`one layout B: back to ${from || 'an empty window'}`);
    if (from) await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.parse(from), { forceReuseWindow: true });
    else await vscode.commands.executeCommand('workbench.action.closeFolder');
  }

  /** On activation in the Overseer window: the layout, home on the right. */
  async startup() {
    await vscode.commands.executeCommand('setContext', 'overseer.overseerWindow', this.isOverseerWindow());
    if (!this.isOverseerWindow()) return;
    const t0 = Date.now();
    this.arrangement.takeover = true;
    for (const c of ['workbench.action.closePanel', 'workbench.action.closeAuxiliaryBar']) await vscode.commands.executeCommand(c).then(undefined, () => {});
    await vscode.commands.executeCommand('workbench.view.extension.overseer').then(undefined, () => {});
    await this.home();
    this.center.setDashboard?.(true);
    this.log(`one layout B: arranged in ${Date.now() - t0} ms`);
  }

  /** The focused agent's review in the middle and Overseer's conversation on the right. */
  async home() {
    const id = this.focusedAgent();
    if (id) { await this.select(id, { force: true }); await this.backToOverseer(); }
    else { await this.arrangement.chatOnly(); this.center.setMode('composer'); }
  }

  // ------------------------------------------------------------ A: the panel in the secondary side bar

  get inA() { return !!this.context.workspaceState.get('overseer.oneLayout.a'); }

  async toggleA() { return this.inA ? this.leaveA() : this.enterA(); }

  /** The view provider for A's panel: it becomes the Overseer view's host while A is on. */
  resolveWebviewView(view) {
    if (!this.inA) return;
    const panel = viewAsPanel(view);
    const old = this.center.panel;
    if (old && !old.sideView) { this.center.panel = undefined; old.dispose(); }
    this.center.attach(panel);
    view.onDidDispose(() => this.log('one layout A: the panel closed'));
  }

  async enterA() {
    let editors;
    try { editors = await vscode.commands.executeCommand('vscode.getEditorLayout'); } catch { editors = undefined; }
    const tabs = snapshotTabs(vscode.window.tabGroups.all, vscode);
    // The editor area's width while Overseer's view is alone in it: the panel's width is set from it.
    await this.center.open({ column: vscode.ViewColumn.One, preserveFocus: true });
    const close = new Set(straysToClose(tabs).map(t => `${t.viewColumn}:${t.label}`));
    const closing = vscode.window.tabGroups.all.flatMap(g => g.tabs.filter(t => close.has(`${g.viewColumn}:${t.label}`)));
    if (closing.length) await vscode.window.tabGroups.close(closing, true).then(undefined, () => {});
    await vscode.commands.executeCommand('vscode.setEditorLayout', { orientation: 0, groups: [{}] }).then(undefined, () => {});
    await vscode.commands.executeCommand('workbench.action.closePanel').then(undefined, () => {});
    await settle(300);
    const editorWidth = (await this.center.measure())?.w || 0;
    await this.context.workspaceState.update('overseer.oneLayout.a', { editors, tabs, at: Date.now() });
    await vscode.commands.executeCommand('setContext', 'overseer.panelLayout', true);
    this.arrangement.takeover = true; this.arrangement.sideView = true;
    // The editor-area Overseer view gives way to the side view.
    const old = this.center.panel;
    if (old && !old.sideView) { this.center.panel = undefined; old.dispose(); await settle(200); }
    await vscode.commands.executeCommand('workbench.view.extension.overseerPanel').then(undefined, error => this.log('one layout A: ' + error.message));
    for (let i = 0; i < 60 && !this.center.panel?.sideView; i++) await settle(100);
    await this.widen(editorWidth);
    await vscode.commands.executeCommand('workbench.view.extension.overseer').then(undefined, () => {});
    await this.home();
    this.center.setDashboard?.(true);
  }

  /**
   * The secondary side bar to about 40% of the editor area, 60 px at a time, with VS Code's own
   * resize commands (extensions cannot set a part's size). Narrowing the editor area gives the
   * width to its neighbours; growing the focused part needs the focus in the side bar itself.
   */
  async widen(editorWidth) {
    const target = Math.max(420, Math.round(editorWidth * 0.42));
    const width = async () => (await this.center.measure())?.w || 0;
    // With the side bar closed, the only neighbour the editor area gives width to is the secondary side bar.
    await vscode.commands.executeCommand('workbench.action.closeSidebar').then(undefined, () => {});
    await settle(200);
    const tries = [
      ['editor narrower', async () => vscode.commands.executeCommand('workbench.action.decreaseViewWidth')],
      ['side bar larger', async () => { await vscode.commands.executeCommand('workbench.action.focusAuxiliaryBar'); await vscode.commands.executeCommand('workbench.action.increaseViewSize'); }],
    ];
    for (const [how, step] of tries) {
      let w = await width();
      const start = w;
      for (let i = 0; i < 30 && w < target; i++) {
        await step().then(undefined, error => this.log(`one layout A: ${how}: ${error.message}`));
        await settle(120);
        const next = await width();
        if (next <= w && i >= 2) break;
        w = next;
      }
      this.log(`one layout A: ${how}: panel ${start} → ${w} px (target ${target} of ${editorWidth})`);
      if (w >= target) break;
    }
    // The agents list comes back (it takes its width from the editor area).
    await vscode.commands.executeCommand('workbench.view.extension.overseer').then(undefined, () => {});
    await settle(300);
    const w = await width();
    this.log(`one layout A: with the side bar back, panel ${w} px`);
    return w;
  }

  async leaveA() {
    const saved = this.context.workspaceState.get('overseer.oneLayout.a');
    await this.context.workspaceState.update('overseer.oneLayout.a', undefined);
    this.arrangement.sideView = false; this.arrangement.takeover = false;
    await this.arrangement.closeReviews();
    await vscode.commands.executeCommand('workbench.action.closeAuxiliaryBar').then(undefined, () => {});
    await vscode.commands.executeCommand('setContext', 'overseer.panelLayout', false);
    if (this.center.panel?.sideView) this.center.panel = undefined;
    this.center.setDashboard?.(false);
    if (saved?.editors) await vscode.commands.executeCommand('vscode.setEditorLayout', saved.editors).then(undefined, () => {});
    for (const group of saved?.tabs || []) for (const t of group.tabs) {
      if (t.dirty || t.kind !== 'text') continue;
      await vscode.window.showTextDocument(vscode.Uri.parse(t.uri), { viewColumn: group.viewColumn, preserveFocus: true, preview: t.preview }).then(undefined, () => {});
    }
  }
}

module.exports = { OneLayout, LOOK };
