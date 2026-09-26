// New Task form host (AC-47). Gathers repositories (open folders and recent task repositories),
// harnesses with capability hints and compatible accounts, then creates the task with the same
// daemon request as the quick-pick flow.
const vscode = require('vscode');
const path = require('path');
const { execFile } = require('child_process');
const { randomBytes } = require('crypto');

const LABELS = { codex: 'Codex', 'codex-app': 'Codex app-server', claude: 'Claude Code', opencode: 'OpenCode', generic: 'Generic program' };

function gitRoot(dir) {
  return new Promise(resolve => execFile('git', ['rev-parse', '--show-toplevel'], { cwd: dir }, (err, out) => resolve(err ? undefined : out.trim())));
}

function hints(caps) {
  const out = [];
  const yes = v => typeof v === 'string' && v.startsWith('supported');
  if (yes(caps.children)) out.push('native children');
  if (yes(caps.approvals)) out.push('approvals');
  if (yes(caps.follow_up)) out.push('follow-ups');
  if (yes(caps.interrupt)) out.push('interrupt');
  if (typeof caps.file_activity === 'string' && caps.file_activity.startsWith('unknown')) out.push('filesystem-only edits');
  return out;
}

class NewTaskPanel {
  constructor(context, client, model, { selectRun, refreshAccounts, column }) {
    this.context = context; this.client = client; this.model = model;
    this.selectRun = selectRun; this.refreshAccounts = refreshAccounts; this.column = column;
  }

  async open() {
    if (this.panel) { this.panel.reveal(); return; }
    const media = vscode.Uri.joinPath(this.context.extensionUri, 'media');
    const panel = vscode.window.createWebviewPanel('overseer.newTask', 'New Task', { viewColumn: this.column() || vscode.ViewColumn.Active, preserveFocus: false }, { enableScripts: true, localResourceRoots: [media], retainContextWhenHidden: true });
    this.panel = panel;
    panel.iconPath = vscode.Uri.joinPath(media, 'overseer.svg');
    const nonce = randomBytes(18).toString('base64');
    const asset = name => panel.webview.asWebviewUri(vscode.Uri.joinPath(media, name));
    panel.webview.html = `<!doctype html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${panel.webview.cspSource}; script-src 'nonce-${nonce}';">
<link rel="stylesheet" href="${asset('new-task.css')}"><title>New Task</title></head><body>
<h1>New task</h1><p class="lede">Start an agent in a repository with an account you are signed in to. Agents keep running when VS Code closes.</p>
<div id="error" class="error" role="alert" hidden></div>
<section><h2 id="repos-h">Repository</h2><div id="repos" class="tiles" aria-labelledby="repos-h"></div></section>
<section><h2 id="harnesses-h">Harness</h2><div id="harnesses" class="tiles" aria-labelledby="harnesses-h"></div></section>
<section id="account-section"><h2 id="accounts-h">Account</h2><div id="accounts" class="tiles" aria-labelledby="accounts-h"></div>
<p id="no-accounts" class="empty" hidden>No account for this harness yet. Add one in Accounts (account login only; no API keys).</p></section>
<section id="generic-section" hidden><h2>Program</h2><div class="row"><div><label for="program">Executable path</label><input id="program" placeholder="/absolute/path/to/program"><div class="help">Runs without a shell.</div></div>
<div><label for="args">Arguments (JSON array of strings)</label><input id="args" value="[]"></div></div></section>
<section><h2 id="modes-h">Workspace</h2><div id="modes" class="tiles" aria-labelledby="modes-h"></div>
<div id="ref-row" class="row" style="margin-top:10px"><div><label for="ref">Start the worktree from</label><select id="ref"></select></div></div></section>
<section id="approval-section" hidden><h2 id="approvals-h">Approval policy</h2><div id="approvals" class="tiles" aria-labelledby="approvals-h"></div>
<div class="help">Requests appear in the run's conversation; they are never approved automatically.</div></section>
<section><div class="row"><div><label for="model">Model (optional)</label><input id="model" placeholder="Harness default"></div></div></section>
<section><label for="prompt">Task</label><textarea id="prompt" placeholder="What should the agent do?"></textarea><div class="help">⌘Enter starts the task.</div></section>
<div class="actions"><button id="start">Start task</button><span id="start-why" class="why" role="status"></span></div>
<script nonce="${nonce}" src="${asset('new-task.js')}"></script></body></html>`;
    panel.onDidDispose(() => { this.panel = undefined; });
    panel.webview.onDidReceiveMessage(m => this.receive(m).catch(error => panel.webview.postMessage({ type: 'error', message: error.message })));
  }

