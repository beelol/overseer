// The daemon owns the library, revision and every turn snapshot. The panel only asks.
const vscode = require('vscode');
const T = require('./mods-text');
const { page, localRoots } = require('./webview-html');
const MUTATIONS = new Set(['preview', 'local', 'install', 'bind', 'unbind', 'remove']);
const REFRESH = new Set(['mods_changed', 'mods_applied', 'turn_started', 'turn_ended', 'status']);
class ModsPanel {
  constructor({ context, client, model, log }) {
    Object.assign(this, { context, client, model, log }); this.story = []; this.disposed = false;
    this.onConnected = () => { this.preview = undefined; this.soon(); };
    this.onDisconnected = () => { this.sequence = (this.sequence || 0) + 1; this.preview = undefined; this.data = undefined; this.applied = undefined; this.error = 'Disconnected. Reconnect to read current daemon state.'; this.push(); };
    this.onEvent = ev => { if (REFRESH.has(ev.kind)) { if (ev.kind.startsWith('mods_')) { this.story = [...this.story, ev].slice(-30); } this.soon(); } };
    client.on('connected', this.onConnected); client.on('disconnected', this.onDisconnected); client.on('event', this.onEvent);
    context.subscriptions.push(this);
    if (model.onDidChange) context.subscriptions.push(model.onDidChange(() => { if (this.panel) this.push(); }));
    if (vscode.workspace.onDidGrantWorkspaceTrust) context.subscriptions.push(vscode.workspace.onDidGrantWorkspaceTrust(() => this.push()));
  }
  soon() { if (!this.panel || this.disposed) return; clearTimeout(this.timer); this.timer = setTimeout(() => this.refresh().catch(e => this.fail(e)), 120); }
  fail(e) { this.error = e.message; this.log?.('mods: ' + e.message); this.push(); }
  async open({ runId } = {}) {
    this.runId = runId || undefined;
    if (!this.panel) {
      const panel = vscode.window.createWebviewPanel('overseer.mods', 'Mods', vscode.ViewColumn.Active, { enableScripts: true, localResourceRoots: localRoots(this.context.extensionUri), retainContextWhenHidden: true });
      this.panel = panel; panel.iconPath = vscode.Uri.joinPath(this.context.extensionUri, 'media', 'overseer-logo.png');
      panel.webview.html = page(panel.webview, this.context.extensionUri, { title: 'Mods', css: ['mods-panel.css'], js: ['mods-panel.js'], body: '<main id="mods" aria-busy="true"><h1>Mods</h1><p role="status">Reading the daemon library…</p></main>' });
      panel.onDidDispose(() => { if (this.panel === panel) { this.panel = undefined; this.preview = undefined; clearTimeout(this.timer); } });
      panel.webview.onDidReceiveMessage(m => this.handle(m).catch(async e => { this.fail(e); if (/revision|changed/i.test(e.message)) { await this.refresh().catch(() => {}); this.fail(e); } }));
    } else this.panel.reveal();
    await this.refresh().catch(e => this.fail(e));
  }
  async refresh() {
    if (this.disposed) return;
    if (!this.client.connected) { this.onDisconnected(); return; }
    const runId = this.runId, seq = this.sequence = (this.sequence || 0) + 1;
    const data = await this.client.request('mods.list', {});
    const applied = runId ? await this.client.request('mods.why', { run_id: runId }) : undefined;
    if (seq !== this.sequence || runId !== this.runId || !this.client.connected) return;
    this.data = data; this.applied = applied; this.error = undefined; this.push();
    // Replay a bounded page on open/reconnect; the daemon remains the story authority.
    try { const history = await this.client.request('events.list', { limit: 1000 }); if (seq === this.sequence) { const merged = new Map([...((history.events || []).filter(e => ['mods_changed', 'mods_applied'].includes(e.kind))), ...this.story].map(e => [e.seq || e.id, e])); this.story = [...merged.values()].slice(-30); this.push(); } } catch { /* Older daemon: current state still works. */ }
  }
  push() {
    this.panel?.webview.postMessage({ type: 'mods', data: this.data, library: T.library(this.data), applied: T.applied(this.applied), preview: this.preview, error: this.error,
      connected: this.client.connected, trusted: vscode.workspace.isTrusted, runId: this.runId,
      runs: (this.model.all?.runs || []).map(r => ({ id: r.id, title: r.title || r.id })),
      repositories: [...new Set((this.model.all?.workspaces || []).map(w => w.common_dir).filter(Boolean))],
      repoKey: this.applied?.context?.repo_key, story: this.story, noise: T.NOISE, qualification: T.QUALIFICATION });
  }
  revision(m) { if (!this.data || m.revision !== this.data.revision) throw new Error('Mods changed. Refresh and review the current bindings before trying again.'); return m.revision; }
  version(fp) { const v = this.data?.installed.find(v => v.fingerprint === fp); if (!v) throw new Error('That pinned version is no longer installed. Refresh the library.'); return v; }
  async confirm(title, detail, label) { return await vscode.window.showWarningMessage(title, { modal: true, detail }, label) === label; }
  async handle(m = {}) {
    if (MUTATIONS.has(m.action) && !vscode.workspace.isTrusted) throw new Error('Managing Mods requires a trusted workspace. This view is read-only.');
    if (m.action === 'ready' || m.action === 'refresh') return this.refresh();
    if (m.action === 'inspect') { this.runId = m.runId || undefined; return this.refresh(); }
    if (m.action === 'link') { const u = T.safeUrl(m.url); if (!u) throw new Error('Only HTTP and HTTPS source links can be opened.'); return vscode.env.openExternal(vscode.Uri.parse(u)); }
    if (!MUTATIONS.has(m.action)) return;
    if (!this.client.connected) throw new Error('Reconnect before managing Mods.');
    if (m.action === 'local') {
      const chosen = await vscode.window.showOpenDialog({ canSelectFolders: true, canSelectFiles: false, canSelectMany: false, openLabel: 'Preview text mod' });
      if (!chosen?.[0]) return this.push(); return this.handle({ action: 'preview', source: chosen[0].fsPath, operation: m.operation === 'update' ? 'update' : 'install' });
    }
    if (m.action === 'preview') {
      if (!['install', 'update'].includes(m.operation)) throw new Error('Choose install or update.');
      this.preview = await this.client.request('mods.preview', { source: String(m.source), operation: m.operation }); this.push(); return;
    }
    if (m.action === 'install') {
      const p = this.preview; if (!p || p.id !== m.previewId) throw new Error('Preview this version again before installing.');
      const op = p.operation === 'update' ? 'update' : 'install';
      if (!await this.confirm(`${op === 'update' ? 'Update' : 'Install'} ${p.version.manifest.name}?`, `Pinned fingerprint: ${p.fingerprint || p.version.fingerprint}.\nInstallation never enables a mod. Existing bindings stay pinned to their current version.`, op === 'update' ? 'Update' : 'Install')) return this.push();
      if (!vscode.workspace.isTrusted || !this.client.connected || this.preview !== p) throw new Error('Trust, connection or preview changed. Review a fresh preview.');
      await this.client.request('mods.install', { preview_id: p.id, confirm: true }); this.preview = undefined;
    } else if (m.action === 'bind') {
      const revision = this.revision(m), v = this.version(m.fingerprint);
      const existing = m.bindingId ? this.data.bindings.find(b => b.id === m.bindingId && b.fingerprint === v.fingerprint) : undefined;
      if (m.bindingId && !existing) throw new Error('Binding changed. Refresh before editing it.');
      const scope = existing?.scope || m.scope;
      if (!scope || !['all_agents', 'repository', 'watchers', 'agent', 'overseer'].includes(scope.kind)) throw new Error('Choose a supported scope.');
      const filters = m.filters || existing?.filters || { harnesses: [], accounts: [], models: [] };
      await this.client.request('mods.bind', { expected_revision: revision, binding: { id: existing?.id || null, mod_id: v.id, version: v.version, fingerprint: v.fingerprint, scope,
        enabled: m.enabled === true, required: m.required ?? existing?.required ?? false, locked: m.locked ?? existing?.locked ?? false, filters } });
    } else if (m.action === 'unbind') {
      const revision = this.revision(m); if (!this.data.bindings.some(b => b.id === m.bindingId)) throw new Error('Binding changed. Refresh before removing it.');
      await this.client.request('mods.unbind', { binding_id: m.bindingId, expected_revision: revision });
    } else if (m.action === 'remove') {
      const revision = this.revision(m), v = this.version(m.fingerprint);
      if (!await this.confirm(`Remove ${v.manifest.name} version ${v.version}?`, 'This removes its bindings for future turns. Active and historical turn snapshots remain. Instructions already in session history are not erased.', 'Remove')) return this.push();
      if (!vscode.workspace.isTrusted || !this.client.connected) throw new Error('Trust or connection changed. Review the library again.');
      this.revision(m); await this.client.request('mods.remove', { mod_id: v.id, fingerprint: v.fingerprint, confirm: true, expected_revision: revision });
    }
    return this.refresh();
  }
  dispose() { this.disposed = true; clearTimeout(this.timer); this.client.off('connected', this.onConnected); this.client.off('disconnected', this.onDisconnected); this.client.off('event', this.onEvent); this.panel?.dispose(); this.panel = undefined; }
}
module.exports = { ModsPanel };
