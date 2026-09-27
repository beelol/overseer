// Speed, measured: a conversation of 5,000 rows that keeps growing, and a store of 1,000 agents
// taking 1,000 events. The numbers are printed; the limits are the task's (1 ms for an event in a
// conversation, 16 ms for 1,000 events in the store).
import { performance } from 'node:perf_hooks';
import { describe, expect, it } from 'vitest';
import { append, appendAll, create, rowCount, rowsOf, setRun, visibleRows } from '../src/conversation.ts';
import type { Conversation } from '../src/conversation.ts';
import { agentRows } from '../src/agents.ts';
import { apply, load, rows, run as runOf } from '../src/store.ts';
import type { PhoneState } from '../src/store.ts';
import type { DaemonEvent, Run, State, Task, Workspace } from '../src/types.ts';
import { seeded } from './helpers/random.ts';

const ROOT = 'r-root';
const median = (values: number[]): number => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)] as number;

function runRecord(id: string, task: string, status: Run['status'], parent: string | null = null, created = 1): Run {
  return { id, task_id: task, parent_run_id: parent, harness: 'claude', harness_version: '2.1', profile_id: 'system-claude', model: 'fixture', workspace_id: 'w-' + task, native_id: null, status, exit_reason: null, created_ms: created, ended_ms: null, title: 'Agent ' + id, relation_source: null, relation_confidence: null, capabilities: { follow_up: 'supported' }, process_generation: 1, attention: null };
}

/** A stream as an agent at work makes it: turns of replies, tool calls with their results, edits, a request now and then. */
function work(count: number, from = 1, shape: 'turns' | 'one turn' = 'turns'): DaemonEvent[] {
  const rnd = seeded(from * 7919 + count);
  const out: DaemonEvent[] = [];
  let seq = from, ts = 1_790_000_000_000 + from * 50, turn = Math.floor(from / 60), tool = from;
  const push = (kind: string, payload: unknown, run = ROOT): void => { out.push({ seq: seq++, ts: (ts += 50), task_id: 't-1', run_id: run, kind, source: 'harness', confidence: 'exact', payload }); };
  if (shape === 'one turn') push('turn_started', { turn: { id: 'u-1', run_id: ROOT, n: 1, prompt: '', snapshot_id: null, started_ms: ts, ended_ms: null, status: 'running' } });
  while (out.length < count) {
    if (shape === 'one turn') { push('output', { role: 'stdout', text: `line ${seq} of the program's output` }); continue; }
    const roll = rnd();
    if (out.length % 60 === 0) {
      if (out.length) { push('usage', { usage: { input_tokens: 1200 + seq, output_tokens: 300 }, total_cost_usd: 0.01 }); push('turn_done', { ok: true, summary: 'done' }); }
      turn++;
      push('turn_started', { turn: { id: `u-${turn}`, run_id: ROOT, n: turn, prompt: `Do step ${turn}`, snapshot_id: null, started_ms: ts, ended_ms: null, status: 'running' } });
    } else if (roll < 0.3) push('output', { role: 'assistant', text: `Step ${seq}: I looked at \`src/file${seq}.ts\` and **changed** it.\n\n- one\n- two` });
    else if (roll < 0.75) {
      const id = `toolu_${tool++}`;
      const name = ['Read', 'Edit', 'Bash', 'Grep'][Math.floor(rnd() * 4)] as string;
      const input = name === 'Bash' ? { command: `npm test -- file${seq}` } : name === 'Grep' ? { pattern: `name${seq}` } : { file_path: `/repo/src/file${seq}.ts`, old_string: 'a', new_string: 'b\nc' };
      push('tool', { id, name, summary: JSON.stringify(input) });
      push('tool_result', { id, input, output: null, status: 'started', is_error: false });
      push('tool_result', { id, input: null, output: 'ok', status: 'completed', is_error: false });
    } else if (roll < 0.9) push('file_activity', { paths: [`src/file${seq}.ts`], kind: 'edit', attribution: 'reported by the agent harness' });
    else push('output', { role: 'reasoning', text: 'Thinking about the next step.' });
  }
  return out.slice(0, count);
}

