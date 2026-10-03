// Voice Mode in VS Code (Gate R, AC-174): a status bar item, toasts for what was answered by voice,
// and the commands. Since AC-227 there is no voice view of its own: the conversation with Overseer
// (home, in the Overseer view) turns into the voice view while Voice Mode is on, with the mark on top
// and the same cards below. VS Code never listens and never speaks: the daemon's listener does both,
// and this only passes on what the daemon's live channel says.
const vscode = require('vscode');

const LABEL = { off: 'Voice off', starting: 'Starting', listening: 'Listening', hearing: 'Hearing you', thinking: 'Thinking', speaking: 'Speaking', muted: 'Muted', paused: 'Paused for a call', failed: 'Voice stopped' };
const ICON = { starting: 'loading~spin', listening: 'mic', hearing: 'record', thinking: 'loading~spin', speaking: 'unmute', muted: 'mute', paused: 'debug-pause', failed: 'warning' };

class Voice {
  constructor(context, client, { selectRun, view, showHome, place } = {}) {
    this.context = context; this.client = client; this.selectRun = selectRun;
    // The zero-friction loop's spoken places (AC-252): open Overseer, follow, Manual edit.
    this.place = place || (async () => {});
    // The Overseer view's webview (home is the voice view) and how to bring home forward.
    this.view = view || (() => undefined); this.showHome = showHome || (async () => {});
    this.voice = null;
    this.connectionEpoch = 0;
    this.refreshTicket = 0;
    this.liveRevision = 0;
    this.liveProjection = {};
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
    client.on('connected', () => this.refresh());
    client.on('disconnected', () => {
      this.connectionEpoch++; this.refreshTicket++;
      this.voice = null; this.liveProjection = {};
      this.targeted = new Set(); this.render();
    });
    // Reduced motion follows VS Code's own setting as well as the system's.
    context.subscriptions.push(vscode.workspace.onDidChangeConfiguration(e => { if (e.affectsConfiguration('workbench.reduceMotion')) this.refresh(); }));
  }

  /** Called when what the side bar, the grid or home show of Voice Mode changes. */
  onChange(cb) { this.listeners.push(cb); }
  changed(what = 'strip') { for (const cb of this.listeners) { try { cb(what); } catch { /* a view that is gone */ } } }

  /** What home's voice strip shows (AC-174): the state, the words as they are heard, mute. */
  summary() {
    const v = this.voice;
    // Off after it failed (four listener crashes): the view keeps the stage to say why (AC-175).
    if (!v || !v.enabled) return { on: false, stopped: (v && v.reason) || '' };
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
    const epoch = this.connectionEpoch, ticket = ++this.refreshTicket;
    const revision = this.liveRevision, hadSnapshot = !!this.voice;
    let v;
    try { v = await this.client.request('voice.get'); } catch { return; }
    if (epoch !== this.connectionEpoch || ticket !== this.refreshTicket || !v) return;
    // Keep the complete snapshot's static fields, with dynamic events that arrived
    // while it was in flight (or before the first snapshot on this connection).
    v = { ...v };
    for (const [kind, projection] of Object.entries(this.liveProjection)) {
      if (hadSnapshot && projection.revision <= revision) continue;
      if (kind === 'state') Object.assign(v, projection.value);
      if (kind === 'target') v.target = projection.value;
      if (kind === 'targets') v.targeted = projection.value;
    }
    v.target_title = !v.target || v.target === 'overseer' ? 'Overseer' : v.target;
    this.voice = v;
    this.targeted = new Set(v.targeted || []);
    this.publishSnapshot();
    // Rendering never waits on roster I/O. A late title may enrich only this
    // still-current snapshot and target, and publishes its current live state.
    await this.titleTarget(v, epoch);
  }

  publishSnapshot() {
    vscode.commands.executeCommand('setContext', 'overseer.voiceSimulated', !!this.voice?.simulated);
    vscode.commands.executeCommand('setContext', 'overseer.voiceOn', !!this.voice?.enabled);
    this.render();
    this.post({ type: 'snapshot', voice: this.voice, reducedMotion: vscode.workspace.getConfiguration('workbench').get('reduceMotion') === 'on' });
    if (this.view()) this.sendRequests();
    this.changed('targets');
  }

