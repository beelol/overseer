// Agents list parity: the rows of VS Code's side bar, from the extension's real views.js and
// rollup.js, against the phone's rows for the same state. Nine agents in two repositories, with a
// nested child and its child, an archived agent, two that need the owner, one that works, and
// finished ones to review (AC-254, AC-255, AC-256).
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';
import { agentRows, counts, emptyText, logoForHarness, needsYou, runHeader, searchLocally } from '../src/agents.ts';
import type { AgentRow, AgentsOptions } from '../src/agents.ts';
import { load } from '../src/store.ts';
import { TEXT } from '../src/text.ts';
import type { State } from '../src/types.ts';
import { differences, fixture, fixtures } from './helpers/fixtures.ts';
import { sideBar } from './helpers/sidebar.ts';
import { constant } from './helpers/source.ts';
import type { SideBarOptions, SideBarRow } from './helpers/sidebar.ts';

const nine = fixture('nine-agents');
const NOW = Math.max(...nine.final.runs.map(r => r.ended_ms ?? r.created_ms)) + 7 * 60_000 + 31_000;

beforeAll(() => { vi.useFakeTimers({ now: NOW, toFake: ['Date'] }); });
afterAll(() => { vi.useRealTimers(); });

/** A phone's row with the fields VS Code's row has. */
function comparable(row: AgentRow): SideBarRow {
  return {
    id: row.id, depth: row.depth, label: row.label, description: row.description, tooltip: row.tooltip, accessibilityLabel: row.accessibilityLabel, context: row.context, logo: row.logo, icon: row.icon,
    badge: row.badge, statusText: row.statusText, emphasized: row.emphasized, tone: row.kind === 'section' || row.emphasized ? row.badgeTone : null, expandable: row.expandable, expanded: row.expanded,
  };
}

async function compare(state: State, mine: Omit<AgentsOptions, 'now'>, theirs: SideBarOptions): Promise<{ rows: number; differences: string[]; listed: SideBarRow[] }> {
  const a = agentRows(load(state), { now: NOW, ...mine }).map(comparable);
  // VS Code's rollup row opens a filter picker when clicked, and its tooltip says so; the phone's
  // filters are the chips above the list, so its row says only the counts.
  const b = (await sideBar(state, theirs)).rows.map(row => (row.id === 'section:rollup' ? { ...row, tooltip: row.tooltip.replace(/\nClick to show only one of them\.$/, '') } : row));
  return { rows: b.length, differences: differences(a, b), listed: b };
}

