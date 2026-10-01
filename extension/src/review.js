// Connects the vendored Branch Diff review to Overseer: the selected run's worktree,
// daemon-computed comparisons (latest run / task start / fork / branch) and Follow.
const vscode = require('vscode');
const path = require('path');
const fs = require('fs').promises;
const { execFile } = require('child_process');
const { ReviewManager } = require('../branch-diff/review/panel');
const { diffLines, splitLines } = require('./line-diff');

// Follow shows a file of up to this size in the review (larger ones say so); changes are marked up to 2 MB.
const FOLLOW_MAX_BYTES = 5 * 1024 * 1024;
const MARK_MAX_CHARS = 2 * 1024 * 1024;

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

// The comparisons the review's header offers in one click (AC-263), in the header's order.
const QUICK_COMPARISONS = ['task_start', 'latest_run', 'entire_worktree'];

class Review {
  constructor(context, client, model, log) {
    this.context = context; this.client = client; this.model = model; this.log = log;
    // Comparison choice and Follow survive reloads and restarts (workspace state). Follow is
    // never auto-resumed: a run that was being followed comes back paused.
    this.comparisons = new Map(Object.entries(context.workspaceState.get('overseer.comparisons', {}))); // runId -> { mode, branch }
    // Which changes the review shows (AC-75): all (against the comparison), staged, unstaged or untracked.
    this.scopes = new Map(Object.entries(context.workspaceState.get('overseer.scopes', {})));
    this.follow = new Map(); // runId -> 'off' | 'following' | 'paused'
    this.followNotes = new Map();
    for (const [runId, state] of Object.entries(context.workspaceState.get('overseer.follow', {}))) {
      if (state === 'following' || state === 'paused') { this.follow.set(runId, 'paused'); this.followNotes.set(runId, 'Follow was on before VS Code reloaded. It stays paused until you resume it.'); }
    }
    // Reviewed marks live in the daemon (Gate N), so every surface shows the same marks.
    this.marks = new Map(); // runId -> Set of hunk keys
    this.marksLoading = new Map(); // runId -> Promise
    this.observed = new Map(); // abs path -> last observed text (bounded)
    this.userSaves = new Map(); // abs path -> ms of last user save
    this.lastReveal = new Map(); // runId -> last reveal message
    // AC-263: the comparisons the review's header offers in one click, as the daemon last listed them.
    this.choices = new Map(); // runId -> { options: [{ mode, label, available, detail }], folderEdits }
    this.viewAt = new Map(); // runId -> when its review's switch was last used (ms)
    this.lastFile = new Map(); // runId -> the file the agent was last in (edit or read), for Follow
    this.followSeq = 0; this.followLatest = new Map(); // runId -> the newest Follow message's number
    this.manager = new ReviewManager(context, {
      helpers: holder => this.helpers(holder),
      statusLetter,
      openFileDiff: (entry, session) => this.openFileDiff(entry, session),
      pickComparison: runId => this.pickComparison(runId),
      // AC-263: Since task start, Latest run and Entire worktree, one click each in the review's header.
      chooseComparison: (runId, mode) => this.chooseComparison(runId, mode),
      choices: runId => this.choices.get(runId),
      followState: runId => this.follow.get(runId) || 'off',
      followNote: runId => this.followNotes.get(runId),
      setFollow: (runId, state) => this.setFollow(runId, state),
      pauseFollow: (runId, reason) => this.pauseFollow(runId, reason),
      followReady: runId => { const last = this.lastReveal.get(runId); if (last && this.follow.get(runId) === 'following') this.manager.reveal(runId, last); },
      restore: state => this.restore(state),
      reviewedKeys: runId => this.reviewedKeys(runId),
      reviewHunk: (session, message) => this.reviewHunk(session, message),
      // Switching an agent to Follow (AC-233) closes its review without closing the agent.
      closed: runId => { if (!this.switching) this.onClosed?.(runId); },
      // Follow or Diffs only (AC-233), switched in the review's own header; both are this review (AC-264).
      view: runId => this.head?.modeFor(runId) || 'diffs',
      // `at` is when the review's own switch was used: messages sent before the host heard of it carry an older one.
      setView: (runId, view, at) => { if (at) this.viewAt.set(runId, at); return this.head?.setMode(runId, view); },
      viewAt: runId => this.viewAt.get(runId) || 0,
      // Follow in the review (AC-264): the agent's file, a file picked in All files, and back to the agent.
      followInit: runId => this.followInit(runId),
      followPick: (runId, rel) => this.showFollow(runId, { path: rel, source: 'user' }),
      followAgain: runId => this.followAgain(runId),
      followPoll: runId => this.followPoll(runId),
      setScope: (runId, scope) => this.setScope(runId, scope),
      // AC-243: the review's Merge / Open PR / Cancel merge buttons, the same as the chat's.
      land: runId => (runId ? this.model.landing?.summary(runId) : undefined),
      land_action: (runId, type) => this.landAction(runId, type),
    });
    context.subscriptions.push(this.manager,
      vscode.window.registerWebviewPanelSerializer('overseer.review', this.manager),
      vscode.workspace.registerTextDocumentContentProvider('overseer-git', { provideTextDocumentContent: uri => this.provideGitContent(uri) }),
      // The owner's own save is not the agent's next edit: Follow compares the agent's writes with it.
      vscode.workspace.onDidSaveTextDocument(doc => { this.userSaves.set(doc.uri.fsPath, Date.now()); if (this.observed.has(doc.uri.fsPath)) this.remember(doc.uri.fsPath, doc.getText()); }),
      vscode.workspace.onDidChangeTextDocument(e => { if (e.document.uri.scheme === 'file' && e.contentChanges.length) this.userSaves.set(e.document.uri.fsPath, Date.now()); }));
    client.on('event', event => this.onEvent(event).catch(error => this.log('follow: ' + error.message)));
  }

