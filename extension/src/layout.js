// Pure layout decisions shared by arrangement.js and dashboard-mode.js (no `vscode` here, so the
// unit tests in test/unit/layout.js run under plain Node).
//   - Which tabs are Overseer's own (its view, chats, reviews, the New Task form).
//   - Whether placing Overseer would merge the owner's editor groups (AC-244): then Overseer opens
//     beside them instead of rebuilding the editor layout.
//   - What the Overseer workspace keeps of the owner's tabs, to put them back exactly (AC-250).
//   - The three columns of the workspace, sized for the screen (AC-250).

const OURS = /overseer\.(center|output|review|newTask|chatEditor)$/;

/** Overseer's own tab: a webview or custom editor whose view type is one of Overseer's. */
function isOurs(tab) {
  const vt = tab && tab.input && tab.input.viewType;
  return typeof vt === 'string' && OURS.test(vt);
}

/** Groups holding something of the owner's (any tab that is not Overseer's). */
function ownerGroups(groups) {
  return groups.filter(g => g.tabs.some(t => !isOurs(t)));
}

/**
 * Overseer opens beside the owner's editors (AC-244) when rebuilding the editor layout would merge
 * or collapse groups the owner arranged: two or more groups, one of them holding only the owner's
 * editors. A file opened from the review into the review's own group leaves that group Overseer's
 * (the usual arrangement goes on). `groups` are the main window's (a review popped out into its own
 * window is not counted).
 */
function besideOwner(groups) {
  return groups.length >= 2 && groups.some(g => g.tabs.length > 0 && g.tabs.every(t => !isOurs(t)));
}

/**
 * The owner's tabs as plain data, group by group, so they can be closed while the workspace is
 * open and reopened exactly afterwards. `kinds` are the vscode Tab input classes. Tabs VS Code does
 * not describe to extensions (the Welcome page, Settings, release notes) are listed as `other`:
 * the workspace closes them and cannot bring them back.
 */
function snapshotTabs(groups, kinds) {
  return groups.map(g => ({
    viewColumn: g.viewColumn, active: !!g.isActive,
    tabs: g.tabs.filter(t => !isOurs(t)).map(t => {
      const i = t.input;
      const base = { label: t.label, active: !!t.isActive, pinned: !!t.isPinned, preview: !!t.isPreview, dirty: !!t.isDirty };
      if (kinds.TabInputTextDiff && i instanceof kinds.TabInputTextDiff) return { ...base, kind: 'diff', original: i.original.toString(), modified: i.modified.toString() };
      if (kinds.TabInputText && i instanceof kinds.TabInputText) return { ...base, kind: 'text', uri: i.uri.toString() };
      if (kinds.TabInputCustom && i instanceof kinds.TabInputCustom) return { ...base, kind: 'custom', uri: i.uri.toString(), viewType: i.viewType };
      if (kinds.TabInputNotebook && i instanceof kinds.TabInputNotebook) return { ...base, kind: 'notebook', uri: i.uri.toString(), viewType: i.notebookType };
      return { ...base, kind: 'other' };
    }),
  }));
}

/** The tabs the workspace closes: every owner tab except unsaved ones (never lose work). */
function straysToClose(snapshot) {
  return snapshot.flatMap(g => g.tabs.filter(t => !t.dirty).map(t => ({ ...t, viewColumn: g.viewColumn })));
}

/**
 * The workspace's three columns: Overseer's conversation, the agent's review (Follow), the agent's
 * chat. `editorWidth` is the editor area in CSS pixels with the side bar as it is now. Each side
 * column keeps at least 380 px (AC-77's 360 plus padding); when the side bar would squeeze them
 * below that, the side bar is hidden for the workspace (the conversation lists the agents).
 */
function workspaceColumns(editorWidth, sideBarWidth = 0) {
  const MIN = 380, REVIEW = 520;
  let width = editorWidth, hideSideBar = false;
  if (width && width < 2 * MIN + REVIEW && sideBarWidth) { hideSideBar = true; width += sideBarWidth; }
  const side = width ? Math.max(0.26, Math.min(0.32, MIN / width)) : 0.3;
  return { hideSideBar, sizes: [side, 1 - 2 * side, side] };
}

module.exports = { OURS, isOurs, ownerGroups, besideOwner, snapshotTabs, straysToClose, workspaceColumns };