describe('agents list parity with VS Code', () => {
  const state = nine.final;
  const summary: string[] = [];
  const say = (what: string, r: { rows: number; differences: string[] }): void => { summary.push(`${what.padEnd(44)} rows ${String(r.rows).padStart(2)}  differences ${r.differences.length}`); };

  it('has nine agents, a child with a child, and an archived one', () => {
    expect(state.tasks).toHaveLength(9);
    expect(state.runs.filter(r => r.parent_run_id)).toHaveLength(2);
    expect(state.tasks.filter(t => t.archived_ms)).toHaveLength(1);
  });

  it('lists the same rows in the same order: the rollup, Needs you, then the agents', async () => {
    const r = await compare(state, {}, {});
    say('the list as it opens', r);
    expect(r.differences).toEqual([]);
    expect(r.listed.slice(0, 2).map(x => x.id)).toEqual(['section:rollup', 'section:needs']);
    expect(r.listed[0]?.label).toBe('1 working · 1 needs you · 3 to review · 3 failed');
    // Done work not looked at since it ended carries its own mark, on its row and its repository.
    expect(r.listed.filter(x => x.badge === '✦').map(x => x.statusText)).toEqual(['Done, to review', 'Done, to review', 'Done, to review']);
    expect(r.listed.filter(x => x.id.startsWith('repo:')).map(x => x.description)).toEqual(['1 · 2 failed', '1 · 3 to review · 1 failed']);
    expect(r.listed.filter(x => x.id.startsWith('agent:'))).toHaveLength(8);
    expect(r.listed.filter(x => x.id.startsWith('run:')).map(x => x.depth)).toEqual([2, 3]);
    console.log(['The side bar, as VS Code lists it:', ...r.listed.map(x => `${'  '.repeat(x.depth)}${x.badge ?? ' '} ${x.label}${x.description ? `  (${x.description})` : ''}  [${x.logo ?? x.icon}]`)].join('\n  '));
  });

  it('lists the same rows for agents seen, pinned, and rows closed', async () => {
    const seen = Object.fromEntries(state.runs.filter(r => r.status === 'failed').slice(0, 2).map(r => [r.id, NOW]));
    const pinned = [String(nine.marks['running']), String(nine.marks['showcase'])];
    const collapsed = ['repo:/fixture/billing-service', `agent:${state.runs.find(r => r.id === nine.marks['nested'])?.task_id}`];
    const r = await compare(state, { seen, pinned, collapsed: new Set(collapsed) }, { seen, pinned, collapsed });
    say('some seen, two pinned, two rows closed', r);
    expect(r.differences).toEqual([]);
    const closed = await compare(state, { collapsed: new Set(['section:needs']) }, { collapsed: ['section:needs'] });
    say('Needs you closed', closed);
    expect(closed.differences).toEqual([]);
  });

  it('lists the same archived agents', async () => {
    const r = await compare(state, { showArchived: true }, { showArchived: true });
    say('archived agents', r);
    expect(r.differences).toEqual([]);
    expect(r.listed.filter(x => x.id.startsWith('agent:'))).toHaveLength(1);
  });

  it('lists the same rows for a search, with what the daemon found', async () => {
    for (const call of nine.calls.filter(c => c.method === 'search')) {
      const query = String((call.params as { query: string }).query);
      const found = (call.result as { task_ids: string[] }).task_ids;
      // VS Code shows the agents whose title holds the words, and the ones the daemon found.
      const titled = state.tasks.filter(t => t.title.toLowerCase().includes(query.toLowerCase())).map(t => t.id);
      for (const showArchived of [false, true]) {
        const r = await compare(state, { query, matches: found, showArchived }, { filter: { query, taskIds: [...titled, ...found] }, showArchived });
        say(`search “${query}”${showArchived ? ', archived' : ''}`, r);
        expect(r.differences, `search ${query}`).toEqual([]);
      }
      // What the phone finds by itself, the daemon finds too.
      const alone = searchLocally(load(state), query);
      expect(alone.filter(id => !found.includes(id)), `the phone alone finds more than the daemon for ${query}`).toEqual([]);
    }
  });

  it('finds by title, repository, harness, model, account and prompt without asking the daemon', () => {
    const s = load(state);
    const title = (ids: ReadonlyArray<string>): string[] => ids.map(id => state.tasks.find(t => t.id === id)?.title ?? id).sort();
    expect(title(searchLocally(s, 'slow MIGRATION'))).toEqual(['A slow migration']);
    expect(title(searchLocally(s, 'billing-service'))).toHaveLength(4);
    expect(title(searchLocally(s, 'generic'))).toEqual(['List the files']);
    expect(title(searchLocally(s, 'fixture-small'))).toEqual(['Echo the prompt']);
    expect(title(searchLocally(s, 'work account'))).toEqual(['Delegate to a sub-agent', 'Rename the tax helper']);
    expect(title(searchLocally(s, 'Summarize the invoices'))).toEqual(['Summarize invoices']);
    expect(searchLocally(s, '   ')).toEqual([]);
  });

  it('says the same when the daemon cannot be reached', async () => {
    const r = await compare(state, { error: 'daemon connection lost; reconnecting' }, { error: 'daemon connection lost; reconnecting' });
    say('daemon unavailable', { rows: r.rows, differences: r.differences.filter(d => !/\.id: |\.context: /.test(d)) });
    expect(r.listed[0]?.label).toBe('Daemon unavailable: daemon connection lost; reconnecting');
    expect(r.differences.filter(d => !/\.id: /.test(d))).toEqual([]);
  });

  it('names the same agents as needing the owner, in the same order', async () => {
    for (const options of [{}, { seen: { [String(nine.marks['showcase'])]: NOW } }, { seen: { [String(nine.marks['failed'])]: NOW } }]) {
      const mine = needsYou(load(state), { now: NOW, ...options });
      const theirs = (await sideBar(state, options)).needs;
      expect(differences(mine, theirs)).toEqual([]);
      expect(counts(load(state), { now: NOW, ...options }).needs).toBe(theirs.length);
    }
    expect(counts(load(state), { now: NOW }).active).toBe(state.runs.filter(r => ['queued', 'starting', 'running', 'waiting_for_user'].includes(r.status)).length);
  });

  it('lists the same rows at every moment of every recording', async () => {
    let compared = 0, rows = 0;
    const different: string[] = [];
    for (const f of fixtures) {
      for (const m of [...f.checkpoints, { cursor: f.final.cursor, state: f.final }]) {
        for (const showArchived of [false, true]) {
          const r = await compare(m.state, { showArchived }, { showArchived });
          compared++;
          rows += r.rows;
          different.push(...r.differences.map(d => `${f.scenario} after event ${m.cursor}: ${d}`));
        }
      }
    }
    summary.push(`${'every moment of every recording'.padEnd(44)} lists ${compared}  rows ${rows}  differences ${different.length}`);
    expect(different.slice(0, 10)).toEqual([]);
  });

  it('uses the side bar\'s marks, words and logos for every status, reviewed or not', async () => {
    const badge = constant('extension/src/views.js', 'STATUS_BADGE') as Record<string, [string, string]>;
    const continuity = constant('extension/media/continuity-text.js', 'STATES') as Record<string, unknown>;
    const logos = constant('extension/src/views.js', 'LOGO_FOR_HARNESS') as Record<string, string>;
    const first = state.runs.find(r => !r.parent_run_id) as State['runs'][number];
    const alone = (status: string, ended: number): State => {
      const run = { ...first, status: status as State['runs'][number]['status'], ended_ms: ended };
      return { ...state, runs: [run], tasks: state.tasks.filter(t => t.id === run.task_id).map(t => ({ ...t, archived_ms: null })) };
    };
    let compared = 0;
    const different: string[] = [];
    // Every status VS Code has a mark for, Continuity's, and one it has never heard of; each just
    // ended, ended and opened since, and ended more than a week ago.
    for (const status of [...Object.keys(badge), ...Object.keys(continuity), 'a_new_status']) {
      for (const [ended, seen] of [[NOW - 60_000, {}], [NOW - 60_000, { [first.id]: NOW }], [NOW - 8 * 86_400_000, {}]] as const) {
        const r = await compare(alone(status, ended), { seen }, { seen });
        compared++;
        different.push(...r.differences.map(d => `${status}, ${Object.keys(seen).length ? 'reviewed' : 'not reviewed'}, ended ${Math.round((NOW - ended) / 60_000)} min ago: ${d}`));
      }
    }
    say('every status, reviewed or not', { rows: compared, differences: different });
    expect(different).toEqual([]);
    expect(Object.keys(TEXT.badge).sort()).toEqual(Object.keys(badge).sort());
    for (const [harness, logo] of Object.entries(logos)) expect(logoForHarness(harness)).toBe(logo);
    expect(logoForHarness('generic')).toBeNull();
    const fresh = agentRows(load(alone('completed', NOW - 60_000)), { now: NOW }).find(r => r.kind === 'agent') as AgentRow;
    expect(fresh).toMatchObject({ badge: '✦', statusText: 'Done, to review', toReview: true, emphasized: true });
  });

  it('says what an empty list means', () => {
    const empty = load({ ...state, tasks: [], runs: [], turns: {} });
    expect(emptyText(empty, { now: NOW })).toBe('No agents yet.');
    expect(emptyText(empty, { now: NOW, filter: 'active' })).toBe('No agents are working.');
    expect(emptyText(empty, { now: NOW, filter: 'needs' })).toBe('Nothing needs you.');
    expect(emptyText(empty, { now: NOW, showArchived: true })).toBe('No archived agents.');
    expect(emptyText(load(state), { now: NOW, query: 'no-such-words-anywhere' })).toBe('No agents match.');
    expect(emptyText(load(state), { now: NOW })).toBeNull();
  });

  it('filters: All, Active, Needs you', () => {
    const s = load(state);
    const agents = (filter: AgentsOptions['filter']): string[] => agentRows(s, { now: NOW, filter }).filter(r => r.kind === 'agent').map(r => r.label);
    expect(agents('all')).toHaveLength(8);
    expect(agents('active').sort()).toEqual(['A slow migration', 'Write the permissions file']);
    expect(agents('needs')).toEqual([]);
    const needs = agentRows(s, { now: NOW, filter: 'needs' });
    expect(needs[0]?.id).toBe('section:needs');
    expect(needs.slice(1).every(r => r.kind === 'needs')).toBe(true);
    expect(needs.slice(1).map(r => r.label)).toContain('Write the permissions file');
    expect(agentRows(s, { now: NOW, filter: 'active' }).filter(r => r.kind === 'needs').map(r => r.label)).toEqual(['Write the permissions file']);
  });

  it('keeps the rows that did not change', () => {
    const s = load(state);
    const first = agentRows(s, { now: NOW });
    expect(agentRows(s, { now: NOW })).toBe(first);
    const later = agentRows(s, { now: NOW + 100, collapsed: new Set(['repo:/fixture/billing-service']) });
    expect(later).not.toBe(first);
    const kept = later.filter(r => first.includes(r));
    expect(kept.length).toBeGreaterThan(5);
  });

  it('gives the head of a conversation', () => {
    const s = load(state);
    const waiting = runHeader(s, String(nine.marks['waiting']));
    expect(waiting).toMatchObject({ title: 'Write the permissions file', statusText: 'Needs you', statusIcon: 'shield', logo: 'claudecode', account: "Mac's default login", accountShort: "Mac's default login", branch: 'write-the-permissions-file', branchIcon: 'git-branch', canSend: true, canStop: true, placeholder: 'Message for when it finishes', child: false, active: true });
    const done = runHeader(s, String(nine.marks['showcase']));
    expect(done).toMatchObject({ statusText: 'Done', statusIcon: 'check', model: 'fixture-large', canStop: false, placeholder: 'Reply…  (@ to mention a file)', exitReason: null });
    const failed = runHeader(s, String(nine.marks['auth']));
    expect(failed?.exitReason).toContain('turn reported failure');
    const child = runHeader(s, state.runs.find(r => r.parent_run_id)?.id ?? '');
    expect(child).toMatchObject({ child: true, canSend: false, canStop: false, placeholder: 'Sub-agents are steered through their parent' });
    const program = runHeader(s, String(nine.marks['generic']));
    expect(program).toMatchObject({ logo: null, icon: 'terminal', account: 'Program' });
    expect(runHeader(s, 'r-nobody')).toBeUndefined();
  });

  it('prints what was compared', () => {
    console.log(['Agents list parity (the phone\'s rows against the tree of the real views.js):', ...summary].join('\n  '));
  });
});
