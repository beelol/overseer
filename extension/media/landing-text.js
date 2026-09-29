// What an agent's work became (AC-243), in the words every surface uses: the side bar, the chat,
// the grid, the review and (tui/src/model.rs `landing_text`) the TUI. `landing` is the daemon's
// `state.landings[workspace_id]`: merged (target, commit), conflicts (files) or pr (url).
// Pure functions, no DOM and no VS Code. (UMD: `require` in Node, `window.OverseerLanding` in a webview.)
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.OverseerLanding = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  const short = commit => String(commit || '').slice(0, 7);
  const prNumber = url => { const n = /\/pull\/(\d+)(?:[/?#]|$)/.exec(String(url || '')); return n ? n[1] : ''; };

  /** "Merged into main (1a2b3c4)", "Merge stopped: conflicts in a.txt", "Pull request #7 open", or ''. */
  function text(landing) {
    if (!landing || !landing.state) return '';
    if (landing.state === 'merged') return `Merged into ${landing.target || 'main'}${landing.commit ? ` (${short(landing.commit)})` : ''}`;
    if (landing.state === 'conflicts') { const f = landing.files || []; return f.length ? `Merge stopped: conflicts in ${f.join(', ')}` : 'Merge stopped: conflicts'; }
    if (landing.state === 'pr') { const n = prNumber(landing.url); return n ? `Pull request #${n} open` : 'Pull request open'; }
    return '';
  }

  /** The label of the Merge button: "Merge into main". */
  const mergeLabel = target => `Merge into ${target || 'main'}`;

  return { text, short, prNumber, mergeLabel };
});
