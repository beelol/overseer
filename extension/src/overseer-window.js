// AC-264: the one Overseer layout. Workspace (⌥⌘⇧O, the status bar's button) reopens this window as
// the Overseer window: the agents list on the left, the agent's review wide in the middle and one
// Overseer panel on the right (home, Voice Mode, or the picked agent's chat, with back). Run again,
// it reopens the owner's own folder, where VS Code restores that window as they left it.
//
// The look is Focus Mode's (no tab rows, no breadcrumbs) without its side effects: VS Code has no
// per-window settings, but a window opened on a workspace file reads that file's settings, and only
// that window does. So the Overseer window is the owner's folder opened on a workspace file Overseer
// keeps in its own storage (never in a repository), whose settings hide the tab rows and breadcrumbs.
// No user setting is written and no other window changes.
//
// Reopening a window ends what runs in it: its unsaved files would get VS Code's own save question
// and its terminals are closed (VS Code keeps terminals only across a reload of the same window). So
// Overseer asks first, once, in its own words: it saves the files with a yes (untitled files VS Code
// keeps with the folder) and says when a terminal command would stop.
const vscode = require('vscode');
const path = require('path');
const crypto = require('crypto');
const { isOurs } = require('./layout');

const LOOK = { 'workbench.editor.showTabs': 'none', 'breadcrumbs.enabled': false, 'workbench.editor.editorActionsLocation': 'hidden' };
const OFFERED = 'overseer.layoutOffered';
const settle = ms => new Promise(r => setTimeout(r, ms));
const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

class OverseerWindow {
  /**
   * handlers: focusedAgent() the agent to show first; select(runId, opts) shows an agent (review in
   * the middle, its chat on the right); backToOverseer() turns the right panel to Overseer.
   */
  constructor({ context, center, arrangement, log, handlers }) {
    Object.assign(this, { context, center, arrangement, log, handlers });
    // Commands running in terminals (shell integration tells when one starts and ends).
    this.running = new Set();
    context.subscriptions.push(
      vscode.window.onDidStartTerminalShellExecution?.(e => this.running.add(e.execution)) || { dispose() {} },
      vscode.window.onDidEndTerminalShellExecution?.(e => this.running.delete(e.execution)) || { dispose() {} });
  }

  /**
   * Where Overseer's workspace files live: its global storage, as a file: path. (globalStorageUri
   * is a vscode-userdata: URI; a window opened on a workspace file there is a "virtual workspace",
   * where VS Code turns off every extension without virtual-workspace support, Overseer included.)
   */
  layoutsDir() { return vscode.Uri.file(path.join(this.context.globalStorageUri.fsPath, 'layouts')); }

  /** This window is the Overseer window (open on one of Overseer's own workspace files). */
  get active() {
    const file = vscode.workspace.workspaceFile;
    return !!file && file.scheme === 'file' && file.fsPath.startsWith(this.layoutsDir().fsPath + path.sep);
  }

  /** Workspace: into the Overseer window, or (run again) back to the owner's own. */
  async toggle() { return this.active ? this.leave() : this.open(); }

  async open() {
    if (!(await this.readyToReopen('open the Overseer layout'))) return false;
    const file = await this.writeFile();
    this.log(`overseer window: reopening on ${file.fsPath}`);
    await vscode.commands.executeCommand('vscode.openFolder', file, { forceReuseWindow: true, noRecentEntry: true });
    return true;
  }

