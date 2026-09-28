// Voice Mode in VS Code (Gate R, AC-174): the voice view with the mark in the middle, a status bar
// item, toasts for what was answered by voice, and the commands. VS Code never listens and never
// speaks: the daemon's listener does both, and this only shows what the daemon's live channel says.
const vscode = require('vscode');
const { page, localRoots } = require('./webview-html');

const LABEL = { off: 'Voice off', starting: 'Starting', listening: 'Listening', hearing: 'Hearing you', thinking: 'Thinking', speaking: 'Speaking', muted: 'Muted', paused: 'Paused for a call', failed: 'Voice stopped' };
const ICON = { starting: 'loading~spin', listening: 'mic', hearing: 'record', thinking: 'loading~spin', speaking: 'unmute', muted: 'mute', paused: 'debug-pause', failed: 'warning' };

class Voice {
  constructor(context, client, { selectRun } = {}) {
    this.context = context; this.client = client; this.selectRun = selectRun;
    this.voice = null;
    this.status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 49);
    this.status.command = 'overseer.voice.open';
    context.subscriptions.push(this.status);
    client.on('voice', m => this.live(m));
    client.on('connected', () => this.refresh());
    client.on('disconnected', () => { this.voice = null; this.render(); });
    // Reduced motion follows VS Code's own setting as well as the system's.
    context.subscriptions.push(vscode.workspace.onDidChangeConfiguration(e => { if (e.affectsConfiguration('workbench.reduceMotion')) this.refresh(); }));
  }

  async refresh() {
    try { this.voice = await this.client.request('voice.get'); } catch { this.voice = null; }
    await this.titleTarget();
    vscode.commands.executeCommand('setContext', 'overseer.voiceSimulated', !!this.voice?.simulated);
    vscode.commands.executeCommand('setContext', 'overseer.voiceOn', !!this.voice?.enabled);
    this.render();
    this.post({ type: 'snapshot', voice: this.voice, reducedMotion: vscode.workspace.getConfiguration('workbench').get('reduceMotion') === 'on' });
    if (this.panel) this.sendRequests();
  }

  async titleTarget() {
    if (!this.voice) return;
    if (this.voice.target && this.voice.target !== 'overseer') {
      try { const { roster } = await this.client.request('agents.roster'); this.voice.target_title = (roster || []).find(a => a.id === this.voice.target)?.title || this.voice.target; }
      catch { this.voice.target_title = this.voice.target; }
    } else this.voice.target_title = 'Overseer';
  }

  render() {
    const v = this.voice;
    if (!v || !v.enabled) { this.status.hide(); return; }
    const state = v.state || 'starting';
    this.status.text = `$(${ICON[state] || 'mic'}) ${LABEL[state] || state}`;
    this.status.tooltip = `Voice Mode: ${LABEL[state] || state}${v.reason ? ` (${v.reason})` : ''} · talking to ${v.target_title || 'Overseer'}. Click to show.`;
    this.status.accessibilityInformation = { label: `Voice Mode, ${LABEL[state] || state}` };
    this.status.show();
  }

  live(m) {
    if (!m) return;
    if (m.kind === 'state' && this.voice) { this.voice.state = m.state; this.voice.reason = m.reason; if (m.state === 'off') this.voice.enabled = false; this.render(); }
    if (m.kind === 'target' && this.voice) { this.voice.target = m.target; this.titleTarget().then(() => { this.render(); this.post({ type: 'live', msg: { ...m, target_title: this.voice.target_title } }); }); return; }
    if (m.kind === 'toast') this.toast(m);
    if (m.kind === 'download') this.downloadProgress?.(m.progress);
    if (m.kind === 'request' && m.request?.proposal) this.fetchCard(m.request.proposal);
    this.post({ type: 'live', msg: m });
  }

  async toast(m) {
    if (m.cancel) {
      const pick = await vscode.window.showInformationMessage(m.text, 'Cancel');
      if (pick === 'Cancel') await this.client.request('voice.cancel', { id: m.request }).catch(e => vscode.window.showWarningMessage(`Overseer: ${e.message}`));
    } else {
      vscode.window.setStatusBarMessage(`$(check) ${m.text}`, 6000);
    }
  }

  post(m) { this.panel?.webview.postMessage(m); }

  async sendRequests() {
    try {
      const { requests } = await this.client.request('voice.requests', { limit: 12 });
      this.post({ type: 'requests', list: requests });
      for (const r of requests) if (r.proposal) this.fetchCard(r.proposal);
    } catch { /* the daemon is restarting */ }
  }

  async fetchCard(id) {
    try { this.post({ type: 'card', card: await this.client.request('overseer.card', { id }) }); } catch { /* not a card yet */ }
  }

  /** Shows the voice view: the mark in the middle of the editor area. */
  async open() {
    if (this.panel) { this.panel.reveal(); return this.panel; }
    const panel = vscode.window.createWebviewPanel('overseer.voice', 'Voice', { viewColumn: vscode.ViewColumn.Active }, { enableScripts: true, retainContextWhenHidden: true, localResourceRoots: localRoots(this.context.extensionUri) });
    this.panel = panel;
    panel.iconPath = vscode.Uri.joinPath(this.context.extensionUri, 'media', 'overseer-logo.png');
    const uri = name => panel.webview.asWebviewUri(vscode.Uri.joinPath(this.context.extensionUri, 'media', 'voice', name)).toString();
    const layers = JSON.stringify({ core: uri('overseer-logo-core.png'), swooshes: uri('overseer-logo-swooshes.png'), star: uri('overseer-logo-star.png'), flat: uri('overseer-logo-flat.png') }).replace(/'/g, '&#39;');
    const body = `<main class="voice" data-layers='${layers}' data-state="off">
  <header class="voice-strip" role="toolbar" aria-label="Voice Mode">
    <span class="voice-state" id="voice-state" role="status">Off</span>
    <button class="voice-target" id="voice-target" aria-label="Who you are talking to">Overseer</button>
    <span class="grow"></span>
    <button id="voice-cancel" hidden>Cancel</button>
    <button id="voice-mute" aria-pressed="false" aria-label="Mute"><i class="codicon codicon-mic" aria-hidden="true"></i></button>
  </header>
  <section class="voice-stage">
    <div class="voice-mark"><canvas id="voice-canvas" role="img" aria-label="The Overseer mark: it moves when Overseer hears you"></canvas><div class="voice-sign" id="voice-sign" hidden></div></div>
    <p class="voice-heard" id="voice-heard" aria-live="polite"></p>
    <div class="voice-meter" id="voice-meter" hidden aria-hidden="true"><i></i></div>
    <div class="voice-off" id="voice-off" hidden><span>Voice Mode is off.</span><span id="voice-off-reason"></span><button id="voice-on">Turn on</button></div>
  </section>
  <section class="voice-requests" id="voice-requests" aria-label="Spoken requests" hidden></section>
</main>`;
    panel.webview.html = page(panel.webview, this.context.extensionUri, { title: 'Voice', css: ['voice.css'], js: ['voice-mark.js', 'voice.js'], body });
    panel.onDidDispose(() => { if (this.panel === panel) this.panel = undefined; });
    panel.webview.onDidReceiveMessage(m => this.receive(m).catch(e => vscode.window.showErrorMessage(`Overseer: ${e.message}`)));
    return panel;
  }

  async receive(m) {
    if (!m || typeof m !== 'object') return;
    if (m.type === 'ready') return this.refresh();
    if (m.type === 'mute') return this.mute();
    if (m.type === 'toggle') return this.toggle();
    if (m.type === 'target') return this.talkTo();
    if (m.type === 'cancel') return this.client.request('voice.cancel', { id: m.id });
    if (m.type === 'open' && m.run && this.selectRun) return this.selectRun(m.run, { reveal: true });
  }

  /** Turns Voice Mode on or off; the first time, the speech model is downloaded after a yes. */
  async toggle() {
    const v = this.voice || await this.client.request('voice.get');
    if (v.enabled) { await this.client.request('voice.set', { enabled: false }); return this.refresh(); }
    if (!v.available) throw new Error('Voice Mode needs the listener, which ships with Overseer on macOS.');
    if (!v.model.downloaded) {
      const mib = Math.round(v.model.bytes / 1024 / 1024);
      const yes = await vscode.window.showInformationMessage(`Voice Mode turns speech into words on this Mac with a speech model (${v.model.name}, ${mib} MiB), downloaded once and kept here. Nothing you say leaves the Mac as sound.`, { modal: true }, `Download ${mib} MiB`);
      if (!yes) return;
      await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: 'Downloading the speech model', cancellable: false }, progress => new Promise((resolve, reject) => {
        let last = 0;
        this.downloadProgress = p => {
          if (p.state === 'downloading') { const pct = Math.floor(p.received / p.bytes * 100); progress.report({ increment: pct - last, message: `${pct}%` }); last = pct; }
          else if (p.state === 'done') { this.downloadProgress = null; resolve(); }
          else if (p.state === 'failed') { this.downloadProgress = null; reject(new Error(p.reason)); }
        };
        this.client.request('voice.download').then(r => { if (r.downloaded) { this.downloadProgress = null; resolve(); } }, reject);
      }));
    }
    await this.client.request('voice.set', { enabled: true });
    await this.refresh();
    await this.open();
  }

  async mute() {
    const v = this.voice || await this.client.request('voice.get');
    await this.client.request('voice.set', { muted: !v.muted });
    return this.refresh();
  }

  /** Who Overseer's voice goes to: Overseer (the default) or one agent (AC-166). */
  async talkTo() {
    const { roster } = await this.client.request('agents.roster').catch(() => ({ roster: [] }));
    const active = (roster || []).filter(a => ['queued', 'starting', 'running', 'waiting_for_user'].includes(a.status));
    const pick = await vscode.window.showQuickPick([{ label: '$(eye) Overseer', description: 'works out the agents from what you say', target: 'overseer' }, ...active.map(a => ({ label: a.title, description: a.status, target: a.id }))], { title: 'Voice Mode: talk to', placeHolder: 'Who hears what you say' });
    if (!pick) return;
    await this.client.request('voice.set', { target: pick.target });
    return this.refresh();
  }

  async cancel() {
    const { requests } = await this.client.request('voice.requests', { limit: 5 });
    const open = requests.find(r => ['settling', 'thinking', 'taken'].includes(r.state));
    await this.client.request('voice.cancel', { id: open ? open.id : '' });
  }

  /** The simulated voice (development: the daemon started with OVERSEER_VOICE_SIMULATE=1). */
  async simulate() {
    const pick = await vscode.window.showQuickPick([
      { label: 'Say a sentence', detail: 'spoken with a system voice; the words go with it', kind: 'speech' },
      { label: 'A made-up voice', detail: 'two seconds of voice with no words', kind: 'speechlike' },
      { label: 'Overseer says a line', kind: 'speak' },
      ...['typing', 'taps', 'cough', 'door', 'cup', 'music', 'fan'].map(n => ({ label: `Noise: ${n}`, detail: 'must not move the mark', kind: 'noise', noise: n })),
    ], { title: 'Voice Mode: simulate' });
    if (!pick) return;
    if (pick.kind === 'speech') {
      const text = await vscode.window.showInputBox({ prompt: 'What to say', value: 'Tell the phone agent to use the new wire format.' });
      if (text) await this.client.request('voice.simulate', { speech: text });
    } else if (pick.kind === 'speechlike') await this.client.request('voice.simulate', { speechlike: 2, words: '' });
    else if (pick.kind === 'speak') await this.client.request('voice.speak', { text: 'On it: telling Phone and Continuity, and starting one agent for the note.' });
    else await this.client.request('voice.simulate', { noise: pick.noise });
  }
}

module.exports = { Voice };
