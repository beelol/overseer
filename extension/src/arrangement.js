// Gate K arrangement of the editor area (AC-72, AC-73, AC-79, AC-80). With nothing to review the
// selected agent's chat (or the new-agent composer) is alone in the middle. Once the agent has
// changes, the editable review opens on the left (about two thirds) and the chat moves to the
// right. Closing the review puts the chat back in the middle and keeps that choice until a review
// is opened again. Built from editor groups; no setting is written.
const vscode = require('vscode');

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
    const split = reviewable && (force || (this.preference === 'auto' && await this.changes(root) > 0));
    if (split) await this.split(root.id, { follow });
    else await this.chatOnly();
  }

  /** The chat (or composer) alone in the middle. */
  async chatOnly() {
    await this.closeReviews();
    if (this.current !== 'chat' && this.current !== 'grid') await vscode.commands.executeCommand('vscode.setEditorLayout', SINGLE);
    await this.center.open({ column: vscode.ViewColumn.One });
    this.current = 'chat';
    this.persist();
  }

  /** The review on the left, the chat on the right. */
  async split(runId, { follow } = {}) {
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

  /** The chat keeps at least 360 px beside the review (AC-77): widen its column on small windows. */
  async fitChat() {
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
    // The agent's head in Follow (AC-233): its files close too, remembering where the owner was.
    this.review.head?.close(keepRunId);
    this.quiet++;
    try {
      for (const [session, panel] of [...this.review.manager.panels]) if (session.overseer?.runId !== keepRunId) panel.dispose();
    } finally { this.quiet--; }
  }

  onReviewClosed(runId) {
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
    if (!this.runId || this.current !== 'chat' || this.preference !== 'auto') return;
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
}

module.exports = { Arrangement };
