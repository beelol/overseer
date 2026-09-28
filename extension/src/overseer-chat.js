// Talk to Overseer (AC-107), docked in the panel under the editor area. Since Gate S (AC-181) the
// conversation lives in the daemon: `overseer.session` holds it, `overseer.send` carries the owner's
// words into a turn of Overseer's own run (hidden from every agents list), and a proposal is a
// card the daemon emits; its Yes or No goes back through `overseer.answer`, once. This view only
// shows the run's feed and relays what is typed; it keeps no state of its own.
const vscode = require('vscode');
const { page, localRoots } = require('./webview-html');

class OverseerChat {
  constructor({ context, client, model, outputs, setPinned, log }) {
    Object.assign(this, { context, client, model, outputs, setPinned, log });
    this.session = undefined;
    // The panel hooks: what is typed in Overseer's chat goes to the daemon's session, and a
    // proposal answered in it goes back as that proposal's one answer.
    outputs.onMessage = (runId, message) => this.intercept(runId, message);
    client.on('event', event => this.onEvent(event));
    client.on('connected', () => this.load().catch(() => {}));
  }

  get harness() { return vscode.workspace.getConfiguration('overseer').get('chat.harness', 'claude'); }
  /** The model for Overseer's own turns (empty: the harness's default). */
  get chatModel() { return vscode.workspace.getConfiguration('overseer').get('chat.model', '') || undefined; }

  /** The daemon's current session; its task is hidden from the agents list. */
  async load() {
    this.session = await this.client.request('overseer.session');
    if (this.session?.task_id) this.model.hide(this.session.task_id);
    return this.session;
  }

  /** The WebviewView in the panel ("Talk to Overseer"). */
  resolveWebviewView(view) {
    this.view = view;
    view.webview.options = { enableScripts: true, localResourceRoots: localRoots(this.context.extensionUri) };
    view.onDidDispose(() => { if (this.view === view) this.view = undefined; });
    this.load().catch(() => undefined).then(async () => {
      if (this.session?.run_id && !this.model.run(this.session.run_id)) await this.model.refresh();
      if (this.session?.run_id && this.model.run(this.session.run_id)) await this.outputs.attach(this.session.run_id, view);
      else this.intro(view);
    });
  }

  /** Before the first message: a composer that starts the conversation. */
  intro(view) {
    view.webview.html = page(view.webview, this.context.extensionUri, { title: 'Talk to Overseer', css: ['run-panel.css'],
      body: `<main class="talk-intro"><h2>Talk to Overseer</h2><p class="note">Ask what your agents are doing, or tell one of them what to do next. Overseer says what it will do and waits for your yes.</p>
<form id="first"><textarea id="prompt" rows="2" aria-label="Message to Overseer" placeholder="What is everyone doing?"></textarea><button class="btn primary" id="send" type="submit">Send</button></form><p class="note" id="why"></p></main>`,
      script: `const api = acquireVsCodeApi(); document.getElementById('first').addEventListener('submit', e => { e.preventDefault(); const t = document.getElementById('prompt').value.trim(); if (t) { document.getElementById('send').disabled = true; api.postMessage({ type: 'first', text: t }); } });
document.getElementById('prompt').addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); document.getElementById('first').requestSubmit(); } });
window.addEventListener('message', e => { if (e.data.type === 'notice') { document.getElementById('why').textContent = e.data.message; document.getElementById('send').disabled = false; } });` });
    view.webview.onDidReceiveMessage(m => { if (m?.type === 'first') this.start(String(m.text || '')).catch(error => view.webview.postMessage({ type: 'notice', message: error.message })); });
  }

  /** The first message: the daemon starts Overseer's run; this view then shows its feed. */
  async start(text) {
    if (!text) return;
    await this.model.refresh();
    await this.client.request('overseer.send', { text, surface: 'vscode', harness: this.harness, ...(this.chatModel ? { model: this.chatModel } : {}) });
    await this.load();
    await this.model.refresh();
    if (this.view && this.session?.run_id) await this.outputs.attach(this.session.run_id, this.view);
  }

  /** Messages from Overseer's own chat: typed words go through the session, Yes and No through the answer. */
  async intercept(runId, message) {
    if (!this.session || runId !== this.session.run_id) return false;
    if (message?.type === 'followUp' || message?.type === 'steer') {
      const text = String(message.text || '').trim();
      if (text) await this.client.request('overseer.send', { text, surface: 'vscode', harness: this.harness, ...(this.chatModel ? { model: this.chatModel } : {}) });
      this.model.scheduleRefresh();
      return true;
    }
    if (message?.type === 'overseerAnswer') {
      try {
        const r = await this.client.request('overseer.answer', { id: String(message.id || ''), yes: !!message.yes, surface: 'vscode', by: 'owner' });
        this.log('overseer chat: ' + r.result);
      } catch (error) {
        this.view?.webview.postMessage({ type: 'proposalStatus', id: message.id, text: error.message });
        this.log('overseer chat: ' + error.message);
      }
      this.model.scheduleRefresh();
      return true;
    }
    return false;
  }

  /** Actions the daemon carried out that the UI performs: pinning an agent to the grid. */
  onEvent(event) {
    if (event?.kind === 'overseer_action' && event.payload?.action === 'pin' && event.run_id) this.setPinned(event.run_id, true);
  }
}

module.exports = { OverseerChat, FROM: 'From Overseer: ', OPEN: '<overseer-state>', CLOSE: '</overseer-state>' };
