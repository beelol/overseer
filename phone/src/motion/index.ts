/**
 * The app's one motion system (AC-137). Every duration, distance, easing and spring is a token
 * (`theme.motion` from VS Code, `theme.phone.motion` for what only a phone has); nothing in a
 * screen writes one. With Reduce Motion on, movement becomes a fade everywhere.
 */
export { Arrive, Pulse, useMotion, type Motion } from './motion';
export { Tap, type TapProps } from './Tap';
