// The search field at the top of the Overseer side bar (AC-112): a real input (VS Code has no text
// field inside a tree view), whose query filters the Agents list through the daemon's search.
const vscode = require('vscode');
const { page, localRoots } = require('./webview-html');

class SearchView {
  /** onQuery(text) runs a search ('' clears it). */
  constructor(extensionUri, { onQuery, onFilter }) {
    this.extensionUri = extensionUri; this.onQuery = onQuery; this.onFilter = onFilter;
    this.view = undefined; this.pending = [];
  }

  resolveWebviewView(view) {
    this.view = view;
    view.webview.options = { enableScripts: true, localResourceRoots: localRoots(this.extensionUri) };
    view.webview.html = page(view.webview, this.extensionUri, { title: 'Search agents', css: ['search-view.css'], js: ['search-view.js'],
      body: `<div class="search-field" role="search">
  <span class="codicon codicon-search" aria-hidden="true"></span>
  <input id="q" type="text" spellcheck="false" autocomplete="off" placeholder="Search agents" aria-label="Search agents (title, message, file, repository, account or status)" title="Search agents: title, message, file, repository, account or status (⌥⌘F)">
  <span id="count" class="count" aria-live="polite"></span>
  <button id="clear" type="button" class="icon" aria-label="Clear search" title="Clear search (Escape)" hidden><span class="codicon codicon-close" aria-hidden="true"></span></button>
</div>
<div class="filters" role="radiogroup" aria-label="Show agents">
  <button type="button" role="radio" data-filter="all" aria-checked="true">All</button>
  <button type="button" role="radio" data-filter="working" aria-checked="false"><span class="dot working" aria-hidden="true"></span>Working</button>
  <button type="button" role="radio" data-filter="needs" aria-checked="false"><span class="dot needs" aria-hidden="true"></span>Needs you</button>
  <button type="button" role="radio" data-filter="done" aria-checked="false"><span class="dot done" aria-hidden="true"></span>Done</button>
  <button type="button" role="radio" data-filter="failed" aria-checked="false"><span class="dot failed" aria-hidden="true"></span>Failed</button>
  <button type="button" role="radio" data-filter="archived" aria-checked="false"><span class="codicon codicon-archive" aria-hidden="true"></span>Archived</button>
</div>` });
    view.webview.onDidReceiveMessage(m => { if (m.type === 'query') this.onQuery(String(m.value || '')); else if (m.type === 'filter') this.onFilter?.(String(m.value || 'all')); });
    view.onDidDispose(() => { this.view = undefined; });
    for (const m of this.pending.splice(0)) view.webview.postMessage(m);
  }

  post(m) { if (this.view) this.view.webview.postMessage(m); else this.pending.push(m); }
  /** Puts the cursor in the field (shows the Overseer side bar first). */
  async focus() {
    await vscode.commands.executeCommand('overseer.search.focus');
    this.post({ type: 'focus' });
    // From the command palette, VS Code gives focus back to where it was as the palette closes;
    // put the cursor in the field again once that has happened.
    setTimeout(() => this.post({ type: 'focus' }), 250);
  }
  setCount(text) { this.post({ type: 'count', text }); }
  clear() { this.post({ type: 'set', value: '' }); }
  setFilter(value) { this.post({ type: 'filter', value }); }
}

module.exports = { SearchView };
