// The agent's head (AC-233, AC-257, AC-264). Opening an agent shows its review in the middle, in one
// of two modes, and each agent remembers its choice:
//   - Follow (the default): the review shows the file the agent is in right now (its latest edit or
//     read), live, at the line it is working on, its changes marked; the list beside it is All files,
//     the whole worktree, and a file picked there shows in the same place (the owner, 2026-09-30:
//     "follow would go where the review section is"). No VS Code editor opens.
//   - Diffs only: the review of what changed (the vendored Branch Diff webview), its list Changed.
//   - Manual edit (AC-252): the file Follow shows opens as a VS Code editor where the review is, at
//     the same line, to edit by hand; ⌥⌘E switches between it and Follow.
// The head also knows the selected agent's worktree: its files, its changes against the comparison
// base, and it marks the agent's changes in any of its files the owner opens as an editor (added and
// changed lines tinted with a bar in the gutter, a changed line saying what it was, removed lines
// leaving a marker). The window's own folder never changes.
const vscode = require('vscode');
const path = require('path');
const fsp = require('fs').promises;
const { execFile } = require('child_process');
const { diffLines, splitLines } = require('./line-diff');

const README = /^readme(\.[a-z]+)?$/i;
const MAX_FILES = 20000;

const within = (root, file) => { const rel = path.relative(root, file); return !!rel && !rel.startsWith('..') && !path.isAbsolute(rel); };
const toRel = (root, file) => path.relative(root, file).split(path.sep).join('/');

class AgentHead {
  /** handlers: ensureShown(runId) puts the agent's review in the editor area (the arrangement's split). */
  constructor({ context, client, model, review, log, handlers = {} }) {
    Object.assign(this, { context, client, model, review, log, handlers });
    this.modes = new Map(Object.entries(context.workspaceState.get('overseer.head.modes', {})));
    this.runId = undefined; this.root = undefined; this.workspaceId = undefined;
    this.files = []; this.changed = new Map(); this.base = undefined;
    this.baseTexts = new Map(); // `${base}\0${rel}` -> text
    this.annotations = new Map(); // abs path -> { added, changed, removed }

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
    context.subscriptions.push(this.status, this.decorationsEmitter, ...Object.values(this.types),
      vscode.window.registerFileDecorationProvider({ onDidChangeFileDecorations: this.decorationsEmitter.event, provideFileDecoration: uri => this.fileDecoration(uri) }),
      vscode.window.onDidChangeVisibleTextEditors(editors => { for (const e of editors) this.annotate(e); this.updateStatus(); }),
      vscode.window.onDidChangeActiveTextEditor(e => { this.updateContext(e); this.updateStatus(); }),
      vscode.workspace.onDidChangeTextDocument(e => this.onDocumentChange(e)),
      vscode.workspace.onDidSaveTextDocument(doc => { if (this.root && within(this.root, doc.uri.fsPath)) this.refreshSoon(); }),
      vscode.window.tabGroups.onDidChangeTabs(() => this.updateStatus()));
    client.on('event', event => this.onEvent(event));
    this.updateContext(vscode.window.activeTextEditor);
  }

  // ------------------------------------------------------------ modes

  defaultMode() { return vscode.workspace.getConfiguration('overseer').get('agent.openIn', 'follow') === 'diffs' ? 'diffs' : 'follow'; }
  modeFor(runId) { return this.modes.get(runId) || this.defaultMode(); }

