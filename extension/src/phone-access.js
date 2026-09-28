// Phone access on the Mac (Gate N, AC-116, AC-117, AC-119, AC-129): the switch in the status bar,
// Pair a Phone, the owner's confirmation, the Devices view and the switch for notifications to
// every phone. The daemon holds the state: this asks for gateway.status once per connection and
// once after each change the daemon reports. Nothing polls.
const vscode = require('vscode');
const words = require('./phone-text');
const { page, localRoots } = require('./webview-html');

// Daemon events that change what this window shows.
const GATEWAY_EVENTS = new Set(['gateway_state', 'gateway_sessions', 'pairing_opened', 'pairing_request', 'pairing_closed', 'device_paired', 'device_revoked', 'device_scope', 'power']);
const NOBODY_WAITING = /no phone is waiting|stopped waiting/;

class DevicesProvider {
  constructor(owner) {
    this.owner = owner;
    this.emitter = new vscode.EventEmitter();
    this.onDidChangeTreeData = this.emitter.event;
  }
  /** Redraws only when a row would read differently (a redraw under the pointer loses a click). */
  refresh() {
    const sig = JSON.stringify(this.devices().map(d => { const r = words.deviceRow(d); return [d.id, r.label, r.description, r.context, r.details]; }));
    if (sig === this.shown) return;
    this.shown = sig;
    this.emitter.fire();
  }
  devices() { return words.paired(this.owner.status).slice().sort((a, b) => (b.connected - a.connected) || (b.last_seen_ms || b.paired_ms || 0) - (a.last_seen_ms || a.paired_ms || 0)); }
  getTreeItem(node) { return node.item; }
  getParent(node) { return node.parent; }
  getChildren(node) {
    if (node) return node.device ? node.details : [];
    return this.devices().map(device => {
      const row = words.deviceRow(device);
      const item = new vscode.TreeItem(row.label, vscode.TreeItemCollapsibleState.Collapsed);
      item.id = 'device:' + device.id;
      item.description = row.description;
      item.iconPath = new vscode.ThemeIcon('device-mobile', device.connected ? new vscode.ThemeColor('charts.green') : undefined);
      item.tooltip = new vscode.MarkdownString([`**${row.label.replace(/([\\`*_{}[\]<>()#+\-.!|])/g, '\\$1')}**`, ...row.details.map(([k, v]) => `${k}: ${v}`)].join('\n\n'));
      item.accessibilityInformation = { label: row.aria };
      item.contextValue = row.context;
      const parent = { item, device };
      parent.details = row.details.map(([k, v]) => {
        const d = new vscode.TreeItem(k);
        d.id = `device:${device.id}:${k}`;
        d.description = v;
        d.accessibilityInformation = { label: `${k}: ${v}` };
        d.contextValue = 'device-detail';
        return { item: d, parent };
      });
      return parent;
    });
  }
}

/**
 * Tells the daemon which agent this window has in front of the owner (`ui.focus`), so no phone is
 * notified about an agent the owner is already looking at (AC-129). It speaks when the window
 * gains or loses focus, when another agent is shown, and once per connection; never on a timer.
 */
class Looking {
  /** `selected()` is the agent the window selected; `center` and `outputs` say which chats are on screen. */
  constructor(context, client, { model, selected, center, outputs, say }) {
    this.client = client; this.model = model; this.selected = selected; this.center = center; this.outputs = outputs; this.say = say;
    const update = () => this.update();
    context.subscriptions.push(vscode.window.onDidChangeWindowState(update), vscode.window.tabGroups.onDidChangeTabs(update), vscode.window.tabGroups.onDidChangeTabGroups(update));
    if (model?.onDidChange) context.subscriptions.push(model.onDidChange(update));
    // The daemon remembers it per connection: say it again on a new one.
    client.on('connected', () => { this.sent = undefined; this.update(); });
  }

  /** The agent in front of the owner: a top-level run whose chat is on screen in this focused window. */
  agent() {
    if (!vscode.window.state.focused) return undefined;
    const root = id => { const run = id && this.model?.run(id); return run ? (this.model.rootRun(run) || run).id : undefined; };
    const shown = [];
    // The Overseer view shows one chat (not in its grid or its new-agent form).
    if (this.center?.panel?.visible && this.center.mode === 'chat' && this.center.chatRun) shown.push(root(this.center.chatRun));
    for (const [id, entry] of this.outputs?.panels || []) if (entry.panel?.visible) shown.push(root(id));
    const selected = root(this.selected?.());
    return shown.includes(selected) ? selected : shown.find(Boolean);
  }

  update() {
    if (!this.client.connected) return;
    const run = this.agent() || null;
    if (this.sent === run) return;
    this.sent = run;
    this.client.request('ui.focus', { run_id: run, focused: !!run }).then(() => this.say(run ? `looking at ${run}` : 'looking at no agent'), error => {
      // A daemon from before this method: nothing to tell.
      if (!(error.code === 'unknown_method' || /unknown method/i.test(error.message))) { this.sent = undefined; this.say('ui.focus: ' + error.message); }
    });
  }
}

class PhoneAccess {
  constructor(context, client, { say, guard, requireTrust, looking }) {
    this.context = context; this.client = client; this.say = say; this.requireTrust = requireTrust;
    if (looking) this.looking = new Looking(context, client, { ...looking, say });
    this.available = true;
    this.status = undefined;
    this.asked = new Map(); // pairing request id -> { name, open, over }
    this.pairing = undefined; // what the pairing panel shows (only in the window that started it)

    this.item = vscode.window.createStatusBarItem('overseer.phoneAccess', vscode.StatusBarAlignment.Left, 49);
    this.item.name = 'Overseer Phone Access';
    this.item.command = 'overseer.phoneAccess';
    this.devices = new DevicesProvider(this);
    this.view = vscode.window.createTreeView('overseer.devices', { treeDataProvider: this.devices });
    // "last seen 5m ago" grows old by itself: redraw the rows once a minute while they are shown.
    const clock = () => { clearInterval(this.clock); if (this.view.visible) { this.refreshIfStale(); this.clock = setInterval(() => this.devices.refresh(), 60000); } };
    context.subscriptions.push(this.item, this.view, this.view.onDidChangeVisibility(clock), { dispose: () => { clearInterval(this.clock); clearTimeout(this.timer); clearTimeout(this.expiry); } });

    client.on('connected', () => { this.refresh(); });
    client.on('replayed', () => { this.refresh(); });
    client.on('disconnected', () => this.render());
    client.on('stopped', () => { this.render(); this.panelOver('Overseer was stopped, so this code no longer works.', 'stopped'); });
    client.on('event', event => { if (client.live && GATEWAY_EVENTS.has(event.kind)) this.onEvent(event); });

    const changing = fn => guard(async (...args) => { requireTrust(); return fn(...args); });
    context.subscriptions.push(
      vscode.commands.registerCommand('overseer.phoneAccess', guard(() => this.menu())),
      vscode.commands.registerCommand('overseer.turnOnPhoneAccess', changing(() => this.turnOn())),
      vscode.commands.registerCommand('overseer.turnOffPhoneAccess', changing(() => this.turnOff())),
      vscode.commands.registerCommand('overseer.pairPhone', changing(() => this.pair())),
      vscode.commands.registerCommand('overseer.showDevices', guard(() => this.showDevices())),
      vscode.commands.registerCommand('overseer.revokeDevice', changing(arg => this.revoke(arg))),
      vscode.commands.registerCommand('overseer.makeDeviceWatchOnly', changing(arg => this.scope(arg, 'watch'))),
      vscode.commands.registerCommand('overseer.giveDeviceFullControl', changing(arg => this.scope(arg, 'full'))),
      vscode.commands.registerCommand('overseer.renameDevice', changing(arg => this.rename(arg))),
      vscode.commands.registerCommand('overseer.turnOffPhoneNotifications', changing(() => this.notifications(false))),
      vscode.commands.registerCommand('overseer.turnOnPhoneNotifications', changing(() => this.notifications(true))),
    );
    this.render();
  }

  // ---------------------------------------------------------------- state

  /** One gateway.status; the newest answer wins. */
  async refresh() {
    clearTimeout(this.timer); this.timer = undefined;
    if (!this.client.connected) { this.render(); return; }
    const mine = this.asking = (this.asking || 0) + 1;
    const sent = Date.now();
    try {
      const status = await this.client.request('gateway.status');
      if (mine !== this.asking) return;
      this.status = status; this.available = true; this.seen = Date.now();
    } catch (error) {
      if (mine !== this.asking) return;
      // A daemon from before Gate N has no phone access: say nothing rather than something false.
      if (error.code === 'unknown_method' || /unknown method/i.test(error.message)) { this.available = false; this.status = undefined; }
      else this.say('gateway.status: ' + error.message);
    }
    this.render();
    // A phone that was already waiting when this window connected or reloaded.
    for (const waiting of this.status?.pairing?.waiting || []) this.ask(waiting);
    // The daemon no longer holds the pairing this panel shows (it restarted, or pairing ended while this window was away).
    if (this.pairing && ['open', 'waiting'].includes(this.pairing.phase) && sent >= this.pairing.since && this.status && !this.status.pairing) this.panelOver(this.status.enabled ? 'This code no longer works.' : 'Phone access was turned off.', 'closed');
  }
  /** Several events in a row (a phone pairs, then connects) ask once. */
  refreshSoon() { if (!this.timer) this.timer = setTimeout(() => this.refresh(), 60); }
  /** Changes made elsewhere without an event (a rename, the notifications switch) show when the owner looks. */
  refreshIfStale() { if (!this.seen || Date.now() - this.seen > 2000) this.refresh(); }

  render() {
    const online = this.client.connected;
    const bar = words.statusBar(this.status, { online, available: this.available });
    if (bar.show) {
      this.item.text = bar.text; this.item.tooltip = bar.tooltip;
      this.item.accessibilityInformation = { label: bar.label, role: 'button' };
      this.item.show();
    } else this.item.hide();
    this.view.description = bar.show && bar.on && bar.phones ? `${bar.phones} connected` : undefined;
    const set = (key, value) => vscode.commands.executeCommand('setContext', key, value);
    set('overseer.phoneAccessAvailable', online && this.available);
    set('overseer.phoneAccessOn', !!this.status?.enabled);
    set('overseer.phoneNotificationsOn', this.status?.settings?.notifications !== false);
    this.devices.refresh();
  }

  quiet(text) { vscode.window.setStatusBarMessage(`$(device-mobile) ${text}`, 6000); this.say(text); }

  onEvent(event) {
    const p = event.payload || {};
    this.refreshSoon();
    if (event.kind === 'pairing_request') {
      // A request the phone has stopped waiting for is history, not a question.
      if (event.ts && p.wait_ms && Date.now() - event.ts > p.wait_ms) return;
      this.ask(p);
    } else if (event.kind === 'pairing_opened') {
      // A code made somewhere else replaces the one this panel shows.
      if (this.starting > 0) this.starting--;
      else this.panelOver('A new code was made somewhere else, so this one no longer works.', 'replaced');
    } else if (event.kind === 'pairing_closed') {
      const asked = p.request && this.asked.get(p.request);
      if (asked && !asked.over) { asked.over = true; if (asked.open) this.quiet(`"${asked.name}" stopped waiting to pair.`); }
      this.panelOver(words.pairingEnded(p.reason, p.name || this.pairing?.phone), 'closed');
    } else if (event.kind === 'device_paired') {
      for (const asked of this.asked.values()) if (!asked.over && asked.name === p.name) asked.over = true;
      if (this.pairing && ['open', 'waiting'].includes(this.pairing.phase)) { clearTimeout(this.expiry); this.pairing = { ...this.pairing, phase: 'paired', phone: p.name, text: `Paired with "${p.name}".` }; this.push(); }
      this.quiet(`Paired with "${p.name}".`);
    } else if (event.kind === 'gateway_state' && p.state === 'off') {
      this.panelOver('Phone access was turned off.', 'off');
    }
  }

  // ---------------------------------------------------------------- the switch

  async turnOn() {
    this.status = await this.client.request('gateway.enable', {});
    this.seen = Date.now();
    this.render();
    this.quiet('Phone access is on.');
  }

  async turnOff() {
    await this.refresh();
    if (!this.status?.enabled) { this.quiet('Phone access is already off.'); return; }
    const here = words.connected(this.status);
    if (here.length) {
      const ok = await vscode.window.showWarningMessage('Turn off phone access?', { modal: true,
        detail: `${words.plural(here.length, 'phone')} will be disconnected: ${here.map(d => d.name).join(', ')}.\nPaired phones connect again when you turn phone access back on.` }, 'Turn Off');
      if (ok !== 'Turn Off') return;
    }
    this.status = await this.client.request('gateway.disable', {});
    this.seen = Date.now();
    this.render();
    this.quiet('Phone access is off.');
  }

  async notifications(on) {
    const settings = await this.client.request('gateway.settings', { notifications: !!on });
    if (this.status) this.status = { ...this.status, settings };
    this.render();
    this.quiet(on ? 'Notifications to phones are on.' : 'Notifications to phones are off. Nothing is sent.');
    this.refreshSoon();
  }

  /** The status bar item's menu: what makes sense now. */
  async menu() {
    await this.refresh();
    if (!this.client.connected || !this.available) { vscode.window.showInformationMessage(this.client.connected ? 'This Overseer daemon has no phone access. Restart the daemon to update it.' : 'Overseer is not connected to its daemon.'); return; }
    const s = this.status || {};
    const here = words.connected(s).length, all = words.paired(s).length;
    const notify = s.settings?.notifications !== false;
    const items = [
      !s.enabled && { label: '$(device-mobile) Turn On Phone Access', detail: 'Let paired phones connect to this Mac.', command: 'overseer.turnOnPhoneAccess' },
      { label: '$(add) Pair a Phone…', detail: s.enabled ? 'Show a code to scan or type on the phone.' : 'Turns phone access on first.', command: 'overseer.pairPhone' },
      { label: '$(list-unordered) Show Devices', detail: all ? `${words.plural(all, 'phone')} paired${here ? `, ${here} connected` : ''}.` : 'No phone is paired yet.', command: 'overseer.showDevices' },
      { label: notify ? '$(bell-slash) Turn Off Notifications to Phones' : '$(bell) Turn On Notifications to Phones', detail: notify ? 'Phones are told when an agent needs you.' : 'Nothing is sent to any phone.', command: notify ? 'overseer.turnOffPhoneNotifications' : 'overseer.turnOnPhoneNotifications' },
      s.enabled && { label: '$(circle-slash) Turn Off Phone Access', detail: here ? `Disconnects ${words.plural(here, 'phone')}.` : 'No phone can connect.', command: 'overseer.turnOffPhoneAccess' },
    ].filter(Boolean);
    const pick = await vscode.window.showQuickPick(items, { title: `Phone access: ${!s.enabled ? 'off' : here ? `on, ${words.plural(here, 'phone')} connected` : 'on'}`, placeHolder: 'Phone access is switched on this Mac only' });
    if (pick) await vscode.commands.executeCommand(pick.command);
  }

  // ---------------------------------------------------------------- devices

  async showDevices() {
    await vscode.commands.executeCommand('overseer.devices.focus');
    await this.refresh();
  }

  /** The device of a row, or one the owner picks (from the command palette). */
  async device(arg, title) {
    if (arg?.device) return arg.device;
    if (arg?.parent?.device) return arg.parent.device;
    await this.refresh();
    const list = this.devices.devices();
    if (!list.length) { vscode.window.showInformationMessage('No phone is paired.'); return undefined; }
    const pick = await vscode.window.showQuickPick(list.map(d => ({ label: `$(device-mobile) ${d.name}`, description: words.deviceRow(d).description, device: d })), { title });
    return pick?.device;
  }

  async revoke(arg) {
    const d = await this.device(arg, 'Revoke device');
    if (!d) return;
    const ok = await vscode.window.showWarningMessage(`Revoke "${d.name}"?`, { modal: true,
      detail: `${d.connected ? 'It is disconnected now. ' : ''}Its key never works again. To use this phone again, pair it again.` }, 'Revoke');
    if (ok !== 'Revoke') return;
    await this.client.request('gateway.device_revoke', { id: d.id });
    await this.refresh();
    this.quiet(`Revoked "${d.name}".`);
  }

  async scope(arg, scope) {
    const d = await this.device(arg, scope === 'watch' ? 'Make watch only' : 'Give full control');
    if (!d) return;
    await this.client.request('gateway.device_scope', { id: d.id, scope });
    await this.refresh();
    this.quiet(scope === 'watch' ? `"${d.name}" can watch only.` : `"${d.name}" has full control.`);
  }

  async rename(arg) {
    const d = await this.device(arg, 'Rename device');
    if (!d) return;
    const name = await vscode.window.showInputBox({ title: 'Rename device', value: d.name, prompt: 'The name this Mac shows for the phone.', validateInput: v => (!v.trim() ? 'A device needs a name' : v.trim().length > 60 ? 'At most 60 characters' : undefined) });
    if (!name || name.trim() === d.name) return;
    await this.client.request('gateway.device_rename', { id: d.id, name: name.trim() });
    await this.refresh();
  }

  // ---------------------------------------------------------------- pairing

  async pair() {
    await this.refresh();
    if (!this.available) throw new Error('This Overseer daemon has no phone access. Restart the daemon to update it.');
    if (!this.status?.enabled) {
      const go = await vscode.window.showInformationMessage('Phone access is off. Turn it on to pair a phone?', { modal: true,
        detail: 'Phones on your network can then reach this Mac. Only a phone you pair here can see or do anything.' }, 'Turn On and Pair');
      if (go !== 'Turn On and Pair') return;
      await this.turnOn();
    }
    this.openPanel();
    await this.newCode();
  }

  openPanel() {
    if (this.panel) { this.panel.reveal(); return; }
    const panel = vscode.window.createWebviewPanel('overseer.pairing', 'Pair a Phone', { viewColumn: vscode.ViewColumn.Active, preserveFocus: false }, { enableScripts: true, localResourceRoots: localRoots(this.context.extensionUri), retainContextWhenHidden: true });
    this.panel = panel;
    panel.iconPath = vscode.Uri.joinPath(this.context.extensionUri, 'media', 'overseer.svg');
    panel.webview.html = page(panel.webview, this.context.extensionUri, { title: 'Pair a Phone', css: ['pairing.css'], js: ['pairing.js'], body: `
<main class="pair" data-audit-view="pairing" data-phase="loading">
<h1>Pair a phone</h1>
<p id="mac" class="mac"></p>
<section id="open" class="open">
<div id="qr" class="qr" role="img" aria-label="The pairing code as a QR code"></div>
<div class="how">
<ol class="steps"><li>Open Overseer on your phone.</li><li>Scan this code, or type it.</li><li>Confirm the phone here, on this Mac.</li></ol>
<p id="left" class="left" role="timer"></p>
</div>
</section>
<section id="typed" class="typed">
<h2 id="code-h" class="sec">Code to type</h2>
<div class="code-row"><code id="code" class="code" aria-labelledby="code-h" tabindex="0"></code><button id="copy" class="btn sm" type="button"><span class="codicon codicon-copy sm" aria-hidden="true"></span><span id="copy-label">Copy</span></button></div>
</section>
<section id="over" class="over" hidden>
<p id="over-text" class="over-text" role="status"></p>
<div class="actions"><button id="new" class="btn primary" type="button">New code</button><button id="done" class="btn" type="button">Done</button></div>
</section>
</main>` });
    panel.onDidDispose(() => {
      this.panel = undefined;
      clearTimeout(this.expiry);
      const live = this.pairing && ['open', 'waiting'].includes(this.pairing.phase);
      this.pairing = undefined;
      // Closing the panel takes the code back.
      if (live && this.client.connected) this.client.request('gateway.pair_cancel', {}).catch(error => this.say('pair_cancel: ' + error.message));
    });
    panel.webview.onDidReceiveMessage(m => this.receive(m).catch(error => { this.say('pairing: ' + error.message); this.panelOver(error.message, 'error'); }));
  }

  async receive(m) {
    if (!m || typeof m !== 'object') return;
    if (m.type === 'ready') this.push();
    else if (m.type === 'new') { this.requireTrust(); await this.refresh(); if (!this.status?.enabled) await this.turnOn(); await this.newCode(); }
    else if (m.type === 'copy' && this.pairing?.code) { await vscode.env.clipboard.writeText(this.pairing.code); this.panel?.webview.postMessage({ type: 'copied' }); }
    else if (m.type === 'done') this.panel?.dispose();
  }

  async newCode() {
    this.starting = (this.starting || 0) + 1;
    let r;
    try { r = await this.client.request('gateway.pair_start', {}); } catch (error) { this.starting--; throw error; }
    const qr = words.qrPath(words.qrMatrix(r.code));
    this.pairing = { phase: 'open', code: r.code, groups: words.codeGroups(r.code), qr, since: Date.now(), expiresAt: Date.now() + r.valid_ms, mac: r.name };
    clearTimeout(this.expiry);
    this.expiry = setTimeout(() => this.panelOver('This code has expired.', 'expired'), r.valid_ms);
    this.push();
  }

  /** The code stopped working: say why and offer a new one. */
  panelOver(text, why) {
    if (!this.pairing || !['open', 'waiting'].includes(this.pairing.phase)) return;
    clearTimeout(this.expiry);
    this.pairing = { phase: 'over', why, text, mac: this.pairing.mac };
    this.push();
  }

  push() { if (this.panel && this.pairing) this.panel.webview.postMessage({ type: 'state', state: this.pairing }); }

  /** The owner's decision about a phone that asks to pair. One answer wins across windows. */
  async ask(request) {
    if (!request?.request || this.asked.has(request.request)) return;
    const asked = { name: request.name, open: true, over: false };
    this.asked.set(request.request, asked);
    if (this.pairing && ['open', 'waiting'].includes(this.pairing.phase)) { this.pairing = { ...this.pairing, phase: 'waiting', phone: request.name, text: `"${request.name}" is asking to pair. Answer on this Mac.` }; this.push(); }
    const q = words.pairQuestion(request);
    const choice = await vscode.window.showInformationMessage(q.message, { modal: true, detail: q.detail }, { title: q.accept }, { title: q.decline, isCloseAffordance: true });
    asked.open = false;
    const accept = choice?.title === q.accept;
    // Answered in another window or in the terminal, or the phone gave up, while this was open.
    if (asked.over) { if (accept) this.quiet(`"${request.name}" is no longer waiting to pair.`); return; }
    try {
      await this.client.request('gateway.pair_confirm', { request: request.request, accept });
    } catch (error) {
      if (!NOBODY_WAITING.test(error.message)) { vscode.window.showErrorMessage(`Overseer: ${error.message}`); return; }
      if (accept) this.quiet(`"${request.name}" is no longer waiting to pair.`);
    } finally { asked.over = true; this.refreshSoon(); }
  }
}

module.exports = { PhoneAccess, DevicesProvider, Looking, GATEWAY_EVENTS };
