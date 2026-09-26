// New Task form (AC-47): every option on one page (the dashboard's composer covers the common
// case). Uses the shared launcher, so both start tasks the same way and share remembered defaults.
const vscode = require('vscode');
const { page, localRoots } = require('./webview-html');

const LABELS = { codex: 'Codex', 'codex-app': 'Codex app-server', claude: 'Claude Code', opencode: 'OpenCode', generic: 'Program' };

class NewTaskPanel {
  constructor(context, client, model, { selectRun, launcher, column }) {
    this.context = context; this.client = client; this.model = model;
    this.selectRun = selectRun; this.launcher = launcher; this.column = column;
  }

  async open() {
    if (this.panel) { this.panel.reveal(); return; }
    const panel = vscode.window.createWebviewPanel('overseer.newTask', 'New Task', { viewColumn: this.column() || vscode.ViewColumn.Active, preserveFocus: false }, { enableScripts: true, localResourceRoots: localRoots(this.context.extensionUri), retainContextWhenHidden: true });
    this.panel = panel;
    panel.iconPath = vscode.Uri.joinPath(this.context.extensionUri, 'media', 'overseer.svg');
    panel.webview.html = page(panel.webview, this.context.extensionUri, { title: 'New Task', css: ['new-task.css'], js: ['new-task.js'], body: `
<main class="form" data-audit-view="new-task">
<h1>New task</h1>
<div id="error" class="error" role="alert" hidden></div>
<section><label for="prompt" class="sec">Task</label><textarea id="prompt" placeholder="Describe the task" aria-describedby="prompt-help"></textarea><div id="prompt-help" class="help">⌘⏎ starts</div></section>
<section><h2 id="repos-h" class="sec">Repository</h2><div id="repos" class="tiles" aria-labelledby="repos-h"></div></section>
<section><h2 id="harnesses-h" class="sec">Agent</h2><div id="harnesses" class="tiles" aria-labelledby="harnesses-h"></div></section>
<section id="account-section"><h2 id="accounts-h" class="sec">Account</h2><div id="accounts" class="tiles" aria-labelledby="accounts-h"></div>
<p id="no-accounts" class="empty" hidden>No account for this agent yet. Add one from Accounts.</p></section>
<section id="generic-section" hidden><h2 class="sec">Program</h2><div class="row"><div><label for="program">Executable</label><input id="program" placeholder="/absolute/path/to/program"></div>
<div><label for="args">Arguments (JSON)</label><input id="args" value="[]"></div></div></section>
<section><h2 id="modes-h" class="sec">Workspace</h2><div id="modes" class="tiles" aria-labelledby="modes-h"></div>
<div id="ref-row" class="row ref-row"><div><label for="ref">Start from</label><select id="ref"></select></div><div><label for="model">Model</label><input id="model" placeholder="Default"></div></div></section>
<section id="approval-section" hidden><h2 id="approvals-h" class="sec">Approvals</h2><div id="approvals" class="tiles" aria-labelledby="approvals-h"></div></section>
<div class="actions"><button id="start" class="btn primary">Start task</button><span id="start-why" class="why" role="status"></span></div>
</main>` });
    panel.onDidDispose(() => { this.panel = undefined; });
    panel.webview.onDidReceiveMessage(m => this.receive(m).catch(error => panel.webview.postMessage({ type: 'error', message: error.message })));
  }

  async receive(m) {
    if (!m || typeof m !== 'object') return;
    if (m.type === 'ready') {
      const d = await this.launcher.data();
      this.panel.webview.postMessage({ type: 'data', data: { ...d, harnesses: d.harnesses.map(h => ({ ...h, label: LABELS[h.harness] || h.harness })) } });
    } else if (m.type === 'branches') {
      const b = await this.launcher.branches(m.repo);
      this.panel.webview.postMessage({ type: 'branches', repo: m.repo, info: { branches: b.list, head: b.head } });
    } else if (m.type === 'browse') {
      const repo = await this.launcher.browse();
      if (repo) this.panel.webview.postMessage({ type: 'repoAdded', repo });
    } else if (m.type === 'start') {
      const runId = await this.launcher.start(m.form || {});
      if (!runId) return;
      this.panel?.webview.postMessage({ type: 'started' });
      this.panel?.dispose();
      await this.selectRun(runId, { follow: vscode.workspace.getConfiguration('overseer').get('followNewRuns', true) });
    }
  }
}

module.exports = { NewTaskPanel };
