// The agent's head (AC-233, AC-257). Opening an agent gives its whole worktree in this window: the
// Files view ("Files in <agent>") in Overseer's side bar is its file tree, and its files open as ordinary editors on
// their real paths (file: URIs), so they are edited, saved, searched and language-served like any
// file and every save lands in the agent's worktree. The window's own folder never changes (no
// workspace folder is added; nothing opens a new window).
//
// Follow (the default) opens the file the agent is editing, at the line it changed, and annotates
// every file of its worktree with the agent's changes against its comparison base: added and changed
// lines are tinted with a bar in the gutter, a changed line says what it was, and removed lines leave
// a marker whose hover holds the removed text. Diffs only is the review (the vendored Branch Diff
// webview); a toggle switches between them and each agent remembers its choice.
//
// Where the owner was in an agent's worktree (its open files, the active one, each cursor and
// scroll position) is kept when its editors close — another agent opened, the chat alone, Diffs
// only — and put back when the agent is opened again, so going to Overseer's conversation and
// back never loses it.
const vscode = require('vscode');
const path = require('path');
const fsp = require('fs').promises;
const fs = require('fs');
const { execFile } = require('child_process');
const { diffLines, splitLines } = require('./line-diff');

const README = /^readme(\.[a-z]+)?$/i;
const MAX_FILES = 20000;
const OWNER_BUSY_MS = 3000;

const within = (root, file) => { const rel = path.relative(root, file); return !!rel && !rel.startsWith('..') && !path.isAbsolute(rel); };
const toRel = (root, file) => path.relative(root, file).split(path.sep).join('/');

class AgentHead {
  /**
   * handlers: ensureShown(runId) puts the agent's head in the editor area (the arrangement's
   * split) when a Worktree file is opened while only the chat is shown.
   */
  constructor({ context, client, model, review, log, handlers = {} }) {
    Object.assign(this, { context, client, model, review, log, handlers });
    this.modes = new Map(Object.entries(context.workspaceState.get('overseer.head.modes', {})));
    this.places = new Map(Object.entries(context.workspaceState.get('overseer.head.places', {})));
    this.runId = undefined; this.root = undefined; this.workspaceId = undefined;
    this.files = []; this.changed = new Map(); this.base = undefined;
    this.baseTexts = new Map(); // `${base}\0${rel}` -> text
    this.positions = new Map(); // abs path -> { line, character, top }
    this.openRoots = new Map(); // run id -> worktree path, for agents whose files the head opened
    this.lastOwnerEdit = 0;
    this.pending = undefined; // an agent edit Follow held back while the owner was typing
    this.quiet = 0;
    this.annotations = new Map(); // abs path -> { added, changed, removed }

    this.tree = new WorktreeTree(this);
    this.view = vscode.window.createTreeView('overseer.worktree', { treeDataProvider: this.tree, showCollapseAll: true });
    this.decorationsEmitter = new vscode.EventEmitter();
    const color = id => new vscode.ThemeColor(id);
    this.types = {
      added: vscode.window.createTextEditorDecorationType({ isWholeLine: true, backgroundColor: color('diffEditor.insertedLineBackground'), borderColor: color('editorGutter.addedBackground'), borderStyle: 'solid', borderWidth: '0 0 0 3px', overviewRulerColor: color('editorOverviewRuler.addedForeground'), overviewRulerLane: vscode.OverviewRulerLane.Left }),
      changed: vscode.window.createTextEditorDecorationType({ isWholeLine: true, backgroundColor: color('diffEditor.insertedLineBackground'), borderColor: color('editorGutter.modifiedBackground'), borderStyle: 'solid', borderWidth: '0 0 0 3px', overviewRulerColor: color('editorOverviewRuler.modifiedForeground'), overviewRulerLane: vscode.OverviewRulerLane.Left }),
      was: vscode.window.createTextEditorDecorationType({ after: { color: color('editorCodeLens.foreground'), fontStyle: 'italic', margin: '0 0 0 2em' } }),
      removed: vscode.window.createTextEditorDecorationType({ borderColor: color('editorGutter.deletedBackground'), borderStyle: 'solid', borderWidth: '0 0 2px 0', overviewRulerColor: color('editorOverviewRuler.deletedForeground'), overviewRulerLane: vscode.OverviewRulerLane.Left,
        after: { color: color('editorGutter.deletedBackground'), fontStyle: 'italic', margin: '0 0 0 2em' } }),
    };
    this.status = vscode.window.createStatusBarItem('overseer.head', vscode.StatusBarAlignment.Left, 48);
    this.status.name = 'Overseer: the agent\'s head';
    this.timers = new Map();
    context.subscriptions.push(this.view, this.status, this.decorationsEmitter, ...Object.values(this.types),
      vscode.window.registerFileDecorationProvider({ onDidChangeFileDecorations: this.decorationsEmitter.event, provideFileDecoration: uri => this.fileDecoration(uri) }),
      vscode.window.onDidChangeVisibleTextEditors(editors => { for (const e of editors) this.annotate(e); this.updateStatus(); }),
      vscode.window.onDidChangeActiveTextEditor(e => { this.updateContext(e); this.revealInTree(e); }),
      vscode.workspace.onDidChangeTextDocument(e => this.onDocumentChange(e)),
      vscode.workspace.onDidSaveTextDocument(doc => { if (this.root && within(this.root, doc.uri.fsPath)) this.refreshSoon(); }),
      vscode.window.onDidChangeTextEditorSelection(e => this.remember(e.textEditor)),
      vscode.window.onDidChangeTextEditorVisibleRanges(e => this.remember(e.textEditor)),
      vscode.window.tabGroups.onDidChangeTabs(() => { this.updateStatus(); this.watchClosed(); }));
    client.on('event', event => this.onEvent(event));
    this.updateContext(vscode.window.activeTextEditor);
  }

