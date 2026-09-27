import type { SyncStore } from '@/platform';

/** What tests may set about the door. */
export type DoorSettings = {
  /** `off` starts the app with no door: for measuring that the door makes nothing slower. */
  door: 'on' | 'off';
};

let started = false;

/**
 * True the first time it is asked in a process, false ever after: the door belongs to a cold
 * start. Coming back from the background keeps the process, so it shows no door.
 */
export function coldStart(): boolean {
  if (started) return false;
  started = true;
  return true;
}

export function doorEnabled(settings: SyncStore<DoorSettings>): boolean {
  try {
    return settings.get('door') !== 'off';
  } catch {
    return true;
  }
}
