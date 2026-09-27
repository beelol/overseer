/**
 * Overseer's look on the phone. Every colour, size, space, radius and duration comes from
 * `tokens.generated.ts`, which is generated from the VS Code themes' source. Nothing else in
 * `src` or `app` may write one by hand; the lint fails on it.
 */
export { lineHeight, themes, type Palette, type Theme, type ThemeName } from './theme';
export { useTheme } from './useTheme';