  // ------------------------------------------------------------ modes

  defaultMode() { return vscode.workspace.getConfiguration('overseer').get('agent.openIn', 'follow') === 'diffs' ? 'diffs' : 'follow'; }
  modeFor(runId) { return this.modes.get(runId) || this.defaultMode(); }

  /** Switches an agent between Follow (its files, annotated) and Diffs only (the review). */
  async setMode(runId, mode) {
    runId = runId || this.runId;
    if (!runId || !['follow', 'diffs'].includes(mode)) return;
    const was = this.modeFor(runId);
    this.modes.set(runId, mode);
    this.context.workspaceState.update('overseer.head.modes', Object.fromEntries([...this.modes].slice(-300)));
    this.updateContext(vscode.window.activeTextEditor);
    if (was === mode && (mode === 'diffs' ? this.review.manager.panelFor(runId) : this.tabs(this.openRoots.get(runId) || this.rootOf(runId)).length)) return;
    // Neither view is on screen (the chat alone): the arrangement brings the head in, in the new mode.
    const shown = this.review.manager.panelFor(runId) || this.tabs(this.openRoots.get(runId)).length;
    if (!shown && this.handlers.ensureShown) { await this.handlers.ensureShown(runId); this.updateStatus(); return; }
    // The new view opens in the old one's group before the old one closes, so the group (and the
    // layout around it) stays.
    if (mode === 'diffs') {
      const column = this.headColumn(runId);
      const root = this.openRoots.get(runId);
      if (root) this.snapshot(runId, root);
      await this.review.open(runId, { viewColumn: column, preserveFocus: false, mode: 'diffs' });
      this.close(undefined, { only: runId, keepPlace: true });
    } else {
      const found = this.review.manager.panelFor(runId);
      const column = found?.panel.viewColumn || this.column;
      await this.open(runId, { viewColumn: column, preserveFocus: false });
      this.review.switching = (this.review.switching || 0) + 1;
      try { found?.panel.dispose(); } finally { this.review.switching--; }
    }
    this.log(`head: ${runId} in ${mode === 'diffs' ? 'Diffs only' : 'Follow'}`);
    this.updateStatus();
  }

  toggleMode(runId) {
    runId = runId || this.runId;
    if (runId) return this.setMode(runId, this.modeFor(runId) === 'follow' ? 'diffs' : 'follow');
  }

  // ------------------------------------------------------------ the agent shown

  rootOf(runId) {
    const run = runId && this.model.run(runId);
    const ws = run && this.model.workspace((this.model.rootRun(run) || run).workspace_id);
    return ws && !ws.removed_ms ? ws.path : undefined;
  }

