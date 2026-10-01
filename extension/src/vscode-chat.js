// AC-258: VS Code's own chat view stays out of Overseer's way all session. The first time
// Overseer's view is on screen in a window, VS Code's built-in chat view (the "Build with Agent"
// panel in the secondary side bar) is closed, once. Overseer's own actions never reopen it; the owner
// opening it again is left alone.
//
// Extensions cannot read which view a part shows, so this measures Overseer's view (as
// dashboard mode did). VS Code's chat lives in the secondary side bar:
//   - Close Secondary Side Bar; if Overseer's view did not grow, the side bar was closed and there is
//     nothing to do (nothing else moves, nothing takes the keyboard).
//   - If it grew, the side bar was open on some view: it is reopened as it was, then VS Code's own
//     Toggle Chat (workbench.action.chat.toggle) hides the side bar if the chat is what it shows
//     (Overseer's view grows again). If it shows another view, Toggle Chat switches it to the chat
//     (same size): the previous view is brought back and the side bar stays.
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
    await run('workbench.action.closeAuxiliaryBar');
    await settle(300);
    let after = await this.center.measure();
    if (!grew(before, after)) this.result = 'not open';
    else {
      // The side bar was open: put it back as it was, then let Toggle Chat hide it if it shows the chat.
      await run('workbench.action.toggleAuxiliaryBar');
      await settle(300);
      const reopened = await this.center.measure();
      await run('workbench.action.chat.toggle');
      await settle(350);
      after = await this.center.measure();
      if (grew(reopened, after)) this.result = 'closed';
      else if (shrank(reopened, after)) { await run('workbench.action.chat.toggle'); this.result = 'was not shown'; }
      else { await run('workbench.action.previousAuxiliaryBarView'); this.result = 'was not shown'; }
      // Toggle Chat focuses the chat's input when it shows it: the keyboard goes back to Overseer.
      if (this.result !== 'closed' && this.center.panel) this.center.panel.reveal(this.center.panel.viewColumn, false);
    }
    this.log(`vscode chat: ${this.result} (Overseer's view ${before.w}×${before.h} → ${after ? `${after.w}×${after.h}` : '?'})`);
    return this.result;
  }
}

module.exports = { VsCodeChat };
