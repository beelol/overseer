// Dashboard mode (AC-57, AC-79): one command turns this window into the Overseer dashboard: the
// panel and secondary side bar are hidden for this window only, the side bar shows the Overseer
// agents list, and the editor area holds the agent (chat, or review and chat). Exit Dashboard puts
// the previous layout back: the editor-group layout and each part that was open. Extensions cannot read part visibility, so the dashboard measures itself: a part was open
// if closing it made the dashboard's webview larger. Reopening uses the focus commands, which open
// a part without toggling it. The only settings written are the immersive ones (AC-102,
// immersive.js), put back exactly on exit. The dashboard can also open in its own window with no
// folder, and optionally when VS Code starts.
//
// The Overseer workspace (AC-250) is the same mode with a fuller layout: Overseer's conversation,
// the focused agent's review (Follow) and its chat in three columns sized for the screen. Entering
// it also closes the owner's tabs (they are listed first; unsaved ones stay open) and the terminal
// panel; closing it puts back the editor groups, their tabs, the active tab of each and the parts
// that were open. VS Code's own chat view is closed before the parts are noted (AC-258), so leaving
// never reopens it.
const vscode = require('vscode');
const { snapshotTabs, straysToClose, workspaceColumns } = require('./layout');

const PARTS = [
  { key: 'sideBar', close: 'workbench.action.closeSidebar', open: 'workbench.action.focusSideBar' },
  { key: 'panel', close: 'workbench.action.closePanel', open: 'workbench.action.focusPanel' },
  { key: 'auxiliaryBar', close: 'workbench.action.closeAuxiliaryBar', open: 'workbench.action.focusAuxiliaryBar' },
];
const settle = ms => new Promise(r => setTimeout(r, ms));

class Dashboard {
  constructor(context, center, log, { arrange, arrangeWorkspace, leaveWorkspace, agentsVisible, immersive, vsChat } = {}) {
    this.context = context; this.center = center; this.log = log; this.immersive = immersive; this.vsChat = vsChat;
    this.arrange = arrange || (() => this.center.open());
    this.arrangeWorkspace = arrangeWorkspace || this.arrange;
    this.leaveWorkspace = leaveWorkspace || (async () => {});
    this.agentsVisible = agentsVisible || (() => false);
  }

  get saved() { return this.context.workspaceState.get('overseer.dashboard.saved'); }
  get inDashboard() { return !!this.saved; }
  /** In the Overseer workspace (AC-250), the three-column form of the dashboard. */
  get inWorkspace() { return !!this.saved?.workspace; }

  /** AC-250: Overseer: Open Workspace. Running it again (or Close Workspace) closes it. */
  async openWorkspace() {
    if (this.inWorkspace) return this.exit();
    if (this.inDashboard) await this.exit();
    return this.enter({ workspace: true });
  }

  async enter({ workspace = false } = {}) {
    if (!this.inDashboard) {
      let editors;
      try { editors = await vscode.commands.executeCommand('vscode.getEditorLayout'); } catch { editors = undefined; }
      const overseerShown = this.agentsVisible();
      // Was Overseer already in the editor area? Then Exit keeps it as it was (AC-79).
      const overseerOpen = !!this.center.active;
      const centerColumn = this.center.panel?.viewColumn;
      let tabs;
      if (workspace) {
        // The owner's tabs, group by group, then the ones that do not belong are closed (AC-250).
        tabs = snapshotTabs(vscode.window.tabGroups.all, vscode);
        const close = new Set(straysToClose(tabs).map(t => `${t.viewColumn}:${t.label}`));
        const closing = vscode.window.tabGroups.all.flatMap(g => g.tabs.filter(t => close.has(`${g.viewColumn}:${t.label}`)));
        if (closing.length) await vscode.window.tabGroups.close(closing, true).then(undefined, error => this.log('workspace: closing tabs: ' + error.message));
        await vscode.commands.executeCommand('vscode.setEditorLayout', { orientation: 0, groups: [{}] }).then(undefined, () => {});
        await this.center.open({ column: vscode.ViewColumn.One });
      } else await this.arrange();
      await settle(400);
      // VS Code's own chat view is closed first (AC-258): it is then not a part to reopen on exit.
      await this.vsChat?.check().catch(() => {});
      // Close each part and see whether the dashboard grew: then that part was open.
      const parts = {};
      let size = await this.center.measure();
      let sideBarWidth = 0;
      for (const p of PARTS) {
        await vscode.commands.executeCommand(p.close).then(undefined, () => {});
        await settle(250);
        const next = await this.center.measure();
        parts[p.key] = !!(size && next && (next.w > size.w + 8 || next.h > size.h + 8));
        if (p.key === 'sideBar' && parts.sideBar) sideBarWidth = next.w - size.w;
        size = next || size;
      }
      // The side bar stays, showing the Overseer agents list (Gate K).
      await vscode.commands.executeCommand('workbench.view.extension.overseer').then(undefined, () => {});
      if (workspace) {
        // Sized for the screen: three columns, the side bar hidden when it would squeeze them.
        await settle(250);
        const shown = await this.center.measure();
        const columns = workspaceColumns(shown?.w, sideBarWidth || (size && shown ? Math.max(0, size.w - shown.w) : 0));
        if (columns.hideSideBar) await vscode.commands.executeCommand('workbench.action.closeSidebar').then(undefined, () => {});
        this.log(`workspace: editor ${shown?.w}px, side bar ${sideBarWidth}px → columns ${columns.sizes.map(x => x.toFixed(2)).join('/')}${columns.hideSideBar ? ', side bar hidden' : ''}`);
        // Whatever happens placing the views, the owner's layout is saved so Close Workspace puts it back.
        try { await this.arrangeWorkspace(columns.sizes); } catch (error) { this.log('workspace: ' + (error.stack || error.message)); }
      } else await this.arrange();
      const saved = { editors, parts, overseerShown, overseerOpen, centerColumn, workspace, tabs, at: Date.now() };
      await this.context.workspaceState.update('overseer.dashboard.saved', saved);
      this.log(`dashboard: entered; saved layout ${JSON.stringify(saved)}`);
    } else {
      await this.arrange();
    }
    // The workspace keeps VS Code's tab strips (each column's tab names it) and writes no setting.
    if (!this.saved?.workspace) await this.immersive?.apply();
    await vscode.commands.executeCommand('setContext', 'overseer.inDashboard', true);
    await vscode.commands.executeCommand('setContext', 'overseer.inWorkspace', !!this.saved?.workspace);
    this.center.setDashboard?.(true);
    this.onChange?.();
  }

