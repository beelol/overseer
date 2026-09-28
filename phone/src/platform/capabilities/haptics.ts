import type { Capability } from '../capability';

/**
 * One set of moments for both platforms. Each platform plays a moment in its own style: the
 * system's feedback generators on iOS, the view haptics of the system on Android.
 */
export type HapticMoment =
  /** A selection moved: a row chosen, a switch flipped. */
  | 'selection'
  /** Something the owner did took effect: a message sent, a request allowed. */
  | 'confirm'
  /** Something was refused or failed. */
  | 'reject'
  /** Something needs the owner. */
  | 'warning'
  /** A thing landed or snapped into place: a sheet, the door. */
  | 'impact';

export interface HapticsApi {
  /**
   * Plays the moment. Returns at once and never throws; on a device without haptics (and on
   * simulators) it does nothing, because a missing buzz is not an error.
   */
  play(moment: HapticMoment): void;
}

export type HapticsCapability = Capability<'haptics', HapticsApi>;
