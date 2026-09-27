// Continuity in VS Code (Gate L): the connection in the status bar and the side bar, the settings
// (`overseer.continuity.*`, which the daemon owns and enforces), one Needs-you item for every
// waiting agent, and what the chats and the composer ask for (use a local model now, retry, stay,
// switch back, allow downloads). The words are in media/continuity-text.js.
const vscode = require('vscode');
const text = require('../media/continuity-text.js');

const REFRESH_ON = new Set(['connection', 'continuity_settings', 'retry', 'handoff', 'back_online', 'local_download', 'ollama_install', 'ollama_server', 'local_load', 'attention', 'memory_valve']);

class Continuity {
  /** views(): { center, outputs, agentsView, newTaskPanel } once they exist. */
  constructor(context, client, model, { say, views }) {
    this.context = context; this.client = client; this.model = model; this.say = say; this.views = views;
    this.data = undefined; this.pushedTo = new WeakSet(); this.pushing = false;
    this.item = vscode.window.createStatusBarItem('overseer.connection', vscode.StatusBarAlignment.Left, 49);
    this.item.name = 'Overseer Connection';
    this.item.command = 'overseer.continuity.show';
    context.subscriptions.push(this.item,
      vscode.workspace.onDidChangeConfiguration(e => { if (e.affectsConfiguration('overseer.continuity')) this.pushSettings().catch(err => this.refused(err)); }),
      vscode.commands.registerCommand('overseer.continuity.show', () => this.show()),
      vscode.commands.registerCommand('overseer.continuity.checkNow', () => this.act({ action: 'check_now' })),
      vscode.commands.registerCommand('overseer.continuity.toggle', () => this.act({ action: 'toggle' })),
      vscode.commands.registerCommand('overseer.continuity.localModels', () => this.localModels()),
      vscode.commands.registerCommand('overseer.continuity.act', args => this.act(args || {})),
      vscode.commands.registerCommand('overseer.continuity.data', () => this.data));
    client.on('connected', () => { this.start().catch(err => say('continuity: ' + err.message)); });
    client.on('disconnected', () => { this.data = undefined; this.draw(); });
    client.on('event', ev => {
      if (REFRESH_ON.has(ev.kind) || (ev.kind === 'status' && (text.STATES[ev.payload?.status] || this.waitingIds().has(ev.run_id)))) this.soon();
    });
    // A webview that opens later gets what is known.
    model.onDidChange(() => this.push());
    this.draw();
  }

  waitingIds() { return new Set(((this.data && this.data.waiting) || []).map(w => w.run_id)); }
  soon(ms = 150) { clearTimeout(this.timer); this.timer = setTimeout(() => this.refresh().catch(err => this.say('continuity: ' + err.message)), ms); }

  async start() {
    await this.refresh();
    await this.pushSettings().catch(err => this.refused(err));
  }

  /** An older daemon does not know Continuity: nothing is shown, and nothing is asked again. */
  async refresh() {
    if (!this.client.connected || this.unsupported) return;
    try { this.data = await this.client.request('continuity.ui'); }
    catch (error) {
      if (/unknown method/i.test(error.message)) { this.unsupported = true; this.data = undefined; this.say('continuity: this daemon has no Continuity'); }
      else throw error;
    }
    this.draw();
    this.push(true);
    // The side bar and Needs you read from here too.
    this.model.emitter.fire();
    await this.mirror().catch(() => {});
  }