  async exit() {
    const saved = this.saved;
    await this.context.workspaceState.update('overseer.dashboard.saved', undefined);
    await vscode.commands.executeCommand('setContext', 'overseer.inDashboard', false);
    await vscode.commands.executeCommand('setContext', 'overseer.inWorkspace', false);
    this.center.setDashboard?.(false);
    await this.immersive?.restore();
    if (saved?.workspace) {
      await this.leaveWorkspace();
      await this.restoreWorkspace(saved);
    } else if (saved && !saved.overseerOpen) {
      // The dashboard opened Overseer: close what it opened and put the editor layout back.
      const ours = [];
      for (const g of vscode.window.tabGroups.all) for (const t of g.tabs) {
        const vt = t.input && t.input.viewType;
        if (typeof vt === 'string' && /overseer\.(center|output|review|newTask)$/.test(vt)) ours.push(t);
      }
      if (ours.length) await vscode.window.tabGroups.close(ours).then(undefined, () => {});
      if (saved.editors) await vscode.commands.executeCommand('vscode.setEditorLayout', saved.editors).then(undefined, () => {});
    }
    if (!saved) return;
    for (const p of PARTS) {
      if (p.key === 'sideBar') {
        // Dashboard mode opened the side bar on Overseer; put back what was there as far as VS Code allows.
        if (!saved.parts?.sideBar) await vscode.commands.executeCommand(p.close).then(undefined, () => {});
        else if (!saved.overseerShown) await vscode.commands.executeCommand('workbench.view.explorer').then(undefined, () => {});
        else if (saved.workspace) await vscode.commands.executeCommand('workbench.view.extension.overseer').then(undefined, () => {}); // the workspace may have hidden it
        continue;
      }
      if (saved.parts?.[p.key]) await vscode.commands.executeCommand(p.open).then(undefined, () => {});
    }
    if (saved.workspace) await this.focusRestoredGroup(saved);
    else await vscode.commands.executeCommand('workbench.action.focusActiveEditorGroup').then(undefined, () => {});
    this.log(`dashboard: exited; restored ${JSON.stringify(saved)}`);
    this.onChange?.();
  }

