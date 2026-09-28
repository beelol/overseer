import { createContext, useContext, useSyncExternalStore, type ReactNode } from 'react';

import type { Capabilities } from './capabilities';
import type { Live } from './live';

const CapabilitiesContext = createContext<Capabilities | null>(null);

interface PlatformProviderProps {
  /** The device's capabilities in the app, the fakes in tests. */
  readonly capabilities: Capabilities;
  readonly children: ReactNode;
}

/** Gives every screen below it the platform layer. */
export function PlatformProvider({ capabilities, children }: PlatformProviderProps) {
  return (
    <CapabilitiesContext.Provider value={capabilities}>{children}</CapabilitiesContext.Provider>
  );
}

/** The platform layer. Screens and shared code reach the platform through this and nothing else. */
export function useCapabilities(): Capabilities {
  const capabilities = useContext(CapabilitiesContext);
  if (capabilities === null) {
    throw new Error('useCapabilities needs a <PlatformProvider> above it');
  }
  return capabilities;
}

/** Follows a `Live` value: the component draws again at once when the value changes. */
export function useLive<T>(live: Live<T>): T {
  return useSyncExternalStore(live.subscribe, live.get, live.get);
}
