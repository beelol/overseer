// Talk to Overseer (AC-107): a chat with Overseer itself, docked in the panel under the editor area.
// It runs on the default account's harness (overseer.chat.harness, Claude Code unless set) as a task
// Overseer keeps for itself: a small scratch repository in the extension's storage, hidden from every
// list. Each message carries a snapshot of every agent (status, last message, changed files) read
// from the daemon, so Overseer can answer questions about them. To act, Overseer proposes actions in
// an `overseer-actions` block; the chat shows them as a card and nothing happens without a Yes.
// Follow-ups it sends appear in that agent's chat as coming from Overseer.
const vscode = require('vscode');
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const { createHash } = require('crypto');
const { page, localRoots } = require('./webview-html');

const OPEN = '<overseer-state>', CLOSE = '</overseer-state>';
const FROM = 'From Overseer: ';
const INSTRUCTIONS = [
  'You are Overseer, the orchestrator of the coding agents listed below. Answer the user\'s questions about them from this state; be brief and concrete.',
  'You never act on your own. To act, say in plain words exactly what you will do, then end your reply with one fenced block tagged overseer-actions holding a JSON array of actions:',
  '{"action":"follow_up","agent":"<run id>","text":"<message>"} sends a message to an agent; {"action":"stop","agent":"<run id>"} stops it; {"action":"pin","agent":"<run id>"} pins it to the grid;',
  '{"action":"start","repo":"<repository path>","title":"<short title>","prompt":"<task>"} starts a new agent.',
  'The user answers Yes or No in the interface; nothing happens without a Yes.',
].join('\n');
const ACTIVE = new Set(['queued', 'starting', 'running', 'waiting_for_user']);

class OverseerChat {
  constructor({ context, client, model, outputs, setPinned, log }) {
    Object.assign(this, { context, client, model, outputs, setPinned, log });
    this.done = new Set(context.globalState.get('overseer.chat.answered', []));
    const saved = context.globalState.get('overseer.chat', {});
    this.taskId = saved.taskId; this.runId = saved.runId;
    if (this.taskId) model.hide(this.taskId);
    // The panel hooks: wrap what the user types with the state; carry out an accepted proposal.
    outputs.transform = (runId, message) => (runId === this.runId && message?.type === 'followUp' ? { ...message, text: this.wrap(String(message.text || '')) } : message);
    outputs.onMessage = (runId, message) => (message?.type === 'overseerActions' || message?.type === 'overseerDecline' ? this.answer(runId, message) : false);
  }

  get harness() { return vscode.workspace.getConfiguration('overseer').get('chat.harness', 'claude'); }

  /** The WebviewView in the panel ("Talk to Overseer"). */
  resolveWebviewView(view) {
    this.view = view;
    view.webview.options = { enableScripts: true, localResourceRoots: localRoots(this.context.extensionUri) };
    view.onDidDispose(() => { if (this.view === view) this.view = undefined; });
    if (this.runId && this.model.run(this.runId)) this.outputs.attach(this.runId, view);
    else this.intro(view);
  }

  /** Before the first message: a composer that starts the conversation. */
  intro(view) {
    view.webview.html = page(view.webview, this.context.extensionUri, { title: 'Talk to Overseer', css: ['run-panel.css'],
      body: `<main class="talk-intro"><h2>Talk to Overseer</h2><p class="note">Ask what your agents are doing, or tell one of them what to do next. Overseer says what it will do and waits for your yes.</p>
<form id="first"><textarea id="prompt" rows="2" aria-label="Message to Overseer" placeholder="What is everyone doing?"></textarea><button class="btn primary" id="send" type="submit">Send</button></form><p class="note" id="why"></p></main>`,
      script: `const api = acquireVsCodeApi(); document.getElementById('first').addEventListener('submit', e => { e.preventDefault(); const t = document.getElementById('prompt').value.trim(); if (t) { document.getElementById('send').disabled = true; api.postMessage({ type: 'first', text: t }); } });
document.getElementById('prompt').addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); document.getElementById('first').requestSubmit(); } });
window.addEventListener('message', e => { if (e.data.type === 'notice') { document.getElementById('why').textContent = e.data.message; document.getElementById('send').disabled = false; } });` });
    view.webview.onDidReceiveMessage(m => { if (m?.type === 'first') this.start(String(m.text || '')).catch(error => view.webview.postMessage({ type: 'notice', message: error.message })); });
  }

