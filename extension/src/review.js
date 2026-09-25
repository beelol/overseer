// Connects the vendored Branch Diff review to Overseer: the selected run's worktree,
// daemon-computed comparisons (latest run / task start / fork / branch) and Follow.
const vscode = require('vscode');
const path = require('path');
const fs = require('fs').promises;
const { execFile } = require('child_process');
const { ReviewManager } = require('../branch-diff/review/panel');

// vscode.git Status values used by the vendored comparison code.
const STATUS = { A: 1, D: 6, R: 3, M: 5, T: 5, C: 1, U: 5 };
function statusLetter(s) {
  if (s === 1 || s === 9) return 'A';
  if (s === 2 || s === 6) return 'D';
  if (s === 7) return 'U';
  if (s === 3) return 'R';
  return 'M';
}

function gitShow(root, spec) {
  return new Promise((resolve, reject) => execFile('git', ['show', spec], { cwd: root, maxBuffer: 64 * 1024 * 1024, encoding: 'buffer' }, (err, out) => err ? reject(err) : resolve(out)));
}

class Review {
  constructor(context, client, model, log) {
    this.context = context; this.client = client; this.model = model; this.log = log;
    // Comparison choice and Follow survive reloads and restarts (workspace state). Follow is
    // never auto-resumed: a run that was being followed comes back paused.
    this.comparisons = new Map(Object.entries(context.workspaceState.get('overseer.comparisons', {}))); // runId -> { mode, branch }
    this.follow = new Map(); // runId -> 'off' | 'following' | 'paused'
    this.followNotes = new Map();
    for (const [runId, state] of Object.entries(context.workspaceState.get('overseer.follow', {}))) {
      if (state === 'following' || state === 'paused') { this.follow.set(runId, 'paused'); this.followNotes.set(runId, 'Follow was on before VS Code reloaded. It stays paused until you resume it.'); }
    }
    this.observed = new Map(); // abs path -> last observed text (bounded)
    this.userSaves = new Map(); // abs path -> ms of last user save
    this.lastReveal = new Map(); // runId -> last reveal message
    this.manager = new ReviewManager(context, {
      helpers: holder => this.helpers(holder),
      statusLetter,
      openFileDiff: (entry, session) => this.openFileDiff(entry, session),
      pickComparison: runId => this.pickComparison(runId),
      followState: runId => this.follow.get(runId) || 'off',
      followNote: runId => this.followNotes.get(runId),
      setFollow: (runId, state) => this.setFollow(runId, state),
      pauseFollow: (runId, reason) => this.pauseFollow(runId, reason),
      followReady: runId => { const last = this.lastReveal.get(runId); if (last && this.follow.get(runId) === 'following') this.manager.reveal(runId, last); },
      restore: state => this.restore(state),
      reviewedKeys: runId => Object.keys(this.reviewed()[runId] || {}),
      reviewHunk: (session, message) => this.reviewHunk(session, message),
    });
    context.subscriptions.push(this.manager,
      vscode.window.registerWebviewPanelSerializer('overseer.review', this.manager),
      vscode.workspace.registerTextDocumentContentProvider('overseer-git', { provideTextDocumentContent: uri => this.provideGitContent(uri) }),
      vscode.workspace.onDidSaveTextDocument(doc => this.userSaves.set(doc.uri.fsPath, Date.now())),
      vscode.workspace.onDidChangeTextDocument(e => { if (e.document.uri.scheme === 'file' && e.contentChanges.length) this.userSaves.set(e.document.uri.fsPath, Date.now()); }));
    client.on('event', event => this.onEvent(event).catch(error => this.log('follow: ' + error.message)));
  }

