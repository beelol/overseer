// Gate K arrangement of the editor area (AC-72, AC-73, AC-79, AC-80). With nothing to review the
// selected agent's chat (or the new-agent composer) is alone in the middle. Once the agent has
// changes, the editable review opens on the left (about two thirds) and the chat moves to the
// right. Closing the review puts the chat back in the middle and keeps that choice until a review
// is opened again. Built from editor groups; no setting is written.
//
// When the owner has split the editor area themselves, Overseer opens beside their groups and never
// rebuilds the layout (AC-244). The Overseer workspace (AC-250) is three columns: Overseer's
// conversation, the agent's review (Follow) and the agent's chat. The review can be popped out into
// its own window, VS Code's floating editor windows, to follow the agent on another screen (AC-251).
const vscode = require('vscode');
const { isOurs, besideOwner } = require('./layout');

const SPLIT = { orientation: 0, groups: [{ size: 0.66 }, { size: 0.34 }] };
const SINGLE = { orientation: 0, groups: [{}] };

class Arrangement {
  constructor({ context, center, review, model, client, log }) {
    Object.assign(this, { context, center, review, model, client, log });
    // 'auto': the review comes forward when the agent has changes. 'chat': the user closed it.
    this.preference = context.workspaceState.get('overseer.arrangement', 'auto');
    this.current = undefined; // 'chat' | 'split' | 'grid'
    this.runId = undefined;
    this.changed = new Map(); // root run id -> changed file count
    this.editing = new Map(); // root run id -> tool id of an edit announced before it was written
    this.lastTool = new Map(); // run id -> the id of its latest tool call (a file edit names its file right after)
    this.quiet = 0; // > 0 while Overseer itself closes reviews
    this.popped = undefined; // { runId, group }: the review popped out into its own window (AC-251)
    this.workspaceChats = new Set(); // agents' chats the workspace opened in its third column
    client.on('event', event => this.onEvent(event).catch(error => log('arrangement: ' + error.message)));
    review.onClosed = runId => this.onReviewClosed(runId);
  }

  setPreference(value) {
    this.preference = value;
    this.context.workspaceState.update('overseer.arrangement', value);
  }

  async changes(run) {
    try {
      const c = await this.client.request('workspace.changes', { workspace_id: run.workspace_id });
      this.changed.set(run.id, c.files || 0);
    } catch { this.changed.set(run.id, 0); }
    return this.changed.get(run.id);
  }

  /** Shows the agent: the chat alone, or the review beside the chat when it has changes. */
  async show(runId, { follow, force } = {}) {
    const run = this.model.run(runId);
    if (!run) return;
    const root = this.model.rootRun(run) || run;
    this.runId = root.id;
    const ws = this.model.workspace(root.workspace_id);
    const reviewable = ws && !ws.removed_ms;
    if (this.current === 'workspace') { await this.workspace(root.id, { follow }); return; }
    // The review is in its own window: that window follows the agent now shown (AC-251).
    if (this.popped) { await this.chatOnly(); if (reviewable) await this.popTo(root.id, { follow }); return; }
    const split = reviewable && (force || (this.preference === 'auto' && await this.changes(root) > 0));
    if (split) await this.split(root.id, { follow });
    else await this.chatOnly();
  }

  /** The main window's editor groups (a review popped out into its own window is not one of them). */
  mainGroups() {
    return vscode.window.tabGroups.all.filter(g => !(this.popped && g === this.popped.group));
  }

  /** AC-244: the owner split the editor area; Overseer places its views beside their groups. */
  beside() { return besideOwner(this.mainGroups()); }

  /** The column for a new Overseer view beside the owner's groups: a new group on the right. */
  besideColumn() {
    const cols = this.mainGroups().map(g => g.viewColumn);
    return Math.max(0, ...cols) + 1;
  }

  /** The chat (or composer) alone in the middle. */
  async chatOnly() {
    if (this.current === 'workspace') { await this.center.open({ column: vscode.ViewColumn.One }); return; }
    await this.closeReviews(this.popped?.runId);
    if (this.popped || this.beside()) {
      // Beside the owner's groups (or with the review in its own window): no layout is rebuilt.
      const column = this.center.panel?.viewColumn || this.besideColumn();
      await this.center.open({ column });
      this.current = 'chat';
      this.persist();
      return;
    }
    if (this.current !== 'chat' && this.current !== 'grid') await vscode.commands.executeCommand('vscode.setEditorLayout', SINGLE);
    await this.center.open({ column: vscode.ViewColumn.One });
    this.current = 'chat';
    this.persist();
  }

