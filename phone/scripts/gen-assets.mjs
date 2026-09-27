#!/usr/bin/env node
// Makes the app icon, Android's adaptive and themed icons, the launch image and the in-app mark
// from the owner's brand files (AC-178) and the design tokens.
//
//   node scripts/gen-assets.mjs           writes phone/assets/
//   node scripts/gen-assets.mjs --check   fails when a file in phone/assets/ differs
//
// The PNG files it writes are kept in the repository, so building the app does not run this.
// The sources are the three files of docs/design/brand/ (see docs/design/brand.md):
//   overseer-app-icon.png   the home-screen icon
//   overseer-logo.png       the colour mark: Android's adaptive foreground, marks inside the app
//   overseer-icon-flat.png  the single-colour silhouette: Android's themed icon, the launch
//                           screen and the door, in grayscale

import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const Jimp = require('jimp-compact');
const { PNG } = require('pngjs');

const here = path.dirname(fileURLToPath(import.meta.url));
const phoneRoot = path.join(here, '..');
const repoRoot = path.join(phoneRoot, '..');
const brand = path.join(repoRoot, 'docs', 'design', 'brand');
const { palettes } = require(path.join(repoRoot, 'extension', 'design', 'tokens.js'));
const assets = path.join(phoneRoot, 'assets');
const CANVAS = 1024;
const check = process.argv.includes('--check');

const hex = (color) => Jimp.cssColorToHex(color);

/** The drawn part of an image: its box of pixels that are not transparent. */
function cropped(image) {
  const { width, height, data } = image.bitmap;
  let x0 = width, y0 = height, x1 = -1, y1 = -1;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (data[(y * width + x) * 4 + 3] > 8) {
        x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y);
      }
    }
  }
  return image.clone().crop(x0, y0, x1 - x0 + 1, y1 - y0 + 1);
}

/** Every drawn pixel in one colour, its transparency kept. */
function tinted(image, color) {
  const { r, g, b } = Jimp.intToRGBA(hex(color));
  const out = image.clone();
  out.scan(0, 0, out.bitmap.width, out.bitmap.height, (_x, _y, i) => {
    out.bitmap.data[i] = r; out.bitmap.data[i + 1] = g; out.bitmap.data[i + 2] = b;
  });
  return out;
}

/** The mark `fraction` of the canvas across (its longer side), in the middle of a canvas. */
function centred(mark, fraction, size = CANVAS, background = 0x00000000) {
  const canvas = new Jimp(size, size, background);
  const scaled = mark.clone().scaleToFit(Math.round(size * fraction), Math.round(size * fraction), Jimp.RESIZE_BICUBIC);
  return canvas.composite(scaled, Math.round((size - scaled.bitmap.width) / 2), Math.round((size - scaled.bitmap.height) / 2));
}

const outputs = [];
/** A PNG with no alpha channel (iOS refuses an app icon that has one). */
function opaquePng(image) {
  const { width, height, data } = image.bitmap;
  const png = new PNG({ width, height, colorType: 2, inputColorType: 6, inputHasAlpha: true });
  data.copy(png.data);
  return PNG.sync.write(png, { colorType: 2, inputColorType: 6, inputHasAlpha: true });
}

async function write(name, image, { opaque = false } = {}) {
  const file = path.join(assets, name);
  const png = opaque ? opaquePng(image) : await image.getBufferAsync(Jimp.MIME_PNG);
  outputs.push(name);
  if (check) {
    if (!fs.existsSync(file) || !fs.readFileSync(file).equals(png)) throw new Error(`assets/${name} is not current: run npm run assets`);
    return;
  }
  fs.writeFileSync(file, png);
  console.log(`assets: wrote assets/${name}`);
}

const { dark, light } = palettes;
fs.mkdirSync(assets, { recursive: true });
const appIcon = await Jimp.read(path.join(brand, 'overseer-app-icon.png'));
const logo = cropped(await Jimp.read(path.join(brand, 'overseer-logo.png')));
const flat = cropped(await Jimp.read(path.join(brand, 'overseer-icon-flat.png')));

// iOS: the full icon at 1024 px with no transparency; the system rounds the corners.
await write('icon.png', new Jimp(CANVAS, CANVAS, hex(dark.chrome)).composite(appIcon.clone().resize(CANVAS, CANVAS, Jimp.RESIZE_BICUBIC), 0, 0), { opaque: true });
// Android's adaptive icon: the colour mark on the tile's dark violet (app.config.ts), inside
// the middle of the canvas that every launcher mask keeps; the silhouette as the themed icon.
await write('android-icon-foreground.png', centred(logo, 0.5));
await write('android-icon-monochrome.png', centred(tinted(flat, '#ffffff'), 0.5));
// The launch screen and the door (AC-136): the silhouette in grayscale, one per appearance.
await write('launch-mark-light.png', centred(tinted(flat, light.silver), 0.5));
await write('launch-mark-dark.png', centred(tinted(flat, dark.silver), 0.5));
// Overseer's own mark inside the app (the pairing screen): the colour mark.
await write('overseer-logo.png', centred(logo, 1, 512));
if (check) console.log(`assets: ${outputs.length} files match docs/design/brand/`);