  async data() {
    const folders = vscode.workspace.workspaceFolders || [];
    const repos = new Map();
    for (const root of (await Promise.all(folders.map(f => gitRoot(f.uri.fsPath)))).filter(Boolean)) repos.set(root, 'open folder');
    for (const t of [...(this.model.state.tasks || [])].sort((a, b) => b.created_ms - a.created_ms)) if (!repos.has(t.repo_root)) repos.set(t.repo_root, 'recent');
    const info = await Promise.all([...repos].map(async ([p, source]) => ({ path: p, name: path.basename(p), source, branch: (await this.client.request('repo.inspect', { path: p }).catch(() => ({}))).branch })));
    const harnesses = (await this.client.request('harness.list')).map(h => ({ harness: h.harness, label: LABELS[h.harness] || h.harness, installed: h.installed, version: h.version, hints: hints(h.capabilities || {}) }));
    await this.refreshAccounts();
    const accounts = (this.model.accounts || []).map(a => {
      const st = this.model.profileStatus.get(a.id);
      return { ...a, signedIn: !!st?.logged_in, plan: st?.identity?.plan, fingerprint: (st?.identity?.account_fingerprint || st?.identity?.fingerprint || '').slice(0, 8) };
    });
    return { repos: info.filter(r => r.branch !== undefined || r.source === 'open folder'), harnesses, accounts, trusted: vscode.workspace.isTrusted };
  }

  async receive(m) {
    if (!m || typeof m !== 'object') return;
    if (m.type === 'ready') this.panel.webview.postMessage({ type: 'data', data: await this.data() });
    else if (m.type === 'branches') {
      const info = await this.client.request('repo.inspect', { path: String(m.repo) });
      this.panel.webview.postMessage({ type: 'branches', repo: m.repo, info: { branches: info.branches || [], head: info.branch } });
    } else if (m.type === 'browse') {
      const uri = await vscode.window.showOpenDialog({ canSelectFolders: true, canSelectFiles: false, title: 'Repository for the agent' });
      if (!uri) return;
      const root = await gitRoot(uri[0].fsPath);
      if (!root) throw new Error('That folder is not in a Git repository.');
      const info = await this.client.request('repo.inspect', { path: root });
      this.panel.webview.postMessage({ type: 'repoAdded', repo: { path: root, name: path.basename(root), source: 'chosen', branch: info.branch } });
    } else if (m.type === 'start') await this.start(m.form || {});
  }

  async start(f) {
    if (!vscode.workspace.isTrusted) throw new Error('Launching agents requires a trusted workspace.');
    const repo = String(f.repo || ''); const harness = String(f.harness || '');
    let program, args = [];
    if (harness === 'generic') {
      program = String(f.program || '');
      if (!path.isAbsolute(program)) throw new Error('Use an absolute path for the program.');
      try { args = JSON.parse(f.args || '[]'); } catch { throw new Error('Arguments must be a JSON array of strings.'); }
      if (!Array.isArray(args) || !args.every(a => typeof a === 'string')) throw new Error('Arguments must be a JSON array of strings.');
    }
    const unsaved = vscode.workspace.textDocuments.filter(d => d.isDirty && d.uri.scheme === 'file' && !path.relative(repo, d.uri.fsPath).startsWith('..'));
    if (f.mode === 'current' && unsaved.length) {
      const choice = await vscode.window.showWarningMessage(`${unsaved.length} unsaved editor(s) in this checkout. They stay as labeled unsaved drafts; the agent only sees files on disk.`, { modal: true }, 'Continue');
      if (choice !== 'Continue') return;
    }
    const prompt = String(f.prompt || '');
    const created = await this.client.request('task.create', { repo, harness, profile_id: harness === 'generic' ? undefined : f.account, workspace_mode: f.mode === 'current' ? 'current' : 'worktree',
      target_ref: f.mode === 'worktree' && f.ref ? f.ref : undefined, model: f.model || undefined, prompt, title: (prompt || path.basename(program || 'task')).slice(0, 60), program, args,
      approval_policy: harness === 'codex-app' ? f.approval : undefined, unsaved: unsaved.map(d => path.relative(repo, d.uri.fsPath)) });
    if (created.launch_error) throw new Error(`Could not launch ${harness}: ${created.launch_error}`);
    this.panel?.webview.postMessage({ type: 'started' });
    await this.model.refresh();
    this.panel?.dispose();
    await this.selectRun(created.run.id, { follow: vscode.workspace.getConfiguration('overseer').get('followNewRuns', true) });
  }
}

module.exports = { NewTaskPanel };