  // ---- Settings: VS Code edits, the daemon owns ----
  /** What the user set in VS Code (values left at their default are not sent). */
  wanted() {
    const config = vscode.workspace.getConfiguration('overseer.continuity');
    const out = {};
    for (const key of Object.keys(text.SETTINGS)) {
      const i = config.inspect(key);
      const set = i && (i.workspaceFolderValue ?? i.workspaceValue ?? i.globalValue);
      if (set !== undefined) out[key] = set;
    }
    return out;
  }
  async pushSettings() {
    if (!this.client.connected || this.unsupported || !this.data) return;
    const values = text.changed(this.wanted(), this.data.settings);
    if (!Object.keys(values).length) return;
    this.say('continuity: settings → daemon ' + JSON.stringify(values));
    await this.client.request('settings.set', { values });
    await this.refresh();
    if (values.allowModelDownloads === true) this.offerPrefetch().catch(err => this.say('continuity: ' + err.message));
  }
  /** The daemon refused a value: say why, and show its own value again. */
  async refused(error) {
    vscode.window.showErrorMessage(`Overseer: ${error.message}`);
    this.say('continuity: refused: ' + error.message);
    await this.mirror(true).catch(() => {});
  }
  /** Values changed elsewhere (another window, `overseerd ctl`) appear in VS Code's settings. */
  async mirror(force) {
    if (!this.data) return;
    const config = vscode.workspace.getConfiguration('overseer.continuity');
    const wanted = this.wanted();
    for (const [key, spec] of Object.entries(text.SETTINGS)) {
      const theirs = this.data.settings[key];
      const shown = key in wanted ? wanted[key] : spec.default;
      if (JSON.stringify(theirs ?? null) === JSON.stringify(shown ?? null)) continue;
      if (!force && key in wanted && this.mirrored?.[key] === JSON.stringify(theirs)) continue;
      (this.mirrored ||= {})[key] = JSON.stringify(theirs);
      await config.update(key, JSON.stringify(theirs ?? null) === JSON.stringify(spec.default ?? null) ? undefined : theirs, vscode.ConfigurationTarget.Global);
    }
  }

  // ---- What is shown ----
  draw() {
    const v = this.views();
    if (!this.data) {
      this.item.hide();
      if (v.agentsView) v.agentsView.message = undefined;
      vscode.commands.executeCommand('setContext', 'overseer.connection', 'unknown');
      return;
    }
    const s = text.statusBar(this.data);
    this.item.text = `$(${s.icon})${s.text ? ' ' + s.text : ''}`;
    this.item.tooltip = s.tooltip + '\nClick for the connection and Continuity.';
    this.item.accessibilityInformation = { label: s.accessible };
    this.item.backgroundColor = s.warn ? new vscode.ThemeColor('statusBarItem.warningBackground') : undefined;
    this.item.show();
    if (v.agentsView) v.agentsView.message = text.sidebar(this.data) || undefined;
    vscode.commands.executeCommand('setContext', 'overseer.connection', this.data.connection?.state || 'unknown');
  }

  /** Sends what is known to every open Overseer webview (again when `fresh`). */
  push(fresh) {
    if (!this.data) return;
    const v = this.views();
    const targets = [v.center?.panel, v.newTaskPanel?.panel, ...[...(v.outputs?.panels?.values() || [])].map(e => e.panel)].filter(Boolean);
    for (const panel of targets) {
      if (!fresh && this.pushedTo.has(panel)) continue;
      if (!this.pushedTo.has(panel)) {
        this.pushedTo.add(panel);
        // A page that says it is ready (or asks for the composer's data) is sent the data again: what was sent before it listened is lost.
        panel.webview.onDidReceiveMessage(m => { if (m && (m.type === 'ready' || m.type === 'composerData')) setTimeout(() => this.data && panel.webview.postMessage({ type: 'continuity', data: this.data }), 50); });
      }
      panel.webview.postMessage({ type: 'continuity', data: this.data });
    }
  }

  /** The one Needs-you item for every waiting agent together. */
  attention() {
    const n = this.data && text.needsYou(this.data);
    if (!n || !this.model.run(n.run_id)) return undefined;
    return { run_id: n.run_id, rank: 3, label: n.label, title: n.title, detail: n.detail };
  }

  /** Handed-off agents of a task, newest first, for the side bar to fold under the agent that took over. */
  predecessors(run) {
    const hands = (this.data && this.data.handoffs) || [];
    const out = []; const seen = new Set();
    for (let id = run.id; ;) {
      const h = hands.filter(x => x.successor === id).pop();
      if (!h || seen.has(h.predecessor)) break;
      seen.add(h.predecessor);
      const r = this.model.run(h.predecessor);
      if (r) out.push({ run: r, reason: h.reason });
      id = h.predecessor;
    }
    return out;
  }

  /** What a composer starts from. */
  snapshot() { return this.data; }

