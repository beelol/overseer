import { createTestApp, makeEvent } from '@/testing';

// This signal carries no mod contents, credentials, snapshot or scope decisions.
describe('public Mods inspection signal', () => {
  test('global changes notify every subscriber with no arguments even when time holds still', async () => {
    const app = await createTestApp();
    const first = jest.fn();
    const second = jest.fn();
    const stop = app.session.subscribeMods('r1', first);
    app.session.subscribeMods('r2', second);
    await app.events(makeEvent('mods_changed', { revision: 1, private: 'never-forward-this' }));
    expect(first.mock.calls).toEqual([[]]);
    expect(second.mock.calls).toEqual([[]]);
    await app.events(makeEvent('mods_changed', { revision: 2 }));
    expect(first).toHaveBeenCalledTimes(2);
    stop();
    await app.events(makeEvent('mods_changed', { revision: 3 }));
    expect(first).toHaveBeenCalledTimes(2);
    expect(second).toHaveBeenCalledTimes(3);
  });

  test('turn outcomes notify only the actual run and coalesce one incoming batch', async () => {
    const app = await createTestApp();
    const mine = jest.fn();
    const other = jest.fn();
    app.session.subscribeMods('r1', mine);
    app.session.subscribeMods('r2', other);
    await app.events(
      makeEvent('mods_applied', { snapshot: { text: 'not public signal data' } }, { run_id: 'r1' }),
      makeEvent('mods_applied', { snapshot: {} }, { run_id: 'r1' }),
      makeEvent('output', { text: 'unrelated' }, { run_id: 'r2' }),
    );
    expect(mine.mock.calls).toEqual([[]]);
    expect(other).not.toHaveBeenCalled();
    await app.events(makeEvent('mods_applied', { snapshot: { run_id: 'r1' } }, { run_id: 'r2' }));
    expect(mine).toHaveBeenCalledTimes(1);
    expect(other.mock.calls).toEqual([[]]);
  });

  test('unsubscribing one listener preserves other listeners on the same run', async () => {
    const app = await createTestApp();
    const first = jest.fn();
    const second = jest.fn();
    const stop = app.session.subscribeMods('r1', first);
    const done = app.session.subscribeMods('r1', second);
    stop();
    await app.events(makeEvent('mods_applied', {}, { run_id: 'r1' }));
    expect(first).not.toHaveBeenCalled();
    expect(second.mock.calls).toEqual([[]]);
    done();
    await app.events(makeEvent('mods_changed', {}));
    expect(second).toHaveBeenCalledTimes(1);
  });
});
