import type { ColorScheme } from '@/platform';

import { motion, palettes, scale } from './tokens.generated';

/** A palette with its colours as plain strings, so dark and light share one type. */
type Widen<T> = { readonly [K in keyof T]: T[K] extends string ? string : Widen<T[K]> };

export type Palette = Widen<(typeof palettes)[ColorScheme]> & { readonly type: ColorScheme };

export type ThemeName = 'Overseer Dark' | 'Overseer Light';

/** Everything a screen may use to draw itself. All of it comes from the generated tokens. */
export interface Theme {
  /** The name of the same theme in VS Code. */
  readonly name: ThemeName;
  readonly scheme: ColorScheme;
  readonly colors: Palette;
  readonly space: typeof scale.space;
  readonly radius: typeof scale.radius;
  readonly font: typeof scale.font;
  readonly weight: typeof scale.weight;
  readonly line: typeof scale.line;
  readonly chat: typeof scale.chat;
  readonly motion: typeof motion;
}

function build(name: ThemeName, scheme: ColorScheme): Theme {
  return Object.freeze({
    name,
    scheme,
    colors: palettes[scheme],
    space: scale.space,
    radius: scale.radius,
    font: scale.font,
    weight: scale.weight,
    line: scale.line,
    chat: scale.chat,
    motion,
  });
}

export const themes: Readonly<Record<ColorScheme, Theme>> = Object.freeze({
  dark: build('Overseer Dark', 'dark'),
  light: build('Overseer Light', 'light'),
});

/**
 * The height of a line of text: the type scale's size times one of the line tokens.
 * React Native wants the height in points where CSS takes the bare ratio.
 */
export function lineHeight(fontSize: number, ratio: number): number {
  return Math.round(fontSize * ratio);
}
