// Actions a chat can ask for, shared by run panels and the dashboard. Every control action requires
// a trusted workspace and only ever targets the given run.
const vscode = require('vscode');

const CONTROL = ['followUp', 'interrupt', 'permission', 'mergeBack', 'signIn', 'openPullRequest', 'cleanup'];

/** Handles one chat message for runId. reply(msg) answers the webview. Returns true if handled. */
async function handleRunMessage({ client, model }, runId, message, reply) {
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

module.exports = { handleRunMessage, changesFetcher };