  /** The Worktree view shows this agent's worktree (the side bar's selection; no editor opens). */
  async select(runId) {
    const run = runId && this.model.run(runId);
    const root = run && (this.model.rootRun(run) || run);
    const ws = root && this.model.workspace(root.workspace_id);
    if (!root) return;
    if (!ws || ws.removed_ms) {
      // Its worktree is gone: the view says so rather than showing another agent's files.
      this.runId = root.id; this.root = undefined; this.files = []; this.changed = new Map();
      this.view.title = this.filesTitle(root); this.view.description = ''; this.view.message = 'This agent\'s worktree was removed.';
      this.tree.rebuild();
      this.onSelect?.(root);
      return;
    }
    if (this.runId === root.id && this.root === ws.path) { this.refreshSoon(); return; }
    this.runId = root.id; this.root = ws.path; this.workspaceId = ws.id;
    this.files = []; this.changed = new Map(); this.base = undefined;
    this.view.title = this.filesTitle(root);
    this.view.description = '';
    this.updateContext(vscode.window.activeTextEditor);
    this.onSelect?.(root);
    this.loading = this.refresh();
    await this.loading;
  }

  /** The agent's files, its changes and its comparison base, then the tree and the annotations. */
  async refresh() {
    const runId = this.runId, root = this.root;
    if (!runId || !root) return;
    let comparison;
    try { comparison = await this.review.currentComparison(runId); } catch (error) { this.log('head: comparison: ' + error.message); }
    const [files, diff] = await Promise.all([
      new Promise(resolve => execFile('git', ['ls-files', '-co', '--exclude-standard', '-z'], { cwd: root, maxBuffer: 64 * 1024 * 1024 }, (err, out) => resolve(err ? [] : out.split('\0').filter(Boolean).slice(0, MAX_FILES)))),
      comparison?.base ? this.client.request('workspace.diff', { workspace_id: this.workspaceId, base: comparison.base, status: false }).catch(() => undefined) : undefined,
    ]);
    if (runId !== this.runId) return;
    const git = await this.review.gitApi().catch(() => undefined);
    const real = await fsp.realpath(root).catch(() => root);
    this.gitKnows = !!git?.repositories.some(r => [root, real].includes(r.rootUri.fsPath));
    if (comparison?.base !== this.base) this.baseTexts.clear();
    this.base = comparison?.base; this.baseLabel = comparison?.label;
    const changed = new Map();
    for (const c of diff?.changes || []) changed.set(c.path, c.status);
    // Deleted files stay in the tree so their removal can be seen.
    const all = new Set(files);
    for (const [rel, status] of changed) if (status === 'D') all.add(rel);
    this.files = [...all].sort();
    const before = this.changed;
    this.changed = changed;
    this.view.description = changed.size ? `${changed.size} changed` : '';
    this.view.message = this.files.length ? undefined : 'No files in this worktree.';
    this.tree.rebuild();
    const touched = [...new Set([...before.keys(), ...changed.keys()])];
    if (touched.length) this.decorationsEmitter.fire(touched.map(rel => vscode.Uri.file(path.join(root, rel))));
    for (const e of vscode.window.visibleTextEditors) this.annotate(e);
  }

  refreshSoon() { clearTimeout(this.refreshTimer); this.refreshTimer = setTimeout(() => this.refresh().catch(error => this.log('head: ' + error.message)), 400); }

  onEvent(event) {
    if (!this.runId || !event?.run_id) return;
    const run = this.model.run(event.run_id);
    const root = run && (this.model.rootRun(run) || run);
    if (!root || root.id !== this.runId) return;
    if (['file_activity', 'turn_done', 'tool_result', 'status'].includes(event.kind)) this.refreshSoon();
  }

  // ------------------------------------------------------------ editors

  /** The editor tabs showing files of this worktree (text editors and removed files' diffs). */
  tabs(root) {
    if (!root) return [];
    const out = [];
    for (const group of vscode.window.tabGroups.all) for (const tab of group.tabs) {
      const uri = tab.input instanceof vscode.TabInputText ? tab.input.uri : tab.input instanceof vscode.TabInputTextDiff ? tab.input.modified : undefined;
      if (uri && uri.scheme === 'file' && within(root, uri.fsPath)) out.push({ tab, group, rel: toRel(root, uri.fsPath), uri });
    }
    return out;
  }

  /**
   * The head's editors: files of the worktree open outside the group holding Overseer's view (a file
   * opened there, by a Look action say, is not the head: focusing it would cover the conversation).
   */
  headTabs(root) {
    const overseer = vscode.window.tabGroups.all.find(g => g.tabs.some(t => t.input?.viewType?.endsWith('overseer.center')));
    return this.tabs(root).filter(t => t.group !== overseer);
  }

