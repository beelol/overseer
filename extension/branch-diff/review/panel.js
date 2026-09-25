// Modified for Overseer from Branch Diff (Local) review/panel.js
// (https://github.com/beelol/branch-diff @ fbc6eb807fd41d8fd1a004977e1aa637a4f7c900, MIT).
// Changes: sessions are opened for an Overseer-selected worktree and comparison base
// (not the active editor's repository), the toolbar shows the comparison/base icon,
// and a Follow checkbox with pause/resume is wired to the host FollowController.
const vscode = require('vscode');
const { randomBytes } = require('crypto');
const { Comparison, contains } = require('./comparison');
const { Editing } = require('./editing');
const escapeAttribute = value => String(value).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

class ReviewManager {
  constructor(context, host) {
    this.editing = new Editing(context);
    this.context = context; this.host = host;
    this.sessions = new Map(); this.panels = new Map(); this.subscriptions = [];
    // Local reconciliation catches missed/excluded watcher events, only while a view is visible.
    this.poll = setInterval(() => {
      for (const session of this.sessions.values()) {
        if (!session.running && this.panels.get(session)?.visible) session.invalidate(true);
      }
    }, 2500);
    this.subscriptions.push(vscode.workspace.onDidChangeConfiguration(e => {
      if (e.affectsConfiguration('editor') || e.affectsConfiguration('workbench.colorTheme') || e.affectsConfiguration('overseer.review') || e.affectsConfiguration('files')) {
        for (const [session, panel] of this.panels) panel.webview.postMessage({ type: 'settings', settings: this.settings(session.repo) });
      }
    }));
  }

  settings(repo) {
    const config = vscode.workspace.getConfiguration('editor', repo.rootUri);
    const review = vscode.workspace.getConfiguration('overseer.review', repo.rootUri);
    const threshold = review.get('largeDiffThreshold', 500);
    return { enableEditing: review.get('enableEditing', true), largeDiffThreshold: Number.isSafeInteger(threshold) && threshold > 0 ? threshold : 500, fontFamily: config.get('fontFamily'), fontSize: config.get('fontSize', 14),
      fontLigatures: config.get('fontLigatures', false), tabSize: config.get('tabSize', 4), wordWrap: config.get('wordWrap', 'off') };
  }

  /** target: { repo, workspaceId, runId, comparison: { base, label, detail, mode } } */
  sessionFor(target) {
    const key = target.repo.rootUri.toString();
    let session = this.sessions.get(key);
    if (!session) {
      const holder = {};
      session = new Comparison(target.repo, { mode: 'workingTree', target: target.comparison.base }, this.host.helpers(holder));
      holder.session = session;
      this.sessions.set(key, session);
      this.subscriptions.push(session.onDidChange(snapshot => this.publish(session, snapshot)),
        session.onDidProgress(progress => this.panels.get(session)?.webview.postMessage({ type: 'progress', ...progress })));
    }
    session.overseer = { ...target, repo: undefined };
    session.configure('workingTree', target.comparison.base);
    return session;
  }

  panelFor(runId) {
    for (const [session, panel] of this.panels) if (session.overseer?.runId === runId) return { session, panel };
    return undefined;
  }

  publish(session, snapshot) {
    const panel = this.panels.get(session);
    if (panel) {
      const o = session.overseer || {};
      panel.title = `Review: ${o.runTitle || 'run'} (${snapshot.entries.length})`;
      panel.webview.postMessage(this.message(session, snapshot));
    }
  }

  overseerInfo(session) {
    const o = session.overseer || {};
    return { runId: o.runId, runTitle: o.runTitle, harness: o.harness, workspacePath: session.repo.rootUri.fsPath, workspaceKind: o.workspaceKind,
      comparison: o.comparison, follow: this.host.followState(o.runId), followNote: this.host.followNote(o.runId), reviewed: this.host.reviewedKeys(o.runId) };
  }

  message(session, snapshot) {
    return { type: 'snapshot', version: snapshot.version, description: snapshot.description, mode: snapshot.mode,
      repository: session.repo.rootUri.toString(), target: session.target || '', overseer: this.overseerInfo(session),
      checking: !!snapshot.checking, warning: snapshot.warning, error: snapshot.error, settings: this.settings(session.repo),
      entries: snapshot.entries.map(e => ({ id: e.id, path: e.relPath, status: this.host.statusLetter(e.status),
        pending: !!e.pending, unsaved: !!e.unsaved, revision: e.revision, problem: e.problem })) };
  }

  postOverseer(session) {
    this.panels.get(session)?.webview.postMessage({ type: 'overseer', overseer: this.overseerInfo(session) });
  }