  // ---- What can be asked for ----
  async act(a) {
    if (!vscode.workspace.isTrusted && !['check_now', 'open', 'dismiss_notice'].includes(a.action)) throw new Error('Controlling agents requires a trusted workspace.');
    const runId = a.runId && String(a.runId);
    switch (a.action) {
      case 'check_now': await this.client.request('connection.check'); break;
      case 'toggle': await this.set({ enabled: !(this.data?.settings?.enabled !== false) }); break;
      case 'allow': if (['allowModelDownloads', 'allowOllamaInstall', 'prefetch', 'allowUnverifiedModels'].includes(a.setting)) await this.set({ [a.setting]: a.on !== false }); break;
      case 'dismiss_notice': await this.client.request('continuity.notice', { dismiss: true }); break;
      case 'open': if (a.target) await vscode.commands.executeCommand('overseer.selectRun', String(a.target)); break;
      case 'stop': await this.client.request('run.interrupt', { run_id: runId }); break;
      case 'retry_now': await this.client.request('run.retry_now', { run_id: runId }); break;
      case 'stay': await this.client.request('run.stay', { run_id: runId }); break;
      case 'handoff': { const moved = await this.handoff(runId, String(a.to || 'local')); if (moved) await vscode.commands.executeCommand('overseer.selectRun', moved); break; }
      case 'pull': await this.pull(String(a.tag)); break;
      case 'cancel_pull': await this.client.request('local.pull_cancel', { tag: String(a.tag) }); break;
      default: return false;
    }
    this.model.scheduleRefresh();
    await this.refresh();
    return true;
  }

  async set(values) {
    await this.client.request('settings.set', { values });
    const config = vscode.workspace.getConfiguration('overseer.continuity');
    for (const [key, value] of Object.entries(values)) await config.update(key, JSON.stringify(value) === JSON.stringify(text.SETTINGS[key].default) ? undefined : value, vscode.ConfigurationTarget.Global);
    if (values.allowModelDownloads === true) this.offerPrefetch().catch(err => this.say('continuity: ' + err.message));
  }

  /** Once per machine, when downloads are first allowed: keep the best-fitting model downloaded? Declining leaves prefetch off. */
  async offerPrefetch() {
    const offer = await this.client.request('continuity.prefetch_offer', {});
    if (!offer.show) return;
    const size = offer.download_bytes ? `${text.gib(offer.download_bytes)} GiB download` : 'already installed';
    const yes = 'Keep it ready';
    const choice = await vscode.window.showInformationMessage(`Keep ${offer.model} ready for offline? (${size})`, { detail: 'While online, Overseer keeps the best-fitting local model downloaded, so going offline needs no download. It never downloads during a paid turn.' }, yes, 'Not now');
    await this.client.request('continuity.prefetch_offer', { dismiss: true });
    if (choice === yes) await this.set({ prefetch: true });
    await this.refresh();
  }

  /** Moves an agent's work. A move that would ask less often than now is said first, and made only on a yes. */
  async handoff(runId, to) {
    try {
      return (await this.client.request('run.handoff', { run_id: runId, to })).successor.id;
    } catch (error) {
      const m = /^(.*); to continue, accept the mode (\S+)$/.exec(error.message);
      if (!m) throw error;
      const mode = text.MODE[m[2]] || m[2];
      const yes = `Continue in ${mode}`;
      const choice = await vscode.window.showWarningMessage('Continue with different permissions?', { modal: true, detail: `${m[1].replace(/^./, c => c.toUpperCase())}.\nThe agent that takes over runs in ${mode}.` }, yes);
      if (choice !== yes) return undefined;
      return (await this.client.request('run.handoff', { run_id: runId, to, accept_mode: m[2] })).successor.id;
    }
  }

  /** Downloads a model; the first download ever is confirmed once, with its size. */
  async pull(tag) {
    let r = await this.client.request('local.pull', { tag });
    if (r.needs_confirmation) {
      const yes = 'Download';
      const choice = await vscode.window.showInformationMessage(`Download ${tag}?`, { modal: true, detail: `${text.gib(r.bytes)} GiB from the Ollama registry. Later downloads follow the setting without asking.` }, yes);
      if (choice !== yes) return;
      r = await this.client.request('local.pull', { tag, confirm: true });
    }
  }

