import { describe, expect, it } from 'vitest';
import { append, create, setRun } from '../src/conversation.ts';
import type { DaemonEvent, Run } from '../src/types.ts';
import { describeConversation } from './helpers/describe.ts';
import { chat } from './helpers/vscode.ts';

const run = { id: 'r-mods', task_id: 't-mods', parent_run_id: null, harness: 'claude', status: 'running', title: 'Agent', created_ms: 1 } as unknown as Run;
const event = (seq: number, kind: string, payload: unknown): DaemonEvent => ({ seq, ts: seq, task_id: run.task_id, run_id: run.id, kind, source: 'daemon', confidence: 'exact', payload } as DaemonEvent);

function started() {
  const web = chat('/fixture'); web.setRun(run, []);
  const start = event(1, 'turn_started', { turn: { id: 'turn-mods', run_id: run.id, n: 1, prompt: 'Check the change.' } });
  web.add(start);
  const phone = append(setRun(create({ rootId: run.id, home: '/fixture' }), run, []).conversation, start).conversation;
  return { web, phone };
}

describe('Mods bookkeeping stays outside the conversation transcript', () => {
  for (const fingerprints of [[], ['clear-prose-pinned']]) {
    for (const outcome of ['prepared', 'transport_accepted', 'failed_before_effect', 'uncertain_after_effect']) {
      it(`keeps ${fingerprints.length ? 'enabled' : 'no-mod'} ${outcome} out of both transcripts`, () => {
        const { web, phone } = started();
        const beforeWeb = web.read(), beforePhone = describeConversation(phone);
        const delivered = event(2, 'mods_applied', { snapshot: { turn_id: 'turn-mods', run_id: run.id, planned_fingerprints: fingerprints, applied_fingerprints: outcome === 'transport_accepted' ? fingerprints : [], outcome, outcome_detail: 'fixture outcome' } });
        web.add(delivered);
        const next = append(phone, delivered).conversation;
        expect(web.read()).toEqual(beforeWeb);
        expect(describeConversation(next)).toEqual(beforePhone);
        // The view must not rewrite the observable event or invent successful delivery.
        expect((delivered.payload as { snapshot: { outcome: string } }).snapshot.outcome).toBe(outcome);
      });
    }
  }
  it('keeps library changes quiet while unknown events remain visible', () => {
    const { web, phone } = started();
    const beforeWeb = web.read(), beforePhone = describeConversation(phone);
    const changed = event(2, 'mods_changed', { revision: 2, operation: 'install' });
    web.add(changed);
    const next = append(phone, changed).conversation;
    expect(web.read()).toEqual(beforeWeb);
    expect(describeConversation(next)).toEqual(beforePhone);
    const unknown = event(3, 'fixture_unknown_notice', {});
    web.add(unknown);
    expect(web.root.textContent).toContain('fixture unknown notice');
    expect(describeConversation(append(next, unknown).conversation)).not.toEqual(beforePhone);
  });
});
