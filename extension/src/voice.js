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
    this.targeted = new Set(); // agents that open spoken requests are for (a voice mark, AC-169)
    this.heard = '';
    this.asking = false; // a read-back or a plan waits for a yes
    this.askReadBack = false; // a permission read back
    this.askPlan = null; // the request whose plan waits for a yes
    this.listeners = [];
    this.status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 49);
    this.status.command = 'overseer.voice.open';
    context.subscriptions.push(this.status);
    client.on('voice', m => this.live(m));
    // A card's rows advance on its agents' turns (held → sent → delivered → picked up → answered).
    this.cardsOfRun = new Map(); // run id -> proposal ids shown
    client.on('event', ev => {
      if (!ev || !['turn_started', 'turn_done', 'queued', 'status'].includes(ev.kind) || !this.panel) return;
      for (const id of this.cardsOfRun.get(ev.run_id) || []) this.fetchCardSoon(id);
    });
    client.on('connected', () => this.refresh());
    client.on('disconnected', () => { this.voice = null; this.render(); });
    // Reduced motion follows VS Code's own setting as well as the system's.
    context.subscriptions.push(vscode.workspace.onDidChangeConfiguration(e => { if (e.affectsConfiguration('workbench.reduceMotion')) this.refresh(); }));
  }

  /** Called when what the side bar, the grid or home show of Voice Mode changes. */
  onChange(cb) { this.listeners.push(cb); }
  changed(what = 'strip') { for (const cb of this.listeners) { try { cb(what); } catch { /* a view that is gone */ } } }

  /** What home's voice strip shows (AC-174): the state, the words as they are heard, mute. */
  summary() {
    const v = this.voice;
    if (!v || !v.enabled) return { on: false };
    const state = v.state || 'starting';
    return { on: true, state, label: LABEL[state] || state, reason: v.reason || '', heard: this.heard, muted: !!v.muted, target: v.target_title || 'Overseer', asking: this.asking };
  }

  /** Asking while a read-back or a plan waits; each clears on its own (a lapsed read-back does not
   * clear a plan that still waits). */
  updateAsking() { this.setAsking(this.askReadBack || !!this.askPlan); }

  setAsking(on) {
    if (this.asking === on) return;
    this.asking = on;
    vscode.commands.executeCommand('setContext', 'overseer.voiceAsking', on);
    this.changed();
  }

  async refresh() {
    try { this.voice = await this.client.request('voice.get'); } catch { this.voice = null; }
    this.targeted = new Set(this.voice?.targeted || []);
    await this.titleTarget();
    vscode.commands.executeCommand('setContext', 'overseer.voiceSimulated', !!this.voice?.simulated);
    vscode.commands.executeCommand('setContext', 'overseer.voiceOn', !!this.voice?.enabled);
    this.render();
    this.post({ type: 'snapshot', voice: this.voice, reducedMotion: vscode.workspace.getConfiguration('workbench').get('reduceMotion') === 'on' });
    if (this.panel) this.sendRequests();
    this.changed('targets');
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
    if (m.kind === 'state' && this.voice) { this.voice.state = m.state; this.voice.reason = m.reason; if (m.state === 'off') this.voice.enabled = false; this.render(); this.changed(); }
    if (m.kind === 'target' && this.voice) { this.voice.target = m.target; this.titleTarget().then(() => { this.render(); this.changed(); this.post({ type: 'live', msg: { ...m, target_title: this.voice.target_title } }); }); return; }
    if (m.kind === 'targets') { this.targeted = new Set(m.runs || []); this.changed('targets'); }
    if (m.kind === 'heard' || m.kind === 'not_meant') { this.heard = m.text || ''; this.changed(); }
    if (m.kind === 'read_back') { this.askReadBack = !m.lapsed && !!m.agent; this.updateAsking(); }
    if (m.kind === 'confirm' && m.lapsed) { this.askPlan = null; this.updateAsking(); }
    if (m.kind === 'request' && m.request) {
      if (m.request.state === 'waiting') this.askPlan = m.request.id;
      else if (this.askPlan === m.request.id) this.askPlan = null;
      this.updateAsking();
    }
    if (m.kind === 'toast') { if (m.cancel) { this.askReadBack = false; this.updateAsking(); } this.toast(m); }
    if (m.kind === 'open') this.openPlace(m.place);
    if (m.kind === 'download') this.downloadProgress?.(m.progress);
    if (m.kind === 'request' && m.request?.proposal) this.fetchCard(m.request.proposal);
    this.post({ type: 'live', msg: m });
  }

  /** A permission answered by voice (AC-171): the toast with Cancel inside the window, then Sent. */
  async toast(m) {
    if (m.cancel) {
      const pick = await vscode.window.showInformationMessage(m.text, 'Cancel');
      if (pick === 'Cancel') await this.client.request('voice.cancel', { id: m.request }).catch(e => vscode.window.showWarningMessage(`Overseer: ${e.message}`));
    } else {
      vscode.window.showInformationMessage(m.text);
    }
  }

  /** What is not done by voice opens its place in the UI (AC-171). */
  openPlace(place) {
    const run = (cmd, ...args) => vscode.commands.executeCommand(cmd, ...args).then(undefined, () => {});
    if (place === 'accounts') return run('overseer.accounts.focus');
    if (place === 'continuity') return run('overseer.continuity.show');
    if (place === 'cleanup') return run('overseer.agents.focus');
    if (place === 'daemon') return run('workbench.action.quickOpen', '>Overseer: Stop Agents and Daemon');
    if (place === 'phone') return run('workbench.action.openSettings', 'overseer phone');
    if (place === 'rules') return run('workbench.action.openSettings', 'overseer voice');
  }

  /** Yes or no from the keyboard to what was read back (AC-171, AC-174). */
  async answer(yes) {
    await this.client.request('voice.answer', { yes });
    this.askReadBack = false; this.askPlan = null;
    this.updateAsking();
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
    try {
      const card = await this.client.request('overseer.card', { id });
      for (const row of card.rows || []) {
        if (!row.run_id) continue;
        const ids = this.cardsOfRun.get(row.run_id) || new Set();
        ids.add(id); this.cardsOfRun.set(row.run_id, ids);
      }
      this.post({ type: 'card', card });
    } catch { /* not a card yet */ }
  }

  fetchCardSoon(id) {
    this.pendingCards ||= new Set();
    this.pendingCards.add(id);
    clearTimeout(this.cardTimer);
    this.cardTimer = setTimeout(() => { const ids = [...this.pendingCards]; this.pendingCards.clear(); for (const x of ids) this.fetchCard(x); }, 250);
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
    <button id="voice-yes" hidden title="Yes to what Overseer read back (⌥⌘⇧Y)">Yes</button>
    <button id="voice-no" hidden title="No to what Overseer read back (⌥⌘⇧N)">No</button>
    <button id="voice-cancel" hidden title="Cancel the open request (⌥⌘⇧.)">Cancel</button>
    <button id="voice-mute" aria-pressed="false" aria-label="Mute"><i class="codicon codicon-mic" aria-hidden="true"></i></button>
  </header>
  <section class="voice-stage">
    <div class="voice-mark"><canvas id="voice-canvas" role="img" aria-label="The Overseer mark: it moves when Overseer hears you"></canvas><div class="voice-sign" id="voice-sign" hidden></div></div>
    <p class="voice-heard" id="voice-heard" aria-live="polite"></p>
    <p class="voice-said" id="voice-said" aria-live="polite"></p>
    <p class="voice-error" id="voice-error" role="alert" hidden></p>
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
    if (m.type === 'answer') return this.answer(!!m.yes);
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