  /** Where the head's editors go: beside its files already open, else where it was asked to open. */
  headColumn(runId = this.runId) {
    const open = this.headTabs(this.openRoots.get(runId) || this.rootOf(runId));
    const active = open.find(t => t.tab.isActive && t.group.isActive) || open.find(t => t.tab.isActive) || open[0];
    return active?.group.viewColumn || this.column || vscode.ViewColumn.One;
  }

  /**
   * Opens the agent's head in the editor area (Follow): where the owner was, else the file it last
   * edited, else its first changed file, else its README. Returns false when there is no file to open.
   */
  async open(runId, { viewColumn, preserveFocus = false, reveal } = {}) {
    await this.select(runId);
    // The first look at its worktree may still be under way (the side bar's selection started it).
    await this.loading?.catch(() => {});
    const root = this.rootOf(runId);
    if (!root || this.runId !== runId) { this.log(`head: ${runId} has no worktree to open`); return false; }
    if (viewColumn) this.column = viewColumn;
    this.openRoots.set(runId, root);
    this.showView(true);
    const open = this.headTabs(root);
    if (open.length && !reveal) {
      // Already open: bring its active file forward where it is.
      const t = open.find(x => x.tab.isActive) || open[0];
      await this.show(t.uri, { viewColumn: t.group.viewColumn, preserveFocus, preview: t.tab.isPreview });
      this.updateStatus();
      return true;
    }
    if (reveal) { await this.openFile(runId, reveal.path, { line: reveal.line, preserveFocus }); return true; }
    const place = this.places.get(runId);
    if (place && await this.restore(runId, place, { preserveFocus })) { this.updateStatus(); return true; }
    const last = this.review.lastReveal?.get(runId);
    let target = last && !String(last.path).startsWith('..') ? { rel: last.path, line: last.line } : undefined;
    if (!target) {
      const first = [...this.changed].find(([, s]) => s !== 'D')?.[0];
      if (first) target = { rel: first, line: await this.firstChangedLine(first) };
    }
    if (!target) { const readme = this.files.find(f => README.test(f)) || this.files.find(f => README.test(path.basename(f))) || this.files[0]; if (readme) target = { rel: readme, line: 1 }; }
    if (!target) { this.log(`head: ${runId}: no file to open`); return false; }
    this.log(`head: ${runId} opens ${target.rel}:${target.line} in column ${this.headColumn(runId)}`);
    await this.openFile(runId, target.rel, { line: target.line, preserveFocus, preview: false });
    this.updateStatus();
    return true;
  }

  async firstChangedLine(rel) {
    const abs = path.join(this.root, rel);
    const text = await fsp.readFile(abs, 'utf8').catch(() => undefined);
    if (text === undefined) return 1;
    const hunks = diffLines(await this.baseText(rel), text);
    return hunks.length ? hunks[0].modStart + 1 : 1;
  }

  /** Opens a file of the agent's worktree in the head, at `line` (1-based). */
  async openFile(runId, rel, { line, preserveFocus = false, preview = false, fromTree = false } = {}) {
    runId = runId || this.runId;
    if (runId !== this.runId) await this.select(runId);
    const root = this.root;
    if (!root || !rel || rel.startsWith('/') || rel.startsWith('..')) return;
    // Opened from the Worktree view while only the chat is on screen: the head comes in first.
    if (fromTree && !this.tabs(root).length && this.handlers.ensureShown) await this.handlers.ensureShown(runId);
    this.openRoots.set(runId, root);
    this.showView(true);
    const abs = path.join(root, rel);
    if (this.changed.get(rel) === 'D' || !fs.existsSync(abs)) {
      // A file the agent removed: its last text against nothing.
      const name = path.basename(rel);
      const left = vscode.Uri.from({ scheme: 'overseer-git', path: '/' + name, query: JSON.stringify({ root, ref: this.base, path: rel }) });
      const right = vscode.Uri.from({ scheme: 'overseer-git', path: '/' + name, query: JSON.stringify({ empty: true }) });
      await vscode.commands.executeCommand('vscode.diff', left, right, `${name} (removed by the agent)`, { viewColumn: this.headColumn(runId), preserveFocus, preview });
      return;
    }
    const editor = await this.show(vscode.Uri.file(abs), { viewColumn: this.headColumn(runId), preserveFocus, preview });
    if (editor && line) {
      const at = new vscode.Position(Math.max(0, Math.min(editor.document.lineCount - 1, line - 1)), 0);
      editor.selection = new vscode.Selection(at, at);
      editor.revealRange(new vscode.Range(at, at), vscode.TextEditorRevealType.InCenterIfOutsideViewport);
    }
    if (editor) await this.annotate(editor);
    return editor;
  }