  async open(target, { reveal = true, preserveFocus = false } = {}) {
    const session = this.sessionFor(target);
    let panel = this.panels.get(session);
    if (panel) {
      if (reveal) panel.reveal(undefined, preserveFocus);
      this.postOverseer(session);
      if (session.display) this.publish(session, session.display);
      return panel;
    }
    panel = vscode.window.createWebviewPanel('overseer.review', 'Review', { viewColumn: vscode.ViewColumn.One, preserveFocus }, { retainContextWhenHidden: false });
    return this.attach(session, panel);
  }

  async attach(session, panel, { waitForComparison = false } = {}) {
    const assets = vscode.Uri.joinPath(this.context.extensionUri, 'branch-diff', 'dist');
    panel.webview.options = { enableScripts: true, localResourceRoots: [assets] };
    this.panels.set(session, panel);
    const sendSnapshot = async () => {
      if (session.display) await panel.webview.postMessage(this.message(session, session.display));
      const snapshot = await session.ready();
      if (this.panels.get(session) === panel) await panel.webview.postMessage(this.message(session, snapshot));
    };
    const subscriptions = [
      panel.onDidChangeViewState(e => {
        if (e.webviewPanel.visible) { session.invalidate(true); sendSnapshot().catch(() => {}); }
      }),
      panel.webview.onDidReceiveMessage(async message => {
        try {
          if (!message || typeof message !== 'object') return;
          const send = value => { if (this.panels.get(session) === panel) panel.webview.postMessage(value).then(undefined, () => {}); };
          if (this.editing.receive(session, message, send)) return;
          if (message.type === 'ready') { this.editing.recoverStored(session).then(async drafts => send({ type: 'editRecovery', drafts, handedOff: await this.editing.handoffDrafts(session, message.drafts) })).catch(() => {}); await panel.webview.postMessage({ type: 'progress', stage: session.stage || 'Finding changed files…' }); await sendSnapshot(); this.host.followReady(session.overseer?.runId); return; }
          if (message.type === 'refresh') { session.invalidate(true); await sendSnapshot(); return; }
          if (message.type === 'pickComparison') { await this.host.pickComparison(session.overseer?.runId); return; }
          if (message.type === 'follow') { this.host.setFollow(session.overseer?.runId, message.enabled ? 'following' : 'off'); this.postOverseer(session); return; }
          if (message.type === 'followPause') { this.host.pauseFollow(session.overseer?.runId, String(message.reason || 'navigation')); this.postOverseer(session); return; }
          if (message.type === 'hunkReview') {
            await this.host.reviewHunk(session, message);
            send({ type: 'hunkReviewed', key: message.key, reviewed: !!message.reviewed });
            return;
          }
          if (message.type === 'followResume') { this.host.setFollow(session.overseer?.runId, 'following'); this.postOverseer(session); return; }
          if (!['body', 'open', 'openFile'].includes(message.type) || typeof message.id !== 'string' || !Number.isSafeInteger(message.version)) return;
          if (message.type === 'open' || message.type === 'openFile') {
            const entry = (session.display || await session.ready()).entries.find(e => e.id === message.id);
            if (!entry || entry.pending) return;
            if (message.type === 'open') await this.host.openFileDiff(entry, session);
            else if (contains(session.repo.rootUri, entry.uri)) {
              // Resolve only host-owned paths. A missing file must not become a new empty editor.
              const dirty = vscode.workspace.textDocuments.some(doc => doc.isDirty && doc.uri.toString() === entry.uri.toString());
              if (!dirty) {
                let stat;
                try { stat = await vscode.workspace.fs.stat(entry.uri); }
                catch (error) {
                  if (error.code !== 'FileNotFound') throw error;
                  throw new Error(`${entry.relPath} no longer exists in the working tree. Use Open in Native Diff to review its changes.`);
                }
                if (stat.type & vscode.FileType.Directory) throw new Error(`${entry.relPath} is a directory, not a file.`);
              }
              await vscode.commands.executeCommand('vscode.open', entry.uri, { viewColumn: vscode.ViewColumn.Beside, preview: false });
            }
          } else {
            const body = await session.body(message.id, message.version, message.revision);
            if (body && !body.problem) {
              const entry = session.display?.entries.find(e => e.id === message.id);
              body.editable = false;
              if (entry?.right && !entry.pending && !entry.symbolicLink && this.editing.enabled(session)) {
                try { await this.editing.writable(session, entry.uri); body.editable = true; } catch { /* The native editor explains unavailable write access. */ }
              }
              body.documentVersion = entry && vscode.workspace.textDocuments.find(doc => doc.uri.toString() === entry.uri.toString())?.version;
            }
            if (this.panels.get(session) !== panel) return;
            await panel.webview.postMessage(body ? { type: 'body', ...body, request: message.request } :
              { type: 'retry', id: message.id, version: message.version, request: message.request });
          }
        } catch (error) {
          if (this.panels.get(session) === panel) await panel.webview.postMessage({ type: 'notice', message: error.message || String(error) });
        }
      }),
    ];
    panel.onDidDispose(() => { this.editing.close(session); this.panels.delete(session); subscriptions.forEach(s => s.dispose()); });
    const nonce = randomBytes(24).toString('base64');
    const asset = name => escapeAttribute(panel.webview.asWebviewUri(vscode.Uri.joinPath(assets, name)));
    const baseIcon = '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 16 16"><circle cx="8" cy="8" r="3" fill="none" stroke="currentColor" stroke-width="1.5"/><path d="M8 1v4M8 11v4" stroke="currentColor" stroke-width="1.5"/></svg>';
    panel.webview.html = `<!doctype html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-${nonce}'; style-src ${panel.webview.cspSource} 'unsafe-inline'; font-src ${panel.webview.cspSource}; img-src ${panel.webview.cspSource} data:; worker-src blob:; connect-src 'none';">
<link rel="stylesheet" href="${asset('review.css')}"><title>Overseer Review</title></head>
<body data-run-id="${escapeAttribute(session.overseer?.runId || '')}" data-monaco="${asset('monaco.js')}" data-monaco-css="${asset('monaco.css')}" data-repository="${escapeAttribute(session.repo.rootUri.toString())}" data-mode="${escapeAttribute(session.mode)}" data-target="${escapeAttribute(session.target || '')}"><header id="toolbar"><button id="toggle-navigator" aria-label="Toggle file navigator" aria-expanded="true">Files</button><button id="base" class="base" title="Comparison base">${baseIcon}<span id="base-label">Comparison</span></button><strong id="comparison">Review</strong><span id="total"></span><span id="loading-stage" role="status"></span><span class="spacer"></span><label class="follow" title="Follow the agent's edits across and within files"><input type="checkbox" id="follow"> Follow</label><button id="resume" hidden>Resume Follow</button><span id="follow-state" role="status"></span><label>Diff layout <select id="layout"><option value="unified">Unified</option><option value="split">Split</option></select></label><button id="refresh">Refresh</button></header>
<div id="notice" role="status" hidden></div><div id="workspace-note" role="note"></div><main id="review"><nav id="navigator" aria-label="Changed files"><input id="filter" placeholder="Filter files…" aria-label="Filter changed files"><div id="tree" role="tree" aria-label="Changed file tree"></div></nav><div id="resize" role="separator" tabindex="0" aria-label="Resize file navigator" aria-orientation="vertical"></div><section id="diffs" aria-label="All file diffs" tabindex="0"><p class="empty" role="status">Finding changed files…</p></section></main>
<script type="module" nonce="${nonce}" src="${asset('review.js')}"></script></body></html>`;
    if (waitForComparison) await session.ready(); else session.ready().catch(() => {});
    return panel;
  }