  /** The connection, what Continuity does now, and the few things to change, as a pick. */
  async show() {
    await this.refresh().catch(() => {});
    const d = this.data;
    if (!d) { vscode.window.showInformationMessage('Overseer is not connected to a daemon that reports its connection.'); return; }
    const c = text.connection(d);
    const items = [
      { label: `$(cloud) ${c.label}`, detail: c.lines.slice(0, -1).join(' '), kind: 'info' },
      { label: `$(${d.settings.enabled ? 'pass' : 'circle-slash'}) Continuity is ${d.settings.enabled ? 'on' : 'off'}`, detail: text.policy(d), action: { action: 'toggle' }, description: d.settings.enabled ? 'Turn off' : 'Turn on' },
      ...(d.waiting || []).map(w => ({ label: `$(cloud) ${this.model.run(w.run_id)?.title || w.run_id}`, description: text.waitCard(w, d, Date.now()).title, detail: text.waitCard(w, d, Date.now()).foot, action: { action: 'open', target: w.run_id } })),
      { label: '$(sync) Check the connection now', action: { action: 'check_now' } },
      { label: '$(server) Local models…', detail: d.local?.pick ? `Best fit: ${d.local.pick.tag} at a ${Math.round(d.local.pick.context / 1024)}k context` : (d.local?.why_no_pick || d.local?.ollama?.detail || ''), command: 'overseer.continuity.localModels' },
      { label: `$(cloud-download) Model downloads: ${d.settings.allowModelDownloads ? 'allowed' : 'off'}`, action: { action: 'allow', setting: 'allowModelDownloads', on: !d.settings.allowModelDownloads }, description: d.settings.allowModelDownloads ? 'Turn off' : 'Allow' },
      { label: `$(desktop-download) Installing and starting Ollama: ${d.settings.allowOllamaInstall ? 'allowed' : 'off'}`, action: { action: 'allow', setting: 'allowOllamaInstall', on: !d.settings.allowOllamaInstall }, description: d.settings.allowOllamaInstall ? 'Turn off' : 'Allow' },
      { label: '$(gear) Continuity settings', command: 'workbench.action.openSettings', args: 'overseer.continuity' },
    ];
    const picked = await vscode.window.showQuickPick(items, { title: 'Overseer: connection and Continuity', placeHolder: c.sentence });
    if (!picked) return;
    if (picked.command) await vscode.commands.executeCommand(picked.command, picked.args);
    else if (picked.action) await this.act(picked.action).catch(error => vscode.window.showErrorMessage(`Overseer: ${error.message}`));
  }

  /** Every local model with its badge; choosing one that is not installed downloads it, when allowed. */
  async localModels() {
    await this.refresh().catch(() => {});
    const choices = text.localChoices(this.data);
    const downloads = ((this.data?.downloads?.downloads) || []).filter(x => text.download(x).active);
    const items = [
      ...downloads.map(x => ({ label: `$(cloud-download) ${text.download(x).text}`, description: 'Cancel', action: { action: 'cancel_pull', tag: x.tag } })),
      ...choices.models.map(m => ({ label: `$(${m.badge.usable ? 'pass' : m.installed ? 'circle-slash' : 'cloud-download'}) ${m.tag}`, description: [m.badge.badge, m.badge.mark].filter(Boolean).join(' · '), detail: m.badge.detail,
        action: m.installed ? undefined : { action: 'pull', tag: m.tag } })),
    ];
    const title = choices.running ? `Local models · budget ${choices.budget ? text.gib(choices.budget.budget) + ' GiB' : 'unknown'}` : choices.ollama;
    const picked = await vscode.window.showQuickPick(items, { title, placeHolder: choices.pick ? `Best fit now: ${choices.pick.tag}` : (choices.why_no_pick || 'No model fits now'), matchOnDescription: true });
    if (picked?.action) await this.act(picked.action).catch(error => vscode.window.showErrorMessage(`Overseer: ${error.message}`));
  }
}

/** One chat message about Continuity for runId (chats, run panels and grid tiles). */
async function handleMessage(continuity, runId, message) {
  if (!continuity) return false;
  return continuity.act({ ...message, runId: message.runId || runId });
}

module.exports = { Continuity, handleMessage };
