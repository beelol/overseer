import { HunkLoader } from '@/screens/review/changes';
import { fromKey, keyOf, resolve } from '@/screens/review/comparison';
import { RunMarks } from '@/screens/review/marks';
import { comparisons, hunk, hunksOf, LATEST, TASK_START } from '@/screens/review/testing';

const tick = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));

describe('the reviewed marks of a run', () => {
  test('are asked for once, however many screens want them', async () => {
    const ask = jest.fn(async () => ['a']);
    const marks = new RunMarks(ask);
    await Promise.all([marks.ensure(), marks.ensure()]);
    await marks.ensure();
    expect(ask).toHaveBeenCalledTimes(1);
    expect(marks.getSnapshot()).toEqual({ keys: new Set(['a']), loaded: true });
  });

  test('a change shows at once, and what the Mac refuses is taken back', async () => {
    const marks = new RunMarks(async () => ['a']);
    await marks.ensure();
    const seen: string[][] = [];
    marks.subscribe(() => seen.push([...marks.getSnapshot().keys].sort()));

    let answer: () => void = () => undefined;
    const sent = marks.change('b', true, () => new Promise<void>((resolve) => (answer = resolve)));
    expect([...marks.getSnapshot().keys].sort()).toEqual(['a', 'b']);
    answer();
    await sent;
    expect([...marks.getSnapshot().keys].sort()).toEqual(['a', 'b']);

    await expect(marks.change('a', false, () => Promise.reject(new Error('conflict')))).rejects.toThrow('conflict');
    expect([...marks.getSnapshot().keys].sort()).toEqual(['a', 'b']);
    expect(seen).toEqual([['a', 'b'], ['b'], ['a', 'b']]);
  });

  test('Accept and then its undoing, before the Mac answered either, ends unmarked', async () => {
    const marks = new RunMarks(async () => []);
    await marks.ensure();
    const answers: (() => void)[] = [];
    const wait = (): Promise<void> => new Promise<void>((resolve) => answers.push(resolve));
    const first = marks.change('k', true, wait);
    const second = marks.change('k', false, wait);
    expect(marks.getSnapshot().keys.has('k')).toBe(false);
    answers[0]?.();
    await first;
    expect(marks.getSnapshot().keys.has('k')).toBe(false);
    answers[1]?.();
    await second;
    expect(marks.getSnapshot().keys.has('k')).toBe(false);
  });

  test('news while an answer is on its way asks once more after it', async () => {
    let keys = ['a'];
    const ask = jest.fn(async () => {
      await tick();
      return keys;
    });
    const marks = new RunMarks(ask);
    const first = marks.load();
    keys = ['a', 'b'];
    void marks.load();
    await first;
    expect(ask).toHaveBeenCalledTimes(2);
    expect([...marks.getSnapshot().keys].sort()).toEqual(['a', 'b']);
  });

  test('what the hunks say counts until the marks are loaded, and not after', async () => {
    const marks = new RunMarks(async () => ['x']);
    const reviewed = hunk('a.ts', 1, ['a'], ['b'], true);
    const open = hunk('a.ts', 9, ['c'], ['d'], false);
    marks.learn([reviewed, open]);
    expect([...marks.getSnapshot().keys]).toEqual([reviewed.key]);
    expect(marks.getSnapshot().loaded).toBe(false);

    await marks.ensure();
    expect([...marks.getSnapshot().keys]).toEqual(['x']);
    marks.learn([reviewed]);
    expect([...marks.getSnapshot().keys]).toEqual(['x']);
  });

  test('an unreachable Mac leaves what is known and is asked again later', async () => {
    let reachable = false;
    const marks = new RunMarks(async () => {
      if (!reachable) throw new Error('no answer');
      return ['a'];
    });
    await marks.ensure();
    expect(marks.getSnapshot().loaded).toBe(false);
    reachable = true;
    await marks.ensure();
    expect(marks.getSnapshot()).toEqual({ keys: new Set(['a']), loaded: true });
  });
});

describe('the hunks of the rows that are drawn', () => {
  test('are asked for a few at a time, each file once', async () => {
    let running = 0;
    let most = 0;
    const answered: string[] = [];
    const loader = new HunkLoader(
      async (path) => {
        running += 1;
        most = Math.max(most, running);
        await tick();
        running -= 1;
        return hunksOf(path, 'base', []);
      },
      (path) => answered.push(path),
      2,
    );
    for (const path of ['a', 'b', 'c', 'a', 'd', 'b']) loader.need(path);
    for (let i = 0; i < 10; i++) await tick();
    expect(answered.sort()).toEqual(['a', 'b', 'c', 'd']);
    expect(most).toBe(2);
  });

  test('stopped, it answers nothing more', async () => {
    const answered: string[] = [];
    const loader = new HunkLoader(async (path) => hunksOf(path, 'base', []), (path) => answered.push(path), 1);
    loader.need('a');
    loader.need('b');
    loader.stop();
    for (let i = 0; i < 5; i++) await tick();
    expect(answered).toEqual([]);
  });
});

describe('the comparison of a run', () => {
  test('a route names it by mode and branch', () => {
    expect(keyOf({ mode: 'latest_run', branch: null })).toBe('latest_run:');
    expect(keyOf({ mode: 'branch_tip', branch: 'release/1.2' })).toBe('branch_tip:release/1.2');
    expect(fromKey('latest_run:')).toEqual({ mode: 'latest_run', branch: null });
    expect(fromKey('turn:2:')).toEqual({ mode: 'turn:2', branch: null });
    expect(fromKey('branch_merge_base:main')).toEqual({ mode: 'branch_merge_base', branch: 'main' });
    expect(fromKey('task_start')).toEqual({ mode: 'task_start', branch: null });
    expect(fromKey(undefined)).toBeUndefined();
    expect(fromKey('')).toBeUndefined();
  });

  test('the one chosen is in use when it is available, else the one the Mac suggests', () => {
    expect(resolve(comparisons(), undefined).current?.base).toBe(LATEST);
    expect(resolve(comparisons(), { mode: 'task_start', branch: null }).current?.base).toBe(TASK_START);
    expect(resolve(comparisons(), { mode: 'fork', branch: null }).current?.base).toBe(LATEST);
  });

  test('none available says why', () => {
    const none = { ...comparisons(), options: [{ mode: 'latest_run', label: 'Latest run', available: false, default: true, detail: 'no run-start snapshot recorded' }] };
    const resolved = resolve(none, undefined);
    expect(resolved.current).toBeNull();
    expect(resolved.why).toBe('no run-start snapshot recorded');
  });
});
