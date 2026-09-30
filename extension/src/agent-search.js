// Searching agents (AC-112, AC-264). The side bar has no Search section: the Agents view's own
// search button (⌥⌘F) opens VS Code's input box at the top of the window. Typing filters the Agents
// list through the daemon's search (title, message, file, repository, account or status) as it
// goes; Enter keeps the filter and closes the box, Escape clears it. The box's filter button (and
// the view's Filter Agents…) picks All, Working, Needs you, To review, Done, Failed or Archived.
// While a search or filter is on, the Agents view's title says how many it shows.
const vscode = require('vscode');

const FILTER_LABELS = { all: 'All', working: 'Working', needs: 'Needs you', review: 'To review', done: 'Done', failed: 'Failed', archived: 'Archived' };

class AgentSearch {
  /** onQuery(text) runs a search ('' clears it); onFilterMenu() shows the status filters. */
  constructor({ onQuery, onFilterMenu }) {
    this.onQuery = onQuery; this.onFilterMenu = onFilterMenu;
    this.value = ''; this.filter = 'all'; this.count = ''; this.box = undefined;
  }

  filterButton() {
    const on = this.filter !== 'all';
    return { iconPath: new vscode.ThemeIcon(on ? 'filter-filled' : 'filter'), tooltip: `Filter agents: ${FILTER_LABELS[this.filter] || 'All'} (All, Working, Needs you, To review, Done, Failed, Archived)` };
  }

  /** Search Agents (⌥⌘F, the Agents view's search button, the command palette). */
  open() {
    if (this.box) { this.box.show(); return; }
    const box = vscode.window.createInputBox();
    this.box = box;
    box.title = 'Search agents';
    box.placeholder = 'Title, message, file, repository, account or status';
    box.value = this.value;
    box.prompt = this.count || 'Type to filter the Agents list. Enter keeps the filter; Escape clears it.';
    box.buttons = [this.filterButton()];
    let accepted = false;
    // Emptying the box clears the list a moment later, so retyping a query (select all, type) makes
    // one change to the Agents list, not two: VS Code holds a tree's second change for 200 ms.
    box.onDidChangeValue(v => {
      this.value = v; clearTimeout(this.clearing);
      if (v.trim()) this.onQuery(v.trim()); else this.clearing = setTimeout(() => { if (!this.value.trim()) this.onQuery(''); }, 300);
    });
    box.onDidAccept(() => { accepted = true; box.hide(); });
    box.onDidTriggerButton(() => { accepted = true; box.hide(); this.onFilterMenu(); });
    box.onDidHide(() => {
      clearTimeout(this.clearing);
      if (!accepted && this.value) { this.value = ''; this.onQuery(''); }
      else if (accepted && !this.value.trim()) this.onQuery('');
      box.dispose(); if (this.box === box) this.box = undefined;
    });
    box.show();
  }

  /** How many agents the list shows ("3 matches", "2 agents"), '' when nothing narrows it. */
  setCount(text) { this.count = text; if (this.box) this.box.prompt = text || 'No search: every agent is listed.'; }
  setFilter(kind) { this.filter = kind || 'all'; if (this.box) this.box.buttons = [this.filterButton()]; }
  clear() { clearTimeout(this.clearing); this.value = ''; if (this.box) this.box.value = ''; }
}

module.exports = { AgentSearch, FILTER_LABELS };
