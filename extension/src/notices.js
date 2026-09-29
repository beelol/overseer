// You hear about it outside VS Code (AC-240). The daemon posts a Mac notification when an agent
// needs the owner, finishes or fails while no VS Code window has the OS focus. This window tells it
// whether it has the focus (`ui.window`) and which kinds the owner wants (`notices.set`, from the
// overseer.notifications.* settings), once per connection and on every change.
const vscode = require('vscode');

/** The settings, as the daemon's kinds (Needs you covers a permission and a question). */
function kindsFrom(config) {
  const on = key => config.get(`notifications.${key}`, true) !== false;
  return [...(on('needsYou') ? ['permission', 'question'] : []), ...(on('failed') ? ['failure'] : []), ...(on('finished') ? ['finished'] : [])];
}

class Notices {
  constructor(context, client, { say = () => {} } = {}) {
    this.client = client; this.say = say;
    this.sentFocus = undefined; this.sentKinds = undefined;
    context.subscriptions.push(
      vscode.window.onDidChangeWindowState(() => this.focus()),
      vscode.workspace.onDidChangeConfiguration(e => { if (e.affectsConfiguration('overseer.notifications')) this.kinds(); }));
    // The daemon remembers the focus per connection: say both again on a new one.
    client.on('connected', () => { this.sentFocus = undefined; this.sentKinds = undefined; this.focus(); this.kinds(); });
  }

  focus() {
    if (!this.client.connected) return;
    const focused = !!vscode.window.state.focused;
    if (this.sentFocus === focused) return;
    this.sentFocus = focused;
    this.client.request('ui.window', { focused }).catch(error => { this.sentFocus = undefined; this.quiet(error, 'ui.window'); });
  }

  kinds() {
    if (!this.client.connected) return;
    const kinds = kindsFrom(vscode.workspace.getConfiguration('overseer'));
    const key = kinds.join(',');
    if (this.sentKinds === key) return;
    this.sentKinds = key;
    this.client.request('notices.set', { kinds }).catch(error => { this.sentKinds = undefined; this.quiet(error, 'notices.set'); });
  }

  /** A daemon from before these methods has nothing to be told. */
  quiet(error, what) {
    if (!(error.code === 'unknown_method' || /unknown method/i.test(error.message))) this.say(`${what}: ${error.message}`);
  }
}

/** The agent a notification's click opens: vscode://beelol.overseer/open-agent?run=<id>. */
function agentFromUri(uri) {
  if (uri.path !== '/open-agent') return undefined;
  const run = new URLSearchParams(uri.query || '').get('run');
  return run && /^[A-Za-z0-9_-]{1,80}$/.test(run) ? run : undefined;
}

module.exports = { Notices, kindsFrom, agentFromUri };
