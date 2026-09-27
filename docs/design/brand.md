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
| Monochrome surfaces still to come (macOS menu bar template image, Android monochrome icon) | Single-colour SVG | `overseer-mark.svg` |

## Rules

- Full colour wherever a surface allows colour; the single-colour glyph only where a surface tints one colour.
- Do not recolour, stretch, add effects or put the full-colour mark on a busy background; on light grounds use the transparent mark as is.
- Provider logos (Claude, Codex, OpenCode and the rest in `extension/media/logos/`) are other companies' marks and stay as they are.
- The old eye glyph is retired: no Overseer surface uses the eye any more. The codicon `eye` still means "watch" or "read only" where it is not Overseer's logo (following an agent's edits, tracking an agent in the grid, the read-only workspace).
- One exception (owner, 2026-09-27, [AC-177](../overseer-rfc.md)): in Voice Mode the mark is animated and moves with the voice. Only there, only the animation the owner picks, and never stretched or recoloured. Design: [Voice Mode RFC](../rfcs/voice-mode.md#the-mark-in-the-middle).