  helpers(holder) {
    const self = this;
    return {
      skipRepoStatus: true,
      async comparisonKey(repo, target) {
        return JSON.stringify([target, repo.state.HEAD?.commit, holder.session?.overseer?.workspaceId]);
      },
      async resolveContext(target, repo) {
        const git = await self.gitApi();
        const c = holder.session?.overseer?.comparison;
        if (!c || !c.base) throw new Error(c?.detail ? `Comparison unavailable: ${c.detail}` : 'No comparison base selected.');
        return { git, repo, base: c.label, mergeBase: c.base };
      },
      async getChangeEntries(git, repo, mode, base) {
        const workspaceId = holder.session?.overseer?.workspaceId;
        const diff = await self.client.request('workspace.diff', { workspace_id: workspaceId, base, status: false });
        const root = repo.rootUri;
        return diff.changes.map(change => {
          const uri = vscode.Uri.joinPath(root, ...change.path.split('/'));
          const original = change.old_path ? vscode.Uri.joinPath(root, ...change.old_path.split('/')) : uri;
          const status = STATUS[change.status] ?? 5;
          return { uri, relPath: change.path, status,
            left: change.status === 'A' ? null : git.toGitUri(original, base),
            right: change.status === 'D' ? null : uri };
        });
      },
      async describeComparison(repo, base, mergeBase, mode) {
        return { base, mergeBase, headName: repo.state.HEAD?.name, headSha: repo.state.HEAD?.commit, mode };
      },
    };
  }

  async gitApi() {
    const ext = vscode.extensions.getExtension('vscode.git');
    if (!ext) throw new Error('The built-in Git extension is not available.');
    const api = (await ext.activate()).getAPI(1);
    if (api.state !== 'initialized') await new Promise(resolve => { const d = api.onDidChangeState(s => { if (s === 'initialized') { d.dispose(); resolve(); } }); setTimeout(resolve, 5000); });
    return api;
  }

  async repoFor(workspacePath) {
    const git = await this.gitApi();
    const uri = vscode.Uri.file(workspacePath);
    let repo = git.repositories.find(r => r.rootUri.fsPath === workspacePath);
    // Worktrees outside the open folders are opened explicitly by path; never substitute another repo.
    if (!repo) repo = await git.openRepository(uri);
    if (!repo || repo.rootUri.fsPath !== workspacePath) {
      const real = await fs.realpath(workspacePath);
      if (!repo || repo.rootUri.fsPath !== real) throw new Error(`Could not open the Git repository at ${workspacePath}.`);
    }
    return repo;
  }

  // ------------------------------------------------------------ reviewed hunks (AC-42)

  reviewed() { return this.context.workspaceState.get('overseer.reviewedHunks', {}); }

  /**
   * Accept = mark a hunk reviewed (no Git staging). Keys hash the hunk's base and working text,
   * so a hunk that changes again is simply no longer reviewed. The hunk is re-checked against
   * the current text first: if the agent changed it meanwhile, that is a conflict.
   */
  async reviewHunk(session, msg) {
    const runId = session.overseer?.runId;
    if (!runId || !/^[a-f0-9]{16}$/.test(String(msg.key)) || typeof msg.path !== 'string') throw new Error('Invalid hunk.');
    const all = this.reviewed();
    const run = { ...(all[runId] || {}) };
    if (!msg.reviewed) { delete run[msg.key]; }
    else {
      const uri = vscode.Uri.joinPath(session.repo.rootUri, ...msg.path.split('/'));
      if (path.relative(session.repo.rootUri.fsPath, uri.fsPath).startsWith('..')) throw new Error('File is outside this review.');
      const open = vscode.workspace.textDocuments.find(d => d.uri.toString() === uri.toString());
      // A clean document may lag an agent's write by a moment; disk is the truth unless it has unsaved edits.
      const text = open?.isDirty ? open.getText() : await fs.readFile(uri.fsPath, 'utf8').catch(() => '');
      const lines = text.split(/\r?\n/);
      const modified = Array.isArray(msg.modified) ? msg.modified : [];
      const start = Number(msg.modifiedStart) || 0, end = Number(msg.modifiedEnd) || 0;
      const same = end ? lines.slice(start - 1, end).join('\n') === modified.join('\n') : (start === 0 || lines[start - 1] === msg.anchor);
      if (!same) throw new Error(`Not marked reviewed: ${msg.path} changed while you were accepting this hunk (conflict). Review the current content.`);
      run[msg.key] = { path: msg.path, at: Date.now() };
    }
    const next = { ...all, [runId]: run };
    // Bound the stored state: the newest 2,000 hunks per run and 200 runs.
    for (const id of Object.keys(next)) { const entries = Object.entries(next[id]).sort((a, b) => b[1].at - a[1].at).slice(0, 2000); next[id] = Object.fromEntries(entries); }
    const runs = Object.keys(next).slice(-200);
    await this.context.workspaceState.update('overseer.reviewedHunks', Object.fromEntries(runs.map(id => [id, next[id]])));
  }