  async show(uri, { viewColumn, preserveFocus, preview }) {
    const doc = await vscode.workspace.openTextDocument(uri);
    return vscode.window.showTextDocument(doc, { viewColumn, preserveFocus, preview });
  }

  /** Follow: the agent edited `message.path`; the head goes there unless the owner is typing. */
  async follow(runId, message) {
    if (runId !== this.runId || this.modeFor(runId) !== 'follow' || !message?.path) return;
    const root = this.root;
    if (!root || !this.tabs(root).length) return; // the head is not on screen; the arrangement brings it
    const active = vscode.window.activeTextEditor;
    const busy = active && within(root, active.document.uri.fsPath) && (active.document.isDirty || Date.now() - this.lastOwnerEdit < OWNER_BUSY_MS);
    if (busy && toRel(root, active.document.uri.fsPath) !== message.path) {
      this.pending = message;
      this.updateStatus();
      return;
    }
    this.pending = undefined;
    await this.openFile(runId, message.path, { line: message.line, preserveFocus: true, preview: true });
    this.refreshSoon();
    this.updateStatus();
  }

  /** Goes to the edit Follow held back while the owner was typing. */
  async catchUp() {
    const m = this.pending;
    this.pending = undefined;
    if (m) await this.openFile(this.runId, m.path, { line: m.line, preserveFocus: false });
    this.updateStatus();
  }

  /** Focuses the head's active file (back to the agent, AC-257). Returns false when none is open. */
  async focus(runId = this.runId) {
    const review = this.review.manager.panelFor(runId);
    if (review && this.modeFor(runId) === 'diffs') { review.panel.reveal(review.panel.viewColumn, false); return true; }
    const open = this.headTabs(this.openRoots.get(runId) || this.rootOf(runId));
    const t = open.find(x => x.tab.isActive && x.group.isActive) || open.find(x => x.tab.isActive) || open[0];
    if (!t) {
      const found = this.review.manager.panelFor(runId);
      if (found) { found.panel.reveal(found.panel.viewColumn, false); return true; }
      return false;
    }
    if (t.tab.input instanceof vscode.TabInputText) {
      const doc = await vscode.workspace.openTextDocument(t.uri);
      const editor = await vscode.window.showTextDocument(doc, { viewColumn: t.group.viewColumn, preserveFocus: false, preview: t.tab.isPreview });
      // Where the owner left it (a showTextDocument of an open file keeps it; this is belt and braces).
      const p = this.positions.get(t.uri.fsPath);
      if (p && editor.visibleRanges[0]?.start.line !== p.top) editor.revealRange(new vscode.Range(p.top, 0, p.top, 0), vscode.TextEditorRevealType.AtTop);
    }
    return true;
  }

  /** Closes the head's editors of every agent but `keepRunId`, remembering where the owner was. */
  close(keepRunId, { only, keepPlace } = {}) {
    const closing = [];
    for (const [runId, root] of [...this.openRoots]) {
      if (runId === keepRunId || (only && runId !== only)) continue;
      if (!keepPlace) this.snapshot(runId, root);
      // An unsaved file stays open: closing it would ask about the owner's edit.
      for (const t of this.tabs(root)) if (!t.tab.isDirty) closing.push(t.tab);
      this.openRoots.delete(runId);
    }
    if (!this.openRoots.size) this.showView(false);
    if (!closing.length) return;
    this.quiet++;
    vscode.window.tabGroups.close(closing, true).then(() => {}, error => this.log('head: close: ' + error.message)).finally(() => setTimeout(() => this.quiet--, 600));
  }

  /** The owner closed every file of the agent's head (Follow): like closing its review. */
  watchClosed() {
    if (this.quiet || !this.runId || this.modeFor(this.runId) !== 'follow') return;
    const runId = this.runId, root = this.openRoots.get(runId);
    if (!root || this.tabs(root).length) return;
    clearTimeout(this.closedTimer);
    this.closedTimer = setTimeout(() => {
      if (this.quiet || this.openRoots.get(runId) !== root || this.tabs(root).length) return;
      this.openRoots.delete(runId);
      if (!this.openRoots.size) this.showView(false);
      this.places.delete(runId); this.savePlaces();
      this.review.onClosed?.(runId);
    }, 500);
  }

