// What the agents are doing, counted once (AC-246, AC-254, AC-255, AC-256): the agents that need
// you, the ones not reviewed yet, and a rollup by state that the side bar and the grid both show.
// Pure functions of the daemon's state and the owner's reviewed marks, with no DOM and no VS Code,
// so the extension host, the webviews and the unit tests count the same way. (UMD: `require` in
// Node, `window.OverseerRollup` in a webview.)
//
// The agents counted are the side bar's: each task's newest top-level run, archived tasks left out.
// - Needs you (action): an agent waiting on a permission or a question, and Overseer's proposals
//   or conflicts waiting for a decision. The TUI and the phone count the same items.
// - To review (review, separate from Needs you): an agent that reached its end (done, stopped or
//   failed) in the last 7 days and whose review has not been opened since, nor merged.
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.OverseerRollup = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  const WAITING = new Set(['waiting_for_user']);
  const WORKING = new Set(['queued', 'starting', 'running', 'waiting_for_connection', 'waiting_for_memory']);
  const DONE = new Set(['completed', 'interrupted']);
  const FAILED = new Set(['failed', 'disconnected']);
  const WEEK = 7 * 86400000;
  const ended = r => r.ended_ms || r.created_ms || 0;

  /** Each non-archived task's newest top-level run (the agents the side bar lists). */
  function agents(state) {
    const archived = new Set((state.tasks || []).filter(t => t.archived_ms).map(t => t.id));
    const hidden = new Set(Object.entries(state.oversight || {}).filter(([, o]) => o && o.role === 'overseer').map(([id]) => id));
    const roots = new Map();
    for (const r of state.runs || []) {
      if (r.parent_run_id || archived.has(r.task_id) || hidden.has(r.id) || r.swarm_membership) continue;
      const cur = roots.get(r.task_id);
      if (!cur || r.created_ms > cur.created_ms) roots.set(r.task_id, r);
    }
    return [...roots.values()];
  }

  const waiting = r => WAITING.has(r.status) || (r.attention && r.attention.kind === 'permission' && !FAILED.has(r.status) && !DONE.has(r.status));

  /** Needs you: what waits for the owner's answer, most urgent first. */
  function needsYou(state) {
    const out = [];
    const ov = state.overseer || {};
    if (ov.run_id && (ov.open_proposals || ov.conflicts_needing_decision)) {
      const parts = [ov.open_proposals && `${ov.open_proposals} proposal${ov.open_proposals === 1 ? '' : 's'}`, ov.conflicts_needing_decision && `${ov.conflicts_needing_decision} conflict${ov.conflicts_needing_decision === 1 ? '' : 's'}`].filter(Boolean);
      out.push({ run_id: 'overseer', overseer: true, rank: 0, label: 'Decide', detail: `Overseer: ${parts.join(', ')} waiting for you` });
    }
    for (const r of agents(state)) {
      if (!waiting(r)) continue;
      const asks = r.attention && r.attention.kind === 'permission';
      out.push({ run_id: r.id, rank: 0, label: asks ? 'Approve' : 'Reply', detail: asks ? `Wants to use ${r.attention.tool || 'a tool'}` : 'Waiting for your reply' });
    }
    return out;
  }

  /** Whether a run at its end still waits to be reviewed (`reviewed`: run id -> when its review was opened). */
  function unreviewed(run, reviewed, now = Date.now()) {
    if (!run || run.parent_run_id || !(DONE.has(run.status) || FAILED.has(run.status))) return false;
    if (now - ended(run) > WEEK) return false;
    const seen = typeof reviewed === 'function' ? reviewed(run.id) : reviewed && (reviewed.get ? reviewed.get(run.id) : reviewed[run.id]);
    return !((seen || 0) >= ended(run));
  }

  /**
   * The rollup by state: working, needs you, done to review, done reviewed, failed. A failed agent
   * counts as failed until its review is opened, then as reviewed; so "to review" and "failed" are
   * together the agents not looked at yet.
   */
  function counts(state, reviewed, now = Date.now()) {
    const c = { working: 0, needs: needsYou(state).length, unreviewed: 0, reviewed: 0, failed: 0 };
    for (const r of agents(state)) {
      if (waiting(r)) continue;
      if (WORKING.has(r.status)) c.working++;
      else if (unreviewed(r, reviewed, now)) { if (FAILED.has(r.status)) c.failed++; else c.unreviewed++; }
      else if (DONE.has(r.status) || FAILED.has(r.status)) c.reviewed++;
    }
    return c;
  }

  /** Per repository (AC-256): agents at work (waiting included), done to review, and failed not looked at. */
  function repos(state, reviewed, now = Date.now()) {
    const out = new Map();
    const tasks = new Map((state.tasks || []).map(t => [t.id, t]));
    for (const r of agents(state)) {
      const repo = (tasks.get(r.task_id) || {}).repo_root; if (!repo) continue;
      const x = out.get(repo) || { active: 0, unreviewed: 0, failed: 0 };
      if (WORKING.has(r.status) || waiting(r)) x.active++;
      else if (unreviewed(r, reviewed, now)) { if (FAILED.has(r.status)) x.failed++; else x.unreviewed++; }
      out.set(repo, x);
    }
    return out;
  }

  /** The rollup in words, only the states that have agents: "2 working · 1 needs you · 6 to review · 3 reviewed · 1 failed". */
  function parts(c) {
    return [['working', c.working, 'working'], ['needs', c.needs, 'needs you'], ['unreviewed', c.unreviewed, 'to review'], ['reviewed', c.reviewed, 'reviewed'], ['failed', c.failed, 'failed']]
      .filter(([, n]) => n > 0).map(([key, n, word]) => ({ key, n, text: `${n} ${word}` }));
  }
  const text = c => parts(c).map(p => p.text).join(' · ');
  /** What is left to look at, in the rollup's words: "6 to review · 1 failed". */
  const reviewText = c => parts({ unreviewed: c.unreviewed, failed: c.failed }).map(p => p.text).join(' · ');

  return { agents, needsYou, unreviewed, counts, repos, parts, text, reviewText, WEEK, DONE, FAILED };
});