  persistFollow() {
    const saved = {};
    for (const [runId, state] of this.follow) if (state !== 'off') saved[runId] = state;
    this.context.workspaceState.update('overseer.follow', saved);
  }

  setComparison(runId, option) {
    this.comparisons.set(runId, { mode: option.mode, branch: option.branch });
    this.context.workspaceState.update('overseer.comparisons', Object.fromEntries([...this.comparisons].slice(-200)));
  }

  /** Why a run's review cannot open, or undefined when its workspace is usable. */
  unavailable(run, ws) {
    if (!ws) return `The workspace for "${run.title}" no longer exists in Overseer's records.`;
    if (!ws.removed_ms) return undefined;
    return `The worktree for "${run.title}" (${ws.path}) was removed on ${new Date(ws.removed_ms).toLocaleString()}.` +
      (ws.branch ? ` Its branch ${ws.branch} was kept, so the commits are still in the repository.` : '') + ' The run panel still has its history.';
  }

  async options(runId, branch) {
    return this.client.request('comparison.options', { run_id: runId, branch });
  }

  async currentComparison(runId) {
    let selected = this.comparisons.get(runId);
    const opts = await this.options(runId, selected?.branch);
    if (selected) {
      const fresh = opts.options.find(o => o.mode === selected.mode && (o.branch || '') === (selected.branch || ''));
      if (fresh) return fresh;
    }
    return opts.options.find(o => o.default) || opts.options[0];
  }

  async open(runId, { preserveFocus = false, follow } = {}) {
    const run = this.model.run(runId);
    if (!run) throw new Error('Unknown run.');
    const ws = this.model.workspace(run.workspace_id);
    const why = this.unavailable(run, ws);
    if (why) throw new Error(why);
    const repo = await this.repoFor(ws.path);
    const comparison = await this.currentComparison(runId);
    if (follow !== undefined) { this.follow.set(runId, follow ? 'following' : 'off'); this.persistFollow(); }
    if (String(run.capabilities?.file_activity || '').startsWith('unknown')) this.followNotes.set(runId, 'Filesystem evidence only: this harness does not report its edits, so Follow cannot attribute or jump to them. The file list still refreshes live.');
    return this.manager.open({ repo, workspaceId: ws.id, runId, runTitle: run.title, harness: run.harness, workspaceKind: ws.kind, comparison }, { preserveFocus });
  }

  /** Opens the run's review at the first changed hunk of `rel` (a file edit clicked in the conversation). */
  async revealEdit(runId, rel) {
    await this.open(runId);
    const run = this.model.run(runId);
    const ws = this.model.workspace(run.workspace_id);
    const base = (await this.currentComparison(runId).catch(() => undefined))?.base;
    let line = 1;
    if (base && rel && !rel.startsWith('/') && !rel.startsWith('..')) {
      const diff = await new Promise(resolve => execFile('git', ['diff', '-U0', '--no-color', '--no-ext-diff', base, '--', rel], { cwd: ws.path, maxBuffer: 8 * 1024 * 1024 }, (err, out) => resolve(err ? '' : out)));
      const hunk = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@/m.exec(diff);
      if (hunk) line = Math.max(1, Number(hunk[1]) || 1);
      else if (!diff) {
        // Untracked files are not in `git diff`; they are new, so the hunk starts at line 1.
      }
    }
    const message = { path: rel, line, attribution: 'opened from the conversation', user: true };
    // A freshly opened review may not have its file list yet; the webview keeps it pending.
    this.manager.reveal(runId, message);
    return message;
  }

