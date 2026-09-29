// Starting agents (AC-47, AC-59): what the composer and the New Task form offer (repositories,
// harnesses, compatible accounts with their sign-in state, remembered defaults) and the one
// daemon request that creates a task.
const vscode = require('vscode');
const path = require('path');
const fs = require('fs');
const os = require('os');
const crypto = require('crypto');
const features = require('./features');
const { execFile } = require('child_process');

const INSTALL = { claude: 'https://docs.anthropic.com/en/docs/claude-code/setup', codex: 'https://developers.openai.com/codex/cli', opencode: 'https://opencode.ai/docs' };

function gitRoot(dir) {
  return new Promise(resolve => execFile('git', ['rev-parse', '--show-toplevel'], { cwd: dir }, (err, out) => resolve(err ? undefined : out.trim())));
}

/** `~` and `~/…` are the home folder; anything else is returned as typed. */
function expandHome(p) { return p === '~' ? os.homedir() : p.startsWith('~/') ? path.join(os.homedir(), p.slice(2)) : p; }

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

  /**
   * The repositories the composer offers (AC-260): open folders, then recent ones (added in the
   * picker, or with agents), then Git repositories beside an open one ("nearby", for the picker's
   * search). Nearby ones are not inspected until chosen.
   */
  async repos() {
    const folders = vscode.workspace.workspaceFolders || [];
    const repos = new Map();
    const open = (await Promise.all(folders.map(f => gitRoot(f.uri.fsPath)))).filter(Boolean);
    for (const root of open) repos.set(root, 'open folder');
    for (const p of this.knownRepos()) if (!repos.has(p)) repos.set(p, 'recent');
    for (const t of [...(this.model.state.tasks || [])].sort((a, b) => b.created_ms - a.created_ms)) if (!repos.has(t.repo_root)) repos.set(t.repo_root, 'recent');
    const info = await Promise.all([...repos].map(async ([p, source]) => ({ path: p, name: path.basename(p), source, branch: (await this.client.request('repo.inspect', { path: p }).catch(() => ({}))).branch })));
    const out = info.filter(r => r.branch !== undefined || r.source === 'open folder');
    const seen = new Set(repos.keys());
    for (const parent of new Set(open.map(r => path.dirname(r)))) {
      let names = [];
      try { names = fs.readdirSync(parent, { withFileTypes: true }).filter(d => d.isDirectory() && !d.name.startsWith('.')).map(d => d.name).sort(); } catch {}
      for (const name of names.slice(0, 200)) {
        const p = path.join(parent, name);
        if (seen.has(p) || !fs.existsSync(path.join(p, '.git'))) continue;
        seen.add(p); out.push({ path: p, name, source: 'nearby' });
        if (out.length >= 60) return out;
      }
    }
    return out;
  }

  /** Repositories added from the composer's picker, most recent first. */
  knownRepos() { return this.context.globalState.get('overseer.knownRepos', []).filter(p => typeof p === 'string'); }
  async rememberRepo(p) { await this.context.globalState.update('overseer.knownRepos', [p, ...this.knownRepos().filter(x => x !== p)].slice(0, 20)); }

  /**
   * A repository from a path typed in the composer's picker (AC-260): `~` is the home folder, a
   * folder inside a repository means that repository. Refusals are plain words for the picker.
   */
  async addRepo(input) {
    const typed = String(input || '').trim();
    if (!typed) throw new Error('Type the path of a folder.');
    const full = expandHome(typed);
    if (!path.isAbsolute(full)) throw new Error('Type the full path, starting with / or ~.');
    let stat; try { stat = fs.statSync(full); } catch { stat = undefined; }
    if (!stat || !stat.isDirectory()) throw new Error(`No folder at ${full}.`);
    const root = await gitRoot(full);
    if (!root) throw new Error(`${full} is not in a Git repository.`);
    const info = await this.client.request('repo.inspect', { path: root });
    await this.rememberRepo(root);
    return { path: root, name: path.basename(root), source: 'chosen', branch: info.branch };
  }

  /** Folders that complete a typed path (the picker's Tab), Git repositories marked. */
  async pathHints(input) {
    const typed = String(input || '');
    const full = expandHome(typed);
    if (!path.isAbsolute(full)) return [];
    const dir = typed.endsWith('/') ? full : path.dirname(full);
    const prefix = typed.endsWith('/') ? '' : path.basename(full).toLowerCase();
    let entries;
    try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return []; }
    const shown = typed.endsWith('/') ? typed : typed.slice(0, typed.length - path.basename(full).length);
    return entries.filter(d => d.isDirectory() && d.name.toLowerCase().startsWith(prefix) && (prefix.startsWith('.') || !d.name.startsWith('.')))
      .map(d => d.name).sort((a, b) => a.localeCompare(b)).slice(0, 8)
      .map(name => ({ path: shown + name + '/', git: fs.existsSync(path.join(dir, name, '.git')) }));
  }

  accounts() {
    return (this.model.accounts || []).map(a => {
      const st = this.model.profileStatus.get(a.id);
      return { ...a, signedIn: !!st?.logged_in, installed: st?.installed === true, plan: a.account?.plan || st?.identity?.plan, label: a.account?.label || a.name, short: a.account?.short || a.name, fingerprint: (st?.identity?.account_fingerprint || st?.identity?.fingerprint || '').slice(0, 8), usage: this.model.accountUsage?.get(a.id) };
    });
  }

  async data() {
    const harnesses = (await this.client.request('harness.list')).map(h => ({ harness: h.harness, installed: h.installed, version: h.version, hints: hints(h.capabilities || {}), install_url: INSTALL[h.harness] }));
    await this.refreshAccounts();
    return { repos: await this.repos(), harnesses, accounts: this.accounts(), trusted: vscode.workspace.isTrusted, defaults: this.defaults(),
      showAppServer: vscode.workspace.getConfiguration('overseer').get('showCodexAppServer', false),
      autoRouting: features.enabled(vscode, 'autoRouting'),
      continuity: this.continuity ? this.continuity() : undefined };
  }

  async browse() {
    const uri = await vscode.window.showOpenDialog({ canSelectFolders: true, canSelectFiles: false, title: 'Repository for the agent' });
    if (!uri) return undefined;
    const root = await gitRoot(uri[0].fsPath);
    if (!root) throw new Error('That folder is not in a Git repository.');
    const info = await this.client.request('repo.inspect', { path: root });
    await this.rememberRepo(root);
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
    if (f.routing === 'auto' && !features.enabled(vscode, 'autoRouting')) throw new Error(features.offMessage('autoRouting'));
    if (f.routing === 'auto') {
      if (!String(f.prompt || '').trim()) throw new Error('Describe the task.');
      if (f.options?.images?.length) throw new Error('Auto starts cannot include images yet. Choose a manual agent for this task.');
    } else if (harness === 'generic') {
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
    if (f.routing === 'auto') return this.startAuto(f, repo, prompt);
    const created = await this.client.request('task.create', { repo, harness, profile_id: harness === 'generic' ? undefined : f.account, workspace_mode: f.mode === 'current' ? 'current' : 'worktree',
      target_ref: f.mode !== 'current' && f.ref ? f.ref : undefined, model: f.model || undefined, prompt, title: titleFor(prompt, program), program, args,
      approval_policy: harness === 'codex-app' ? (f.approval || 'on-request') : undefined, unsaved: unsaved.map(d => path.relative(repo, d.uri.fsPath)),
      effort: f.options?.effort, permission_mode: f.options?.permission_mode, images: f.options?.images });
    if (created.launch_error) throw new Error(`Could not start ${harness}: ${created.launch_error}`);
    await this.saveDefaults({ repo, routing: 'manual', harness, account: f.account, model: f.model || '', mode: f.mode === 'current' ? 'current' : 'worktree', ...(f.approval ? { approval: f.approval } : {}) });
    await this.model.refresh();
    return created.run.id;
  }

  async startAuto(f, repo, prompt) {
    // Continuity's isolated Ollama profile needs its own verified Auto route
    // adapter and memory admission; project-configured OpenCode already has one.
    const accounts = this.accounts().filter(a => a.id !== 'local-ollama' && ((a.signedIn &&
      (a.harnesses || []).some(h => ['codex', 'claude'].includes(h))) ||
      (a.installed && (a.harnesses || []).includes('opencode'))));
    if (!accounts.length) throw new Error('Connect Codex or Claude Code, or set up a project-local OpenCode route for Auto routing.');
    if (accounts.length > 8) throw new Error('Auto routing currently supports up to eight signed-in accounts.');
    const preferred = String(f.preferredHarness || '');
    if (preferred && !['codex-app', 'claude', 'opencode'].includes(preferred)) throw new Error('Unsupported Auto harness preference.');
    const request = { repo, prompt, title: titleFor(prompt),
      workspace_mode: f.mode === 'current' ? 'current' : 'worktree',
      ...(f.mode !== 'current' && f.ref ? { target_ref: f.ref } : {}),
      allowed_profiles: accounts.map(a => a.id),
      ...(preferred ? { preferred_harness: preferred } : {}),
      approval_policy: 'on-request' };
    const hash = crypto.createHash('sha256').update(JSON.stringify(request)).digest('hex');
    const previous = this.context.globalState.get('overseer.autoPendingStart');
    const workUnitId = previous?.hash === hash ? previous.workUnitId : `ui-${crypto.randomUUID()}`;
    await this.context.globalState.update('overseer.autoPendingStart', { hash, workUnitId });
    await this.client.request('auto.mode.set', { enabled: true });
    const created = await this.client.request('auto.start', { ...request, work_unit_id: workUnitId });
    if (!created.run?.id) throw new Error(autoPauseMessage(created));
    await this.context.globalState.update('overseer.autoPendingStart', undefined);
    await this.saveDefaults({ repo, routing: 'auto', preferredHarness: preferred,
      mode: f.mode === 'current' ? 'current' : 'worktree' });
    await this.model.refresh();
    return created.run.id;
  }
}

function autoPauseMessage(created) {
  const reasons = (created.decision?.exclusions || []).map(x => x.reason);
  if (reasons.length && reasons.every(x => x === 'quota_exhausted')) return 'Auto routing paused: available account allowance is exhausted. Choose a manual agent or wait for a reset.';
  if (reasons.length && reasons.every(x => x === 'pool_in_flight_unknown_draw')) return 'Auto routing paused: an agent is already using the available account. Try again when it finishes.';
  if (created.discovery_failures?.length && !reasons.length) return 'Auto routing paused: account or model availability could not be checked. Refresh and try again, or choose a manual agent.';
  return 'Auto routing paused: no eligible route. Refresh account usage or choose a manual agent.';
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
