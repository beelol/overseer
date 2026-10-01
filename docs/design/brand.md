# Overseer brand mark

Owner decision (2026-09-26, [AC-142](../overseer-rfc.md)): one mark for Overseer on every surface. Agents building a surface (VS Code extension, the macOS notification helper or a Mac app, the Gate N phone app) use these files and nothing else.

The mark: three violet-to-blue swooshes orbiting a dark core with a bright four-point star. The owner supplied three images; the single-colour SVG is drawn from the flat one.

## Sources (`docs/design/brand/`)

| File | What |
| --- | --- |
| `overseer-app-icon.png` | Full-colour app icon: the mark on a dark violet rounded tile, 1254 px, the tile filling the square |
| `overseer-logo.png` | Full-colour mark on a transparent ground, 1254 px |
| `overseer-icon-flat.png` | Single-colour silhouette (white on transparent), 1254 px |
| `overseer-mark.svg` | The flat silhouette as one `currentColor` path on a 24-unit square: three crescents around a disc with a round hole where the star is. Drawn for 16–24 px |

## Exports (`docs/design/brand/exports/`)

Made from the sources with `sips -z N N <source> --out <export>`; the macOS icon also with `sips -p 1024 1024` (824 px tile centred on a transparent 1024 px square, the macOS icon grid).

| Export | Sizes |
| --- | --- |
| `overseer-app-icon-N.png` | 128, 256, 512, 1024 |
| `overseer-app-icon-macos-1024.png` | 1024 (tile 824) |
| `overseer-logo-N.png` | 64, 128, 256 |
| `overseer-icon-flat-N.png` | 32, 64, 128 |

## Where each is used

| Surface | Version | File in the product |
| --- | --- | --- |
| VS Code Marketplace and Extensions view icon | Full-colour app icon, 256 px | `extension/media/overseer-app-icon.png` (`icon` in `extension/package.json`) |
| VS Code activity bar and the Overseer panel container | Single-colour SVG, tinted by VS Code | `extension/media/overseer.svg` (a copy of `overseer-mark.svg`) |
| VS Code status bar (`$(overseer-mark) Overseer 2 active`) | Single-colour glyph in an icon font | `extension/media/overseer-mark.woff`, built from `overseer-mark.svg` by `extension/design/build-mark-font.py`; contributed as the `overseer-mark` icon |
| Overseer webview tabs (Overseer view, New Task, agent panels) | Full-colour mark, 128 px | `extension/media/overseer-logo.png` (`iconPath`) |
| Inside Overseer's views: the composer's "What's next?" heading, "From Overseer" on messages, "Overseer will" on proposals | Full-colour mark, 128 px | `extension/media/overseer-logo.png` (the `.overseer-mark` class in `base.css`) |
| macOS notification helper (`Overseer Notifier.app`) | Full-colour app icon on the macOS grid | `extension/notifier/AppIcon.png` (`overseer-app-icon-macos-1024.png`), made into `AppIcon.icns` at build |
| Phone app icon and splash (Gate N) | Full-colour app icon; grayscale mark on the door (AC-136) | The phone app's generators read `extension/media/overseer.svg`, so they pick up the single-colour mark; its full-colour icon comes from `overseer-app-icon-1024.png` when Gate N adopts it |
| macOS menu-bar item (`Overseer Menu.app`, AC-262) | Single-colour silhouette as a template image, 18 pt, so macOS paints it the menu bar's colour; the dot beside it is Overseer's violet from `tokens.js`; a dev daemon's item adds a "!" in the lower corner | `StatusIcon.png` and `StatusIcon@2x.png`, made from `overseer-icon-flat.png` by `extension/menubar/build.js` (`sips -z 18 18`, `36 36`) |
| Monochrome surfaces still to come (Android monochrome icon) | Single-colour SVG | `overseer-mark.svg` |

## Rules

- Full colour wherever a surface allows colour; the single-colour glyph only where a surface tints one colour.
- Keep the mark's shape and proportions: do not stretch it, redraw it or swap in another mark. On light grounds use the transparent mark as is.
- Effects are welcome (owner, 2026-09-27: "we do have effects"): motion, light and gradients moving through the mark, as on the phone's door (AC-136) and in Voice Mode (AC-177). An earlier version of this file forbade effects; that was not the owner's rule.
- Provider logos (Claude, Codex, OpenCode and the rest in `extension/media/logos/`) are other companies' marks and stay as they are.
- The old eye glyph is retired: no Overseer surface uses the eye any more. The codicon `eye` still means "watch" or "read only" where it is not Overseer's logo (following an agent's edits, tracking an agent in the grid, the read-only workspace).

## The mark in layers (`docs/design/brand/layers/`)

The owner's mark is one transparent image. For animation (Voice Mode, AC-177) a copy is cut into
three layers on the same canvas, so they stack without offsets. The owner's file is not changed.

| File | What |
| --- | --- |
| `overseer-logo-core.png` | The dark core with its rim and glow, as a whole disc. The part hidden behind the swooshes is rebuilt. |
| `overseer-logo-swooshes.png` | The three swooshes as one ring. It turns around the core's centre. |
| `overseer-logo-star.png` | The star and its light, lifted off the core. |
| `split.py` | Makes the three from `overseer-logo.png`: `python3 docs/design/brand/layers/split.py`. Standard library only. |

Stack them core, swooshes, star. On the 1254 px canvas the swooshes turn around 632.8, 642.8 and
the star grows around 637.4, 640.6.

Limits, stated plainly:

- Stacked again they match the original closely, not exactly: the largest difference is 31 of 255,
  and 50 pixels of 1.57 million differ by more than 8.
- The three swooshes are not separated from each other. They overlap, and the hidden parts are not
  in the image.
- The inner edge of the swooshes has small flaws that can show in a slow full turn.
- A layered file from whoever drew the mark would remove all three limits and replaces these files.
