import { parseManualAddress, sameAddress } from './addresses';
import {
  CapabilityUnsupportedError,
  defineCapability,
  type Listener,
  type Support,
  type Unsubscribe,
} from './capability';
import type {
  DiscoveredGateway,
  DiscoveryCapability,
  DiscoveryConfig,
  GatewayAddress,
  GatewayCandidate,
} from './capabilities/discovery';
import type { KeyValueCapability } from './capabilities/keyValue';
import type { LaunchCapability } from './capabilities/launch';
import { createLive } from './live';

/** The part of discovery that differs per platform: browsing the local network. */
export interface DiscoveryBrowser {
  /** Known without waiting: it depends on the device and the build only. */
  support(): Support;
  /** Called only when `support()` reports supported. */
  browse(config: DiscoveryConfig, listener: Listener<readonly DiscoveredGateway[]>): Unsubscribe;
}

interface DiscoveryParts {
  readonly config: DiscoveryConfig;
  readonly launch: LaunchCapability;
  readonly keyValue: KeyValueCapability;
  readonly browser: DiscoveryBrowser;
}

type Stored = { manual: readonly GatewayAddress[] };

function isAddress(value: unknown): value is GatewayAddress {
  if (typeof value !== 'object' || value === null) return false;
  const { host, port } = value as { host?: unknown; port?: unknown };
  return typeof host === 'string' && typeof port === 'number';
}

/**
 * Discovery, the same on every platform except for the browser: manual addresses kept in the
 * key-value store, the platform's own addresses from `launch`, and browsing where it exists.
 */
export function createDiscovery(parts: DiscoveryParts): DiscoveryCapability {
  const { config, launch, keyValue, browser } = parts;
  const store = keyValue.scope<Stored>('discovery');
  const manual = createLive<readonly GatewayAddress[]>(Object.freeze(readStored()));

  /** What an earlier launch kept. Storage that cannot be read must never stop the app starting. */
  function readStored(): GatewayAddress[] {
    try {
      const stored: unknown = store.get('manual');
      return Array.isArray(stored) ? stored.filter(isAddress) : [];
    } catch {
      return [];
    }
  }

  function save(next: readonly GatewayAddress[]): void {
    store.set('manual', next);
    manual.set(Object.freeze([...next]));
  }

  return defineCapability<DiscoveryCapability>('discovery', async () => browser.support(), {
    config,
    candidates() {
      const found: GatewayCandidate[] = [];
      const add = (candidate: GatewayCandidate): void => {
        if (!found.some((other) => sameAddress(other, candidate))) found.push(candidate);
      };
      for (const host of launch.info().hostAddresses) {
        add({ host, port: config.defaultPort, source: 'platform' });
      }
      for (const address of manual.get()) add({ ...address, source: 'manual' });
      return found;
    },
    manual: { get: manual.get, subscribe: manual.subscribe },
    addManual(input) {
      const result = parseManualAddress(input, config.defaultPort);
      if (result.ok && !manual.get().some((other) => sameAddress(other, result.address))) {
        save([...manual.get(), result.address]);
      }
      return result;
    },
    removeManual(address) {
      const next = manual.get().filter((other) => !sameAddress(other, address));
      if (next.length !== manual.get().length) save(next);
    },
    browse(listener) {
      const support = browser.support();
      if (!support.supported) throw new CapabilityUnsupportedError('discovery', support.reason);
      return browser.browse(config, listener);
    },
  });
}
