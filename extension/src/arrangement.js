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
    if (this.current !== 'split') { await vscode.commands.executeCommand('vscode.setEditorLayout', SPLIT); this.chatShare = SPLIT.groups[1].size; }
    // Review first, then move the chat: VS Code closes a group the moment it is empty.
    await this.review.open(runId, { viewColumn: vscode.ViewColumn.One, preserveFocus: true, follow });
    await this.center.open({ column: vscode.ViewColumn.Two, preserveFocus: true });
    await this.keepChat();
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

  /** A moved editor becomes a preview tab, which the next opened file would replace; keep the chat. */
  async keepChat() {
    const chatTab = () => vscode.window.tabGroups.all.flatMap(g => g.tabs).find(t => t.input?.viewType?.endsWith('overseer.center'));
    // reveal() moves the panel asynchronously; wait until it sits in the chat's column.
    for (let i = 0; i < 25 && chatTab()?.group.viewColumn !== vscode.ViewColumn.Two; i++) await new Promise(r => setTimeout(r, 20));
    const tab = chatTab();
    // (The tab API does not report preview for webview tabs, so keep it whenever it moved.)
    if (!tab || tab.group.viewColumn !== vscode.ViewColumn.Two) return;
    const active = vscode.window.activeTextEditor;
    this.center.panel?.reveal(vscode.ViewColumn.Two, false);
    for (let i = 0; i < 75 && !this.center.panel?.active; i++) await new Promise(r => setTimeout(r, 20));
    await vscode.commands.executeCommand('workbench.action.keepEditor');
    // Hand focus back to where it was (the side bar or the review).
    if (active) await vscode.window.showTextDocument(active.document, { viewColumn: active.viewColumn, preserveFocus: false }).then(undefined, () => {});
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

  async leaveGrid() {
    if (this.current !== 'grid') return;
    this.current = 'chat';
    if (this.beforeGrid === 'split' && this.runId) await this.split(this.runId);
    else await this.chatOnly();
  }

  async closeReviews(keepRunId) {
    this.quiet++;
    try {
      for (const [session, panel] of [...this.review.manager.panels]) if (session.overseer?.runId !== keepRunId) panel.dispose();
    } finally { this.quiet--; }
  }

  onReviewClosed(runId) {
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
    if (!['file_activity', 'turn_done'].includes(event.kind)) return;
    const run = this.model.run(event.run_id);
    const root = run && this.model.rootRun(run);
    if (!root || root.id !== this.runId) return;
    // The agent's first edit brings the review forward (AC-73).
    if (await this.changes(root) > 0 && this.current === 'chat' && this.runId === root.id) await this.split(root.id, { follow: true });
  }

  persist() {
    this.context.workspaceState.update('overseer.arrangementState', { current: this.current, runId: this.runId });
  }
}

module.exports = { Arrangement };