  /** The view's title in plain words: whose files these are (AC-264). */
  filesTitle(root) { return `Files in ${this.model.task?.(root.task_id)?.title || root.title || 'the agent'}`; }

  /** The Files view is in the side bar while an agent's files are open in Follow (not in Diffs only, whose review lists them). */
  showView(on) {
    if (this.viewShown === on) return;
    this.viewShown = on;
    if (on) this.place().catch(error => this.log('head: place: ' + error.message));
    vscode.commands.executeCommand('setContext', 'overseer.headOpen', on);
  }

  /**
   * AC-264: the Files view always sits in Overseer's side bar under the agents list, never over
   * Overseer's panel on the right. VS Code keeps a view where it was last dragged (the secondary
   * side bar, the panel), so once a session it is moved back into Overseer's view container (VS
   * Code's own Move Views; nothing moves when it is already there). Moving opens that container, so
   * the keyboard goes back to the editor after.
   */
  async place() {
    if (this.placed) return;
    this.placed = true;
    const focused = vscode.window.activeTextEditor || vscode.window.tabGroups.activeTabGroup.activeTab;
    await vscode.commands.executeCommand('vscode.moveViews', { viewIds: ['overseer.worktree'], destinationId: 'workbench.view.extension.overseer' });
    if (focused) await vscode.commands.executeCommand('workbench.action.focusActiveEditorGroup').then(undefined, () => {});
  }

  // ------------------------------------------------------------ where the owner was

  remember(editor) {
    const file = editor?.document.uri;
    if (!file || file.scheme !== 'file') return;
    const root = [...this.openRoots.values()].find(r => within(r, file.fsPath));
    if (!root) return;
    const s = editor.selection.active;
    this.positions.set(file.fsPath, { line: s.line, character: s.character, top: editor.visibleRanges[0]?.start.line ?? 0 });
    if (this.positions.size > 500) this.positions.delete(this.positions.keys().next().value);
  }

  snapshot(runId, root) {
    const open = this.tabs(root).filter(t => t.tab.input instanceof vscode.TabInputText);
    if (!open.length) return;
    for (const e of vscode.window.visibleTextEditors) this.remember(e);
    const files = open.map(t => ({ rel: t.rel, column: t.group.viewColumn, ...(this.positions.get(t.uri.fsPath) || {}) }));
    const active = (open.find(t => t.tab.isActive && t.group.isActive) || open.find(t => t.tab.isActive))?.rel;
    this.places.set(runId, { files, active, at: Date.now() });
    this.savePlaces();
  }

  savePlaces() { this.context.workspaceState.update('overseer.head.places', Object.fromEntries([...this.places].slice(-100))); }

  async restore(runId, place, { preserveFocus }) {
    const files = (place.files || []).filter(f => f.rel && fs.existsSync(path.join(this.root, f.rel))).slice(0, 12);
    if (!files.length) return false;
    const order = [...files.filter(f => f.rel !== place.active), ...files.filter(f => f.rel === place.active)];
    for (const f of order) {
      const editor = await this.show(vscode.Uri.file(path.join(this.root, f.rel)), { viewColumn: this.headColumn(runId), preserveFocus: f.rel === place.active ? preserveFocus : true, preview: false });
      if (!editor) continue;
      if (f.line !== undefined) {
        const at = new vscode.Position(Math.min(editor.document.lineCount - 1, f.line), f.character || 0);
        editor.selection = new vscode.Selection(at, at);
      }
      this.annotate(editor);
    }
    // VS Code puts back its own remembered scroll for a reopened file a moment later: where the
    // owner was wins.
    const active = files.find(f => f.rel === place.active) || files[files.length - 1];
    await new Promise(r => setTimeout(r, 200));
    const editor = vscode.window.visibleTextEditors.find(e => e.document.uri.fsPath === path.join(this.root, active.rel));
    if (editor && active.top !== undefined && editor.visibleRanges[0]?.start.line !== active.top) editor.revealRange(new vscode.Range(active.top, 0, active.top, 0), vscode.TextEditorRevealType.AtTop);
    return true;
  }

  // ------------------------------------------------------------ the agent's changes, inline

  async baseText(rel) {
    if (!this.base || !this.root) return '';
    const key = `${this.base}\0${rel}`;
    if (!this.baseTexts.has(key)) {
      // A file the agent added has no base text; one outside the comparison is unchanged.
      const status = this.changed.get(rel);
      const text = status === 'A' ? '' : await this.review.baseText(this.root, this.base, rel).catch(() => '');
      this.baseTexts.set(key, text);
      if (this.baseTexts.size > 400) this.baseTexts.delete(this.baseTexts.keys().next().value);
    }
    return this.baseTexts.get(key);
  }