  /**
   * Switches an agent between Follow and Diffs only. Both are the same review in the middle, so the
   * review stays on screen: it changes what it shows and its list (All files or Changed).
   */
  async setMode(runId, mode) {
    runId = runId || this.runId;
    if (!runId || !['follow', 'diffs'].includes(mode)) return;
    this.modes.set(runId, mode);
    this.context.workspaceState.update('overseer.head.modes', Object.fromEntries([...this.modes].slice(-300)));
    this.updateContext(vscode.window.activeTextEditor);
    const found = this.review.manager.panelFor(runId);
    // Not on screen (the chat alone): the arrangement brings the review in, in the new mode.
    if (!found) { if (this.handlers.ensureShown) await this.handlers.ensureShown(runId); else await this.review.open(runId); }
    else {
      this.review.manager.postOverseer(found.session);
      if (mode === 'follow') await this.review.followInit(runId);
    }
    this.log(`head: ${runId} in ${mode === 'diffs' ? 'Diffs only' : 'Follow'}`);
    this.updateStatus();
  }

  toggleMode(runId) {
    runId = runId || this.runId;
    if (runId) return this.setMode(runId, this.modeFor(runId) === 'follow' ? 'diffs' : 'follow');
  }

  // ------------------------------------------------------------ Follow and Manual edit (AC-252)

  /** Remembers an agent's mode without showing anything (the caller opens it). */
  remember(runId, mode) {
    this.modes.set(runId, mode);
    this.context.workspaceState.update('overseer.head.modes', Object.fromEntries([...this.modes].slice(-300)));
  }

  /** True while the owner edits a file of the agent's worktree in a VS Code editor (Manual edit). */
  inManualEdit() {
    const e = vscode.window.activeTextEditor;
    return !!(e && this.root && e.document.uri.scheme === 'file' && within(this.root, e.document.uri.fsPath));
  }

  /**
   * Follow: the agent's review in Follow, in front (from Manual edit, back to the review where the
   * editor was). Opened when it is not on screen.
   */
  async follow(runId) {
    runId = runId || this.runId;
    if (!runId) return;
    this.remember(runId, 'follow');
    this.updateContext(vscode.window.activeTextEditor);
    const found = this.review.manager.panelFor(runId);
    if (!found) { if (this.handlers.ensureShown) await this.handlers.ensureShown(runId); else await this.review.open(runId); }
    else { this.review.manager.postOverseer(found.session); await this.review.followAgain(runId); }
    await this.focus(runId);
    this.log(`head: ${runId} in Follow`);
    this.updateStatus();
  }

  /**
   * Manual edit: the file Follow shows (else the one the agent was in last, else its first change)
   * opens as a VS Code editor where the review is, at the same line, the agent's changes marked;
   * saves land in the agent's worktree. Follow goes on behind it.
   */
  async manualEdit(runId) {
    runId = runId || this.runId;
    if (!runId) { vscode.window.showInformationMessage('No agent to edit: open one first (⌥⌘A).'); return; }
    if (this.runId !== runId || !this.root) await this.select(runId);
    if (!this.root) { vscode.window.showInformationMessage('That agent\'s worktree was removed: there is nothing to edit.'); return; }
    const found = this.review.manager.panelFor(runId);
    const shown = found?.session.followShown?.runId === runId && found.session.followShown.path ? found.session.followShown
      : (this.review.lastFile?.get(runId) || await this.defaultTarget(runId).catch(() => undefined));
    if (!shown?.path) { vscode.window.showInformationMessage('The agent has not opened a file yet: nothing to edit.'); return; }
    const line = Math.max(0, Number(shown.line || 1) - 1);
    const editor = await vscode.window.showTextDocument(vscode.Uri.file(path.join(this.root, shown.path)), {
      viewColumn: found?.panel.viewColumn || vscode.ViewColumn.Active, preview: false, preserveFocus: false, selection: new vscode.Range(line, 0, line, 0) });
    editor.revealRange(new vscode.Range(line, 0, line, 0), vscode.TextEditorRevealType.InCenterIfOutsideViewport);
    this.log(`head: ${runId} in Manual edit (${shown.path}:${line + 1})`);
    this.updateStatus();
  }

  /** One key for both (⌥⌘E): in Manual edit, back to Follow; anywhere else, Manual edit. */
  toggleManualEdit(runId) {
    if (this.inManualEdit() && (!runId || runId === this.runId)) return this.follow(this.runId);
    return this.manualEdit(runId || this.runId);
  }

