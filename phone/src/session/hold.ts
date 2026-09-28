import type { Session } from './session';
import type { SessionSnapshot } from './types';

export interface Holdable {
  /** The screens keep what they show now: nothing new is drawn until `release`. */
  hold(): void;
  /** What arrived meanwhile is drawn now, at once. */
  release(): void;
}

/**
 * The session as the screens read it, which can hold still for a moment: while the door opens,
 * what the Mac sends is taken in as always but not drawn, so drawing it takes no frame from
 * the opening. Requests, pairing and everything else go straight to the session.
 */
export function holdable(session: Session): Session & Holdable {
  let frozen: SessionSnapshot | null = null;
  const waiting = new Set<() => void>();
  const getSnapshot = (): SessionSnapshot => frozen ?? session.getSnapshot();
  const subscribe = (listener: () => void): (() => void) => {
    const off = session.subscribe(() => {
      if (frozen) waiting.add(listener);
      else listener();
    });
    return () => {
      waiting.delete(listener);
      off();
    };
  };
  const hold = (): void => {
    frozen ??= session.getSnapshot();
  };
  const release = (): void => {
    if (frozen === null) return;
    frozen = null;
    const all = [...waiting];
    waiting.clear();
    for (const listener of all) listener();
  };
  const own: Record<PropertyKey, unknown> = { getSnapshot, subscribe, hold, release };
  return new Proxy(session, {
    get(target, key) {
      if (key in own) return own[key];
      const value: unknown = Reflect.get(target, key, target);
      return typeof value === 'function'
        ? (value as (...args: unknown[]) => unknown).bind(target)
        : value;
    },
  }) as Session & Holdable;
}
