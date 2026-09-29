// Landing an agent's work (AC-232, AC-243): the Merge button on a finished agent's chat and review,
// one confirmation that lists the files (the untracked ones apart), Cancel for a merge that stopped
// on conflicts, and, for a repository with no GitHub remote, the local merge (or Publish to GitHub
// with a yes) instead of Open PR. Never a system alert for "no remote". What the work became comes
// from the daemon (`state.landings`), so every surface reads "Merged into main (commit)".
const vscode = require('vscode');
const path = require('path');
const Text = require('../media/landing-text.js');

const ACTIVE = new Set(['queued', 'starting', 'running', 'waiting_for_user', 'waiting_for_connection', 'waiting_for_memory']);
const HARNESS = { claude: 'Claude Code', codex: 'Codex', 'codex-app': 'Codex', opencode: 'OpenCode', 'opencode-serve': 'the local model', generic: 'the program' };

class Landing {
  /** deps: { pickAgent(arg, opts), hasWorktree(run), notice(runId, text), log(text), onMerged(runId) } */
  constructor(client, model, deps) {
    this.client = client; this.model = model; this.deps = deps;
    this.plans = new Map(); // workspace id -> { plan, key, at, loading }
    this.listeners = new Set();
  }

  onDidChange(fn) { this.listeners.add(fn); return { dispose: () => this.listeners.delete(fn) }; }

  root(runId) { const run = this.model.run(runId); return run && (this.model.rootRun(run) || run); }

  /** When a workspace's cached plan is out of date: its agent changed state, or its work landed. */
  key(run) {
    const turns = this.model.state?.turns?.[run.id] || [];
    return JSON.stringify([run.status, run.ended_ms, turns.length, this.model.state?.landings?.[run.workspace_id] || null]);
  }

  /** Fetches the merge plan again (in the background unless awaited). */
  async refresh(runId, { force = false } = {}) {
    const run = this.root(runId);
    const ws = run && this.model.workspace(run.workspace_id);
    if (!run || !ws || ws.kind !== 'worktree' || ws.removed_ms || ACTIVE.has(run.status) || !this.client.connected) return undefined;
    const key = this.key(run);
    const had = this.plans.get(ws.id);
    if (!force && had && (had.loading || (had.key === key && Date.now() - had.at < 20000))) return had.loading || had.plan;
    const loading = this.client.request('workspace.merge_plan', { workspace_id: ws.id }).then(plan => {
      const before = JSON.stringify(this.plans.get(ws.id)?.plan);
      this.plans.set(ws.id, { plan, key, at: Date.now() });
      if (JSON.stringify(plan) !== before) for (const fn of this.listeners) fn(run.id);
      return plan;
    }, error => { this.plans.set(ws.id, { plan: had?.plan, key, at: Date.now() }); this.deps.log?.('merge plan: ' + error.message); return had?.plan; });
    this.plans.set(ws.id, { ...(had || {}), loading });
    return loading;
  }

  /**
   * What a finished agent's chat, review and menus offer, from the cached plan (fetched when out of
   * date). merged / conflicts / pr come from the daemon's state; canMerge from the plan.
   */
  summary(runId) {
    const run = this.root(runId);
    if (!run) return undefined;
    const ws = this.model.workspace(run.workspace_id);
    const landing = this.model.state?.landings?.[run.workspace_id] || null;
    const active = ACTIVE.has(run.status);
    this.refresh(run.id);
    const plan = this.plans.get(run.workspace_id)?.plan;
    const midMerge = !!plan && (plan.state === 'resolving' || plan.state === 'resolved');
    return {
      runId: run.id, worktree: ws?.kind === 'worktree', removed: !!ws?.removed_ms, active,
      landing, text: Text.text(landing), merged: landing?.state === 'merged',
      conflicts: !active && (midMerge || landing?.state === 'conflicts') ? (plan?.conflicts?.length ? plan.conflicts : landing?.files || []) : null,
      canMerge: !active && !!plan?.ok, target: plan?.target || landing?.target || 'main',
      files: plan?.ok ? (plan.files || []).length : 0,
      github: !!plan?.remote?.github, remote: plan?.remote ? plan.remote.name : plan ? null : undefined,
      why: plan && !plan.ok ? plan.reason : '',
    };
  }

