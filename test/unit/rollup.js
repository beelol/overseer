// AC-246, AC-254, AC-255, AC-256: the counts the side bar, the grid, the badge and ⌥⌘J share
// (extension/media/rollup.js). Run: node test/unit/rollup.js
const R = require('../../extension/media/rollup.js');
let failures = 0;
const check = (name, ok, detail) => { if (!ok) failures++; console.log(ok ? 'ok  ' : 'FAIL', name, ok ? '' : JSON.stringify(detail)); };

const now = 1_000_000_000_000;
const task = (id, repo, extra = {}) => ({ id, repo_root: repo, title: id, ...extra });
const run = (id, taskId, status, extra = {}) => ({ id, task_id: taskId, status, created_ms: now - 60_000, ended_ms: ['completed', 'failed', 'interrupted', 'disconnected'].includes(status) ? now - 30_000 : null, ...extra });
const state = {
  tasks: [task('t1', '/site'), task('t2', '/site'), task('t3', '/site'), task('t4', '/notes'), task('t5', '/notes'), task('t6', '/notes', { archived_ms: now }), task('t7', '/notes'), task('t8', '/site')],
  runs: [
    run('r1', 't1', 'running'),
    run('r2', 't2', 'waiting_for_user', { attention: { kind: 'permission', tool: 'Write' } }),
    run('r3', 't3', 'completed'),
    run('r4', 't4', 'completed'),
    run('r5', 't5', 'failed'),
    run('r6', 't6', 'waiting_for_user'), // archived: not counted
    run('r7', 't7', 'completed', { ended_ms: now - 8 * 86400000, created_ms: now - 9 * 86400000 }), // older than a week: not to review
    run('r8-old', 't8', 'completed', { created_ms: now - 120_000 }), run('r8', 't8', 'completed'), // the newest run of a task stands for it
    run('c1', 't3', 'running', { parent_run_id: 'r3' }), // a native child: not an agent of its own
  ],
  overseer: { run_id: 'ov', open_proposals: 1 },
  oversight: { ov: { role: 'overseer' } },
};
state.runs.push(run('ov', 't-ov', 'running'));

const needs = R.needsYou(state);
check('Needs you: the waiting agent and Overseer\'s proposal, nothing archived, nothing finished or failed', needs.length === 2 && needs.some(n => n.overseer) && needs.some(n => n.run_id === 'r2' && n.label === 'Approve'), needs);
const reviewed = { r4: now - 10_000 };
check('an agent at its end is to review until its review is opened after it ended', R.unreviewed(state.runs[2], reviewed, now) && !R.unreviewed(state.runs[3], reviewed, now), null);
check('a review opened before the agent ended again does not count', !R.unreviewed(state.runs[3], { r4: now - 40_000 }, now) === false, null);
check('a working or waiting agent is never to review; nor one that ended over a week ago', !R.unreviewed(state.runs[0], {}, now) && !R.unreviewed(state.runs[1], {}, now) && !R.unreviewed(state.runs[6], {}, now), null);
const c = R.counts(state, reviewed, now);
check('the rollup by state: 1 working, 2 need you, 2 to review, 2 reviewed (one reviewed, one older than a week), 1 failed', c.working === 1 && c.needs === 2 && c.unreviewed === 2 && c.reviewed === 2 && c.failed === 1, c);
check('the rollup in words, only the states that have agents', R.text(c) === '1 working · 2 needs you · 2 to review · 2 reviewed · 1 failed' && R.text({ working: 0, needs: 0, unreviewed: 0, reviewed: 0, failed: 0 }) === '', R.text(c));
const repos = R.repos(state, reviewed, now);
check('per repository (AC-256): site has 2 at work and 2 to review; notes has 1 failed to look at and nothing at work', JSON.stringify(repos.get('/site')) === JSON.stringify({ active: 2, unreviewed: 2, failed: 0 }) && JSON.stringify(repos.get('/notes')) === JSON.stringify({ active: 0, unreviewed: 0, failed: 1 }), [...repos]);
const all = R.counts(state, { r3: now, r4: now, r5: now, r8: now }, now);
check('once every agent at its end is reviewed, nothing is to review or failed-unseen', all.unreviewed === 0 && all.failed === 0 && all.reviewed === 5, all);
// AC-246: the same recorded state gives the same Needs-you count here (the badge), in the TUI's
// header (tui/src/model.rs `needs_you_count`) and on the phone (phone/model `counts`): the daemon's
// recording of nine agents, one of them waiting on a permission.
const nine = require('../../phone/model/test/fixtures/nine-agents.json').final;
check('the nine-agents recording: 1 needs you (the same number the TUI and the phone count)', R.needsYou(nine).length === 1, R.needsYou(nine));
const withProposal = { ...nine, overseer: { run_id: 'ov', open_proposals: 2 } };
check('with Overseer\'s proposals waiting it is 2 (the TUI counts the same; the phone does not receive proposals yet)', R.needsYou(withProposal).length === 2, R.needsYou(withProposal));
console.log(failures ? `${failures} failed` : 'the counts agree');
process.exit(failures ? 1 : 0);
