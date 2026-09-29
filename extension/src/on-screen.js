// Keys act only on what you can see (AC-242). A shortcut or a palette command that needs an agent
// uses the one whose chat (or review) is on screen; with none, or several, it asks which one first
// and shows the agent and what it asks, never the "first waiting" or an empty id.

/** Top-level run ids whose chat or review is visible in this window, in no particular order. */
function agentsOnScreen({ model, center, outputs, review }) {
  const root = id => { const run = id && model.run(id); return run ? (model.rootRun(run) || run).id : undefined; };
  const shown = new Set();
  // The Overseer view shows one agent's chat (not in its grid or its composer).
  if (center?.panel?.visible && center.mode === 'chat' && center.chatRun) shown.add(root(center.chatRun));
  for (const [id, entry] of outputs?.panels || []) if (entry?.panel?.visible) shown.add(root(id));
  for (const [id, panel] of review?.manager?.panels || []) if ((panel?.panel || panel)?.visible) shown.add(root(id));
  shown.delete(undefined);
  return [...shown];
}

/**
 * Which permission request ⌥⌘Y / ⌥⌘⌫ answer: `{ run }` when exactly one agent on screen waits on
 * one; `{ choose }` (the waiting requests, on-screen ones first) when none or several on screen
 * wait; `{ none: true }` when nothing waits anywhere.
 */
function permissionTarget(runs, onScreen, rootOf) {
  const waiting = (runs || []).filter(r => r.attention?.kind === 'permission' && r.attention.request_id !== undefined);
  if (!waiting.length) return { none: true };
  const seen = new Set(onScreen || []);
  const here = waiting.filter(r => seen.has(rootOf(r)));
  if (here.length === 1) return { run: here[0] };
  const pool = here.length ? here : waiting;
  return { choose: pool };
}

/**
 * The agent a palette command acts on: the one it was given, else the one on screen, else a choice.
 * `fits(run)` says whether the command applies to a run (for example Stop needs a running agent).
 * Returns `{ id }` or `{ choose: runs }` (possibly empty).
 */
function commandTarget(arg, { runs, onScreen, fits = () => true, selected }) {
  const given = typeof arg === 'string' ? arg : arg?.run?.id;
  if (given) return { id: given };
  const roots = (runs || []).filter(r => !r.parent_run_id);
  const byId = new Map(roots.map(r => [r.id, r]));
  const here = (onScreen || []).map(id => byId.get(id)).filter(r => r && fits(r));
  if (here.length === 1) return { id: here[0].id };
  const pool = (here.length ? here : roots.filter(fits))
    .sort((a, b) => (b.id === selected) - (a.id === selected) || (b.created_ms || 0) - (a.created_ms || 0));
  return { choose: pool };
}

module.exports = { agentsOnScreen, permissionTarget, commandTarget };
