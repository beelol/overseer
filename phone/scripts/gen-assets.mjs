#!/usr/bin/env node
// Draws the app icon and the launch image from Overseer's mark and the design tokens.
//
//   node scripts/gen-assets.mjs     uses `sips`, which is part of macOS
//
// The PNG files it writes are kept in the repository, so building the app does not run this.
//
// The mark is read from ONE place, MARK. Today that is the stand-in the RFC names
// (extension/media/overseer.svg), because the owner's mark of AC-142 is not in
// docs/design/brand/ yet. When it is, point MARK at it and run this again: that is the swap.

import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const phoneRoot = path.join(here, '..');
const repoRoot = path.join(phoneRoot, '..');
const MARK = path.join(repoRoot, 'extension', 'media', 'overseer.svg');
const { palettes } = createRequire(import.meta.url)(
  path.join(repoRoot, 'extension', 'design', 'tokens.js'),
);
const assets = path.join(phoneRoot, 'assets');
const CANVAS = 1024;

/**
 * One SVG of the whole canvas: the background (a colour, or nothing for a transparent image)
 * and the mark in one colour, `fraction` of the canvas wide, in the middle.
 */
function canvasSvg({ color, background, fraction }) {
  const size = Math.round(CANVAS * fraction);
  const offset = Math.round((CANVAS - size) / 2);
  const mark = fs
    .readFileSync(MARK, 'utf8')
    .replace(/width="[^"]*"/, `x="${offset}" y="${offset}" width="${size}"`)
    .replace(/height="[^"]*"/, `height="${size}"`)
    .replaceAll('currentColor', color);
  const ground =
    background === null ? '' : `<rect width="${CANVAS}" height="${CANVAS}" fill="${background}"/>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${CANVAS}" height="${CANVAS}" viewBox="0 0 ${CANVAS} ${CANVAS}">${ground}${mark}</svg>`;
}

function draw(name, options) {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'overseer-assets-'));
  try {
    const svg = path.join(work, 'canvas.svg');
    fs.writeFileSync(svg, canvasSvg(options));
    execFileSync('sips', ['-s', 'format', 'png', svg, '--out', path.join(assets, name)], {
      stdio: 'pipe',
    });
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
  console.log(`assets: wrote assets/${name}`);
}

const { dark, light } = palettes;
fs.mkdirSync(assets, { recursive: true });
// The icon fills its square; iOS and Android cut the corners themselves.
draw('icon.png', { color: dark.accent, background: dark.chrome, fraction: 0.6 });
// Android's adaptive icon: the launcher shows the middle two thirds of the foreground.
draw('android-icon-foreground.png', { color: dark.accent, background: null, fraction: 0.42 });
draw('android-icon-monochrome.png', { color: dark.onAccent, background: null, fraction: 0.42 });
// The launch image, one per appearance, on the background colour set in app.config.ts.
draw('launch-mark-light.png', { color: light.silver, background: null, fraction: 0.5 });
draw('launch-mark-dark.png', { color: dark.silver, background: null, fraction: 0.5 });
