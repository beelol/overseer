// Pure layout decisions shared by arrangement.js and overseer-window.js (no `vscode` here, so the
// unit tests in test/unit/layout.js run under plain Node).
//   - Which tabs are Overseer's own (its view, chats, reviews, the New Task form).
//   - Whether placing Overseer would merge the owner's editor groups (AC-244): then Overseer opens
//     beside them instead of rebuilding the editor layout.

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

module.exports = { OURS, isOurs, ownerGroups, besideOwner };
