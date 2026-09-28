import type { Capability } from '../capability';
import type { Live } from '../live';

/**
 * `foreground`: the app is on screen, including the moments the system covers part of it
 * (a permission prompt, the notification shade). `background`: it is not on screen.
 */
export type AppPhase = 'foreground' | 'background';

/** Whether the app is on screen. The session reconnects when it returns to the foreground. */
export type AppStateApi = Live<AppPhase>;

export type AppStateCapability = Capability<'appState', AppStateApi>;