  /** The review on the left, the chat on the right. */
  async split(runId, { follow } = {}) {
    if (this.current === 'workspace') { await this.workspace(runId, { follow }); return; }
    if (this.popped) { await this.chatOnly(); await this.popTo(runId, { follow }); return; }
    if (this.beside()) { await this.splitBeside(runId, { follow }); return; }
    await this.closeReviews(runId);
    // Review first (in the chat's group when the chat is alone), then move the chat right: a moved
    // editor keeps its pinned tab, while reveal() into another column would reopen it as a preview.
    await this.review.open(runId, { viewColumn: vscode.ViewColumn.One, preserveFocus: true, follow });
    await this.moveChatRight();
    if (this.current !== 'split') { await vscode.commands.executeCommand('vscode.setEditorLayout', SPLIT); this.chatShare = SPLIT.groups[1].size; }
    await this.fitChat();
    this.current = 'split';
    this.persist();
  }

  /** AC-244: the review and the chat side by side, right of the owner's groups; their groups stay. */
  async splitBeside(runId, { follow } = {}) {
    await this.closeReviews(runId);
    const chatTab = () => vscode.window.tabGroups.all.flatMap(g => g.tabs).find(t => t.input?.viewType?.endsWith('overseer.center'));
    // The chat's own group (only Overseer's views in it), or a new group on the right.
    let group = chatTab()?.group;
    if (!group || group.tabs.some(t => !isOurs(t))) { await this.center.open({ column: this.besideColumn(), preserveFocus: true }); group = chatTab()?.group; }
    const column = group?.viewColumn || this.besideColumn();
    await this.review.open(runId, { viewColumn: column, preserveFocus: true, follow });
    // The chat moves to a group of its own on the review's right (a moved tab stays pinned).
    const reviewTab = () => vscode.window.tabGroups.all.flatMap(g => g.tabs).find(t => t.input?.viewType?.endsWith('overseer.review'));
    if (this.center.panel && chatTab()?.group === reviewTab()?.group) {
      this.center.panel.reveal(chatTab().group.viewColumn, false);
      for (let i = 0; i < 50 && !(this.center.panel?.active); i++) await new Promise(r => setTimeout(r, 10));
      await vscode.commands.executeCommand('workbench.action.moveEditorToRightGroup');
    }
    this.current = 'split';
    this.persist();
  }

  /** The chat keeps at least 360 px beside the review (AC-77): widen its column on small windows. */
  async fitChat() {
    if (this.beside() || this.popped) return;
    const size = await this.center.measure?.();
    if (!size || !size.w) return;
    // About a third, but never under 360 px (up to half the editor area on small windows).
    const current = this.chatShare || SPLIT.groups[1].size;
    const editorWidth = size.w / current;
    const share = Math.max(SPLIT.groups[1].size, Math.min(0.5, 372 / editorWidth));
    if (Math.abs(share - current) < 0.02) return;
    await vscode.commands.executeCommand('vscode.setEditorLayout', { orientation: 0, groups: [{ size: 1 - share }, { size: share }] });
    this.chatShare = share;
  }

