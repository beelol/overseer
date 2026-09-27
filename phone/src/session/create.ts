import { PhoneClient, webSocketFactory, type KeyValueStore } from '@/core';
import { perf } from '@/perf';
import type { AsyncStore, Capabilities, SyncStore } from '@/platform';

import { Session, type SessionCache } from './session';
import type { Connection } from './types';

/** The keys `phone/core` keeps: it names them itself, so the stores take any name. */
type Anything = Record<string, string>;

/** `phone/core` stores strings under its own names; the platform's stores are typed views. */
function asKeyValue(store: SyncStore<Anything>): KeyValueStore {
  return {
    get: async (key) => store.get(clean(key)),
    set: async (key, value) => store.set(clean(key), value),
    delete: async (key) => store.delete(clean(key)),
  };
}

function asSecrets(store: AsyncStore<Anything>): KeyValueStore {
  return {
    get: (key) => store.get(clean(key)),
    set: (key, value) => store.set(clean(key), value),
    delete: (key) => store.delete(clean(key)),
  };
}

/** The library's names start with its namespace and a dot; the platform's scope is the namespace. */
const clean = (key: string): string => key.replace(/^overseer\./, '');

/**
 * Addresses beside the ones the pairing code and the network give: the platform's own (the
 * Mac's loopback as a simulator sees it), on the port the Mac was paired on, and the ones the
 * owner typed, exactly as typed.
 */
function extrasOf(capabilities: Capabilities): { host: string; port?: number }[] {
  const typed = capabilities.discovery.manual.get().map(({ host, port }) => ({ host, port }));
  const own = capabilities.launch.info().hostAddresses.map((host) => ({ host }));
  return [...typed, ...own];
}

export interface CreateSessionOptions {
  readonly capabilities: Capabilities;
  /** The app's version, sent in every handshake. */
  readonly app: string;
  readonly log?: (message: string) => void;
}

/** The app's session on a real device: the connection library wired to the platform layer. */
export function createSession({ capabilities, app, log }: CreateSessionOptions): Session {
  const client = new PhoneClient({
    socketFactory: webSocketFactory(WebSocket as never),
    store: asKeyValue(capabilities.keyValue.scope<Anything>('overseer')),
    secrets: asSecrets(capabilities.secretStore.scope<Anything>('overseer')),
    random: (n) => capabilities.random.bytes(n),
    now: () => Date.now(),
    app,
    extras: extrasOf(capabilities),
    ...(log ? { log } : {}),
  });
  // An address the owner typed is tried from the moment it is typed.
  capabilities.discovery.manual.subscribe(() => {
    client.setExtras(extrasOf(capabilities));
    client.wake();
  });
  return new Session({
    connection: client as unknown as Connection,
    cache: capabilities.keyValue.scope<SessionCache>('cache'),
    // Read by the scenario run, which compares it with the Mac's own log.
    onStream: (stream) => {
      try {
        const measured = capabilities.keyValue.scope<{ stream: string; delay: string }>('perf');
        measured.set('stream', JSON.stringify(stream));
        const delay = perf.report().summary['stream.delay'];
        if (delay) measured.set('delay', JSON.stringify(delay));
      } catch {
        // Measuring never breaks the app.
      }
    },
    onDelay: (ms) => perf.record('stream.delay', ms),
    now: () => Date.now(),
    nextFrame: (callback) => {
      const id = requestAnimationFrame(callback);
      return () => cancelAnimationFrame(id);
    },
    ...(log ? { log } : {}),
  });
}
