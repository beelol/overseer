// AC-258: VS Code's own chat view stays out of Overseer's way all session. The first time
// Overseer's view is on screen in a window, VS Code's built-in chat view (the "Build with Agent"
// panel in the secondary side bar) is closed, once. Overseer's own actions never reopen it (the
// dashboard and the workspace see it closed before they note which parts were open); the owner
// opening it again is left alone.
//
// Extensions cannot read which view a part shows, so this uses VS Code's own "Toggle Chat"
// (workbench.action.chat.toggle), which hides the chat's part when the chat view is visible and
// shows the chat otherwise, and measures Overseer's view around it (as dashboard-mode.js does):
//   - the view grew: the chat was open and is now closed;
//   - it shrank: the chat was not open and Toggle Chat opened it; it is toggled closed again;
//   - same size: the part was open on another view and now shows the chat; the part's previous
//     view is brought back.
// With VS Code's AI features turned off (`chat.disableAIFeatures`) there is no chat view: nothing runs.
const vscode = require('vscode');

const settle = ms => new Promise(r => setTimeout(r, ms));

class VsCodeChat {
  constructor(center, log) { this.center = center; this.log = log; this.done = false; this.running = undefined; }

  get enabled() { return vscode.workspace.getConfiguration('overseer').get('hideVsCodeChat', true); }
  get aiOff() { return vscode.workspace.getConfiguration('chat').get('disableAIFeatures', false) === true; }

  /** Once per window session, when Overseer's view is visible. Safe to call often. */
  check() {
    if (this.done) return Promise.resolve(this.result);
    if (!this.running) this.running = this.run().finally(() => { this.running = undefined; });
    return this.running;
  }

  async run() {
    if (!this.enabled || this.aiOff) { this.done = true; this.result = 'not needed'; return this.result; }
    const panel = this.center.panel;
    if (!panel || !panel.visible) return 'waiting for the Overseer view';
    this.done = true;
    const grew = (a, b) => a && b && (b.w > a.w + 8 || b.h > a.h + 8);
    const shrank = (a, b) => a && b && (b.w < a.w - 8 || b.h < a.h - 8);
    const before = await this.center.measure();
    if (!before) { this.done = false; return 'could not measure'; }
    const run = id => vscode.commands.executeCommand(id).then(() => true, () => false);
    if (!(await run('workbench.action.chat.toggle'))) { this.result = 'no chat view'; return this.result; }
    await settle(350);
    const after = await this.center.measure();
    if (grew(before, after)) this.result = 'closed';
    else if (shrank(before, after)) { await run('workbench.action.chat.toggle'); await settle(250); this.result = 'was not open'; }
    else { await run('workbench.action.previousAuxiliaryBarView'); this.result = 'was not shown'; }
    // Toggle Chat focuses the chat's input when it opens it: the keyboard goes back to Overseer.
    if (this.result !== 'closed' && this.center.panel) this.center.panel.reveal(this.center.panel.viewColumn, false);
    this.log(`vscode chat: ${this.result} (Overseer's view ${before.w}×${before.h} → ${after ? `${after.w}×${after.h}` : '?'})`);
    return this.result;
  }
}

module.exports = { VsCodeChat };
