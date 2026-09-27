import { useEffect, useMemo, useState } from 'react';

import { decodePairingCode, fingerprint, wipe } from '@/core';
import { useCapabilities, type DiscoveredGateway } from '@/platform';

import { useSupport } from '../settings/useSupport';

/** What a pairing code says about the Mac, without its secret: enough to find the Mac's name. */
export interface Target {
  readonly fingerprint: string;
  readonly addresses: readonly string[];
}

/**
 * Reads a code as scanned or typed and throws when it is not a pairing code. The secret in it
 * is erased at once: only the code itself is handed on, to pair.
 */
export function targetOf(code: string): Target {
  const decoded = decodePairingCode(code);
  try {
    return { fingerprint: fingerprint(decoded.gatewayPublicKey), addresses: decoded.addresses };
  } finally {
    wipe(decoded.secret);
  }
}

/** The Mac announces itself as "Overseer on <its name>". */
const ANNOUNCED = /^Overseer on /;

/**
 * The name of the Mac a code belongs to, when the network shows it: the Mac that announces the
 * key of the code, or one at an address of the code. `null` when it is not known.
 */
export function useMacName(target: Target | null): string | null {
  const { discovery } = useCapabilities();
  const support = useSupport(discovery);
  const [found, setFound] = useState<readonly DiscoveredGateway[]>([]);
  const browsing = target !== null && support?.supported === true;

  useEffect(() => {
    if (!browsing) return undefined;
    try {
      return discovery.browse(setFound);
    } catch {
      return undefined;
    }
  }, [browsing, discovery]);

  return useMemo(() => {
    if (!target) return null;
    const mac =
      found.find((gateway) => gateway.txt['fp'] === target.fingerprint) ??
      found.find((gateway) => target.addresses.includes(gateway.host));
    const name = mac?.serviceName.replace(ANNOUNCED, '').trim();
    return name ? name : null;
  }, [target, found]);
}
