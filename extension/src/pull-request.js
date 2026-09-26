// Open a pull request from a run (AC-50). Uses the GitHub sign-in VS Code already has (the
// built-in `github` authentication provider); the token stays in the extension host, is sent
// to git only as an in-memory http header via environment variables, and never reaches the
// daemon, events or logs. Never automatic, and nothing is merged.
const vscode = require('vscode');
const path = require('path');
const { execFile } = require('child_process');

const isLoopback = url => { try { return ['127.0.0.1', 'localhost', '[::1]'].includes(new URL(url).hostname); } catch { return false; } };

function git(cwd, args, env = {}) {
  return new Promise((resolve, reject) => execFile('git', args, { cwd, env: { ...process.env, GIT_TERMINAL_PROMPT: '0', ...env }, maxBuffer: 16 * 1024 * 1024 },
    (err, stdout, stderr) => err ? reject(new Error((stderr || err.message).replace(/AUTHORIZATION: [^\s'"]+ [^\s'"]+/gi, 'AUTHORIZATION: ***').trim())) : resolve(stdout.trim())));
}

/** Pull-request text: what the agent was asked, what changed, and that Overseer never merges. */
function describe(plan, prep) {
  const lines = [`Opened by Overseer from run \`${plan.run_id}\`${plan.harness ? ` (${plan.harness}${plan.model ? ', ' + plan.model : ''})` : ''}.`, ''];
  if (plan.prompt) lines.push('**Task**', '', ...String(plan.prompt).slice(0, 1500).split('\n').map(l => '> ' + l), '');
  if (prep.commits?.length) lines.push('**Commits**', '', ...prep.commits.slice(0, 30).map(c => '- ' + c), '');
  if (prep.files?.length) lines.push(`**Files changed** (${prep.files.length})`, '', ...prep.files.slice(0, 50).map(f => `- \`${f.status}\` ${f.path}`), ...(prep.files.length > 50 ? ['- …'] : []), '');
  lines.push('_Review before merging. Overseer never merges automatically._');
  return lines.join('\n');
}

class PullRequests {
  constructor(client, model, log) { this.client = client; this.model = model; this.log = log; }

  apiUrl() { return String(vscode.workspace.getConfiguration('overseer.github').get('apiUrl', 'https://api.github.com')).replace(/\/+$/, ''); }

  /** VS Code's GitHub session token, or undefined after explaining how to sign in. */
  async token() {
    const api = this.apiUrl();
    // Test-only: a mock GitHub API on this machine may use a fixed token; real GitHub never does.
    if (isLoopback(api) && process.env.OVERSEER_TEST_GITHUB_TOKEN) return process.env.OVERSEER_TEST_GITHUB_TOKEN;
    let session = await vscode.authentication.getSession('github', ['repo'], { createIfNone: false });
    if (!session) {
      this.log('open PR: VS Code has no GitHub session with repo access');
      const choice = await vscode.window.showWarningMessage('VS Code is not signed in to GitHub.', { modal: true, detail: 'Open PR uses the GitHub sign-in VS Code already has. No personal access token is needed.' }, 'Sign in to GitHub');
      if (choice !== 'Sign in to GitHub') return undefined;
      session = await vscode.authentication.getSession('github', ['repo'], { createIfNone: true });
    }
    return session?.accessToken;
  }

  async request(method, url, token, body) {
    const res = await fetch(url, { method, headers: { Accept: 'application/vnd.github+json', Authorization: `Bearer ${token}`, 'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'overseer-vscode', ...(body ? { 'Content-Type': 'application/json' } : {}) }, body: body ? JSON.stringify(body) : undefined });
    const text = await res.text();
    let json; try { json = JSON.parse(text); } catch { json = { message: text }; }
    return { status: res.status, json };
  }

  /** Quick pick of finished top-level runs in worktrees, newest first. */
  async pick() {
    const runs = this.model.state.runs.filter(r => !r.parent_run_id && this.model.workspace(r.workspace_id)?.kind === 'worktree' && !this.model.workspace(r.workspace_id)?.removed_ms)
      .sort((a, b) => b.created_ms - a.created_ms);
    if (!runs.length) { vscode.window.showInformationMessage('No run to open a pull request from.', { modal: true, detail: 'Open PR works on a run in its own worktree, and there are none yet. Start a task in a new worktree first.' }); return undefined; }
    const choice = await vscode.window.showQuickPick(runs.map(r => {
      const task = this.model.task(r.task_id), ws = this.model.workspace(r.workspace_id);
      return { label: task?.title || r.title, description: `${path.basename(task?.repo_root || '')} · ${ws.branch}`, detail: `${r.harness}${r.model ? ' · ' + r.model : ''} · ${r.status}`, run: r };
    }), { title: 'Open a pull request for which run?', matchOnDescription: true });
    return choice?.run;
  }

  async open(runId) {
    if (!vscode.workspace.isTrusted) throw new Error('Opening pull requests requires a trusted workspace.');
    // From the Command Palette with no run selected: ask which run, never do nothing silently.
    const picked = this.model.run(runId) || await this.pick();
    if (!picked) return;
    const run = this.model.rootRun(picked);
    const plan = await this.client.request('workspace.pr_plan', { workspace_id: run.workspace_id });
    this.log(`open PR for ${run.id}: ${plan.ok ? `${plan.owner}/${plan.repo} ${plan.branch} → ${plan.target}` : plan.reason}`);
    // Dialogs, not toasts: this answers a click, and VS Code's Do Not Disturb hides warning and info toasts.
    if (!plan.ok) { vscode.window.showWarningMessage('Open PR is unavailable.', { modal: true, detail: plan.reason }); return; }
    const token = await this.token();
    if (!token) return;
    const detail = [`${plan.owner}/${plan.repo}: ${plan.branch} → ${plan.target}`,
      plan.uncommitted.length ? `Commits ${plan.uncommitted.length} uncommitted worktree file(s) to ${plan.branch} first.` : `${plan.commits.length} commit(s) on ${plan.branch}.`,
      `Pushes ${plan.branch} to ${plan.remote} with your VS Code GitHub sign-in and opens a pull request. Nothing is merged.`].join('\n');
    const go = await vscode.window.showInformationMessage(`Open a pull request for ${plan.branch}?`, { modal: true, detail }, 'Open PR');
    if (go !== 'Open PR') return;
    return vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: `Opening a pull request on ${plan.owner}/${plan.repo}…` }, async () => {
      const prep = await this.client.request('workspace.pr_prepare', { workspace_id: run.workspace_id });
      const ws = this.model.workspace(run.workspace_id);
      // In-memory auth for this push only (not in argv, not in git config, not logged).
      const auth = /^https:\/\/github\.com\//.test(plan.remote_url) ? { GIT_CONFIG_COUNT: '1', GIT_CONFIG_KEY_0: 'http.https://github.com/.extraheader', GIT_CONFIG_VALUE_0: `AUTHORIZATION: basic ${Buffer.from(`x-access-token:${token}`).toString('base64')}` } : {};
      await git(ws.path, ['push', '--no-verify', plan.remote, `HEAD:refs/heads/${plan.branch}`], auth);
      const api = this.apiUrl();
      let res = await this.request('POST', `${api}/repos/${plan.owner}/${plan.repo}/pulls`, token, { title: plan.title, head: plan.branch, base: plan.target, body: describe(plan, prep), draft: false });
      let pr = res.json;
      if (res.status === 422 && /already exists/i.test(JSON.stringify(res.json))) {
        const existing = await this.request('GET', `${api}/repos/${plan.owner}/${plan.repo}/pulls?head=${encodeURIComponent(`${plan.owner}:${plan.branch}`)}&state=open`, token);
        pr = Array.isArray(existing.json) && existing.json[0];
        if (!pr) throw new Error('GitHub says a pull request already exists for this branch, but it could not be found.');
      } else if (res.status === 401 || res.status === 403) {
        throw new Error(`GitHub refused the request (${res.status}: ${res.json.message || 'not allowed'}). Check that your VS Code GitHub account can push to ${plan.owner}/${plan.repo}.`);
      } else if (res.status !== 201) {
        throw new Error(`GitHub could not create the pull request (${res.status}: ${res.json.message || 'error'}${res.json.errors ? ' — ' + res.json.errors.map(e => e.message || e.code).join('; ') : ''}).`);
      }
      await this.client.request('workspace.pr_opened', { workspace_id: run.workspace_id, url: pr.html_url, number: pr.number || 0 });
      this.log(`pull request ${pr.html_url}`);
      vscode.window.showInformationMessage(`Pull request #${pr.number} is open.`, { modal: true, detail: `${pr.html_url}\n\nNothing was merged.` }, 'Open on GitHub').then(choice => { if (choice) vscode.env.openExternal(vscode.Uri.parse(pr.html_url)); });
      return pr;
    });
  }
}

module.exports = { PullRequests, describe };