  helpers(holder) {
    const self = this;
    return {
      skipRepoStatus: true,
      async comparisonKey(repo, target) {
        return JSON.stringify([target, repo.state.HEAD?.commit, holder.session?.overseer?.workspaceId, holder.session?.overseer?.scope]);
      },
      async resolveContext(target, repo) {
        const git = await self.gitApi();
        const c = holder.session?.overseer?.comparison;
        if (!c || !c.base) throw new Error(c?.detail ? `Comparison unavailable: ${c.detail}` : 'No comparison base selected.');
        return { git, repo, base: c.label, mergeBase: c.base };
      },
      async getChangeEntries(git, repo, mode, base) {
        const workspaceId = holder.session?.overseer?.workspaceId;
        const scope = holder.session?.overseer?.scope || 'all';
        const root = repo.rootUri;
        const at = rel => vscode.Uri.joinPath(root, ...rel.split('/'));
        const status = await self.client.request('workspace.status', { workspace_id: workspaceId }).catch(() => undefined);
        const conflicted = new Set(status?.conflicted || []);
        if (scope !== 'all' && status) return self.scopeEntries(git, scope, status, at);
        const diff = await self.client.request('workspace.diff', { workspace_id: workspaceId, base, status: false });
        const entries = diff.changes.map(change => {
          const uri = at(change.path);
          const original = change.old_path ? at(change.old_path) : uri;
          return { uri, relPath: change.path, status: STATUS[change.status] ?? 5, conflicted: conflicted.has(change.path),
            left: change.status === 'A' ? null : git.toGitUri(original, base),
            right: change.status === 'D' ? null : uri };
        });
        // A conflicted file stays in the list even when the comparison does not include it.
        for (const rel of conflicted) if (!entries.some(e => e.relPath === rel)) entries.push({ uri: at(rel), relPath: rel, status: STATUS.U, conflicted: true, left: git.toGitUri(at(rel), 'HEAD'), right: at(rel) });
        return entries;
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

  /** The keys marked reviewed for a run, from the daemon. Loads them the first time they are asked for. */
  reviewedKeys(runId) {
    if (!runId) return [];
    this.loadMarks(runId);
    return [...(this.marks.get(runId) || [])];
  }

  loadMarks(runId) {
    if (this.marksLoading.has(runId)) return this.marksLoading.get(runId);
    const loading = (async () => {
      // Marks VS Code kept itself, before the daemon kept them, are handed over once.
      const old = this.context.workspaceState.get('overseer.reviewedHunks', {});
      if (old[runId] && Object.keys(old[runId]).length) {
        await this.client.request('review.import', { run_id: runId, marks: Object.entries(old[runId]).map(([key, v]) => ({ key, path: v.path, at: v.at })) });
        const rest = { ...old }; delete rest[runId];
        await this.context.workspaceState.update('overseer.reviewedHunks', rest);
      }
      const got = await this.client.request('review.marks', { run_id: runId });
      this.marks.set(runId, new Set(got.keys));
      this.showMarks(runId);
    })().catch(error => { this.marksLoading.delete(runId); this.log('reviewed marks: ' + error.message); });
    this.marksLoading.set(runId, loading);
    return loading;
  }

  /** A merge button pressed in the review: the chat's commands, for the review's agent only. */
  async landAction(runId, type) {
    const command = { merge: 'overseer.mergeBack', openPullRequest: 'overseer.openPullRequest', cancelMerge: 'overseer.cancelMerge', publish: 'overseer.publishToGitHub', cleanup: 'overseer.cleanupWorkspace' }[type];
    if (runId && command) await vscode.commands.executeCommand(command, runId);
  }

  /** What the agent's work became changed (merged, stopped, cancelled): the review's buttons follow. */
  landingChanged(runId) {
    const run = runId && this.model.run(runId);
    const root = run && this.model.rootRun(run);
    for (const id of new Set([runId, root?.id])) { const found = id && this.manager.panelFor(id); if (found) this.manager.postOverseer(found.session); }
  }

  showMarks(runId) {
    const found = this.manager.panelFor(runId);
    if (found) this.manager.postOverseer(found.session);
  }

  /**
   * Accept = mark a hunk reviewed (no Git staging). Keys hash the hunk's base and working text,
   * so a hunk that changes again is simply no longer reviewed. The daemon checks the hunk against
   * the file first: if the agent changed it meanwhile, that is a conflict. An unsaved edit in
   * VS Code is newer than the file, so it is checked here.
   */
  async reviewHunk(session, msg) {
    const runId = session.overseer?.runId;
    if (!runId || !/^[a-f0-9]{16}$/.test(String(msg.key)) || typeof msg.path !== 'string') throw new Error('Invalid hunk.');
    await this.loadMarks(runId);
    const keys = this.marks.get(runId) || new Set();
    this.marks.set(runId, keys);
    if (!msg.reviewed) {
      await this.client.request('review.unaccept', { run_id: runId, key: msg.key });
      keys.delete(msg.key);
      return;
    }
    const uri = vscode.Uri.joinPath(session.repo.rootUri, ...msg.path.split('/'));
    if (path.relative(session.repo.rootUri.fsPath, uri.fsPath).startsWith('..')) throw new Error('File is outside this review.');
    const modified = Array.isArray(msg.modified) ? msg.modified : [];
    const start = Number(msg.modifiedStart) || 0, end = Number(msg.modifiedEnd) || 0;
    const open = vscode.workspace.textDocuments.find(d => d.uri.toString() === uri.toString());
    if (open?.isDirty) {
      const lines = open.getText().split(/\r?\n/);
      const same = end ? lines.slice(start - 1, end).join('\n') === modified.join('\n') : (start === 0 || lines[start - 1] === msg.anchor);
      if (!same) throw new Error(`Not accepted: ${msg.path} changed while you were accepting this change (conflict). Review the current content.`);
      // The unsaved text is what was reviewed; the file on disk may differ until it is saved.
      await this.client.request('review.accept', { run_id: runId, path: msg.path, key: msg.key, modified_start: 0, modified_lines: [] });
    } else {
      await this.client.request('review.accept', { run_id: runId, path: msg.path, key: msg.key, modified_start: start, modified_lines: end ? modified : [], anchor: end ? undefined : msg.anchor });
    }
    keys.add(msg.key);
  }

  /** Staged (HEAD → index, read-only), unstaged (index → working tree) or untracked files. */
  scopeEntries(git, scope, st, at) {
    const conflicted = new Set(st.conflicted || []);
    if (scope === 'staged') return (st.staged || []).map(f => ({ uri: at(f.path), relPath: f.path, status: STATUS[f.status] ?? 5, readOnly: true, conflicted: conflicted.has(f.path),
      left: f.status === 'A' ? null : git.toGitUri(at(f.old_path || f.path), 'HEAD'), right: f.status === 'D' ? null : git.toGitUri(at(f.path), '') }));
    if (scope === 'unstaged') return (st.unstaged || []).map(f => ({ uri: at(f.path), relPath: f.path, status: STATUS[f.status] ?? 5, conflicted: conflicted.has(f.path),
      left: git.toGitUri(at(f.path), ''), right: f.status === 'D' ? null : at(f.path) }));
    return (st.untracked || []).map(rel => ({ uri: at(rel), relPath: rel, status: STATUS.A, left: null, right: at(rel) }));
  }

  setScope(runId, scope) {
    if (!runId || !['all', 'staged', 'unstaged', 'untracked'].includes(scope)) return;
    this.scopes.set(runId, scope);
    this.context.workspaceState.update('overseer.scopes', Object.fromEntries([...this.scopes].slice(-200)));
    const found = this.manager.panelFor(runId);
    if (found) { found.session.overseer.scope = scope; found.session.invalidate(true); this.manager.postOverseer(found.session); }
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
    this.rememberChoices(runId, opts);
    if (selected) {
      const fresh = opts.options.find(o => o.mode === selected.mode && (o.branch || '') === (selected.branch || ''));
      if (fresh) return fresh;
    }
    return opts.options.find(o => o.default) || opts.options[0];
  }

  /**
   * Looks up what opening a run's review needs (its repository and comparison) ahead of open(), so
   * the arrangement can do it while it still asks whether the agent has changes (AC-73's 500 ms).
   */
  prepare(runId) {
    const run = this.model.run(runId);
    const ws = run && this.model.workspace(run.workspace_id);
    if (!run || this.unavailable(run, ws)) return Promise.resolve();
    const lookup = Promise.all([this.repoFor(ws.path), this.currentComparison(runId)]);
    this.prepared = { runId, path: ws.path, at: Date.now(), lookup };
    return lookup.then(() => {}, () => {});
  }

  /**
   * Opens the agent's review (AC-233, AC-264): in Follow (the file the agent is in, live, with All
   * files) or Diffs only (what changed, with Changed), as the agent was last switched.
   */
  async open(runId, { preserveFocus = false, follow, viewColumn } = {}) {
    const run = this.model.run(runId);
    if (!run) throw new Error('Unknown run.');
    const ws = this.model.workspace(run.workspace_id);
    const why = this.unavailable(run, ws);
    if (why) throw new Error(why);
    const ready = this.prepared?.runId === runId && this.prepared.path === ws.path && Date.now() - this.prepared.at < 60000 ? this.prepared.lookup.catch(() => null) : null;
    this.prepared = undefined;
    const [repo, comparison] = (ready && await ready) || [await this.repoFor(ws.path), await this.currentComparison(runId)];
    if (follow !== undefined) { this.follow.set(runId, follow ? 'following' : 'off'); this.persistFollow(); }
    if (String(run.capabilities?.file_activity || '').startsWith('unknown')) this.followNotes.set(runId, 'Filesystem evidence only: this harness does not report its edits, so Follow cannot attribute or jump to them. The file list still refreshes live.');
    return this.manager.open({ repo, workspaceId: ws.id, runId, runTitle: run.title, harness: run.harness, workspaceKind: ws.kind, comparison, scope: this.scopes.get(runId) || 'all' }, { preserveFocus, viewColumn: viewColumn || this.reviewColumn?.() });
  }

  /** Opens the run's review at the first changed hunk of `rel` (a file edit clicked in the conversation). */
  async revealEdit(runId, rel) {
    const inFollow = this.head && this.head.modeFor(runId) === 'follow';
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
    // Follow (AC-264): the file itself, at the change, in the review.
    if (inFollow) { await this.showFollow(runId, { path: rel, line, source: 'user', attribution: message.attribution }); return message; }
    // A freshly opened review may not have its file list yet; the webview keeps it pending.
    this.manager.reveal(runId, message);
    return message;
  }

  /** AC-263: what the header's one-click comparisons show, and whether this folder holds the owner's edits too. */
  rememberChoices(runId, opts) {
    const options = QUICK_COMPARISONS.map(mode => opts.options.find(o => o.mode === mode)).filter(Boolean)
      .map(o => ({ mode: o.mode, label: o.label, available: !!o.available, detail: o.detail || '' }));
    this.choices.set(runId, { options, folderEdits: opts.folder_edits ?? opts.workspace?.kind === 'current' });
  }

  /** One click in the review's header: Since task start, Latest run or Entire worktree (AC-263). */
  async chooseComparison(runId, mode) {
    if (!runId || !QUICK_COMPARISONS.includes(mode)) return;
    const opts = await this.options(runId);
    this.rememberChoices(runId, opts);
    const option = opts.options.find(o => o.mode === mode);
    if (!option) return;
    if (!option.available) { vscode.window.showWarningMessage(`${option.label} is unavailable: ${option.detail}`); return; }
    this.setComparison(runId, option);
    await this.open(runId, { preserveFocus: true });
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
    return this.baseText(q.root, q.ref, q.path);
  }

  /** A file at a comparison, from the daemon (the same method a phone uses); Git directly when the daemon does not know the folder. */
  async baseText(root, ref, rel) {
    const ws = (this.model.state?.workspaces || []).find(w => w.path === root && !w.removed_ms);
    if (ws) {
      const file = await this.client.request('workspace.file', { workspace_id: ws.id, path: rel, base: ref }).catch(() => undefined);
      if (file) return file.before.kind === 'text' ? file.before.text : '';
    }
    return gitShow(root, `${ref}:${rel}`).then(b => b.toString('utf8'), () => '');
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
    return { repo: await this.repoFor(ws.path), workspaceId: ws.id, runId: run.id, runTitle: run.title, harness: run.harness, workspaceKind: ws.kind, comparison: await this.currentComparison(run.id), scope: this.scopes.get(run.id) || 'all' };
  }

  // ---------------------------------------------------------------- Follow

  setFollow(runId, state) {
    if (!runId) return;
    this.follow.set(runId, state);
    this.persistFollow();
    if (/paused/.test(this.followNotes.get(runId) || '')) this.followNotes.delete(runId);
    if (state === 'following') {
      // After the caller's Follow state reaches the review (panel.js posts it right after this
      // returns): a reveal that arrived first found Follow still paused and was dropped, and Resume
      // stayed where it was.
      const last = this.lastReveal.get(runId);
      if (last) queueMicrotask(() => this.manager.reveal(runId, last));
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
    // A mark made on another surface (a phone, another window) shows here at once.
    if (event.kind === 'review_mark' && event.run_id && this.marks.has(event.run_id)) {
      const keys = this.marks.get(event.run_id);
      if (event.payload.reviewed) keys.add(event.payload.key); else keys.delete(event.payload.key);
      this.showMarks(event.run_id);
      return;
    }
    if (event.kind === 'review_reject' && event.run_id) { this.manager.panelFor(event.run_id)?.session.invalidate(true); this.showMarks(event.run_id); return; }
    if (event.kind === 'tool' && event.run_id) { this.onRead(event); return; }
    if (event.kind !== 'file_activity' || !event.run_id) return;
    const run = this.model.run(event.run_id) || (await this.model.refresh(), this.model.run(event.run_id));
    if (!run) return;
    const root = this.model.rootRun(run);
    const ws = this.model.workspace(run.workspace_id);
    if (!ws) return;
    // In words, never the confidence tag itself (AC-245).
    const attribution = event.confidence === 'reported' ? 'agent-reported edit' : 'from what the agent asked to change';
    for (const rel of event.payload.paths || []) {
      if (rel.startsWith('/') || rel.startsWith('..')) continue; // outside the workspace
      const abs = path.join(ws.path, rel);
      // Tool-input events can precede the write; retry briefly for the new content.
      let before = this.observed.get(abs);
      if (before === undefined) {
        // Not observed yet: compare with the selected comparison base instead of guessing line 1.
        const base = (await this.currentComparison(root.id).catch(() => undefined))?.base;
        if (base) before = await this.baseText(ws.path, base, rel);
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
      // Follow (AC-264): the review shows the file the agent is in.
      this.followAgent(root.id, { path: rel, line, attribution: message.attribution });
    }
  }

  // ---------------------------------------------------------------- Follow, in the review (AC-264)

  /** A file the agent read (Claude's Read, OpenCode's read): Follow goes there too. */
  onRead(event) {
    const p = event.payload || {};
    if (!/^(read|view|notebookread)$/i.test(String(p.name || ''))) return;
    let input;
    try { input = JSON.parse(p.summary || ''); } catch { return; }
    const file = input?.file_path || input?.filePath || input?.path;
    const run = this.model.run(event.run_id);
    const root = run && this.model.rootRun(run);
    const ws = run && this.model.workspace(run.workspace_id);
    if (typeof file !== 'string' || !root || !ws) return;
    const rel = path.isAbsolute(file) ? path.relative(ws.path, file) : file;
    if (!rel || rel.startsWith('..') || path.isAbsolute(rel)) return;
    const line = Math.max(1, Number(input.offset) || 1);
    this.followAgent(root.id, { path: rel.split(path.sep).join('/'), line, attribution: 'the agent is reading it' });
  }

  /** The agent is in `target.path`: remembered, and shown when its review is in Follow. */
  followAgent(runId, target) {
    const value = { ...target, source: 'agent' };
    this.lastFile.set(runId, value);
    if (this.head?.modeFor(runId) === 'follow') this.showFollow(runId, value).catch(error => this.log('follow: ' + error.message));
  }

  /** The review came up (or switched) in Follow: what it showed, else where the agent is. */
  async followInit(runId) {
    if (this.head?.modeFor(runId) !== 'follow') return;
    const found = this.manager.panelFor(runId);
    if (!found) return;
    const shown = found.session.followShown;
    if (shown?.runId === runId) return this.showFollow(runId, { ...shown, refresh: true });
    return this.followAgain(runId);
  }

  /** "Follow the agent": back to the file the agent is in (or, before it has been in one, its first change). */
  async followAgain(runId) {
    const target = this.lastFile.get(runId) || { ...(await this.head?.defaultTarget(runId).catch(() => undefined)), source: 'agent' };
    if (target.path) return this.showFollow(runId, { ...target, source: 'agent' });
    this.manager.postFollow(runId, { seq: ++this.followSeq, path: '', source: 'agent', problem: 'The agent has not opened a file yet. Follow shows its file as soon as it reads or edits one.' });
  }

  /** The file shown changed on disk, or joined or left the agent's changes: shown again, where it was. */
  async followPoll(runId) {
    const found = this.manager.panelFor(runId);
    const shown = found?.session.followShown;
    if (!shown || shown.runId !== runId || this.head?.modeFor(runId) !== 'follow') return;
    const abs = path.join(found.session.repo.rootUri.fsPath, shown.path);
    const mtimeMs = await fs.stat(abs).then(st => st.mtimeMs, () => -1);
    if (mtimeMs !== shown.mtimeMs || this.isChanged(found.session, shown.path) !== shown.changed) await this.showFollow(runId, { ...shown, refresh: true });
  }

  isChanged(session, rel) { return !!session.display?.entries.some(e => !e.browsed && e.relPath === rel); }

  /**
   * Shows a file of the agent's worktree in its review's Follow view: its current text, the agent's
   * changes marked (against the comparison base), at `line`. `source` is 'agent' (where the agent is)
   * or 'user' (picked in All files, or opened from the conversation).
   */
  async showFollow(runId, { path: rel, line, source = 'agent', attribution, refresh = false }) {
    const found = this.manager.panelFor(runId);
    if (!found || typeof rel !== 'string') return;
    const root = found.session.repo.rootUri.fsPath;
    rel = rel.split(path.sep).join('/');
    const abs = path.join(root, rel);
    const inside = path.relative(root, abs);
    if (!inside || inside.startsWith('..') || path.isAbsolute(inside)) return;
    const seq = ++this.followSeq;
    this.followLatest.set(runId, seq);
    let text, problem, mtimeMs = -1;
    try {
      const st = await fs.stat(abs);
      mtimeMs = st.mtimeMs;
      if (!st.isFile()) problem = `${rel} is a folder.`;
      else if (st.size > FOLLOW_MAX_BYTES) problem = `${rel} is too large to show here (${(st.size / 1048576).toFixed(1)} MB).`;
      else {
        const buf = await fs.readFile(abs);
        if (buf.subarray(0, 8000).includes(0)) problem = `${rel} is a binary file.`;
        else text = buf.toString('utf8');
      }
    } catch (error) { problem = error.code === 'ENOENT' ? `${rel} is no longer in the agent's worktree (it was removed).` : error.message; }
    const changed = this.isChanged(found.session, rel);
    const base = found.session.overseer?.comparison?.base;
    let marks;
    if (text !== undefined && changed && base && text.length <= MARK_MAX_CHARS) {
      const before = await this.baseText(root, base, rel).catch(() => undefined);
      if (before !== undefined) marks = this.followMarks(before, text);
    }
    if (this.followLatest.get(runId) !== seq) return; // a newer one is on its way
    found.session.followShown = { runId, path: rel, line, source, attribution, mtimeMs, changed };
    this.manager.postFollow(runId, { seq, path: rel, line, source, attribution, text, problem, marks, reveal: !refresh });
  }

  /** The agent's changes to a file as line marks: added and changed ranges (1-based), and removals. */
  followMarks(before, after) {
    const old = splitLines(before);
    const added = [], changed = [], removed = [];
    for (const h of diffLines(old, after)) {
      const gone = old.slice(h.origStart, h.origStart + h.origLen);
      if (!h.modLen) { removed.push({ line: h.modStart, count: h.origLen }); continue; }
      const range = { start: h.modStart + 1, end: h.modStart + h.modLen };
      if (!h.origLen) { added.push(range); continue; }
      changed.push(h.origLen === h.modLen ? { ...range, was: gone.map(l => l.trim().slice(0, 90)) } : { ...range, replaced: h.origLen });
    }
    return { added, changed, removed };
  }
}

module.exports = { Review, statusLetter };