  async plan(wsId) {
    const plan = await this.client.request('workspace.merge_plan', { workspace_id: wsId });
    return plan;
  }

  say(runId, text) { this.deps.notice?.(runId, text); this.deps.log?.(`landing ${runId}: ${text}`); }

  /** The Merge button (and Merge Back in the palette): one confirmation listing the files, then the merge. */
  async merge(arg) {
    if (!vscode.workspace.isTrusted) throw new Error('Overseer only controls agents in a trusted workspace.');
    const picked = this.model.run(await this.deps.pickAgent(arg, { title: 'Merge which agent?', fits: r => this.deps.hasWorktree(r) && !ACTIVE.has(r.status), none: 'No finished agent has a worktree to merge.' }));
    if (!picked) return undefined;
    const run = this.root(picked.id);
    const wsId = run.workspace_id;
    let plan = await this.plan(wsId);
    if (!plan.ok) {
      const landed = Text.text(plan.landing);
      this.say(run.id, plan.landing?.state === 'merged' ? `${landed}. Nothing new to merge.` : `Nothing to merge: ${plan.reason}`);
      vscode.window.showInformationMessage(plan.landing?.state === 'merged' ? `“${run.title}” is already ${landed.replace(/^M/, 'm')}.` : `Nothing to merge: ${plan.reason}`);
      return undefined;
    }
    // A merge that stopped on conflicts: finish it once the files are resolved.
    if (plan.state === 'resolving' || plan.state === 'resolved') {
      const res = await this.client.request('workspace.merge_resolved', { workspace_id: wsId });
      if (res.state !== 'ready') {
        this.say(run.id, `Conflict markers remain in ${res.remaining.join(', ')}. Resolve them (or ask the agent), then press Finish merge again, or cancel the merge.`);
        await this.refresh(run.id, { force: true });
        return undefined;
      }
      plan = await this.plan(wsId);
    }
    if (plan.blockers.length) {
      // Answers a click, so a dialog (Do Not Disturb hides toasts).
      await vscode.window.showWarningMessage(`Cannot merge into ${plan.target} yet.`, { modal: true, detail: plan.blockers.join('\n\n') });
      return undefined;
    }
    const label = Text.mergeLabel(plan.target);
    const files = plan.files || [], untracked = plan.untracked || [];
    const detail = [
      `${files.length} file${files.length === 1 ? '' : 's'} land${files.length === 1 ? 's' : ''} on ${plan.target} in ${path.basename(plan.repo)}:`,
      ...files.slice(0, 25).map(f => `${f.status}  ${f.path}`), ...(files.length > 25 ? [`… and ${files.length - 25} more`] : []),
      ...(untracked.length ? ['', `Not tracked by Git yet (committed to the agent's branch first, check none is a secret):`, ...untracked.slice(0, 15).map(f => `?  ${f}`), ...(untracked.length > 15 ? [`… and ${untracked.length - 15} more`] : [])] : []),
      '', `Git's hooks run. If ${plan.target} changed the same lines, ${HARNESS[run.harness] || run.harness} is asked to combine them, and you can cancel the merge from its chat.`,
      'The worktree is kept until you clean it up.',
    ].join('\n');
    const go = await vscode.window.showInformationMessage(`${label}: “${run.title}”?`, { modal: true, detail }, label);
    if (go !== label) return undefined;
    if (plan.state === 'idle') {
      const prep = await this.client.request('workspace.merge_prepare', { workspace_id: wsId, handoff: true });
      await this.model.refresh();
      if (prep.state === 'conflicts') {
        const how = prep.handoff?.sent ? `${HARNESS[run.harness] || 'The agent'} was asked to combine them; press Finish merge when it is done` : 'Resolve them in the worktree, then press Finish merge';
        this.say(run.id, `The merge stopped: ${plan.target} changed the same lines in ${prep.files.join(', ')}. ${how}, or Cancel merge.`);
        await this.refresh(run.id, { force: true });
        return prep;
      }
    }
    const done = await this.client.request('workspace.merge_complete', { workspace_id: wsId });
    await this.model.refresh();
    await this.refresh(run.id, { force: true });
    const text = `Merged into ${done.target} (${Text.short(done.commit)})`;
    this.say(run.id, `${text}. The worktree is kept until you clean it up.`);
    this.deps.onMerged?.(run.id);
    vscode.window.showInformationMessage(`${text}: “${run.title}”.`, 'Clean Up').then(choice => { if (choice) vscode.commands.executeCommand('overseer.cleanupWorkspace', run.id); });
    return done;
  }