function grown(shape: 'turns' | 'one turn', atLeast: number): { conversation: Conversation; next: number } {
  let c = setRun(create({ rootId: ROOT }), runRecord(ROOT, 't-1', 'running'), []).conversation;
  let from = 1;
  while (rowCount(c) < atLeast) {
    const events = work(500, from, shape);
    c = appendAll(c, events).conversation;
    from = (events.at(-1) as DaemonEvent).seq + 1;
  }
  return { conversation: c, next: from };
}

describe('speed of a conversation', () => {
  for (const shape of ['turns', 'one turn'] as const) {
    it(`5,000 rows in ${shape === 'turns' ? 'many turns' : 'one turn'}: an event takes well under 1 ms and touches a few rows`, () => {
      const { conversation, next } = grown(shape, 5000);
      const events = work(1000, next, shape === 'one turn' ? 'turns' : shape).filter(e => e.kind !== 'turn_started' || shape === 'turns');
      // Once without the clock, so what is measured is the code and not its first run.
      let warm = conversation;
      for (const event of events) warm = append(warm, event).conversation;
      const rounds: number[] = [];
      let most = 0, touched = 0, shifted = 0, c = conversation;
      for (let round = 0; round < 5; round++) {
        c = conversation;
        most = 0; touched = 0; shifted = 0;
        const t0 = performance.now();
        for (const event of events) {
          const change = append(c, event);
          c = change.conversation;
          touched += change.changed.length;
          if (change.changed.length > most) most = change.changed.length;
          if (change.movedFrom >= 0) shifted++;
        }
        rounds.push((performance.now() - t0) / events.length);
      }
      // What a list does once for every frame: the rows as an array, and the ones to show.
      const frames: number[] = [];
      c = conversation;
      for (let i = 0; i < events.length; i += 3) {
        c = appendAll(c, events.slice(i, i + 3)).conversation;
        const t0 = performance.now();
        rowsOf(c);
        visibleRows(c);
        frames.push(performance.now() - t0);
      }
      const perEvent = median(rounds);
      console.log(`Conversation of ${rowCount(conversation)} rows in ${shape === 'turns' ? 'many turns' : 'one turn'}, ${events.length} more events one at a time: ${(perEvent * 1000).toFixed(1)} µs for an event (median of 5 rounds; slowest round ${(Math.max(...rounds) * 1000).toFixed(1)} µs), `
        + `${(touched / events.length).toFixed(2)} rows touched by an event on average, at most ${most}, ${shifted} events moved other rows; the rows as an array and the visible ones, once a frame: ${(median(frames) * 1000).toFixed(0)} µs (median), ${(Math.max(...frames) * 1000).toFixed(0)} µs (slowest)`);
      expect(rowCount(conversation)).toBeGreaterThanOrEqual(5000);
      expect(rowCount(c)).toBeGreaterThan(rowCount(conversation));
      expect(perEvent).toBeLessThan(1);
      expect(most).toBeLessThanOrEqual(8);
    });
  }

  it('20 events a second for a minute on 5,000 rows: a twentieth of a second of work in all', () => {
    const { conversation, next } = grown('turns', 5000);
    const events = work(1200, next);
    let c = conversation;
    for (const event of events.slice(0, 300)) c = append(c, event).conversation;
    c = conversation;
    const t0 = performance.now();
    // A batch for every frame the events fall into: three events at 20 a second and 60 frames... one event in most frames.
    for (const event of events) {
      c = append(c, event).conversation;
      rowsOf(c);
    }
    const total = performance.now() - t0;
    console.log(`A minute of 20 events a second on ${rowCount(conversation)} rows, the rows read as an array after every event: ${total.toFixed(1)} ms of work in all, ${(total / events.length * 1000).toFixed(1)} µs for an event`);
    expect(total / events.length).toBeLessThan(1);
  });

  it('shares what it did not change', () => {
    const { conversation, next } = grown('turns', 5000);
    const [event] = work(1, next).map(e => ({ ...e, kind: 'output', payload: { role: 'assistant', text: 'one more' } }));
    const after = append(conversation, event as DaemonEvent).conversation;
    const shared = after.rows.chunks.filter((chunk, i) => chunk === conversation.rows.chunks[i]).length;
    console.log(`One event on ${rowCount(conversation)} rows: ${shared} of ${after.rows.chunks.length} pieces of the list are the ones the conversation had before`);
    expect(after.rows.chunks.length - shared).toBeLessThanOrEqual(1);
    expect(after.tools).toBe(conversation.tools);
    expect(after.groups).toBe(conversation.groups);
  });
});

