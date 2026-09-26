// Dashboard mode (AC-57): one command turns this window into the Overseer dashboard (the dashboard
// fills the editor area; side bar, panel and secondary side bar are hidden for this window only)
// and Exit Dashboard puts the previous layout back: the editor-group layout and the visibility of
// each part. Nothing is written to user or workspace settings. The dashboard can also open in its
// own window with no folder, and optionally when VS Code starts.
const vscode = require('vscode');

const PARTS = [
  { key: 'sideBarVisible', close: 'workbench.action.closeSidebar', open: 'workbench.action.toggleSidebarVisibility' },
  { key: 'panelVisible', close: 'workbench.action.closePanel', open: 'workbench.action.togglePanel' },
  { key: 'auxiliaryBarVisible', close: 'workbench.action.closeAuxiliaryBar', open: 'workbench.action.toggleAuxiliaryBar' },
];

class Dashboard {
  constructor(context, center, log) {
    this.context = context; this.center = center; this.log = log;
  }

  get saved() { return this.context.workspaceState.get('overseer.dashboard.saved'); }
  get inDashboard() { return !!this.saved; }

  /** Reads a workbench context key (side bar, panel visibility); undefined when unavailable. */
  async contextKey(key) {
    try { return await vscode.commands.executeCommand('getContextKeyValue', key); } catch { return undefined; }
  }

  async layoutState() {
    const parts = {};
    for (const p of PARTS) parts[p.key] = await this.contextKey(p.key);
    let editors;
    try { editors = await vscode.commands.executeCommand('vscode.getEditorLayout'); } catch { editors = undefined; }
    return { parts, editors };
  }

  async enter() {
    if (!this.inDashboard) {
      const before = await this.layoutState();
      await this.context.workspaceState.update('overseer.dashboard.saved', { ...before, at: Date.now() });
      this.log(`dashboard: entering; saved layout ${JSON.stringify(before)}`);
      for (const p of PARTS) if (before.parts[p.key] !== false) await vscode.commands.executeCommand(p.close).then(undefined, () => {});
    }
    await vscode.commands.executeCommand('setContext', 'overseer.inDashboard', true);
    await this.center.open({ layout: true });
  }

  async exit() {
    const saved = this.saved;
    await this.context.workspaceState.update('overseer.dashboard.saved', undefined);
    await vscode.commands.executeCommand('setContext', 'overseer.inDashboard', false);
    // Close what the dashboard opened: the dashboard itself and review/agent tabs.
    const ours = [];
    for (const g of vscode.window.tabGroups.all) for (const t of g.tabs) {
      const vt = t.input && t.input.viewType;
      if (typeof vt === 'string' && /overseer\.(center|output)$|overseer\.review|branchDiff/i.test(vt)) ours.push(t);
    }
    if (ours.length) await vscode.window.tabGroups.close(ours).then(undefined, () => {});
    if (!saved) return;
    if (saved.editors) await vscode.commands.executeCommand('vscode.setEditorLayout', saved.editors).then(undefined, () => {});
    for (const p of PARTS) {
      if (saved.parts[p.key] !== true) continue;
      const now = await this.contextKey(p.key);
      if (now === false || now === undefined) await vscode.commands.executeCommand(p.open).then(undefined, () => {});
    }
    this.log(`dashboard: exited; restored ${JSON.stringify(saved)}; now ${JSON.stringify(await this.layoutState())}`);
  }

  /** Opens a new window without a folder that starts in the dashboard. */
  async openWindow() {
    await this.context.globalState.update('overseer.dashboard.nextWindow', Date.now());
    await vscode.commands.executeCommand('workbench.action.newWindow');
  }

  /** On activation: open the dashboard for a window asked for by openWindow(), or at startup when set. */
  async startup() {
    const asked = this.context.globalState.get('overseer.dashboard.nextWindow', 0);
    const fresh = asked && Date.now() - asked < 60000 && !(vscode.workspace.workspaceFolders || []).length;
    if (fresh) { await this.context.globalState.update('overseer.dashboard.nextWindow', 0); await this.enter(); return; }
    if (this.inDashboard) { await vscode.commands.executeCommand('setContext', 'overseer.inDashboard', true); return; }
    if (vscode.workspace.getConfiguration('overseer').get('dashboard.openOnStartup', false)) await this.enter();
  }
}

module.exports = { Dashboard };
