import type { Capability } from '../capability';
import type { JsonSchema, SyncStore } from '../stores';

/**
 * Fast storage for things that are not secret: the cursor, cached state, settings, manual
 * addresses. Reads are synchronous, so the first screen can be drawn from the cache at once.
 */
export interface KeyValueApi {
  /**
   * A view of the storage under `namespace`, typed by what it holds.
   *
   * @example
   * type SessionState = { cursor: number };
   * const session = keyValue.scope<SessionState>('session');
   * session.set('cursor', 42);
   */
  scope<Schema extends JsonSchema<Schema>>(namespace: string): SyncStore<Schema>;
}

export type KeyValueCapability = Capability<'keyValue', KeyValueApi>;