  /** Cancel merge: the worktree goes back to how it was before the merge (AC-243). */
  async cancel(runId) {
    if (!vscode.workspace.isTrusted) throw new Error('Overseer only controls agents in a trusted workspace.');
    const run = this.root(runId);
    if (!run) return undefined;
    const res = await this.client.request('workspace.merge_abort', { workspace_id: run.workspace_id });
    await this.model.refresh();
    await this.refresh(run.id, { force: true });
    this.say(run.id, res.was_merging ? `Merge cancelled. The worktree is as it was before the merge${res.uncommitted ? ', its changes uncommitted again' : ''}.` : 'No merge was in progress.');
    return res;
  }

  /**
   * A repository with no GitHub remote (AC-232): offered the local merge, or Publish to GitHub with
   * a yes, in the window (a quick pick), never a system alert.
   */
  async offerLocal(runId, reason) {
    const run = this.root(runId);
    if (!run) return undefined;
    const plan = await this.plan(run.workspace_id).catch(() => undefined);
    const target = plan?.target || 'main';
    const noRemote = !plan?.remote;
    const items = [
      { label: `$(git-merge) ${Text.mergeLabel(target)}`, detail: `Merge the agent's work into ${target} in this repository, on this Mac. Nothing is pushed.`, act: 'merge' },
      ...(noRemote ? [{ label: '$(github) Publish to GitHub…', detail: 'Create a GitHub repository for it first; you are asked before anything is created.', act: 'publish' }] : []),
    ];
    const title = noRemote ? 'This repository has no GitHub remote, so there is no pull request to open' : `No pull request: ${reason || 'the remote is not on GitHub'}`;
    const pick = await vscode.window.showQuickPick(items, { title, placeHolder: 'Choose what to do with the agent\'s work' });
    if (pick?.act === 'merge') return this.merge(run.id);
    if (pick?.act === 'publish') return this.publish(run.id);
    return undefined;
  }

  /** Publish to GitHub, only after a yes: VS Code's own Publish to GitHub (it asks private or public). */
  async publish(runId) {
    if (!vscode.workspace.isTrusted) throw new Error('Overseer only controls agents in a trusted workspace.');
    const run = this.root(runId);
    const task = run && this.model.task(run.task_id);
    if (!task) return undefined;
    const yes = await vscode.window.showQuickPick([
      { label: '$(github) Yes, publish to GitHub', detail: `Creates a GitHub repository for ${path.basename(task.repo_root)} with VS Code's GitHub sign-in and pushes it. VS Code asks whether it is private or public.`, yes: true },
      { label: 'No', detail: 'Nothing is created or pushed.' },
    ], { title: `Publish ${path.basename(task.repo_root)} to GitHub?` });
    if (!yes?.yes) return undefined;
    await vscode.commands.executeCommand('github.publish', vscode.Uri.file(task.repo_root));
    await this.refresh(run.id, { force: true });
    return true;
  }
}

module.exports = { Landing };
