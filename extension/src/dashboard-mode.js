// Dashboard mode (AC-57): one command turns this window into the Overseer dashboard (the dashboard
// fills the editor area; side bar, panel and secondary side bar are hidden for this window only)
// and Exit Dashboard puts the previous layout back: the editor-group layout and each part that was
// open. Extensions cannot read part visibility, so the dashboard measures itself: a part was open
// if closing it made the dashboard's webview larger. Reopening uses the focus commands, which open
// a part without toggling it. Nothing is written to user or workspace settings. The dashboard can
// also open in its own window with no folder, and optionally when VS Code starts.
const vscode = require('vscode');

const PARTS = [
  { key: 'sideBar', close: 'workbench.action.closeSidebar', open: 'workbench.action.focusSideBar' },
  { key: 'panel', close: 'workbench.action.closePanel', open: 'workbench.action.focusPanel' },
  { key: 'auxiliaryBar', close: 'workbench.action.closeAuxiliaryBar', open: 'workbench.action.focusAuxiliaryBar' },
];
const settle = ms => new Promise(r => setTimeout(r, ms));

class Dashboard {
  constructor(context, center, log) {
    this.context = context; this.center = center; this.log = log;
  }

  get saved() { return this.context.workspaceState.get('overseer.dashboard.saved'); }
  get inDashboard() { return !!this.saved; }

  async enter() {
    if (!this.inDashboard) {
      let editors;
      try { editors = await vscode.commands.executeCommand('vscode.getEditorLayout'); } catch { editors = undefined; }
      await this.center.open({ layout: true });
      await settle(400);
      // Close each part and see whether the dashboard grew: then that part was open.
      const parts = {};
      let size = await this.center.measure();
      for (const p of PARTS) {
        await vscode.commands.executeCommand(p.close).then(undefined, () => {});
        await settle(250);
        const next = await this.center.measure();
        parts[p.key] = !!(size && next && (next.w > size.w + 8 || next.h > size.h + 8));
        size = next || size;
      }
      const saved = { editors, parts, at: Date.now() };
      await this.context.workspaceState.update('overseer.dashboard.saved', saved);
      this.log(`dashboard: entered; saved layout ${JSON.stringify(saved)}`);
    } else {
      await this.center.open({ layout: false });
    }
    await vscode.commands.executeCommand('setContext', 'overseer.inDashboard', true);
    this.center.setDashboard?.(true);
  }

  async exit() {
    const saved = this.saved;
    await this.context.workspaceState.update('overseer.dashboard.saved', undefined);
    await vscode.commands.executeCommand('setContext', 'overseer.inDashboard', false);
    this.center.setDashboard?.(false);
    // Close what the dashboard opened: the dashboard itself, reviews and agent tabs.
    const ours = [];
    for (const g of vscode.window.tabGroups.all) for (const t of g.tabs) {
      const vt = t.input && t.input.viewType;
      if (typeof vt === 'string' && /overseer\.(center|output|review|newTask)$/.test(vt)) ours.push(t);
    }
    if (ours.length) await vscode.window.tabGroups.close(ours).then(undefined, () => {});
    if (!saved) return;
    if (saved.editors) await vscode.commands.executeCommand('vscode.setEditorLayout', saved.editors).then(undefined, () => {});
    for (const p of PARTS) if (saved.parts?.[p.key]) await vscode.commands.executeCommand(p.open).then(undefined, () => {});
    await vscode.commands.executeCommand('workbench.action.focusActiveEditorGroup').then(undefined, () => {});
    this.log(`dashboard: exited; restored ${JSON.stringify(saved)}`);
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
    if (this.inDashboard) { await vscode.commands.executeCommand('setContext', 'overseer.inDashboard', true); this.center.setDashboard?.(true); return; }
    if (vscode.workspace.getConfiguration('overseer').get('dashboard.openOnStartup', false)) await this.enter();
  }
}

module.exports = { Dashboard };
