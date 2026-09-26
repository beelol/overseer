// Run panel webview: one chat filling the tab. State (run, view, scroll, draft) survives reloads.
(function () {
  const vscode = acquireVsCodeApi();
  const runId = document.body.dataset.runId;
  const saved = vscode.getState() || {};
  const same = saved.runId === runId;
  const app = document.getElementById('app');
  let chat;
  const persist = () => chat && vscode.setState({ runId, ...chat.state() });
  chat = new window.OverseerChat(app, { post: m => vscode.postMessage(m), mode: 'panel', onState: persist });
  chat.reset(runId);
  if (same && saved.draft) { chat.prompt.value = saved.draft; chat.grow(); }
  if (same && saved.view === 'log') chat.show('log');
  persist();
  window.addEventListener('message', e => {
    const m = e.data;
    if (m.type === 'run') chat.setRun(m);
    else if (m.type === 'history') chat.history(m.events, m.truncated, same ? saved : undefined);
    else if (m.type === 'events') chat.events(m.items);
    else if (m.type === 'notice') chat.notice(m.message);
    else if (m.type === 'raw') chat.raw(m.raw);
    else if (m.type === 'changes') chat.changes(m.changes);
  });
  vscode.postMessage({ type: 'ready' });
})();
