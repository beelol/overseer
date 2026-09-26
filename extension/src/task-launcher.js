// Starting agents (AC-47, AC-59): what the composer and the New Task form offer (repositories,
// harnesses, compatible accounts with their sign-in state, remembered defaults) and the one
// daemon request that creates a task.
const vscode = require('vscode');
const path = require('path');
const { execFile } = require('child_process');

const INSTALL = { claude: 'https://docs.anthropic.com/en/docs/claude-code/setup', codex: 'https://developers.openai.com/codex/cli', opencode: 'https://opencode.ai/docs' };

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

class TaskLauncher {
  constructor(context, client, model, refreshAccounts) {
    this.context = context; this.client = client; this.model = model; this.refreshAccounts = refreshAccounts;
  }

  defaults() { return this.context.globalState.get('overseer.composerDefaults', {}); }
  saveDefaults(d) { return this.context.globalState.update('overseer.composerDefaults', { ...this.defaults(), ...d }); }

  async repos() {
    const folders = vscode.workspace.workspaceFolders || [];
    const repos = new Map();
    for (const root of (await Promise.all(folders.map(f => gitRoot(f.uri.fsPath)))).filter(Boolean)) repos.set(root, 'open folder');
    for (const t of [...(this.model.state.tasks || [])].sort((a, b) => b.created_ms - a.created_ms)) if (!repos.has(t.repo_root)) repos.set(t.repo_root, 'recent');
    const info = await Promise.all([...repos].map(async ([p, source]) => ({ path: p, name: path.basename(p), source, branch: (await this.client.request('repo.inspect', { path: p }).catch(() => ({}))).branch })));
    return info.filter(r => r.branch !== undefined || r.source === 'open folder');
  }

  accounts() {
    return (this.model.accounts || []).map(a => {
      const st = this.model.profileStatus.get(a.id);
      return { ...a, signedIn: !!st?.logged_in, plan: st?.identity?.plan, fingerprint: (st?.identity?.account_fingerprint || st?.identity?.fingerprint || '').slice(0, 8), usage: this.model.accountUsage?.get(a.id) };
    });
  }

  async data() {
    const harnesses = (await this.client.request('harness.list')).map(h => ({ harness: h.harness, installed: h.installed, version: h.version, hints: hints(h.capabilities || {}), install_url: INSTALL[h.harness] }));
    await this.refreshAccounts();
    return { repos: await this.repos(), harnesses, accounts: this.accounts(), trusted: vscode.workspace.isTrusted, defaults: this.defaults(),
      showAppServer: vscode.workspace.getConfiguration('overseer').get('showCodexAppServer', false) };
  }

  async browse() {
    const uri = await vscode.window.showOpenDialog({ canSelectFolders: true, canSelectFiles: false, title: 'Repository for the agent' });
    if (!uri) return undefined;
    const root = await gitRoot(uri[0].fsPath);
    if (!root) throw new Error('That folder is not in a Git repository.');
    const info = await this.client.request('repo.inspect', { path: root });
    return { path: root, name: path.basename(root), source: 'chosen', branch: info.branch };
  }

  async branches(repo) {
    const info = await this.client.request('repo.inspect', { path: String(repo) });
    return { repo, list: info.branches || [], head: info.branch };
  }

  /** Creates the task; returns the new run id. Throws a message suitable for inline display. */
  async start(f) {
    if (!vscode.workspace.isTrusted) throw new Error('Trust this workspace to start agents.');
    const repo = String(f.repo || ''); const harness = String(f.harness || '');
    if (!repo) throw new Error('Choose a repository.');
    let program, args = [];
    if (harness === 'generic') {
      program = String(f.program || '');
      if (!path.isAbsolute(program)) throw new Error('Use an absolute path for the program.');
      try { args = JSON.parse(f.args || '[]'); } catch { throw new Error('Arguments must be a JSON array of strings.'); }
      if (!Array.isArray(args) || !args.every(a => typeof a === 'string')) throw new Error('Arguments must be a JSON array of strings.');
    } else if (!f.account) throw new Error('Choose an account.');
    const unsaved = vscode.workspace.textDocuments.filter(d => d.isDirty && d.uri.scheme === 'file' && !path.relative(repo, d.uri.fsPath).startsWith('..'));
    if (f.mode === 'current' && unsaved.length) {
      const choice = await vscode.window.showWarningMessage(`${unsaved.length} unsaved editor(s) in this checkout.`, { modal: true, detail: 'They stay as labeled unsaved drafts; the agent only sees files on disk.' }, 'Continue');
      if (choice !== 'Continue') return undefined;
    }
    const prompt = String(f.prompt || '');
    const created = await this.client.request('task.create', { repo, harness, profile_id: harness === 'generic' ? undefined : f.account, workspace_mode: f.mode === 'current' ? 'current' : 'worktree',
      target_ref: f.mode !== 'current' && f.ref ? f.ref : undefined, model: f.model || undefined, prompt, title: titleFor(prompt, program), program, args,
      approval_policy: harness === 'codex-app' ? (f.approval || 'on-request') : undefined, unsaved: unsaved.map(d => path.relative(repo, d.uri.fsPath)),
      effort: f.options?.effort, permission_mode: f.options?.permission_mode, images: f.options?.images });
    if (created.launch_error) throw new Error(`Could not start ${harness}: ${created.launch_error}`);
    await this.saveDefaults({ repo, harness, account: f.account, model: f.model || '', mode: f.mode === 'current' ? 'current' : 'worktree' });
    await this.model.refresh();
    return created.run.id;
  }
}

/** A short title from the first line of the prompt (the full prompt stays on the task). */
function titleFor(prompt, program) {
  const line = String(prompt || '').split('\n').map(l => l.trim()).find(Boolean);
  if (!line) return path.basename(program || 'task');
  const clean = line.replace(/\s+/g, ' ');
  if (clean.length <= 60) return clean;
  const cut = clean.slice(0, 60); const space = cut.lastIndexOf(' ');
  return (space > 30 ? cut.slice(0, space) : cut).replace(/[,.;:]$/, '') + '…';
}

module.exports = { TaskLauncher, gitRoot, hints, titleFor };