  /** Called by the follow controller, or with `user: true` for an edit opened from the conversation. */
  reveal(runId, message) {
    const found = this.panelFor(runId);
    if (!found) return false;
    const entry = found.session.display?.entries.find(e => e.relPath === message.path);
    found.panel.webview.postMessage({ type: 'reveal', id: entry?.id, path: message.path, line: message.line, attribution: message.attribution, user: !!message.user });
    return true;
  }

  async deserializeWebviewPanel(panel, state) {
    try {
      const target = state && typeof state.repository === 'string' ? await this.host.restore(state) : undefined;
      if (!target) throw new Error('The saved review has no matching Overseer run. Select a run in the Overseer view to open its review.');
      const session = this.sessionFor(target);
      this.panels.get(session)?.dispose();
      await this.attach(session, panel);
    } catch (error) {
      // Explain instead of failing (for example a worktree that was cleaned up since).
      panel.webview.options = { enableScripts: false, localResourceRoots: [] };
      if (error.runTitle) panel.title = `Review: ${error.runTitle} (unavailable)`;
      panel.webview.html = `<!doctype html><html><head><meta charset="UTF-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline';"></head><body style="font-family:var(--vscode-font-family);color:var(--vscode-foreground);padding:16px"><h2 style="font-size:1.1em">Review unavailable</h2><p id="restore-error">${escapeAttribute(error.message)}</p></body></html>`;
    }
  }

  dispose() {
    this.disposed = true; clearInterval(this.poll);
    for (const panel of this.panels.values()) panel.dispose();
    for (const session of this.sessions.values()) session.dispose();
    this.subscriptions.forEach(d => d.dispose());
    this.sessions.clear(); this.panels.clear();
  }
}
module.exports = { ReviewManager };