  async leave() {
    let from;
    try { from = JSON.parse(Buffer.from(await vscode.workspace.fs.readFile(vscode.workspace.workspaceFile)).toString('utf8')).overseer?.from; }
    catch (error) { this.log('overseer window: ' + error.message); }
    if (!(await this.readyToReopen('go back to your layout'))) return false;
    this.log(`overseer window: back to ${from || 'an empty window'}`);
    if (from) await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.parse(from), { forceReuseWindow: true });
    else await vscode.commands.executeCommand('workbench.action.closeFolder');
    return true;
  }

  /** The Overseer workspace file for this window's folders (or workspace), written each time. */
  async writeFile() {
    const own = vscode.workspace.workspaceFile;
    const from = own || (vscode.workspace.workspaceFolders || [])[0]?.uri;
    const folders = (vscode.workspace.workspaceFolders || []).map(f => f.uri);
    const name = folders.length ? (vscode.workspace.name || path.basename(folders[0].fsPath)).replace(/ \(Workspace\)$/, '') : 'Overseer';
    // The owner's own workspace settings come along (a window on a folder has none of its own here).
    let settings = {};
    if (own && own.scheme === 'file') { try { settings = JSON.parse(Buffer.from(await vscode.workspace.fs.readFile(own)).toString('utf8').replace(/^\s*\/\/.*$/gm, '')).settings || {}; } catch { settings = {}; } }
    // The window's title keeps the owner's own form, with the folder's name where VS Code would say "<name> (Workspace)".
    const inspect = vscode.workspace.getConfiguration('window').inspect('title');
    const title = String(inspect?.globalValue ?? inspect?.defaultValue ?? '${activeEditorShort}${separator}${rootName}').replace(/\$\{rootName(Short)?\}/g, name.replace(/\$/g, ''));
    const key = crypto.createHash('sha1').update(from ? from.toString() : 'empty-window').digest('hex').slice(0, 12);
    const dir = vscode.Uri.joinPath(this.layoutsDir(), key);
    const file = vscode.Uri.joinPath(dir, `${name.replace(/[\\/:*?"<>|]/g, '-')}.code-workspace`);
    const content = {
      folders: folders.map(f => (f.scheme === 'file' ? { path: f.fsPath } : { uri: f.toString() })),
      settings: { ...settings, ...LOOK, 'window.title': title },
      overseer: { from: from ? from.toString() : null },
    };
    await vscode.workspace.fs.createDirectory(dir);
    await vscode.workspace.fs.writeFile(file, Buffer.from(JSON.stringify(content, null, 2) + '\n'));
    return file;
  }

  /**
   * Before the window reopens: unsaved files are saved and running terminal commands named, after
   * one question in Overseer's words. Returns false when the owner says no (or a save failed).
   */
  async readyToReopen(what) {
    const dirty = vscode.workspace.textDocuments.filter(d => d.isDirty);
    const files = dirty.filter(d => !d.isUntitled);
    const untitled = dirty.length - files.length;
    const commands = [...this.running].length;
    // A terminal without shell integration may be running something Overseer cannot see.
    const unknown = vscode.window.terminals.filter(t => !t.exitStatus && !t.shellIntegration).length;
    if (!files.length && !commands && !unknown) return true;
    const lines = ['This window reopens.'];
    if (commands) lines.push(`${commands === 1 ? 'The command' : `${commands} commands`} running in the terminal stop${commands === 1 ? 's' : ''}.`);
    else if (unknown) lines.push(`Its ${plural(unknown, 'terminal closes', 'terminals close')}, with anything running in ${unknown === 1 ? 'it' : 'them'}.`);
    if (untitled) lines.push(`${plural(untitled, 'untitled file stays', 'untitled files stay')} with this folder and come${untitled === 1 ? 's' : ''} back with it.`);
    const question = files.length ? `Save ${plural(files.length, 'file', 'files')} and ${what}?` : `${what[0].toUpperCase()}${what.slice(1)}?`;
    const yes = files.length ? 'Save All' : what.startsWith('open') ? 'Open' : 'Go Back';
    const answer = await vscode.window.showWarningMessage(question, { modal: true, detail: lines.join(' ') }, yes);
    if (answer !== yes) { this.log(`overseer window: not reopened (${files.length} unsaved, ${commands} running)`); return false; }
    for (const doc of files) {
      if (!(await doc.save())) {
        vscode.window.showWarningMessage(`${path.basename(doc.fileName)} could not be saved, so the window stays as it is.`);
        return false;
      }
    }
    this.log(`overseer window: saved ${files.length} file(s); ${commands} terminal command(s) stop`);
    return true;
  }

  /** On activation in the Overseer window: the layout, with home on the right. */
  async startup() {
    const on = this.active;
    await vscode.commands.executeCommand('setContext', 'overseer.inWorkspace', on);
    if (!on) return;
    const t0 = Date.now();
    this.arrangement.takeover = true;
    for (const c of ['workbench.action.closePanel', 'workbench.action.closeAuxiliaryBar']) await vscode.commands.executeCommand(c).then(undefined, () => {});
    // What VS Code put back from last time closes: files (hidden behind the review, with no tab
    // rows; unsaved ones stay) and Overseer's reviews and chats, which the layout opens afresh.
    const strays = vscode.window.tabGroups.all.flatMap(g => g.tabs).filter(t => (!isOurs(t) && !t.isDirty) || /overseer\.(review|output|chatEditor|newTask)$/.test(t.input?.viewType || ''));
    if (strays.length) await vscode.window.tabGroups.close(strays, true).then(undefined, () => {});
    await vscode.commands.executeCommand('workbench.view.extension.overseer').then(undefined, () => {});
    await this.home();
    // VS Code may still be restoring the window's editors: once they settle, the layout is checked.
    await settle(1500);
    const reviews = vscode.window.tabGroups.all.flatMap(g => g.tabs).filter(t => /overseer\.review$/.test(t.input?.viewType || '')).length;
    if (this.handlers.focusedAgent() && (vscode.window.tabGroups.all.length < 2 || !reviews)) { this.log('overseer window: arranging again after the restore'); await this.home(); }
    this.center.setDashboard?.(true);
    this.log(`overseer window: arranged in ${Date.now() - t0} ms`);
  }

  /** The focused agent's review in the middle and Overseer's conversation on the right. */
  async home() {
    const id = this.handlers.focusedAgent();
    if (id) { await this.handlers.select(id, { force: true }); await this.handlers.backToOverseer(); }
    else { await this.arrangement.chatOnly(); this.center.setMode('composer'); }
  }

  /** The first launch offers the layout once, with one click (AC-264). */
  async offer() {
    if (this.active || this.context.globalState.get(OFFERED)) return;
    if (!vscode.workspace.getConfiguration('overseer').get('layout.offerOnStartup', true)) return;
    await this.context.globalState.update(OFFERED, Date.now());
    const pick = await vscode.window.showInformationMessage('Set up the Overseer layout? Your agents on the left, the review in the middle and Overseer on the right. Workspace (⌥⌘⇧O) switches back and forth.', 'Set Up', 'Not Now');
    if (pick === 'Set Up') await this.open();
  }
}