  /** The scratch repository Overseer's own task runs in (the task needs a Git repository). */
  scratch() {
    const dir = path.join(this.context.globalStorageUri.fsPath, 'overseer-chat');
    if (!fs.existsSync(path.join(dir, '.git'))) {
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(path.join(dir, 'README.md'), '# Overseer\n\nOverseer\'s own conversation runs here. Nothing in this folder is part of your projects.\n');
      const git = (...args) => cp.execFileSync('git', args, { cwd: dir, stdio: 'ignore', env: { ...process.env, GIT_AUTHOR_NAME: 'Overseer', GIT_AUTHOR_EMAIL: 'overseer@localhost', GIT_COMMITTER_NAME: 'Overseer', GIT_COMMITTER_EMAIL: 'overseer@localhost' } });
      git('init', '-q'); git('add', '.'); git('commit', '-q', '-m', 'Overseer conversation');
    }
    return dir;
  }

  async start(text) {
    if (!text) return;
    await this.model.refresh();
    const created = await this.client.request('task.create', { repo: this.scratch(), harness: this.harness, prompt: this.wrap(text), title: 'Talk to Overseer', workspace_mode: 'current' });
    this.taskId = created.task?.id || created.run.task_id; this.runId = created.run.id;
    this.model.hide(this.taskId);
    await this.context.globalState.update('overseer.chat', { taskId: this.taskId, runId: this.runId });
    await this.model.refresh();
    if (this.view) await this.outputs.attach(this.runId, this.view);
  }

  /** What the harness receives: the instructions, a snapshot of every agent, then the user's words. */
  wrap(text) {
    const state = this.model.state;
    const agents = state.runs.filter(r => !r.parent_run_id).map(r => {
      const ws = this.model.workspace(r.workspace_id);
      return { id: r.id, title: r.title, status: r.status, harness: r.harness, repo: this.model.task(r.task_id)?.repo_root, worktree: ws?.path, last: (r.last_message || r.summary || '').slice(0, 400), active: ACTIVE.has(r.status) };
    });
    return `${OPEN}\n${INSTRUCTIONS}\n\nAgents (JSON):\n${JSON.stringify(agents, null, 1)}\n${CLOSE}\n\n${text}`;
  }

  async answer(runId, message) {
    if (runId !== this.runId) return false;
    const actions = Array.isArray(message.actions) ? message.actions : [];
    const key = createHash('sha256').update(String(message.key || '') + JSON.stringify(actions)).digest('hex');
    const reply = text => this.view?.webview.postMessage({ type: 'overseerAnswered', key: message.key, text });
    if (this.done.has(key)) { reply('Already answered.'); return true; }
    this.done.add(key); await this.context.globalState.update('overseer.chat.answered', [...this.done].slice(-500));
    if (message.type === 'overseerDecline') { reply('Declined: nothing was done.'); this.log('overseer chat: proposal declined'); return true; }
    const done = [];
    for (const a of actions) {
      const run = a.agent && this.model.run(String(a.agent));
      try {
        if (a.action === 'follow_up' && run) { await this.client.request('run.follow_up', { run_id: run.id, prompt: FROM + String(a.text || '') }); done.push(`sent "${a.text}" to ${run.title}`); }
        else if (a.action === 'stop' && run) { await this.client.request('run.interrupt', { run_id: run.id }); done.push(`stopped ${run.title}`); }
        else if (a.action === 'pin' && run) { await this.setPinned(run.id, true); done.push(`pinned ${run.title}`); }
        else if (a.action === 'start' && a.repo && a.prompt) {
          const c = await this.client.request('task.create', { repo: String(a.repo), harness: this.harness, prompt: FROM + String(a.prompt), title: String(a.title || a.prompt).slice(0, 60) });
          done.push(`started ${c.run.title || a.title}`);
        } else done.push(`skipped ${a.action || 'an action'} (unknown agent or missing details)`);
      } catch (error) { done.push(`${a.action} failed: ${error.message}`); }
    }
    this.model.scheduleRefresh();
    reply('Done: ' + done.join('; ') + '.');
    this.log('overseer chat: ' + done.join('; '));
    return true;
  }
}

module.exports = { OverseerChat, FROM, OPEN, CLOSE };
