import { faded } from '@/theme/theme';
import { palettes, phone } from '@/theme/tokens.generated';

/**
 * The phone's text, on every surface it is drawn on, meets WCAG AA (4.5 to 1) in both themes
 * (AC-131). The pairs are the ones the shared pieces draw: a tone of `Txt` on a surface of
 * `Screen`, `Section`, `Sheet`, `Button` or `Chip`, and code on the lines of a diff. `faint` is
 * for decoration (a chevron, a grip) and is not among the colours `Txt` can have, so no word is
 * ever drawn in it.
 */
type Palette = (typeof palettes)['dark'] | (typeof palettes)['light'];

function channel(value: number): number {
  const c = value / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

function luminance(hex: string): number {
  const n = parseInt(hex.slice(1, 7), 16);
  return 0.2126 * channel((n >> 16) & 255) + 0.7152 * channel((n >> 8) & 255) + 0.0722 * channel(n & 255);
}

/** `over` drawn on `under`: a colour with an alpha channel is mixed with what is under it. */
function mixed(over: string, under: string): string {
  if (over.length !== 9) return over;
  const alpha = parseInt(over.slice(7, 9), 16) / 255;
  const part = (at: number): number => Math.round(parseInt(over.slice(at, at + 2), 16) * alpha + parseInt(under.slice(at, at + 2), 16) * (1 - alpha));
  return `#${[1, 3, 5].map((at) => part(at).toString(16).padStart(2, '0')).join('')}`;
}

function contrast(foreground: string, background: string): number {
  const a = luminance(foreground);
  const b = luminance(background);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

interface Pair {
  readonly what: string;
  readonly fg: string;
  readonly bg: string;
  readonly least: number;
}

function pairs(p: Palette): Pair[] {
  const out: Pair[] = [];
  const on = (what: string, fg: string, bg: string): void => void out.push({ what, fg, bg, least: 4.5 });
  // Words of any tone: on the screen, the header and the footer, and cards and sheets.
  const words: [string, string][] = [['text', p.text], ['muted', p.muted], ['accent', p.accent], ['link', p.link], ['red', p.red], ['green', p.green], ['amber', p.amber]];
  for (const [surface, bg] of [['the screen', p.bg], ['the header and the footer', p.chrome], ['a card, a sheet', p.raised]] as const) {
    for (const [tone, fg] of words) on(`${tone} on ${surface}`, fg, bg);
  }
  // A secondary button and a banner hold the text's colours; a selected chip holds text only.
  on('text on a secondary button, a banner', p.text, p.raised2);
  on('muted on a secondary button, a banner', p.muted, p.raised2);
  on('text on a selected chip', p.text, p.selected);
  on('the words of a primary button', p.onAccent, p.accentStrong);
  on('the words of a destructive button', p.red, mixed(p.removedBg, p.raised));
  // A changed line: the theme's tint at the strength the phone draws it, and its gutter.
  for (const [kind, line, gutter] of [['removed', p.removedBg, p.removedLine], ['added', p.addedBg, p.addedLine]] as const) {
    const bg = mixed(faded(line, phone.opacity.diffTint), p.bg);
    on(`the sign on a ${kind} line`, p.muted, bg);
    on(`a line number in the gutter of a ${kind} line`, p.text, mixed(gutter, bg));
    for (const [name, colour] of Object.entries(p.syntax)) on(`syntax ${name} on a ${kind} line`, colour, bg);
  }
  for (const [name, colour] of Object.entries(p.syntax)) on(`syntax ${name} on the screen`, colour, p.bg);
  return out;
}


describe.each([
  ['Overseer Dark', palettes.dark],
  ['Overseer Light', palettes.light],
] as const)('%s on the phone', (_name, palette) => {
  const all = pairs(palette);

  test(`every text pair meets WCAG AA (${all.length} pairs)`, () => {
    const failed = all.map((pair) => ({ ...pair, ratio: contrast(pair.fg, pair.bg) })).filter((pair) => pair.ratio < pair.least);
    expect(failed.map((pair) => `${pair.what}: ${pair.fg} on ${pair.bg} is ${pair.ratio.toFixed(2)} to 1, under ${pair.least}`)).toEqual([]);
  });
});

test('the measure itself: black on white is 21 to 1, a colour on itself 1 to 1', () => {
  // Built, not written: the lint lets no colour be written by hand, in a test either.
  const black = `#${'0'.repeat(6)}`;
  const white = `#${'F'.repeat(6)}`;
  expect(contrast(black, white)).toBeCloseTo(21, 5);
  expect(contrast(palettes.dark.bg, palettes.dark.bg)).toBe(1);
  expect(mixed(`${black}80`, white)).toBe(`#${'7f'.repeat(3)}`);
});