  async pickComparison(runId) {
    if (!runId) return;
    const opts = await this.options(runId, this.comparisons.get(runId)?.branch);
    const items = opts.options.map(o => ({ label: `$(git-compare) ${o.label}`, description: o.available ? (o.base || '').slice(0, 10) : 'unavailable', detail: o.detail || '', option: o }));
    items.push({ label: '$(git-branch) Other branch…', detail: 'Choose a branch for merge-base (PR-style) or direct tip comparison', other: true });
    const pick = await vscode.window.showQuickPick(items, { title: 'Compare the working tree with…', matchOnDetail: true });
    if (!pick) return;
    let option = pick.option;
    if (pick.other) {
      const branch = await vscode.window.showQuickPick(opts.branches, { title: 'Branch to compare with' });
      if (!branch) return;
      const kind = await vscode.window.showQuickPick([{ label: 'Merge-base (PR-style)', mode: 'branch_merge_base' }, { label: 'Branch tip (direct)', mode: 'branch_tip' }], { title: `Compare with ${branch}` });
      if (!kind) return;
      const withBranch = await this.options(runId, branch);
      option = withBranch.options.find(o => o.mode === kind.mode && o.branch === branch) || withBranch.options.find(o => o.mode === 'branch_merge_base');
    }
    if (!option) return;
    if (!option.available) { vscode.window.showWarningMessage(`${option.label} is unavailable: ${option.detail}`); return; }
    this.setComparison(runId, option);
    await this.open(runId);
  }

  async openFileDiff(entry, session) {
    const name = entry.relPath.split('/').pop();
    const empty = vscode.Uri.from({ scheme: 'overseer-git', path: '/' + name, query: JSON.stringify({ empty: true }) });
    await vscode.commands.executeCommand('vscode.diff', entry.left || empty, entry.right || empty, `${name} (${session.overseer?.comparison?.label || 'review'})`);
  }

  provideGitContent(uri) {
    const q = JSON.parse(uri.query || '{}');
    if (q.empty) return '';
    return gitShow(q.root, `${q.ref}:${q.path}`).then(b => b.toString('utf8'), () => '');
  }

  /** Native diffs for the Workspace Dirty layers. The right side is the editable file when it is the working tree. */
  async openDirtyDiff(kind, root, file) {
    const rel = file.path;
    const abs = vscode.Uri.file(path.join(root, rel));
    const at = ref => vscode.Uri.from({ scheme: 'overseer-git', path: '/' + rel, query: JSON.stringify({ root, ref, path: rel }) });
    const name = path.basename(rel);
    if (kind === 'staged') await vscode.commands.executeCommand('vscode.diff', at('HEAD'), at(''), `${name} (HEAD ↔ index, staged)`);
    else if (kind === 'unstaged') await vscode.commands.executeCommand('vscode.diff', at(''), abs, `${name} (index ↔ working tree, unstaged)`);
    else await vscode.commands.executeCommand('vscode.open', abs);
  }

