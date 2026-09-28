import type { Capability } from '../capability';
import type { Live } from '../live';

/**
 * The system's Reduce Motion setting (Remove animations on Android). While it is true,
 * movement is replaced by fades everywhere.
 *
 * The system answers asynchronously, so `get()` is `false` until the first answer arrives;
 * `refresh()` resolves with the system's current answer.
 */
export interface ReduceMotionApi extends Live<boolean> {
  refresh(): Promise<boolean>;
}

export type ReduceMotionCapability = Capability<'reduceMotion', ReduceMotionApi>;