  /** Puts the chat in the second column (moving its tab, so it stays pinned) and focuses the review. */
  async moveChatRight() {
    const chatTab = () => vscode.window.tabGroups.all.flatMap(g => g.tabs).find(t => t.input?.viewType?.endsWith('overseer.center'));
    const panel = this.center.panel;
    if (!panel) { await this.center.open({ column: vscode.ViewColumn.Two, preserveFocus: true }); return; }
    // Someone typing in the chat (a new agent's first edit arrives) keeps typing there.
    const chatHadFocus = panel.active && vscode.window.tabGroups.activeTabGroup.activeTab?.input?.viewType?.endsWith('overseer.center');
    if (chatTab()?.group.viewColumn !== vscode.ViewColumn.Two) {
      panel.reveal(chatTab()?.group.viewColumn ?? vscode.ViewColumn.One, false);
      for (let i = 0; i < 50 && !(panel.active && vscode.window.tabGroups.activeTabGroup.activeTab?.input?.viewType?.endsWith('overseer.center')); i++) await new Promise(r => setTimeout(r, 10));
      await vscode.commands.executeCommand('workbench.action.moveEditorToRightGroup');
    }
    // Otherwise focus goes to the review's group; a text editor left open there stays behind the review.
    if (chatHadFocus) { this.center.panel?.reveal(vscode.ViewColumn.Two, false); this.center.focus?.('chat'); }
    else await vscode.commands.executeCommand('workbench.action.focusFirstEditorGroup');
  }

  /** The grid takes the editor area; leaving it returns to the arrangement before (AC-79). */
  async enterGrid() {
    if (this.current === 'grid') return;
    this.beforeGrid = this.current;
    await this.closeReviews();
    await vscode.commands.executeCommand('vscode.setEditorLayout', SINGLE);
    await this.center.open({ column: vscode.ViewColumn.One });
    this.current = 'grid';
  }

  /** AC-105: clicking a tile tracks its agent: the review opens beside the grid, following it. */
  async track(runId) {
    if (this.current !== 'grid' && this.current !== 'grid-track') return;
    const run = this.model.run(runId);
    if (!run) return;
    const root = this.model.rootRun(run) || run;
    const first = this.current === 'grid';
    this.tracked = root.id;
    await this.closeReviews(root.id);
    await this.review.open(root.id, { viewColumn: vscode.ViewColumn.Two, preserveFocus: true, follow: true });
    if (first) await vscode.commands.executeCommand('vscode.setEditorLayout', { orientation: 0, groups: [{ size: 0.6 }, { size: 0.4 }] });
    this.current = 'grid-track';
    this.center.panel?.webview.postMessage({ type: 'tracked', runId: root.id });
  }

  /** Back to the grid alone (Escape or the grid's control); the grid's layout is untouched. */
  async untrack() {
    if (this.current !== 'grid-track') return;
    this.tracked = undefined;
    this.current = 'grid';
    await this.closeReviews();
    await vscode.commands.executeCommand('vscode.setEditorLayout', SINGLE);
    this.center.panel?.reveal(vscode.ViewColumn.One, false);
    this.center.panel?.webview.postMessage({ type: 'tracked', runId: null });
  }

  async leaveGrid() {
    if (this.current === 'grid-track') { this.tracked = undefined; await this.closeReviews(); this.current = 'grid'; }
    if (this.current !== 'grid') return;
    this.current = 'chat';
    if (this.beforeGrid === 'split' && this.runId) await this.split(this.runId);
    else await this.chatOnly();
  }

  async closeReviews(keepRunId) {
    this.quiet++;
    try {
      for (const [session, panel] of [...this.review.manager.panels]) {
        if (session.overseer?.runId === keepRunId) continue;
        // The review in its own window stays there; only another review replaces it (popTo).
        if (this.popped && session.overseer?.runId === this.popped.runId) continue;
        panel.dispose();
      }
    } finally { this.quiet--; }
  }

  onReviewClosed(runId) {
    // AC-251: the popped-out review's window was closed (or its tab): the review comes back to the
    // main window beside the chat.
    if (!this.quiet && this.popped && runId === this.popped.runId) {
      this.popped = undefined;
      this.log(`arrangement: the review of ${runId} left its own window; back beside the chat`);
      if (this.current === 'workspace') return;
      this.current = 'chat';
      setTimeout(() => { if (this.runId === runId) this.split(runId).catch(error => this.log('arrangement: ' + error.message)); }, 0);
      return;
    }
    // Closing the tracked review is the same as going back to the grid alone.
    if (!this.quiet && this.current === 'grid-track' && runId === this.tracked) {
      this.tracked = undefined; this.current = 'grid';
      this.center.panel?.webview.postMessage({ type: 'tracked', runId: null });
      return;
    }
    if (this.quiet || runId !== this.runId || this.current !== 'split') return;
    // The user closed the review: the chat goes back to the middle and stays there.
    this.setPreference('chat');
    this.current = 'split-closing';
    setTimeout(() => this.chatOnly().catch(error => this.log('arrangement: ' + error.message)), 0);
  }

