import type { ConnectionState, GatewayInfo, OutboxEntry } from '@/core';
import type { Connection } from '@/session';

type Handler = (...args: never[]) => void;

export const FAKE_GATEWAY: GatewayInfo = { deviceId: 'd1', deviceName: 'Phone', platform: 'ios', gatewayName: 'Mac', gatewayFingerprint: '0123456789abcdef', scope: 'full', pairedAt: 1 };

/**
 * A connection a test drives by hand: it answers requests from `answers`, records what was
 * asked, and says what the test tells it to (`go`, `emit`).
 */
export class FakeConnection implements Connection {
  state: ConnectionState = 'unpaired';
  lastContact: number | null = null;
  gateway: GatewayInfo | null = null;
  hello: Record<string, unknown> | null = null;
  cursor = 0;
  entries: OutboxEntry[] = [];
  /** Every request made, in order. */
  readonly asked: { method: string; params: unknown; options?: unknown }[] = [];
  /** What each method answers. A method with no answer fails like an unreachable Mac. */
  answers: Record<string, (params: never) => unknown> = {};
  /** What `pair` does. The default pairs with `FAKE_GATEWAY`. */
  pairing: (code: string, name: string, platform: string) => Promise<GatewayInfo> = async () => FAKE_GATEWAY;
  woken = 0;
  private readonly handlers = new Map<string, Set<Handler>>();

  outbox = (): readonly OutboxEntry[] => this.entries;
  dismiss = (id: string): void => {
    this.entries = this.entries.filter((e) => e.requestId !== id);
  };
  on(event: string, listener: Handler): () => void {
    const set = this.handlers.get(event) ?? new Set();
    set.add(listener);
    this.handlers.set(event, set);
    return () => void set.delete(listener);
  }
  emit(event: string, ...args: unknown[]): void {
    for (const h of this.handlers.get(event) ?? []) (h as (...a: unknown[]) => void)(...args);
  }
  start = async (): Promise<void> => undefined;
  stop = async (): Promise<void> => undefined;
  wake = (): void => {
    this.woken++;
  };
  pair = async (code: string, name: string, platform: string): Promise<GatewayInfo> => {
    const gateway = await this.pairing(code, name, platform);
    this.gateway = gateway;
    this.emit('paired', gateway);
    return gateway;
  };
  forget = async (): Promise<void> => {
    this.gateway = null;
    this.go('unpaired');
    this.emit('forgotten', 'forgotten');
  };
  setDiscovered = (): void => undefined;
  request = (async (method: string, params: unknown, options?: unknown) => {
    this.asked.push({ method, params, ...(options === undefined ? {} : { options }) });
    const answer = this.answers[method];
    if (!answer) throw new Error(`no answer for ${method}`);
    return answer(params as never);
  }) as Connection['request'];

  /** The connection changes state. */
  go(state: ConnectionState): void {
    const before = this.state;
    this.state = state;
    this.emit('state', state, before);
  }

  /** The outbox changed: `entries` is what it holds now. */
  setOutbox(entries: OutboxEntry[]): void {
    this.entries = entries;
    const last = entries[entries.length - 1];
    if (last) this.emit('outbox', last);
  }

  /** What was asked of `method`, in order. */
  calls(method: string): unknown[] {
    return this.asked.filter((a) => a.method === method).map((a) => a.params);
  }
}
