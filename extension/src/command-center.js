// Overseer editor view (AC-55, AC-58, AC-59; Gate K): one webview with the selected agent's chat,
// the new-agent composer or the agent grid, plus the files panel. The agents list is the side bar;
// arrangement.js places this view alone or beside the review. Not tied to the folder open in the
// window. Restored after reloads by a webview serializer.
const vscode = require('vscode');
const { RunFeed, runMessage } = require('./run-feed');
const { handleRunMessage, changesFetcher } = require('./run-actions');
const { page, localRoots } = require('./webview-html');
const { ACTIVE } = require('./views');
const Plain = require('../media/plain-words.js');


class CommandCenter {
  constructor(context, model, handlers) {
    this.context = context; this.model = model; this.handlers = handlers; this.client = handlers.client;
    this.changes = changesFetcher(this.client);
    this.mode = 'chat';
    this.activity = {}; // root run id -> { text, at }: what each agent is doing now (AC-228)
    model.onDidChange(() => this.push());
    this.client.on('event', event => {
      this.noteActivity(event);
      if (!this.panel) return;
      if (['file_activity', 'turn_done', 'status'].includes(event.kind) && this.chatRun && this.chatFeed?.roots.get(this.chatRun)?.has(event.run_id)) this.pushChanges();
      // Home's conversation (AC-182): what the daemon's session gained.
      if (['overseer_message', 'proposal', 'proposal_answered', 'overseer_level', 'overseer_session'].includes(event.kind)) this.pushOverseer();
      // A card's rows advance on its agents' turns, and a request's stage on their status (AC-228).
      else if (['turn_started', 'turn_done', 'queued', 'status', 'permission', 'permission_answered', 'task_created'].includes(event.kind)) this.pushOverseerSoon();
    });
    this.client.on('connected', () => this.pushOverseer());
  }

  get active() { return !!this.panel; }

  /** Opens (or moves) the view into an editor column; the arrangement decides which (Gate K). */
  async open({ column = vscode.ViewColumn.One, preserveFocus = false, reveal = true } = {}) {
    // VS Code restores a webview tab lazily: until the tab is shown there is no panel yet. Show the
    // restored tab (it then comes back through deserializeWebviewPanel) rather than open a copy.
    if (!this.panel) await this.showRestoredTab();
    if (this.panel) {
      if (reveal && (this.panel.viewColumn !== column || !this.panel.visible)) this.panel.reveal(column, preserveFocus);
      return this.panel;
    }
    const panel = vscode.window.createWebviewPanel('overseer.center', 'Overseer', { viewColumn: column, preserveFocus }, { enableScripts: true, retainContextWhenHidden: true, localResourceRoots: localRoots(this.context.extensionUri) });
    this.attach(panel);
    return panel;
  }