  onDocumentChange(e) {
    const doc = e.document;
    if (doc.uri.scheme !== 'file' || !this.root || !within(this.root, doc.uri.fsPath) || !e.contentChanges.length) return;
    if (doc.isDirty) this.lastOwnerEdit = Date.now();
    clearTimeout(this.timers.get(doc.uri.fsPath));
    this.timers.set(doc.uri.fsPath, setTimeout(() => {
      this.timers.delete(doc.uri.fsPath);
      for (const editor of vscode.window.visibleTextEditors) if (editor.document === doc) this.annotate(editor);
    }, 150));
  }

  /** Annotates an editor of the agent's worktree with its changes (added, changed, removed lines). */
  async annotate(editor) {
    const file = editor?.document.uri;
    if (!file || file.scheme !== 'file' || !this.root || !within(this.root, file.fsPath)) return;
    const rel = toRel(this.root, file.fsPath);
    const version = editor.document.version;
    const base = this.changed.has(rel) ? await this.baseText(rel) : undefined;
    if (editor.document.version !== version) return; // a newer change annotates it
    const hunks = base === undefined ? [] : diffLines(base, editor.document.getText());
    const old = base === undefined ? [] : splitLines(base);
    const added = [], changed = [], was = [], removed = [];
    const lang = editor.document.languageId;
    const lines = editor.document.lineCount;
    const md = (title, text) => { const m = new vscode.MarkdownString(); m.appendMarkdown(`**${title}**`); m.appendCodeblock(text, lang); return m; };
    for (const h of hunks) {
      const gone = old.slice(h.origStart, h.origStart + h.origLen);
      if (!h.modLen) {
        const at = h.modStart > 0 ? Math.min(lines - 1, h.modStart - 1) : 0;
        const where = h.modStart > 0 ? 'below' : 'above';
        removed.push({ range: editor.document.lineAt(at).range, hoverMessage: md(`Removed by the agent (${gone.length} line${gone.length === 1 ? '' : 's'})`, gone.join('\n')),
          renderOptions: { after: { contentText: `− ${gone.length} line${gone.length === 1 ? '' : 's'} removed ${where}` } } });
        continue;
      }
      const range = new vscode.Range(h.modStart, 0, Math.min(lines - 1, h.modStart + h.modLen - 1), 0);
      if (!h.origLen) { added.push({ range, hoverMessage: new vscode.MarkdownString(`**Added by the agent** (${h.modLen} line${h.modLen === 1 ? '' : 's'})`) }); continue; }
      changed.push({ range, hoverMessage: md(`Changed by the agent — it was (${gone.length} line${gone.length === 1 ? '' : 's'}):`, gone.join('\n')) });
      if (h.origLen === h.modLen) {
        for (let i = 0; i < h.modLen && h.modStart + i < lines; i++) {
          const before = gone[i].trim();
          was.push({ range: editor.document.lineAt(h.modStart + i).range, renderOptions: { after: { contentText: `was: ${before ? (before.length > 90 ? before.slice(0, 89) + '…' : before) : '(empty line)'}` } } });
        }
      } else if (h.modStart < lines) {
        was.push({ range: editor.document.lineAt(h.modStart).range, renderOptions: { after: { contentText: `replaced ${h.origLen} line${h.origLen === 1 ? '' : 's'}` } } });
      }
    }
    editor.setDecorations(this.types.added, added);
    editor.setDecorations(this.types.changed, changed);
    editor.setDecorations(this.types.was, was);
    editor.setDecorations(this.types.removed, removed);
    this.annotations.set(file.fsPath, { added: added.length, changed: changed.length, removed: removed.length });
  }

  fileDecoration(uri) {
    if (uri.scheme !== 'file' || !this.root || !within(this.root, uri.fsPath)) return undefined;
    const status = this.changed.get(toRel(this.root, uri.fsPath));
    if (!status) return undefined;
    const kind = { A: ['A', 'gitDecoration.addedResourceForeground', 'Added by the agent'], D: ['D', 'gitDecoration.deletedResourceForeground', 'Removed by the agent'], R: ['R', 'gitDecoration.renamedResourceForeground', 'Renamed by the agent'] }[status] || ['M', 'gitDecoration.modifiedResourceForeground', 'Changed by the agent'];
    // Git's own badge is there already when it knows the worktree: the colour and the tooltip say it's the agent's.
    const decoration = new vscode.FileDecoration(this.gitKnows ? undefined : kind[0], kind[2], new vscode.ThemeColor(kind[1]));
    decoration.propagate = true;
    return decoration;
  }