  /** Opening a review by hand brings the review back for every agent. */
  async openReview(runId, opts = {}) {
    this.setPreference('auto');
    const run = this.model.run(runId);
    if (run) await this.split((this.model.rootRun(run) || run).id, opts);
  }

  async onEvent(event) {
    if (!this.runId || this.current !== 'chat' || this.preference !== 'auto' || this.popped) return;
    if (!['tool', 'file_activity', 'tool_result', 'turn_done'].includes(event.kind)) return;
    const run = this.model.run(event.run_id);
    const root = run && this.model.rootRun(run);
    if (!root || root.id !== this.runId) return;
    if (event.kind === 'tool') { this.lastTool.set(event.run_id, event.payload?.id); return; }
    // The agent's first edit brings the review forward (AC-73), within 500 ms of the write. A
    // harness names the file when it starts the edit (Claude, before its permission request): the
    // review's lookups are done then, and the tool's successful end is the edit, with no second
    // question to the daemon (counting changes takes it a few hundred milliseconds on a loaded Mac).
    if (event.kind === 'tool_result') {
      const edit = this.editing.get(root.id);
      if (!edit || event.payload?.id !== edit || event.payload?.status !== 'completed' || event.payload?.is_error) return;
      this.editing.delete(root.id);
      if (this.current === 'chat' && this.runId === root.id) await this.split(root.id, { follow: true });
      return;
    }
    const [changed] = await Promise.all([this.changes(root), this.review.prepare?.(root.id)]);
    if (changed > 0 && this.current === 'chat' && this.runId === root.id) await this.split(root.id, { follow: true });
    // An edit announced before it is written: its tool's end brings the review.
    else if (event.kind === 'file_activity' && this.lastTool.get(event.run_id)) this.editing.set(root.id, this.lastTool.get(event.run_id));
  }

  persist() {
    this.context.workspaceState.update('overseer.arrangementState', { current: this.current, runId: this.runId });
  }

  // ---- AC-250: the Overseer workspace, three columns ----

  /**
   * Overseer's conversation on the left, the agent's review (following it while it works) in the
   * middle and the agent's chat on the right. `sizes` are the columns' shares (layout.js). With no
   * agent to show, the conversation alone.
   */
  async workspace(runId, { follow, sizes } = {}) {
    if (sizes) this.workspaceSizes = sizes;
    const run = runId && this.model.run(runId);
    const root = run && (this.model.rootRun(run) || run);
    this.current = 'workspace';
    if (root) this.runId = root.id;
    const ws = root && this.model.workspace(root.workspace_id);
    const reviewable = ws && !ws.removed_ms;
    await this.closeReviews(reviewable ? root.id : undefined);
    const shares = this.workspaceSizes || [0.3, 0.4, 0.3];
    const three = reviewable && !this.popped;
    const layout = !root ? SINGLE : three ? { orientation: 0, groups: shares.map(size => ({ size })) } : { orientation: 0, groups: [{ size: 0.4 }, { size: 0.6 }] };
    // vscode.setEditorLayout acts on the active window's editor area: with the review in its own
    // window, the main window's is made active first.
    if (this.popped) await this.center.open({ column: vscode.ViewColumn.One, preserveFocus: false });
    await vscode.commands.executeCommand('vscode.setEditorLayout', layout);
    await this.center.open({ column: vscode.ViewColumn.One, preserveFocus: true });
    this.center.setMode('composer');
    this.center.panel?.webview.postMessage({ type: 'askOverseer', text: '' });
    if (root) {
      const active = ['queued', 'starting', 'running', 'waiting_for_user'].includes(root.status);
      if (three) await this.review.open(root.id, { viewColumn: vscode.ViewColumn.Two, preserveFocus: true, follow: follow ?? (active || undefined) });
      else if (reviewable) await this.popTo(root.id, { follow });
      const chatColumn = three ? vscode.ViewColumn.Three : vscode.ViewColumn.Two;
      for (const id of [...this.workspaceChats]) if (id !== root.id) { this.outputs.panels.get(id)?.panel.dispose(); this.workspaceChats.delete(id); }
      await this.outputs.show(root.id, { viewColumn: chatColumn, preserveFocus: true });
      this.workspaceChats.add(root.id);
    }
    this.persist();
  }

