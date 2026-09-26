// Actions a chat can ask for, shared by run panels and the dashboard. Every control action requires
// a trusted workspace and only ever targets the given run.
const vscode = require('vscode');

const CONTROL = ['followUp', 'interrupt', 'permission', 'mergeBack', 'signIn', 'openPullRequest', 'cleanup', 'steer'];
const ACTIVE = new Set(['queued', 'starting', 'running']);

/**
 * Steering a working agent (AC-60): a message queued for when the turn ends, or "stop and send"
 * (interrupt, then send once the run has stopped). One pending message per run; any window.
 */
class Steering {
  constructor(client, model) {
    this.client = client; this.model = model; this.pending = new Map(); // run id -> { text, options, how }
    client.on('event', ev => { if (['status', 'turn_done'].includes(ev.kind) && this.pending.has(ev.run_id)) setTimeout(() => this.flush(ev.run_id), 300); });
  }
  queued(runId) { return this.pending.get(runId); }
  async set(runId, text, options, how) {
    if (how === 'cancel') { this.pending.delete(runId); this.model.emitter.fire(); return; }
    this.pending.set(runId, { text, options, how });
    this.model.emitter.fire();
    if (how === 'interrupt') await this.client.request('run.interrupt', { run_id: runId });
    await this.flush(runId);
  }
  async flush(runId) {
    const p = this.pending.get(runId); if (!p) return;
    const state = await this.client.request('state');
    const run = state.runs.find(r => r.id === runId);
    if (!run || ACTIVE.has(run.status)) return; // still working: try again on the next status event
    this.pending.delete(runId);
    try { await this.client.request('run.follow_up', { run_id: runId, prompt: p.text, ...(p.options || {}) }); }
    finally { this.model.scheduleRefresh(); }
  }
}

/** Handles one chat message for runId. reply(msg) answers the webview. Returns true if handled. */
async function handleRunMessage({ client, model, steering }, runId, message, reply) {
  if (!message || typeof message !== 'object') return false;
  if (!vscode.workspace.isTrusted && CONTROL.includes(message.type)) throw new Error('Controlling agents requires a trusted workspace.');
  switch (message.type) {
    case 'followUp': {
      const text = String(message.text || '').trim();
      if (!text) return true;
      await client.request('run.follow_up', { run_id: runId, prompt: text, ...(message.options || {}) });
      model.scheduleRefresh();
      return true;
    }
    case 'steer': await steering.set(runId, String(message.text || ''), message.options, String(message.how || 'queue')); return true;
    case 'mentionFiles': {
      const run = model.run(runId);
      const params = message.workspace_id ? { workspace_id: String(message.workspace_id) } : message.repo ? { repo: String(message.repo) } : run ? { workspace_id: run.workspace_id } : undefined;
      if (!params) return true;
      const res = await client.request('repo.files', { ...params, query: String(message.query || ''), limit: 30 }).catch(() => ({ files: [] }));
      reply({ type: 'mentionFiles', files: res.files || [], seq: message.seq, scope: message.scope });
      return true;
    }
    case 'interrupt': await client.request('run.interrupt', { run_id: runId }); return true;
    case 'permission':
      await client.request('run.permission', { run_id: runId, request_id: String(message.request_id), allow: !!message.allow });
      model.scheduleRefresh();
      return true;
    case 'raw': reply({ type: 'raw', raw: await client.request('run.raw_output', { run_id: runId, max_bytes: 512 * 1024 }) }); return true;
    case 'signIn': {
      const run = model.rootRun(model.run(runId) || {}) || model.run(runId);
      const profile = run?.profile_id && model.profile(run.profile_id);
      if (!profile) throw new Error('This run has no account to sign in.');
      await vscode.commands.executeCommand('overseer.signIn', { profile });
      return true;
    }
    case 'openPullRequest': await vscode.commands.executeCommand('overseer.openPullRequest', runId); return true;
    case 'mergeBack': await vscode.commands.executeCommand('overseer.mergeBack', runId); return true;
    case 'openReview': await vscode.commands.executeCommand('overseer.openReview', runId); return true;
    case 'openEdit': await vscode.commands.executeCommand('overseer.openEdit', String(message.runId || runId), String(message.path || '')); return true;
    case 'cleanup': await vscode.commands.executeCommand('overseer.cleanupWorkspace', runId); return true;
    case 'openPanel': await vscode.commands.executeCommand('overseer.showOutput', runId); return true;
    case 'archive': await client.request('task.archive', { task_id: String(message.taskId), archived: message.archived !== false }); await model.refresh(); return true;
    case 'copy': await vscode.env.clipboard.writeText(String(message.text || '')); return true;
    case 'openExternal': {
      const url = String(message.url || '');
      if (/^https?:\/\//i.test(url)) await vscode.env.openExternal(vscode.Uri.parse(url));
      return true;
    }
    default: return false;
  }
}

/** Changes since the task started (files, +/−), throttled per workspace. */
function changesFetcher(client) {
  const last = new Map();
  return async (workspaceId, { force } = {}) => {
    const now = Date.now(), prev = last.get(workspaceId);
    if (!force && prev && now - prev.at < 1500) return prev.value;
    try { const value = await client.request('workspace.changes', { workspace_id: workspaceId }); last.set(workspaceId, { at: now, value }); return value; }
    catch { return prev?.value; }
  };
}

module.exports = { handleRunMessage, changesFetcher, Steering };
