// Immersive editor area (AC-102). While the window is in Overseer's mode (the dashboard), VS Code's
// chrome above Overseer's views recedes: no tab strip, no breadcrumbs and no editor actions row;
// each Overseer view names what it shows in its own header. These are the only settings Overseer
// changes for this, listed in SETTINGS below and in the overseer.dashboard.immersive description.
//
// VS Code has no per-window settings, and workspace settings would write .vscode/settings.json into
// the user's repository, so the values go to user settings: applied on entering the dashboard and
// put back exactly on leaving it (a value that was not set is removed again, not written as the
// default). The snapshot is saved before anything changes, so a window that closed while in the
// dashboard restores the settings the next time it starts outside it.
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

class Immersive {
  constructor(context, log) { this.context = context; this.log = log; }

  get enabled() { return vscode.workspace.getConfiguration('overseer').get('dashboard.immersive', true); }
  get saved() { return this.context.globalState.get(KEY); }

  async apply() {
    if (!this.enabled || this.saved) return;
    const prev = SETTINGS.map(([key]) => {
      const [section, ...rest] = key.split('.');
      const inspect = vscode.workspace.getConfiguration(section).inspect(rest.join('.'));
      return { key, value: inspect ? inspect.globalValue : undefined };
    });
    await this.context.globalState.update(KEY, { window: windowKey(), prev, at: Date.now() });
    for (const [key, value] of SETTINGS) await this.update(key, value);
    this.log(`immersive: applied ${SETTINGS.map(([k, v]) => `${k}=${JSON.stringify(v)}`).join(', ')}`);
  }

  async restore() {
    const saved = this.saved;
    if (!saved) return;
    for (const { key, value } of saved.prev) await this.update(key, value);
    await this.context.globalState.update(KEY, undefined);
    this.log(`immersive: restored ${saved.prev.map(p => `${p.key}=${p.value === undefined ? '(unset)' : JSON.stringify(p.value)}`).join(', ')}`);
  }

  /** At startup outside the dashboard: put back settings this window left applied (it closed in the dashboard). */
  async recover(inDashboard) {
    const saved = this.saved;
    if (saved && !inDashboard && saved.window === windowKey()) await this.restore();
  }

  async update(key, value) {
    const [section, ...rest] = key.split('.');
    await vscode.workspace.getConfiguration(section).update(rest.join('.'), value, vscode.ConfigurationTarget.Global).then(undefined, error => this.log(`immersive: ${key}: ${error.message}`));
  }
}

module.exports = { Immersive, SETTINGS };
