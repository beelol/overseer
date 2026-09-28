// Parity where the recordings do not reach: streams made from a seed (test/helpers/random.ts), fed
// to VS Code's real conversation.js and to the phone's model, compared after every step.
import { describe, expect, it } from 'vitest';
import { append, create, setRun } from '../src/conversation.ts';
import { describeConversation } from './helpers/describe.ts';
import { differences } from './helpers/fixtures.ts';
import { ROOT, stream } from './helpers/random.ts';
import { chat } from './helpers/vscode.ts';

const STREAMS = 24;
const STEPS = 120;

describe('conversation parity with VS Code, on streams made from a seed', () => {
  it(`${STREAMS} streams of ${STEPS} steps: the same rows after every step`, () => {
    let compared = 0, rows = 0;
    const failures: string[] = [];
    // What the streams end with, to show they reach the cases that matter.
    const seen: Record<string, number> = {};
    const count = (what: string): void => { seen[what] = (seen[what] ?? 0) + 1; };
    for (let seed = 1; seed <= STREAMS && failures.length < 5; seed++) {
      const theirs = chat('/Users/fixture');
      let mine = create({ rootId: ROOT, home: '/Users/fixture' });
      // VS Code's feed passes on the events of the agent and of the children it has heard of.
      const known = new Set([ROOT]);
      let at = 0;
      for (const step of stream(seed, STEPS)) {
        at++;
        if (step.refresh) {
          for (const c of step.refresh.children) known.add(c.id);
          theirs.setRun(step.refresh.run, step.refresh.children);
          mine = setRun(mine, step.refresh.run, step.refresh.children).conversation;
        } else if (step.event) {
          const event = step.event;
          mine = append(mine, event).conversation;
          if (event.run_id && known.has(event.run_id)) {
            const child = (event.payload as { child?: { id?: string } } | null)?.child?.id;
            if (event.kind === 'child' && child) known.add(child);
            theirs.add(event);
          }
        }
        const a = describeConversation(mine), b = theirs.read();
        compared++;
        rows += b.lines.length;
        const d = differences(a, b);
        if (d.length) {
          failures.push(`seed ${seed}, step ${at} (${step.event ? `${step.event.kind} of ${step.event.run_id}, event ${step.event.seq}: ${JSON.stringify(step.event.payload).slice(0, 200)}` : 'the state read again'}):\n    ${d.slice(0, 6).join('\n    ')}`);
          break;
        }
      }
      const last = theirs.read().lines;
      last.forEach((l, i) => {
        count(l.kind);
        if (l.kind === 'child' && l.depth > 0) count(last[i - 1]?.kind === 'tool' || (last[i - 1]?.depth ?? 0) >= l.depth ? 'child under a tool call or a child' : 'child, deeper');
        if (l.kind === 'edit' && last.slice(0, i).reverse().find(x => x.depth === l.depth && x.kind !== 'edit')?.kind === 'steps') count('edit under a fold');
        if (l.kind === 'tool' && l.depth > 0) count('tool in a fold or a child');
        if (l.kind === 'permission') count(`permission ${String(l['state'])}`);
        if (l.kind === 'footer') count(`footer ${String(l['state'])}`);
      });
    }
    console.log(`What the made streams end with: ${Object.entries(seen).sort().map(([k, n]) => `${k} ${n}`).join(', ')}`);
    console.log(`Conversation parity on made streams: ${STREAMS} streams, ${compared} conversations compared, ${rows} rows compared, ${failures.length} streams with a difference`);
    expect(failures).toEqual([]);
  }, 300_000);
});
