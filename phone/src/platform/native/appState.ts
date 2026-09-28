import { AppState, type AppStateStatus } from 'react-native';

import { SUPPORTED, defineCapability } from '../capability';
import type { AppPhase, AppStateCapability } from '../capabilities/appState';
import { createLive } from '../live';

function toPhase(status: AppStateStatus): AppPhase {
  // `inactive` is the app still on screen with the system in front of part of it.
  return status === 'active' || status === 'inactive' ? 'foreground' : 'background';
}

/** Whether the app is on screen, from React Native's AppState. */
export function createAppState(): AppStateCapability {
  const phase = createLive<AppPhase>(toPhase(AppState.currentState));
  AppState.addEventListener('change', (status) => phase.set(toPhase(status)));
  return defineCapability<AppStateCapability>('appState', async () => SUPPORTED, {
    get: phase.get,
    subscribe: phase.subscribe,
  });
}