  attach(panel) {
    this.panel = panel;
    panel.webview.options = { enableScripts: true, localResourceRoots: localRoots(this.context.extensionUri) };
    panel.iconPath = vscode.Uri.joinPath(this.context.extensionUri, 'media', 'overseer-logo.png');
    // The mark's layers for Voice Mode's stage (AC-227: the voice view is this view).
    const layer = name => panel.webview.asWebviewUri(vscode.Uri.joinPath(this.context.extensionUri, 'media', 'voice', name)).toString();
    const layers = JSON.stringify({ core: layer('overseer-logo-core.png'), swooshes: layer('overseer-logo-swooshes.png'), star: layer('overseer-logo-star.png'), flat: layer('overseer-logo-flat.png') }).replace(/'/g, '&#39;');
    panel.webview.html = page(panel.webview, this.context.extensionUri, { title: 'Overseer', chat: true, css: ['dashboard.css', 'voice.css'], js: ['composer.js', 'voice-mark.js', 'voice.js', 'rollup.js', 'home.js', 'grid.js', 'dashboard.js'], bodyAttrs: `data-layers='${layers}'` });
    const post = m => panel.webview.postMessage(m);
    this.chatFeed = new RunFeed(this.client, this.model, m => post({ ...m, channel: 'chat' }));
    this.gridFeed = new RunFeed(this.client, this.model, m => post({ ...m, channel: 'grid' }));
    // Remembered for the next start (AC-80): was the Overseer editor open when VS Code closed?
    this.context.workspaceState.update('overseer.editorOpen', true);
    panel.onDidDispose(() => {
      this.chatFeed.dispose(); this.gridFeed.dispose();
      // Closing the window also disposes panels; only a user's close (window still running) counts.
      setTimeout(() => { if (!this.shuttingDown && !this.panel) this.context.workspaceState.update('overseer.editorOpen', false); }, 1500);
      if (this.panel === panel) { this.panel = undefined; this.chatRun = undefined; }
      vscode.commands.executeCommand('setContext', 'overseer.dashboardOpen', false);
    });
    panel.onDidChangeViewState(e => vscode.commands.executeCommand('setContext', 'overseer.dashboardFocus', e.webviewPanel.active));
    vscode.commands.executeCommand('setContext', 'overseer.dashboardOpen', true);
    panel.webview.onDidReceiveMessage(message => this.receive(message).catch(error => {
      if (message?.type === 'start') post({ type: 'notice', scope: 'composer', message: Plain.plain(error.message, 300) });
      else post({ type: 'notice', message: Plain.plain(error.message, 300) });
    }));
  }

  async receive(m) {
    if (!m || typeof m !== 'object') return;
    const post = x => this.panel?.webview.postMessage(x);
    switch (m.type) {
      // Opening home is a visit: what happened while the owner was away leads (AC-253).
      case 'ready': if (this.pendingTarget) post({ type: 'composerTarget', target: this.pendingTarget }); await this.push(); await this.client.request('overseer.visit', { surface: 'vscode' }).catch(() => undefined); await this.pushOverseer(); this.pushVoice(); this.voiceSource?.refresh(); this.pushActivity(); if (this.inDashboard) post({ type: 'dashboard', on: true }); if (this.aside) post({ type: 'aside', on: true }); return;
      // Voice Mode's controls in the view (AC-227).
      case 'voiceMute': await vscode.commands.executeCommand('overseer.voice.mute'); return;
      case 'voiceToggle': await vscode.commands.executeCommand('overseer.voice.toggle'); return;
      case 'voiceTarget': await vscode.commands.executeCommand('overseer.voice.talkTo'); return;
      case 'voiceCancel': await this.client.request('voice.cancel', { id: String(m.id || '') }); return;
      case 'voiceAnswer': await this.voiceSource?.answer(!!m.yes); return;
      // A card, a stage or a Needs-you item opens its agent or its work (AC-226, AC-227).
      case 'openAgent': if (typeof m.runId === 'string' && m.runId) await this.handlers.openAgent?.(m.runId, { work: !!m.work, from: m.from }); return;
      case 'aside': this.aside = !!m.on; if (!m.on) post({ type: 'aside', on: false }); return;
      case 'asideShown': this.aside = !!m.on; return;
      // Home (AC-182): the one conversation with Overseer, from the daemon.
      case 'overseerSend': {
        const text = String(m.text || '').trim(); if (!text) return;
        const cfg = vscode.workspace.getConfiguration('overseer');
        try { await this.client.request('overseer.send', { text, surface: 'vscode', harness: cfg.get('chat.harness', 'claude'), ...(cfg.get('chat.model', '') ? { model: cfg.get('chat.model', '') } : {}) }); }
        catch (error) { post({ type: 'overseerNotice', message: Plain.plain(error.message, 300) }); }
        await this.model.refresh(); await this.pushOverseer(); return;
      }
      case 'overseerAnswer': {
        try { await this.client.request('overseer.answer', { id: String(m.id || ''), yes: !!m.yes, surface: 'vscode', by: 'owner' }); }
        catch (error) { post({ type: 'overseerNotice', id: m.id, message: Plain.plain(error.message, 300) }); }
        await this.pushOverseer(); return;
      }
      case 'overseerFresh': await this.client.request('overseer.fresh', {}); await this.pushOverseer(); return;
      case 'overseerUndoStart': {
        // Meant for Overseer: stop the agent just started, remove its untouched worktree, and put the words back for Overseer.
        const runId = String(m.runId || ''); const run = this.model.run(runId);
        if (run) {
          try { if (ACTIVE.has(run.status)) await this.client.request('run.interrupt', { run_id: runId }); } catch { /* already done */ }
          for (let i = 0; i < 40 && ACTIVE.has(this.model.run(runId)?.status); i++) { await new Promise(r => setTimeout(r, 250)); await this.model.refresh(); }
          try {
            const changes = await this.client.request('workspace.changes', { workspace_id: run.workspace_id });
            if (!changes.files) { await this.client.request('task.archive', { task_id: run.task_id, archived: true }); await this.client.request('workspace.cleanup', { workspace_id: run.workspace_id, discard_dirty: false }); }
          } catch (error) { post({ type: 'overseerNotice', message: `The agent was stopped; its worktree stays: ${Plain.plain(error.message, 300)}` }); }
          await this.model.refresh();
        }
        post({ type: 'askOverseer', text: String(m.text || '') });
        return;
      }
      case 'measured': { const done = this.measuring?.get(m.id); if (done) { this.measuring.delete(m.id); done({ w: Number(m.w), h: Number(m.h) }); } return; }
      case 'select': if (typeof m.runId === 'string') { if (m.restore) await this.showChat(m.runId); else await this.handlers.select(m.runId); } return;
      case 'mode': { const was = this.mode; this.mode = m.mode; if (was !== m.mode) await this.handlers.onMode?.(m.mode, was); return; }
      case 'focusComposer': post({ type: 'mode', mode: 'composer' }); return;
      case 'gridEmpty': if (this.mode === 'grid') await this.handlers.gridEmpty?.(); return;
      case 'track': if (typeof m.runId === 'string') await this.handlers.track?.(m.runId); return;
      case 'untrack': await this.handlers.untrack?.(); return;
      case 'gridSubscribe': {
        const ids = (m.runIds || []).filter(id => this.model.run(id));
        // Metadata first: the tile needs its root run id before history arrives.
        for (const id of ids) { const msg = runMessage(this.model, id, this.handlers.steering); if (msg) post({ type: 'run', channel: 'grid', ...msg }); }
        await this.gridFeed.set(ids, { limit: 400 });
        return;
      }
      // Home talks to Overseer first (AC-236); "Start directly" is remembered for the owner.
      case 'composerData': post({ type: 'composerData', data: { ...await this.handlers.launcher.data(), sendTo: vscode.workspace.getConfiguration('overseer').get('home.sendTo', 'overseer') === 'agent' ? 'agent' : 'overseer' } }); return;
      case 'composerTargetSeen': this.pendingTarget = undefined; return;
      case 'composerSendTo': await vscode.workspace.getConfiguration('overseer').update('home.sendTo', m.target === 'agent' ? 'agent' : 'overseer', vscode.ConfigurationTarget.Global); return;
      case 'composerDefaults': await this.handlers.launcher.saveDefaults(m.defaults || {}); return;
      case 'composerBrowse': { const repo = await this.handlers.launcher.browse(); if (repo) post({ type: 'notice', scope: 'composer', kind: 'repo', repo }); return; }
      // The repository chip's own picker (AC-260): a typed path, and its Tab completions; no dialog.
      case 'composerAddRepo': {
        try { post({ type: 'notice', scope: 'composer', kind: 'repo', repo: await this.handlers.launcher.addRepo(m.path) }); }
        catch (error) { post({ type: 'notice', scope: 'composer', kind: 'repoError', path: m.path, message: Plain.plain(error.message, 300) }); }
        return;
      }
      case 'composerPathHints': post({ type: 'notice', scope: 'composer', kind: 'pathHints', input: m.input, hints: await this.handlers.launcher.pathHints(m.input) }); return;
      case 'composerBranches': post({ type: 'notice', scope: 'composer', kind: 'branches', branches: await this.handlers.launcher.branches(m.repo) }); return;
      case 'composerModel': {
        const model = await vscode.window.showInputBox({ title: 'Model', prompt: 'Any model name this harness accepts. Leave empty for the default.', value: m.current || '' });
        if (model !== undefined) post({ type: 'notice', scope: 'composer', kind: 'model', model: model.trim() });
        return;
      }
      case 'start': {
        const runId = await this.handlers.launcher.start(m.form || {});
        if (!runId) { post({ type: 'notice', scope: 'composer', message: 'Not started.' }); return; }
        post({ type: 'notice', scope: 'composer', kind: 'started', runId });
        await this.handlers.select(runId, { follow: vscode.workspace.getConfiguration('overseer').get('followNewRuns', true) });
        return;
      }
      case 'mentionFiles': await handleRunMessage(this.handlers, this.chatRun, { ...m, scope: m.scope || 'composer' }, x => post(x)); return;
      case 'pin': await this.handlers.setPinned(m.runId, !!m.on); await this.push(); return;
      case 'archive': await this.client.request('task.archive', { task_id: String(m.taskId), archived: m.archived !== false }); await this.model.refresh(); return;
      case 'search': post({ type: 'searchHits', q: m.q, taskIds: await this.handlers.search(String(m.q || '')) }); return;
      case 'command': await vscode.commands.executeCommand(String(m.command), ...(m.args !== undefined ? [m.args] : [])); return;
      case 'openExternal': { const url = String(m.url || ''); if (/^https?:\/\//i.test(url)) await vscode.env.openExternal(vscode.Uri.parse(url)); return; }
      case 'exitDashboard': await vscode.commands.executeCommand('overseer.exitDashboard'); return;
      case 'dashboardWindow': await vscode.commands.executeCommand('overseer.openDashboardWindow'); return;
      case 'cleanupArchived': await vscode.commands.executeCommand('overseer.cleanupArchived'); return;
      default: {
        const runId = typeof m.runId === 'string' ? m.runId : this.chatRun;
        if (!runId) return;
        await handleRunMessage(this.handlers, runId, m, x => post(x));
      }
    }
  }

  /** Voice Mode (Gate R): home's voice strip and the voice mark on the grid's tiles. */
  pushVoice() {
    if (!this.panel || !this.voiceSource) return;
    this.panel.webview.postMessage({ type: 'voice', voice: this.voiceSource.summary(), targets: [...this.voiceSource.targeted] });
  }

  /** Voice Mode's stage in the view (the mark, the words, the spoken requests' states). */
  postVoice(m) { this.panel?.webview.postMessage({ type: 'voiceView', m }); }

  /** Beside the agent a request started (AC-226): its chat on the left, the view slid right. */
  setAside(on, runId) { this.aside = !!on; this.panel?.webview.postMessage({ type: 'aside', on: !!on, runId }); }

  /** What an agent is doing now: its last tool, file or words (AC-228's live activity). */
  noteActivity(event) {
    if (!event || !event.run_id || !['tool', 'file_activity', 'output'].includes(event.kind)) return;
    const p = event.payload || {};
    let text = '';
    if (event.kind === 'tool') {
      // A tool's summary may be its raw input: say what it touches in plain words (AC-228).
      let what = p.summary || '';
      if (/^\s*\{/.test(what)) { try { const x = JSON.parse(what); what = x.command || x.file_path || x.path || x.pattern || x.description || x.query || ''; } catch { const m = /"(?:file_path|path|command)"\s*:\s*"([^"]*)/.exec(what); what = m ? m[1] : ''; } }
      if (/^\//.test(what) || /\/[^\s]+$/.test(what)) what = String(what).split('/').pop();
      text = [p.name, what].filter(Boolean).join(': ');
    }
    else if (event.kind === 'file_activity') text = `Editing ${(p.paths || []).map(x => String(x).split('/').pop()).slice(0, 2).join(', ')}`;
    else if (p.role === 'assistant' && p.text) text = String(p.text).split('\n').find(l => l.trim()) || '';
    if (!text) return;
    const run = this.model.run(event.run_id);
    const root = (run && this.model.rootRun(run)?.id) || event.run_id;
    this.activity[root] = { text: text.length > 70 ? text.slice(0, 69).trimEnd() + '…' : text, at: event.ts || Date.now() };
    clearTimeout(this.activityTimer);
    this.activityTimer = setTimeout(() => this.pushActivity(), 400);
  }
  pushActivity() { this.panel?.webview.postMessage({ type: 'activity', runs: this.activity }); }

  pushOverseerSoon() { clearTimeout(this.overseerTimer); this.overseerTimer = setTimeout(() => this.pushOverseer(), 250); }

  /** Home's conversation: the daemon's session, whole (its messages are few and their ids stable). */
  async pushOverseer() {
    if (!this.panel || !this.client.connected) return;
    if (this.pushingOverseer) { this.pushOverseerAgain = true; return; }
    this.pushingOverseer = true;
    try {
      const session = await this.client.request('overseer.session', {});
      this.panel?.webview.postMessage({ type: 'overseer', session });
    } catch (error) { this.handlers.log?.('home: ' + error.message); }
    this.pushingOverseer = false;
    if (this.pushOverseerAgain) { this.pushOverseerAgain = false; await this.pushOverseer(); }
  }

  /** Shows a run's chat in the dashboard: its metadata, history and live events. */
  async showChat(runId) {
    if (!this.panel || !this.model.run(runId)) return;
    this.chatRun = runId;
    const msg = runMessage(this.model, runId, this.handlers.steering);
    this.panel.webview.postMessage({ type: 'run', channel: 'chat', ...msg });
    await this.chatFeed.set([runId]);
    this.pushChanges(true);
  }

  async pushChanges(force) {
    const run = this.chatRun && this.model.run(this.chatRun);
    if (!run || run.parent_run_id) return;
    const changes = await this.changes(run.workspace_id, { force });
    if (changes) this.panel?.webview.postMessage({ type: 'changes', runId: run.id, changes });
  }

  async push() {
    if (!this.panel) return;
    const { tasks, runs, workspaces, profiles } = this.model.state;
    const state = { tasks, runs, workspaces, profiles, oversight: this.model.state.oversight || {}, overseer: this.model.state.overseer || {}, accounts: this.handlers.launcher.accounts(), attention: this.handlers.attention(), rollup: this.handlers.rollup?.(), pinned: this.handlers.pinned(),
      gridMax: Math.max(1, Math.min(16, vscode.workspace.getConfiguration('overseer').get('grid.maxTiles', 6))), archived: this.handlers.archived() };
    await this.panel.webview.postMessage({ type: 'state', state, selected: this.handlers.selected() });
    if (this.chatRun) { const msg = runMessage(this.model, this.chatRun, this.handlers.steering); if (msg) { this.chatFeed.refreshDescendants(); this.panel.webview.postMessage({ type: 'run', channel: 'chat', ...msg }); } }
    for (const id of this.gridFeed?.roots.keys() || []) { const msg = runMessage(this.model, id, this.handlers.steering); if (msg) this.panel.webview.postMessage({ type: 'run', channel: 'grid', ...msg }); }
  }

  /** Shows an agent's chat (host-driven selection from the side bar, commands or keys). */
  async select(runId) {
    if (!this.panel) return;
    this.panel.webview.postMessage({ type: 'selected', runId });
    await this.showChat(runId);
  }
  selected(runId) { this.panel?.webview.postMessage({ type: 'selected', runId }); }
  setDashboard(on) { this.inDashboard = on; this.panel?.webview.postMessage({ type: 'dashboard', on }); }

  /** The dashboard webview's size (dashboard mode uses it to see which parts were open). */
  measure() {
    if (!this.panel) return Promise.resolve(undefined);
    const id = Math.random().toString(36).slice(2);
    return new Promise(resolve => {
      const timer = setTimeout(() => { this.measuring?.delete(id); resolve(undefined); }, 1500);
      (this.measuring ||= new Map()).set(id, size => { clearTimeout(timer); resolve(size); });
      this.panel.webview.postMessage({ type: 'measure', id });
    });
  }
  setMode(mode) { this.panel?.webview.postMessage({ type: 'mode', mode }); }
  focus(target) { this.panel?.webview.postMessage({ type: 'focus', target }); }
  /** Where home's box sends this time (AC-236): kept until the view is ready to hear it. */
  composerTarget(target) { this.pendingTarget = target; this.panel?.webview.postMessage({ type: 'composerTarget', target }); }

  async deserializeWebviewPanel(panel) {
    // One Overseer view per window: a second restored copy (from an earlier session) is closed.
    if (this.panel && this.panel !== panel) { panel.dispose(); return; }
    this.attach(panel);
  }

  /** Brings a restored-but-not-yet-shown Overseer tab forward and waits for VS Code to hand it over. */
  async showRestoredTab() {
    for (const group of vscode.window.tabGroups.all) {
      const index = group.tabs.findIndex(t => t.input?.viewType?.endsWith('overseer.center'));
      if (index < 0) continue;
      const focus = ['workbench.action.focusFirstEditorGroup', 'workbench.action.focusSecondEditorGroup', 'workbench.action.focusThirdEditorGroup'][group.viewColumn - 1];
      if (!focus) return;
      await vscode.commands.executeCommand(focus);
      await vscode.commands.executeCommand('workbench.action.openEditorAtIndex', index);
      for (let i = 0; i < 40 && !this.panel; i++) await new Promise(r => setTimeout(r, 50));
      return;
    }
  }
}

module.exports = { CommandCenter, ACTIVE };