  /** AC-250: the editor groups as they were, each with its tabs in order and its active tab. */
  async restoreWorkspace(saved) {
    // Overseer's own views close unless Overseer was open before; then it goes back to its column.
    const ours = [];
    for (const g of vscode.window.tabGroups.all) for (const t of g.tabs) {
      const vt = t.input && t.input.viewType;
      if (typeof vt === 'string' && /overseer\.(center|output|review|newTask)$/.test(vt) && !(saved.overseerOpen && /overseer\.center$/.test(vt))) ours.push(t);
    }
    if (ours.length) await vscode.window.tabGroups.close(ours).then(undefined, () => {});
    if (saved.editors) await vscode.commands.executeCommand('vscode.setEditorLayout', saved.editors).then(undefined, () => {});
    if (saved.overseerOpen && this.center.panel && saved.centerColumn) this.center.panel.reveal(saved.centerColumn, true);
    const parse = u => vscode.Uri.parse(u);
    for (const group of saved.tabs || []) {
      for (const t of group.tabs) {
        if (t.dirty) continue; // never closed
        // A pinned tab is opened focused so that Pin Editor applies to it.
        const options = { viewColumn: group.viewColumn, preserveFocus: !t.pinned, preview: t.preview };
        try {
          if (t.kind === 'text') await vscode.window.showTextDocument(parse(t.uri), options);
          else if (t.kind === 'diff') await vscode.commands.executeCommand('vscode.diff', parse(t.original), parse(t.modified), t.label, options);
          else if (t.kind === 'custom' || t.kind === 'notebook') await vscode.commands.executeCommand('vscode.openWith', parse(t.uri), t.viewType, options);
          else continue;
          if (t.pinned) await vscode.commands.executeCommand('workbench.action.pinEditor').then(undefined, () => {});
        } catch (error) { this.log(`workspace: could not reopen ${t.label}: ${error.message}`); }
      }
      // The tab that was active in this group comes forward again.
      const active = group.tabs.find(t => t.active && t.kind !== 'other');
      if (active) {
        const live = vscode.window.tabGroups.all.find(g => g.viewColumn === group.viewColumn)?.tabs.findIndex(t => t.label === active.label);
        if (live >= 0) await this.openAt(group.viewColumn, live);
      }
    }
  }

  async openAt(viewColumn, index) {
    const focus = ['workbench.action.focusFirstEditorGroup', 'workbench.action.focusSecondEditorGroup', 'workbench.action.focusThirdEditorGroup', 'workbench.action.focusFourthEditorGroup',
      'workbench.action.focusFifthEditorGroup', 'workbench.action.focusSixthEditorGroup', 'workbench.action.focusSeventhEditorGroup', 'workbench.action.focusEighthEditorGroup'][viewColumn - 1];
    if (!focus) return;
    await vscode.commands.executeCommand(focus).then(undefined, () => {});
    await vscode.commands.executeCommand('workbench.action.openEditorAtIndex', index).then(undefined, () => {});
  }

  /** The group the owner was in gets the keyboard back. */
  async focusRestoredGroup(saved) {
    const group = (saved.tabs || []).find(g => g.active);
    if (!group) { await vscode.commands.executeCommand('workbench.action.focusActiveEditorGroup').then(undefined, () => {}); return; }
    const active = group.tabs.find(t => t.active && t.kind !== 'other');
    const live = vscode.window.tabGroups.all.find(g => g.viewColumn === group.viewColumn)?.tabs.findIndex(t => active && t.label === active.label);
    await this.openAt(group.viewColumn, live >= 0 ? live : 0);
  }

  /**
   * Opens a new window without a folder that starts in the dashboard. It is opened on Overseer's own
   * workspace file (no folders), so its immersive settings stay in that window (AC-244).
   */
  async openWindow() {
    await this.context.globalState.update('overseer.dashboard.nextWindow', Date.now());
    const dir = this.context.globalStorageUri;
    const file = vscode.Uri.joinPath(dir, 'Overseer.code-workspace');
    try {
      await vscode.workspace.fs.createDirectory(dir);
      try { await vscode.workspace.fs.stat(file); } catch { await vscode.workspace.fs.writeFile(file, Buffer.from(JSON.stringify({ folders: [], settings: {} }, null, 2) + '\n')); }
      await vscode.commands.executeCommand('vscode.openFolder', file, { forceNewWindow: true });
    } catch (error) {
      this.log('dashboard window: ' + error.message);
      await vscode.commands.executeCommand('workbench.action.newWindow');
    }
  }

  /** On activation: open the dashboard for a window asked for by openWindow(), or at startup when set. */
  async startup() {
    const asked = this.context.globalState.get('overseer.dashboard.nextWindow', 0);
    const fresh = asked && Date.now() - asked < 60000 && !(vscode.workspace.workspaceFolders || []).length;
    if (fresh) { await this.context.globalState.update('overseer.dashboard.nextWindow', 0); await this.enter(); return; }
    if (this.inDashboard) {
      await vscode.commands.executeCommand('setContext', 'overseer.inDashboard', true);
      await vscode.commands.executeCommand('setContext', 'overseer.inWorkspace', !!this.saved.workspace);
      this.center.setDashboard?.(true);
      if (!this.saved.workspace) await this.immersive?.apply();
      this.onChange?.();
      return;
    }
    await this.immersive?.recover(false);
    if (vscode.workspace.getConfiguration('overseer').get('dashboard.openOnStartup', false)) await this.enter();
  }
}

module.exports = { Dashboard };