  async restore(state) {
    // Serializers run during activation, possibly before the daemon connection is up.
    await this.client.waitConnected(20000);
    await this.model.refresh();
    let run = state.runId && this.model.run(state.runId);
    if (!run && !state.runId) {
      // Reviews saved before run ids were recorded: the newest top-level run in that worktree.
      const repoPath = vscode.Uri.parse(state.repository).fsPath;
      run = this.model.state.runs.filter(r => !r.parent_run_id && this.model.workspace(r.workspace_id)?.path === repoPath).sort((a, b) => b.created_ms - a.created_ms)[0];
    }
    if (!run) return undefined;
    const ws = this.model.workspace(run.workspace_id);
    const why = this.unavailable(run, ws);
    if (why) throw Object.assign(new Error(why), { runTitle: run.title });
    return { repo: await this.repoFor(ws.path), workspaceId: ws.id, runId: run.id, runTitle: run.title, harness: run.harness, workspaceKind: ws.kind, comparison: await this.currentComparison(run.id) };
  }

  // ---------------------------------------------------------------- Follow

  setFollow(runId, state) {
    if (!runId) return;
    this.follow.set(runId, state);
    this.persistFollow();
    if (/paused/.test(this.followNotes.get(runId) || '')) this.followNotes.delete(runId);
    if (state === 'following') {
      const last = this.lastReveal.get(runId);
      if (last) this.manager.reveal(runId, last);
    }
  }

  pauseFollow(runId, reason) {
    if (runId && this.follow.get(runId) === 'following') { this.follow.set(runId, 'paused'); this.persistFollow(); this.log(`follow paused for ${runId}: ${reason}`); }
  }

  async readText(abs) {
    try {
      const stat = await fs.stat(abs);
      if (!stat.isFile() || stat.size > 2 * 1024 * 1024) return undefined;
      const buf = await fs.readFile(abs);
      if (buf.includes(0)) return undefined;
      return buf.toString('utf8');
    } catch { return undefined; }
  }

  remember(abs, text) {
    this.observed.delete(abs); this.observed.set(abs, text);
    while (this.observed.size > 300) this.observed.delete(this.observed.keys().next().value);
  }

  /** First changed line (1-based) between two texts, or 1. */
  changedLine(before, after) {
    if (before === undefined) return 1;
    const a = before.split('\n'), b = after.split('\n');
    let i = 0; while (i < a.length && i < b.length && a[i] === b[i]) i++;
    return Math.min(i + 1, b.length);
  }

  async onEvent(event) {
    if (event.kind !== 'file_activity' || !event.run_id) return;
    const run = this.model.run(event.run_id) || (await this.model.refresh(), this.model.run(event.run_id));
    if (!run) return;
    const root = this.model.rootRun(run);
    const ws = this.model.workspace(run.workspace_id);
    if (!ws) return;
    const attribution = event.confidence === 'reported' ? 'agent-reported edit' : `agent tool input (${event.confidence})`;
    for (const rel of event.payload.paths || []) {
      if (rel.startsWith('/') || rel.startsWith('..')) continue; // outside the workspace
      const abs = path.join(ws.path, rel);
      // Tool-input events can precede the write; retry briefly for the new content.
      let before = this.observed.get(abs);
      if (before === undefined) {
        // Not observed yet: compare with the selected comparison base instead of guessing line 1.
        const base = (await this.currentComparison(root.id).catch(() => undefined))?.base;
        if (base) before = await gitShow(ws.path, `${base}:${rel}`).then(b => b.toString('utf8'), () => '');
      }
      let text;
      for (const delay of [0, 250, 750, 1500]) {
        if (delay) await new Promise(r => setTimeout(r, delay));
        text = await this.readText(abs);
        if (text !== undefined && text !== before) break;
      }
      if (text === undefined) continue;
      const line = this.changedLine(before, text);
      this.remember(abs, text);
      const message = { path: rel, line, attribution: run.id === root.id ? attribution : `${attribution}, native child ${run.title}` };
      for (const target of [root.id, run.id]) {
        this.lastReveal.set(target, message);
        this.followNotes.set(target, `Following agent edits (${attribution})`);
        if (this.follow.get(target) === 'following') this.manager.reveal(target, message);
      }
    }
  }
}

module.exports = { Review, statusLetter };
