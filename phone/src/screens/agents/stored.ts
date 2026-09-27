import { useCallback, useMemo, useRef, useState } from 'react';

import { useCapabilities, type JsonSchema, type KeyOf, type SyncStore } from '@/platform';

/** The phone's own storage under `namespace`. The same view for the life of the screen. */
export function useStore<Schema extends JsonSchema<Schema>>(namespace: string): SyncStore<Schema> {
  const { keyValue } = useCapabilities();
  return useMemo(() => keyValue.scope<Schema>(namespace), [keyValue, namespace]);
}

/** What is stored under `key`, or `fallback` when nothing readable is. Never throws. */
export function readStored<Schema, K extends KeyOf<Schema>>(store: SyncStore<Schema>, key: K, fallback: Schema[K]): Schema[K] {
  try {
    return store.get(key) ?? fallback;
  } catch {
    // Storage that cannot be read is storage with nothing in it: the screen still opens.
    return fallback;
  }
}

/** Stores `value` under `key`. What cannot be stored is kept for this launch only. */
export function writeStored<Schema, K extends KeyOf<Schema>>(store: SyncStore<Schema>, key: K, value: Schema[K]): void {
  try {
    store.set(key, value);
  } catch {
    // Nothing to do: the value holds until the app closes.
  }
}

/**
 * A value the phone keeps between launches: read once, synchronously, when the screen is first
 * drawn, and stored whenever it changes. `set` is the same function for the life of the screen.
 */
export function useStored<Schema, K extends KeyOf<Schema>>(store: SyncStore<Schema>, key: K, fallback: Schema[K]): readonly [Schema[K], (next: Schema[K] | ((was: Schema[K]) => Schema[K])) => void] {
  const [value, setValue] = useState<Schema[K]>(() => readStored(store, key, fallback));
  const latest = useRef(value);
  const set = useCallback(
    (next: Schema[K] | ((was: Schema[K]) => Schema[K])) => {
      const resolved = typeof next === 'function' ? (next as (was: Schema[K]) => Schema[K])(latest.current) : next;
      latest.current = resolved;
      writeStored(store, key, resolved);
      setValue(resolved);
    },
    [store, key],
  );
  return [value, set] as const;
}
