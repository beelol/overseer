import { createContext, useCallback, useContext, useSyncExternalStore } from 'react';

type Listener = () => void;

/**
 * What is open inside the rows (a thought, a long code block, a long token, the field of a
 * Deny), by name. It lives beside the list and not in the rows, so what was opened stays open
 * when its row leaves the screen and comes back, and opening one thing draws one row again.
 */
export interface OpenStore {
  isOpen(id: string): boolean;
  set(id: string, open: boolean): void;
  toggle(id: string): void;
  subscribe(id: string, listener: Listener): () => void;
}

export function createOpenStore(): OpenStore {
  const open = new Set<string>();
  const listeners = new Map<string, Set<Listener>>();
  const set = (id: string, to: boolean): void => {
    if (open.has(id) === to) return;
    if (to) open.add(id);
    else open.delete(id);
    for (const listener of [...(listeners.get(id) ?? [])]) listener();
  };
  return {
    isOpen: (id) => open.has(id),
    set,
    toggle: (id) => set(id, !open.has(id)),
    subscribe(id, listener) {
      const mine = listeners.get(id) ?? new Set<Listener>();
      mine.add(listener);
      listeners.set(id, mine);
      return () => {
        mine.delete(listener);
        if (mine.size === 0) listeners.delete(id);
      };
    },
  };
}

const OpenContext = createContext<OpenStore>(createOpenStore());

export const OpenProvider = OpenContext.Provider;

/** Whether the thing with this name is open, and a way to open or close it. */
export function useOpen(id: string): readonly [boolean, () => void, (open: boolean) => void] {
  const store = useContext(OpenContext);
  const subscribe = useCallback((listener: Listener) => store.subscribe(id, listener), [store, id]);
  const get = useCallback(() => store.isOpen(id), [store, id]);
  const open = useSyncExternalStore(subscribe, get, get);
  const toggle = useCallback(() => store.toggle(id), [store, id]);
  const set = useCallback((to: boolean) => store.set(id, to), [store, id]);
  return [open, toggle, set];
}