  // ------------------------------------------------------------ the agent shown

  rootOf(runId) {
    const run = runId && this.model.run(runId);
    const ws = run && this.model.workspace((this.model.rootRun(run) || run).workspace_id);
    return ws && !ws.removed_ms ? ws.path : undefined;
  }

  /** The agent the head is about: its worktree is read (the side bar's selection; no editor opens). */
  async select(runId) {
    const run = runId && this.model.run(runId);
    const root = run && (this.model.rootRun(run) || run);
    const ws = root && this.model.workspace(root.workspace_id);
    if (!root) return;
    if (!ws || ws.removed_ms) {
      // Its worktree is gone: the view says so rather than showing another agent's files.
      this.runId = root.id; this.root = undefined; this.files = []; this.changed = new Map();
      this.onSelect?.(root);
      return;
    }
    if (this.runId === root.id && this.root === ws.path) { this.refreshSoon(); return; }
    this.runId = root.id; this.root = ws.path; this.workspaceId = ws.id;
    this.files = []; this.changed = new Map(); this.base = undefined;
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

  // ------------------------------------------------------------ Follow's file

  async firstChangedLine(rel) {
    const abs = path.join(this.root, rel);
    const text = await fsp.readFile(abs, 'utf8').catch(() => undefined);
    if (text === undefined) return 1;
    const hunks = diffLines(await this.baseText(rel), text);
    return hunks.length ? hunks[0].modStart + 1 : 1;
  }

  /**
   * What Follow shows before the agent has touched a file this session: its first changed file at
   * the first change, else its README, else its first file.
   */
  async defaultTarget(runId) {
    await this.select(runId);
    await this.loading?.catch(() => {});
    if (!this.root || this.runId !== runId) return undefined;
    const first = [...this.changed].find(([, s]) => s !== 'D')?.[0];
    if (first) return { path: first, line: await this.firstChangedLine(first), attribution: 'its first changed file' };
    const readme = this.files.find(f => README.test(f)) || this.files.find(f => README.test(path.basename(f))) || this.files[0];
    return readme ? { path: readme, line: 1, attribution: 'no edits yet' } : undefined;
  }

  /** Brings the agent's review forward (back to the agent, AC-257). Returns false when none is open. */
  async focus(runId = this.runId) {
    const found = runId && this.review.manager.panelFor(runId);
    if (!found) return false;
    found.panel.reveal(found.panel.viewColumn, false);
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
    vscode.commands.executeCommand('setContext', 'overseer.headMode', this.runId ? (inHead ? 'manual' : this.modeFor(this.runId)) : '');
  }



  updateStatus() {
    const runId = this.runId;
    const title = this.model.run(runId)?.title || 'the agent';
    if (runId && this.inManualEdit()) {
      this.status.text = '$(edit) Manual edit';
      this.status.tooltip = `Editing ${title}'s worktree yourself; saves land there. Click to follow it again (⌥⌘E).`;
      this.status.command = 'overseer.head.toggleManualEdit';
      this.status.accessibilityInformation = { label: `Manual edit: ${title}` };
      this.status.show();
      return;
    }
    if (!runId || !this.review.manager.panelFor(runId)) { this.status.hide(); return; }
    const mode = this.modeFor(runId);
    this.status.text = mode === 'follow' ? '$(eye) Follow' : '$(diff-multiple) Diffs only';
    this.status.tooltip = mode === 'follow' ? `Following ${title}: the review shows the file it is in, live. Click for Diffs only.` : `${title}: Diffs only (the review of what changed). Click to follow it.`;
    this.status.command = 'overseer.head.toggleMode';
    // Read aloud (and by the scenarios): which agent the review is about.
    this.status.accessibilityInformation = { label: mode === 'follow' ? `Follow: ${title}` : `Diffs only: ${title}` };
    this.status.show();
  }
}

module.exports = { AgentHead };