  // ------------------------------------------------------------ status, context, tree

  updateContext(editor) {
    const inHead = !!(editor && this.root && editor.document.uri.scheme === 'file' && within(this.root, editor.document.uri.fsPath));
    vscode.commands.executeCommand('setContext', 'overseer.headEditor', inHead);
    vscode.commands.executeCommand('setContext', 'overseer.headMode', this.runId ? this.modeFor(this.runId) : '');
  }

  revealInTree(editor) {
    if (!editor || !this.root || !this.view.visible || editor.document.uri.scheme !== 'file' || !within(this.root, editor.document.uri.fsPath)) return;
    const node = this.tree.node(toRel(this.root, editor.document.uri.fsPath));
    if (node) this.view.reveal(node, { select: true, focus: false, expand: true }).then(undefined, () => {});
  }

  updateStatus() {
    const runId = this.runId;
    const shown = runId && (this.tabs(this.openRoots.get(runId)).length || this.review.manager.panelFor(runId));
    if (!shown) { this.status.hide(); return; }
    const mode = this.modeFor(runId), title = this.model.run(runId)?.title || 'the agent';
    if (this.pending && mode === 'follow') {
      this.status.text = `$(debug-pause) ${path.basename(this.pending.path)} changed`;
      this.status.tooltip = `${title} edited ${this.pending.path} while you were typing. Follow waited; click to go there.`;
      this.status.command = 'overseer.head.catchUp';
    } else {
      this.status.text = mode === 'follow' ? '$(eye) Follow' : '$(diff-multiple) Diffs only';
      this.status.tooltip = mode === 'follow' ? `Following ${title} in its worktree, its changes shown in the files. Click for Diffs only.` : `${title}: Diffs only (the review of what changed). Click to follow it in its files.`;
      this.status.command = 'overseer.head.toggleMode';
    }
    this.status.show();
  }
}

/** The Worktree view: the agent's files as a tree (git's tracked and untracked files, not ignored ones). */
class WorktreeTree {
  constructor(head) {
    this.head = head;
    this.emitter = new vscode.EventEmitter();
    this.onDidChangeTreeData = this.emitter.event;
    this.nodes = new Map(); // rel -> node
    this.children = new Map(); // dir rel ('' for the root) -> [node]
  }

  rebuild() {
    const nodes = new Map(), children = new Map([['', []]]);
    const add = (rel, dir) => {
      if (nodes.has(rel)) return nodes.get(rel);
      const parent = rel.includes('/') ? rel.slice(0, rel.lastIndexOf('/')) : '';
      if (parent && !nodes.has(parent)) add(parent, true);
      const node = { rel, dir, name: rel.split('/').pop(), parent };
      nodes.set(rel, node);
      if (!children.has(parent)) children.set(parent, []);
      children.get(parent).push(node);
      if (dir) children.set(rel, children.get(rel) || []);
      return node;
    };
    for (const rel of this.head.files) add(rel, false);
    for (const list of children.values()) list.sort((a, b) => (b.dir - a.dir) || a.name.localeCompare(b.name));
    this.nodes = nodes; this.children = children;
    this.emitter.fire();
  }

  node(rel) { return this.nodes.get(rel); }
  getChildren(node) { return this.children.get(node ? node.rel : '') || []; }
  getParent(node) { return node.parent ? this.nodes.get(node.parent) : undefined; }

  getTreeItem(node) {
    const uri = vscode.Uri.file(path.join(this.head.root, node.rel));
    const item = new vscode.TreeItem(uri, node.dir ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None);
    item.id = `${this.head.runId}:${node.rel}`;
    if (node.dir) { item.contextValue = 'head-dir'; return item; }
    const status = this.head.changed.get(node.rel);
    item.contextValue = status === 'D' ? 'head-removed' : 'head-file';
    item.tooltip = `${node.rel}${status ? ` — ${{ A: 'added', D: 'removed', R: 'renamed' }[status] || 'changed'} by the agent` : ''}`;
    item.command = { command: 'overseer.head.openFile', title: 'Open', arguments: [node.rel] };
    return item;
  }
}

module.exports = { AgentHead };