/**
 * Focus Mode is retired (AC-264). A window that used it may still have its look applied: the
 * settings it changed (tab rows, breadcrumbs, the editor actions) are put back exactly as they were,
 * in user settings or in the workspace file it wrote them to, and its saved layout is dropped.
 */
async function retireFocusMode(context, log) {
  const KEY = 'overseer.immersive.saved';
  for (const store of [context.globalState, context.workspaceState]) {
    const saved = store.get(KEY);
    if (!saved) continue;
    const target = saved.scope === 'workspace' ? vscode.ConfigurationTarget.Workspace : vscode.ConfigurationTarget.Global;
    for (const { key, value } of saved.prev || []) {
      const [section, ...rest] = key.split('.');
      await vscode.workspace.getConfiguration(section).update(rest.join('.'), value, target).then(undefined, error => log(`focus mode: ${key}: ${error.message}`));
    }
    await store.update(KEY, undefined);
    log(`focus mode retired: put back ${(saved.prev || []).map(p => `${p.key}=${p.value === undefined ? '(unset)' : JSON.stringify(p.value)}`).join(', ')} (${saved.scope || 'user'} settings)`);
  }
  if (context.workspaceState.get('overseer.dashboard.saved')) await context.workspaceState.update('overseer.dashboard.saved', undefined);
}

module.exports = { OverseerWindow, retireFocusMode, LOOK };
