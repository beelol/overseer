// Streams of events shaped like the daemon's, made from a seed: every kind the chat reads, in
// orders a recording of a fixture harness never has (a result before its call, a child before the
// call that started it, two children at once, a request asked twice). They are not recordings.
// They are there to compare the phone's model with VS Code's where the recordings do not reach.
import type { DaemonEvent, Run } from '../../src/types.ts';

export function seeded(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export const ROOT = 'r-root';
const CHILDREN = ['r-child-a', 'r-child-b', 'r-child-c', 'r-child-d'];
const STRANGER = 'r-someone-else';
const TOOLS = ['Read', 'Write', 'Edit', 'MultiEdit', 'Grep', 'Glob', 'LS', 'Bash', 'shell', 'apply_patch', 'WebFetch', 'WebSearch', 'TodoWrite', 'Agent', 'Task', 'collab:spawn_agent', 'collab:wait', 'mcp__linear__get_issue', 'Skill', ''];
const TEXTS = [
  'Done.', 'I will read the file first.', '## Plan\n\n1. Read\n2. Edit\n\n- [x] done\n- [ ] not yet', 'Use `npm test` to **check**; see [the docs](https://example.com/docs).',
  '| a | b |\n| --- | :-: |\n| 1 | 2 |', '```ts\nconst x = 1;\n```', 'A path: /Users/fixture/projects/overseer/extension/media/some/very/long/path/that/does/not/break.ts here', '', 'line one\nline two',
  '<script>alert(1)</script> after', '> quoted\n\ntext', 'a_b_c and *emphasis* and ~~gone~~',
];
// Continuity's waiting states (waiting_for_connection, waiting_for_memory) and its back_online
// event are left out: VS Code draws a card for them (Use a local model now, Retry now, Stay on the
// local model) from the daemon's Continuity status and actions, which the phone does not have yet.
const STATUSES = ['queued', 'starting', 'running', 'waiting_for_user', 'completed', 'failed', 'interrupted', 'disconnected', 'unknown', 'handed_off'];
/** What Continuity (Gate L) says in a chat: its own event kinds, and system lines marked as its own. */
const CONTINUITY = [
  { kind: 'handoff', payload: { predecessor: ROOT, successor: 'r-next', reason: 'offline' } }, { kind: 'handoff', payload: { predecessor: 'r-before', successor: ROOT, reason: 'back_online' } },
  { kind: 'stall', payload: {} }, { kind: 'memory_valve', payload: {} }, { kind: 'retry', payload: { sending: true } }, { kind: 'retry', payload: { attempt: 2, next_in_ms: 5000, reason: 'offline' } }, { kind: 'retry', payload: { retry_now: true } },
  { kind: 'local_model', payload: { model: 'ollama/qwen3-coder:30b', base: 'ollama/qwen3-coder:30b', context: 32768, bytes: 19_327_352_832 } }, { kind: 'local_model', payload: { model: 'ollama/qwen3-coder:30b', base: 'ollama/qwen3-coder:30b', context: 32768, bytes: 19_327_352_832 } },
  { kind: 'local_model', payload: { model: 'gemma3:4b', context: 8192, already_loaded: true } }, { kind: 'attention', payload: { kind: 'connection', reason: 'the connection was lost.' } }, { kind: 'attention', payload: { kind: 'memory' } },
  { kind: 'output', payload: { role: 'system', continuity: true, text: 'Back online. The agent **continues** here.' } }, { kind: 'output', payload: { role: 'system', continuity: true, text: 'Queued until a connection is back.' } },
  { kind: 'output', payload: { role: 'system', continuity: true, text: 'Memory is short: waiting.' } }, { kind: 'output', payload: { role: 'system', continuity: true, text: 'Handed off to a local model.' } },
  { kind: 'local_load', payload: {} }, { kind: 'connection', payload: { state: 'offline' } }, { kind: 'continuity_settings', payload: {} },
];

export interface Step {
  /** An event, or the state read again (what VS Code does after a change). */
  event?: DaemonEvent;
  refresh?: { run: Run; children: Run[] };
}

function runRecord(id: string, status: string, parent: string | null, title: string, attention: unknown, source: string | null): Run {
  return {
    id, task_id: 't-1', parent_run_id: parent, harness: 'claude', harness_version: null, profile_id: null, model: null, workspace_id: 'w-1', native_id: null, status: status as Run['status'], exit_reason: null,
    created_ms: 1, ended_ms: null, title, relation_source: source, relation_confidence: null, capabilities: {}, process_generation: 1, attention: attention as Run['attention'],
  };
}

/** `count` steps from a seed. */
export function stream(seed: number, count: number): Step[] {
  const rnd = seeded(seed);
  const pick = <T>(list: ReadonlyArray<T>): T => list[Math.floor(rnd() * list.length)] as T;
  const chance = (p: number): boolean => rnd() < p;
  const steps: Step[] = [];
  let seq = 100 + Math.floor(rnd() * 5);
  let ts = 1_700_000_000_000;
  let turns = 0;
  const born: string[] = [];
  const parents = new Map<string, string>();
  const statuses = new Map<string, string>([[ROOT, 'running']]);
  const toolIds: string[] = [];
  const requests: string[] = [];
  let attention: unknown = null;
  const anyRun = (): string => (born.length && chance(0.35) ? pick(born) : ROOT);
  const push = (kind: string, run: string | null, payload: unknown, confidence = 'exact', source = 'harness'): void => {
    seq += chance(0.1) ? 2 : 1;
    ts += Math.floor(rnd() * 4000);
    steps.push({ event: { seq, ts, task_id: 't-1', run_id: run, kind, source, confidence, payload } });
  };
  const refresh = (): void => {
    const children = born.map(id => runRecord(id, statuses.get(id) ?? 'running', parents.get(id) ?? ROOT, `child ${id.slice(-1)}`, null, chance(0.5) ? `claude tool_use Agent ${pick(toolIds.length ? toolIds : ['toolu_x'])}` : null));
    steps.push({ refresh: { run: runRecord(ROOT, statuses.get(ROOT) ?? 'running', null, 'root', attention, null), children } });
  };
  refresh();
  while (steps.length < count) {
    const roll = rnd();
    if (roll < 0.06) {
      turns++;
      push('turn_started', ROOT, chance(0.9) ? { turn: { id: `u-${turns}`, run_id: ROOT, n: turns, prompt: chance(0.8) ? pick(TEXTS) : '', snapshot_id: null, started_ms: ts, ended_ms: null, status: 'running' } } : {}, 'exact', 'daemon');
      statuses.set(ROOT, 'running');
    } else if (roll < 0.22) {
      push('output', anyRun(), { role: pick(['assistant', 'assistant', 'assistant', 'reasoning', 'plan', 'system', 'stdout', 'stderr', 'user', 'notice']), text: pick(TEXTS) });
    } else if (roll < 0.42) {
      const name = pick(TOOLS);
      const id = chance(0.2) && toolIds.length ? pick(toolIds) : chance(0.9) ? `toolu_${seq}` : null;
      if (id && !toolIds.includes(id)) toolIds.push(id);
      const input = pick<unknown>([
        { file_path: '/repo/src/a.ts' }, { file_path: '/repo/b/c.md', content: 'one\ntwo\n' }, { file_path: '/repo/x.ts', old_string: 'a\nb', new_string: 'c' }, { edits: [{ old_string: 'a', new_string: 'b\nc' }], file_path: '/r/m.ts' },
        { command: 'npm test\n--watch', description: 'Run the tests' }, { pattern: 'TODO' }, { url: 'https://Example.com:443/a?b#c' }, { query: 'rust sqlite' }, { todos: [1, 2, 3] }, { description: 'a child', prompt: 'do it\nnow' }, { path: '/repo/dir/' }, {},
      ]);
      const summary = pick([JSON.stringify(input), JSON.stringify(input).slice(0, 25), 'touch a.txt [inProgress]', 'make build [completed, exit 0]', 'make test [failed, exit 2]', '/repo/a.ts, /repo/b.ts', '', 'plain words']);
      push('tool', anyRun(), { name, id, summary });
      if (chance(0.5) && id) push('tool_result', steps[steps.length - 1]?.event?.run_id ?? ROOT, { id, input, output: null, status: 'started', is_error: false });
    } else if (roll < 0.55) {
      const id = chance(0.85) && toolIds.length ? pick(toolIds) : chance(0.5) ? `toolu_late_${seq}` : null;
      if (id && !toolIds.includes(id)) toolIds.push(id);
      push('tool_result', anyRun(), { id, input: chance(0.3) ? { command: 'ls -la' } : null, output: pick([null, 'ok', 'a\nb\nc', '']), status: pick(['completed', 'failed', 'started', 'declined', null]), is_error: pick([false, false, true, null]) });
    } else if (roll < 0.61) {
      push('file_activity', anyRun(), { paths: pick([['src/a.ts'], ['a/b/c.md', 'd.txt'], []]), kind: 'edit', attribution: 'reported by the agent harness' }, pick(['reported', 'tool-input', 'inferred']));
    } else if (roll < 0.66) {
      const id = chance(0.2) && requests.length ? pick(requests) : `req-${seq}`;
      if (!requests.includes(id)) requests.push(id);
      const asked = { kind: 'permission', request_id: id, tool: pick(['Write', 'Bash', 'command: rm -rf build', 'Edit']), input: pick<unknown>([{ file_path: '/repo/p.txt', content: '1\n2\n3\n4\n5\n6\n7\n8\n9\n10' }, { command: 'rm -rf build' }, { new_string: 'x' }, {}, null]) };
      const run = anyRun();
      push('permission', run, asked);
      if (run === ROOT) { attention = asked; statuses.set(ROOT, 'waiting_for_user'); push('status', ROOT, { status: 'waiting_for_user' }); }
    } else if (roll < 0.70) {
      const id = chance(0.9) && requests.length ? pick(requests) : 'req-nobody';
      push('permission_answered', ROOT, { request_id: id, allow: chance(0.5), by: pick(['the Mac', "phone:Bilal's iPhone"]) }, 'exact', 'user');
      if (attention && (attention as { request_id: string }).request_id === id) { attention = null; statuses.set(ROOT, 'running'); push('status', ROOT, { status: 'running' }, 'exact', 'daemon'); }
    } else if (roll < 0.74) {
      push('error', anyRun(), { class: pick(['auth', 'rate_limit', 'quota', 'network', 'network', 'other', null]), message: pick(['Failed to authenticate', 'API Error: 429', '', '   ', 'boom\nsecond line', 'error sending request for url (https://api.openai.com/v1/responses)', 'Connection refused (os error 61)']) });
    } else if (roll < 0.80) {
      const unborn = CHILDREN.filter(c => !born.includes(c));
      if (unborn.length) {
        const id = unborn[0] as string;
        const parent = born.length && chance(0.4) ? pick(born) : ROOT;
        const native = chance(0.5) && toolIds.length ? pick(toolIds) : `toolu_native_${seq}`;
        born.push(id);
        parents.set(id, parent);
        statuses.set(id, 'running');
        const evidence = pick([`claude tool_use Agent ${native}`, `claude tool_use Agent ${native} inside ${pick(toolIds.length ? toolIds : ['toolu_none'])}`, 'codex collab_tool_call spawn_agent', null]);
        const child = runRecord(id, 'running', parent, `child ${id.slice(-1)}`, null, evidence);
        push('child', parent, { child: { ...child, native_id: native }, evidence, workspace: 'shared with parent' }, pick(['exact', 'inferred']));
      } else if (born.length > 1 && chance(0.5)) {
        const moved = pick(born), to = pick([ROOT, ...born.filter(b => b !== moved)]);
        parents.set(moved, to);
        push('child_reparented', moved, { child_run_id: moved, parent_run_id: to });
      }
    } else if (roll < 0.85) {
      const run = anyRun();
      const status = run === ROOT ? pick(STATUSES) : pick(['running', 'completed', 'failed', 'unknown']);
      statuses.set(run, status);
      if (run === ROOT && status !== 'waiting_for_user') attention = null;
      push('status', run, { status, ...(chance(0.5) ? { reason: 'exit 1: something happened' } : {}) }, 'exact', 'daemon');
    } else if (roll < 0.89) {
      push('usage', anyRun(), pick<unknown>([
        { usage: { input_tokens: 18423, output_tokens: 1204, cache_read_input_tokens: 9321 }, total_cost_usd: 0.0412 }, { total: { inputTokens: 1, outputTokens: 1 } }, { input_tokens: 121101, output_tokens: 325, cached_input_tokens: 89856 },
        { rate_limits: { primary: 0.5 } }, { usage: { input_tokens: 2500000, output_tokens: 0 }, total_cost_usd: 0.004 }, { cost: 12.5 }, { usage: { output_tokens: 999 }, total_cost_usd: null }, {},
      ]));
    } else if (roll < 0.93) {
      push('turn_done', anyRun(), { ok: chance(0.6), summary: pick([null, 'all done', 'Migration failed: relation users_v2 does not exist\nmore', '']) });
      statuses.set(ROOT, statuses.get(ROOT) === 'waiting_for_user' ? 'waiting_for_user' : statuses.get(ROOT) ?? 'running');
    } else if (roll < 0.95) {
      push('interrupt_requested', ROOT, {}, 'exact', 'user');
    } else if (roll < 0.965) {
      const said = pick(CONTINUITY);
      push(said.kind, anyRun(), said.payload, 'exact', 'daemon');
    } else if (roll < 0.985) {
      push(pick(['retention', 'raw_unparsed', 'session', 'remote_command', 'review_mark', 'push', 'reattached', 'merge_back', 'pull_request', 'daemon_error', 'task_created', 'a_new_kind']), anyRun(), pick<unknown>([{}, { text: 'x' }, { method: 'run.follow_up', device: 'd-1', request_id: `request-${seq}` }, null]));
    } else {
      // Another agent's event, and one of no agent: the chat shows neither.
      push(pick(['output', 'tool', 'status', 'error']), chance(0.5) ? STRANGER : null, { role: 'assistant', text: 'not for this chat', name: 'Read', id: 'toolu_other', status: 'failed', message: 'x', class: 'other' });
    }
    if (chance(0.12)) refresh();
    // A replay that overlaps: the same event again.
    if (chance(0.03) && steps.length) { const again = [...steps].reverse().find(s => s.event)?.event; if (again) steps.push({ event: again }); }
  }
  refresh();
  return steps;
}
