import type { Capability } from '../capability';
import type { Live } from '../live';

export type ColorScheme = 'light' | 'dark';

/**
 * The phone's light or dark setting. It changes while the app is open, and the theme follows
 * at once.
 */
export type AppearanceApi = Live<ColorScheme>;

export type AppearanceCapability = Capability<'appearance', AppearanceApi>;