  async titleTarget(v = this.voice, epoch = this.connectionEpoch) {
    if (!v) return false;
    const target = v.target, targetRevision = this.liveProjection.target?.revision;
    if (!target || target === 'overseer') return false;
    let title = target;
    try { const { roster } = await this.client.request('agents.roster'); title = (roster || []).find(a => a.id === target)?.title || target; }
    catch { /* Keep the captured target as a fallback. */ }
    if (epoch !== this.connectionEpoch || this.voice !== v || v.target !== target || this.liveProjection.target?.revision !== targetRevision) return false;
    if (v.target_title === title) return false;
    v.target_title = title;
    this.publishSnapshot();
    return true;
  }

  render() {
    const v = this.voice;
    if (!v || !v.enabled) { this.status.hide(); return; }
    const state = v.state || 'starting';
    this.status.text = `$(${ICON[state] || 'mic'}) ${LABEL[state] || state}`;
    this.status.tooltip = `Voice Mode: ${LABEL[state] || state}${v.reason ? ` (${v.reason})` : ''} · talking to ${v.target_title || 'Overseer'}. Click to show the conversation.`;
    this.status.accessibilityInformation = { label: `Voice Mode, ${LABEL[state] || state}` };
    this.status.show();
  }

  live(m) {
    if (!m) return;
    if (m.kind === 'state') this.liveProjection.state = { revision: ++this.liveRevision, value: { state: m.state, reason: m.reason, enabled: m.state !== 'off', muted: m.state === 'muted' } };
    if (m.kind === 'target') this.liveProjection.target = { revision: ++this.liveRevision, value: m.target };
    if (m.kind === 'targets') this.liveProjection.targets = { revision: ++this.liveRevision, value: [...(m.runs || [])] };
    // On and off from another surface too (the terminal's ctrl+v, T-35): any state but off is on.
    if (m.kind === 'state' && this.voice) {
      const was = !!this.voice.enabled;
      this.voice.state = m.state; this.voice.reason = m.reason; this.voice.enabled = m.state !== 'off'; this.voice.muted = m.state === 'muted';
      if (was !== this.voice.enabled) vscode.commands.executeCommand('setContext', 'overseer.voiceOn', this.voice.enabled);
      this.render(); this.changed();
    }
    if (m.kind === 'target' && this.voice) {
      const v = this.voice, epoch = this.connectionEpoch;
      v.target = m.target; v.target_title = !m.target || m.target === 'overseer' ? 'Overseer' : m.target;
      this.render(); this.changed(); this.post({ type: 'live', msg: { ...m, target_title: v.target_title } });
      this.titleTarget(v, epoch);
      return;
    }
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
    if (m.kind === 'open') this.openPlace(m.place, m);
    if (m.kind === 'download') this.downloadProgress?.(m.progress);
    this.post({ type: 'live', msg: m });
  }

  /** A permission answered by voice (AC-171): the toast with Cancel inside the window, then Sent. */
  async toast(m) {
    if (m.cancel) {
      const pick = await vscode.window.showInformationMessage(m.text, 'Cancel');
      if (pick === 'Cancel') await this.client.request('voice.cancel', { id: m.request }).catch(e => vscode.window.showWarningMessage(`Overseer: ${require('../media/plain-words.js').plain(e.message, 300)}`));
    } else {
      vscode.window.showInformationMessage(m.text);
    }
  }

  /** What is not done by voice opens its place in the UI (AC-171); the loop's places are shown (AC-252). */
  openPlace(place, m = {}) {
    const run = (cmd, ...args) => vscode.commands.executeCommand(cmd, ...args).then(undefined, () => {});
    if (['overseer', 'follow', 'manual_edit'].includes(place)) return Promise.resolve(this.place(place, m)).catch(() => {});
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

  /** To the voice stage in the Overseer view (home). */
  post(m) { const panel = this.view(); if (panel) panel.webview.postMessage({ type: 'voiceView', m }); }

  async sendRequests() {
    try {
      const { requests } = await this.client.request('voice.requests', { limit: 20 });
      this.post({ type: 'requests', list: requests });
    } catch { /* the daemon is restarting */ }
  }

  /** Shows the voice view: home in the Overseer view, which is the voice view while Voice Mode is on. */
  async open() {
    await this.showHome();
    await this.refresh();
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
    // The conversation with Overseer becomes the voice view; nothing opens anywhere else (AC-227).
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
    const pick = await vscode.window.showQuickPick([{ label: '$(overseer-mark) Overseer', description: 'works out the agents from what you say', target: 'overseer' }, ...active.map(a => ({ label: a.title, description: a.status, target: a.id }))], { title: 'Voice Mode: talk to', placeHolder: 'Who hears what you say' });
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