  /** Leaving the workspace: its reviews and chats close; the dashboard puts the owner's layout back. */
  async leaveWorkspace() {
    for (const id of this.workspaceChats) this.outputs.panels.get(id)?.panel.dispose();
    this.workspaceChats.clear();
    // Its review closes wherever it is, its own window included.
    this.popped = undefined;
    await this.closeReviews();
    this.current = undefined;
    this.persist();
  }

  // ---- AC-251: the review (Follow) in its own window ----

  /**
   * Moves the agent's review into its own window with VS Code's "Move Editor into New Window"
   * (auxiliary windows, VS Code 1.85+). The review stays live there; the main window keeps
   * Overseer and the chat. Returns false when VS Code could not move it.
   */
  async popOut(runId, { follow } = {}) {
    const run = this.model.run(runId);
    if (!run) return false;
    const root = this.model.rootRun(run) || run;
    if (this.popped?.runId === root.id) return true;
    if (this.popped) { await this.popTo(root.id, { follow }); return true; }
    let entry = this.review.manager.panelFor(root.id);
    if (!entry) { await this.review.open(root.id, { viewColumn: this.current === 'workspace' ? vscode.ViewColumn.Two : undefined, preserveFocus: true, follow }); entry = this.review.manager.panelFor(root.id); }
    const panel = entry?.panel;
    if (!panel) return false;
    // "Move Editor into New Window" moves the active editor: bring the review forward, focused.
    panel.reveal(panel.viewColumn, false);
    for (let i = 0; i < 60 && !(panel.active && vscode.window.tabGroups.activeTabGroup.activeTab?.input?.viewType?.endsWith('overseer.review')); i++) await new Promise(r => setTimeout(r, 25));
    const reviewGroup = () => vscode.window.tabGroups.all.find(g => g.tabs.some(t => t.input?.viewType?.endsWith('overseer.review')));
    const before = reviewGroup();
    try { await vscode.commands.executeCommand('workbench.action.moveEditorToNewWindow'); } catch (error) { this.log('pop out: ' + error.message); return false; }
    // The review's tab is then in a group of the new window.
    let group;
    for (let i = 0; i < 80 && !group; i++) {
      const g = reviewGroup();
      if (g && g !== before) group = g; else await new Promise(r => setTimeout(r, 50));
    }
    if (!group) return false;
    this.popped = { runId: root.id, group };
    this.runId = root.id;
    this.log(`arrangement: review of ${root.id} popped out (its window's group is column ${group.viewColumn}; main window: ${this.mainGroups().map(g => g.viewColumn).join(',')})`);
    if (this.current === 'split') this.current = 'chat';
    this.persist();
    return true;
  }

  /** The popped-out window shows another agent's review in place of the one it had. */
  async popTo(runId, { follow } = {}) {
    if (!this.popped || this.popped.runId === runId) return;
    const old = this.popped.runId;
    const group = vscode.window.tabGroups.all.includes(this.popped.group) ? this.popped.group : undefined;
    if (!group) { this.popped = undefined; return; }
    await this.review.open(runId, { viewColumn: group.viewColumn, preserveFocus: true, follow });
    this.popped = { runId, group };
    this.quiet++;
    try { for (const [session, panel] of [...this.review.manager.panels]) if (session.overseer?.runId === old) panel.dispose(); } finally { this.quiet--; }
    this.log(`arrangement: the review window now follows ${runId}`);
  }

  /** Brings the popped-out review back into the main window beside the chat (its window closes). */
  async popIn() {
    if (!this.popped) return false;
    const runId = this.popped.runId;
    this.quiet++;
    try { for (const [session, panel] of [...this.review.manager.panels]) if (session.overseer?.runId === runId) panel.dispose(); } finally { this.quiet--; }
    this.popped = undefined;
    if (this.current === 'workspace') await this.workspace(runId);
    else { this.current = 'chat'; await this.split(runId); }
    return true;
  }
}

module.exports = { Arrangement };