describe('speed of the store', () => {
  const AGENTS = 1000;
  const state: State = (() => {
    const tasks: Task[] = [], runs: Run[] = [], workspaces: Workspace[] = [];
    const turns: State['turns'] = {};
    for (let i = 0; i < AGENTS; i++) {
      const t = `t-${i}`;
      tasks.push({ id: t, title: `Task ${i}`, prompt: `Do thing ${i}`, repo_root: `/repo/${i % 7}`, target_ref: null, workspace_id: 'w-' + t, start_snapshot: null, fork_commit: null, fork_provenance: null, created_ms: 1000 + i, archived_ms: null });
      workspaces.push({ id: 'w-' + t, path: `/w/${i}`, repo_root: `/repo/${i % 7}`, common_dir: `/repo/${i % 7}/.git`, kind: 'worktree', branch: `overseer/task-${i}`, owner_run_id: `r-${i}`, initial_dirty: { clean: true }, created_ms: 1000 + i, removed_ms: null });
      runs.push(runRecord(`r-${i}`, t, 'running', null, 1000 + i));
      turns[`r-${i}`] = [{ id: `u-${i}`, run_id: `r-${i}`, n: 1, prompt: `Do thing ${i}`, snapshot_id: null, started_ms: 1000 + i, ended_ms: null, status: 'running' }];
    }
    return { cursor: 10, tasks, runs, workspaces, profiles: [{ id: 'system-claude', name: 'claude (existing login)', harness: 'claude', home: null, is_system: true, created_ms: 1 }], turns, daemon: { pid: 1, started_ms: 1, version: '0.1.0', parser_version: 'x' } };
  })();

  /** 1,000 events as a busy hour has them: mostly output and tools, with statuses, turns, requests, children and new agents among them. */
  function events(): DaemonEvent[] {
    const rnd = seeded(42);
    const out: DaemonEvent[] = [];
    let seq = 11;
    const push = (kind: string, run: string | null, payload: unknown, task: string | null = null): void => { out.push({ seq: seq++, ts: 2_000_000 + seq, task_id: task, run_id: run, kind, source: 'daemon', confidence: 'exact', payload }); };
    let made = 0;
    while (out.length < 1000) {
      const i = Math.floor(rnd() * AGENTS), run = `r-${i}`, roll = rnd();
      if (roll < 0.35) push('output', run, { role: 'assistant', text: 'working' });
      else if (roll < 0.6) push('tool', run, { id: `toolu_${seq}`, name: 'Read', summary: '{}' });
      else if (roll < 0.7) push('status', run, { status: ['running', 'waiting_for_user', 'completed', 'failed'][Math.floor(rnd() * 4)], reason: 'exit 0' });
      else if (roll < 0.76) push('permission', run, { kind: 'permission', request_id: `req-${seq}`, tool: 'Write', input: { file_path: '/x' } });
      else if (roll < 0.82) push('permission_answered', run, { request_id: `req-${seq - 1}`, allow: true, by: 'the Mac' });
      else if (roll < 0.88) push('turn_done', run, { ok: true, summary: 'done' });
      else if (roll < 0.93) push('turn_started', run, { turn: { id: `u-new-${seq}`, run_id: run, n: 2 + (seq % 5), prompt: 'more', snapshot_id: null, started_ms: seq, ended_ms: null, status: 'running' } });
      else if (roll < 0.96) push('child', run, { child: runRecord(`r-child-${seq}`, `t-${i}`, 'running', run, 3_000_000 + seq), evidence: 'x', workspace: 'shared with parent' });
      else if (roll < 0.98) { const t = `t-new-${made++}`; push('task_created', `r-new-${made}`, { task: { ...(state.tasks[0] as Task), id: t, created_ms: 4_000_000 + seq, workspace_id: 'w-' + t }, workspace: { ...(state.workspaces[0] as Workspace), id: 'w-' + t, created_ms: 4_000_000 + seq }, run: runRecord(`r-new-${made}`, t, 'queued', null, 4_000_000 + seq) }, t); }
      else push('task_archived', null, { archived: true }, `t-${i}`);
    }
    return out;
  }

  it('1,000 events on 1,000 agents take well under 16 ms', () => {
    const list = events();
    const start = load(state);
    let warm = start;
    for (let round = 0; round < 3; round++) { warm = start; for (const event of list) warm = apply(warm, event); }
    const rounds: number[] = [];
    let end: PhoneState = start;
    for (let round = 0; round < 7; round++) {
      end = start;
      const t0 = performance.now();
      for (const event of list) end = apply(end, event);
      rounds.push(performance.now() - t0);
    }
    const kinds = new Map<string, number>();
    for (const e of list) kinds.set(e.kind, (kinds.get(e.kind) ?? 0) + 1);
    console.log(`Store of ${AGENTS} agents, ${list.length} events (${[...kinds].map(([k, n]) => `${n} ${k}`).join(', ')}): ${median(rounds).toFixed(2)} ms (median of 7 rounds; slowest ${Math.max(...rounds).toFixed(2)} ms), ${(median(rounds) / list.length * 1000).toFixed(2)} µs for an event`);
    expect(end.cursor).toBe((list.at(-1) as DaemonEvent).seq);
    expect(rows(end.runs).length).toBeGreaterThan(AGENTS);
    expect(median(rounds)).toBeLessThan(16);
  });

  it('an event copies one small piece of a table, not the table', () => {
    const start = load(state);
    const after = apply(start, { seq: 11, ts: 5, task_id: 't-500', run_id: 'r-500', kind: 'status', source: 'daemon', confidence: 'exact', payload: { status: 'waiting_for_user' } });
    const shared = after.runs.byId.buckets.filter((b, i) => b === start.runs.byId.buckets[i]).length;
    const inBucket = Object.keys(after.runs.byId.buckets.find((b, i) => b !== start.runs.byId.buckets[i]) ?? {}).length;
    console.log(`One status event on ${AGENTS} agents: ${shared} of ${after.runs.byId.buckets.length} buckets of the runs are shared, the one copied holds ${inBucket} runs; the order of the runs, the tasks, the workspaces and the turns are the ones from before`);
    expect(shared).toBe(after.runs.byId.buckets.length - 1);
    expect(inBucket).toBeLessThan(40);
    expect(after.runs.order).toBe(start.runs.order);
    expect(after.tasks).toBe(start.tasks);
    expect(after.workspaces).toBe(start.workspaces);
    expect(after.turns).toBe(start.turns);
    expect(runOf(after, 'r-500')?.status).toBe('waiting_for_user');
    expect(runOf(after, 'r-499')).toBe(runOf(start, 'r-499'));
    const quiet = apply(start, { seq: 11, ts: 5, task_id: 't-500', run_id: 'r-500', kind: 'output', source: 'harness', confidence: 'exact', payload: { role: 'assistant', text: 'x' } });
    expect(quiet.runs).toBe(start.runs);
    expect(rows(quiet.runs)).toBe(rows(start.runs));
  });

  it('the agents list of 1,000 agents', () => {
    const s = load(state);
    agentRows(s, { now: 10_000_000 });
    const rounds: number[] = [];
    for (let round = 0; round < 5; round++) {
      const changed = apply(s, { seq: 11 + round, ts: 5, task_id: 't-5', run_id: `r-${round}`, kind: 'status', source: 'daemon', confidence: 'exact', payload: { status: 'completed', reason: 'exit 0' } });
      const t0 = performance.now();
      const list = agentRows(changed, { now: 10_000_000 + round });
      rounds.push(performance.now() - t0);
      expect(list.length).toBeGreaterThan(AGENTS);
    }
    console.log(`Agents list of ${AGENTS} agents after a change: ${median(rounds).toFixed(2)} ms (median of 5; slowest ${Math.max(...rounds).toFixed(2)} ms)`);
    expect(median(rounds)).toBeLessThan(50);
  });
});
