// Immersive editor area (AC-102). While the window is in Overseer's mode (the dashboard), VS Code's
// chrome above Overseer's views recedes: no tab strip, no breadcrumbs and no editor actions row;
// each Overseer view names what it shows in its own header. These are the only settings Overseer
// changes for this, listed in SETTINGS below and in the overseer.dashboard.immersive description.
//
// VS Code has no per-window settings. A window opened on a workspace file (.code-workspace) has
// workspace settings kept in that file, which only that window reads: there the values are written
// (AC-244), and every other VS Code window keeps its tabs. "Open Dashboard in New Window" opens such
// a window on Overseer's own workspace file (in its global storage, never in a repository). In a
// window opened on a single folder, workspace settings would be the folder's .vscode/settings.json,
// written into the owner's repository (often a tracked file, which agents working in the checkout
// would then see and commit), so there, and in a window with no folder, the values still go to user
// settings. Either way they are applied on entering the dashboard and put back exactly on leaving it
// (a value that was not set is removed again, not written as the default). The snapshot is saved
// before anything changes, so a window that closed while in the dashboard restores the settings the
// next time it starts outside it.
const vscode = require('vscode');

const SETTINGS = [
  ['workbench.editor.showTabs', 'none'],
  ['breadcrumbs.enabled', false],
  ['workbench.editor.editorActionsLocation', 'hidden'],
];
const KEY = 'overseer.immersive.saved';

/** Identifies this window across restarts: its workspace file, first folder, or none. */
function windowKey() {
  const ws = vscode.workspace.workspaceFile || (vscode.workspace.workspaceFolders || [])[0]?.uri;
  return ws ? ws.toString() : 'empty-window';
}

/** This window only (its workspace file) when it has one; otherwise user settings (see above). */
function windowOnly() { return !!vscode.workspace.workspaceFile; }

class Immersive {
  constructor(context, log) { this.context = context; this.log = log; }

  get enabled() { return vscode.workspace.getConfiguration('overseer').get('dashboard.immersive', true); }
  /** Where this window's snapshot lives: its own workspace state when the settings are window-only. */
  get store() { return windowOnly() ? this.context.workspaceState : this.context.globalState; }
  get saved() { return this.store.get(KEY); }
  get scope() { return windowOnly() ? 'workspace' : 'user'; }

  async apply() {
    if (!this.enabled || this.saved) return;
    const scope = this.scope;
    const prev = SETTINGS.map(([key]) => {
      const [section, ...rest] = key.split('.');
      const inspect = vscode.workspace.getConfiguration(section).inspect(rest.join('.'));
      return { key, value: inspect ? (scope === 'workspace' ? inspect.workspaceValue : inspect.globalValue) : undefined };
    });
    await this.store.update(KEY, { window: windowKey(), scope, prev, at: Date.now() });
    for (const [key, value] of SETTINGS) await this.update(key, value, scope);
    this.log(`immersive: applied (${scope} settings) ${SETTINGS.map(([k, v]) => `${k}=${JSON.stringify(v)}`).join(', ')}`);
  }

  async restore() {
    const saved = this.saved;
    if (!saved) return;
    for (const { key, value } of saved.prev) await this.update(key, value, saved.scope || 'user');
    await this.store.update(KEY, undefined);
    this.log(`immersive: restored (${saved.scope || 'user'} settings) ${saved.prev.map(p => `${p.key}=${p.value === undefined ? '(unset)' : JSON.stringify(p.value)}`).join(', ')}`);
  }

  /** At startup outside the dashboard: put back settings this window left applied (it closed in the dashboard). */
  async recover(inDashboard) {
    const saved = this.saved;
    if (saved && !inDashboard && saved.window === windowKey()) await this.restore();
  }

  async update(key, value, scope = 'user') {
    const [section, ...rest] = key.split('.');
    const target = scope === 'workspace' ? vscode.ConfigurationTarget.Workspace : vscode.ConfigurationTarget.Global;
    await vscode.workspace.getConfiguration(section).update(rest.join('.'), value, target).then(undefined, error => this.log(`immersive: ${key}: ${error.message}`));
  }
}

module.exports = { Immersive, SETTINGS };
