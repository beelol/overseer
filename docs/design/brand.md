# Overseer brand mark

Owner decision (2026-09-26, [AC-142](../overseer-rfc.md)): one mark for Overseer on every surface. Agents building a surface (VS Code extension, the macOS notification helper or a Mac app, the Gate N phone app) use these files and nothing else.

The mark: three violet-to-blue swooshes orbiting a dark core with a bright four-point star. Two versions came from the owner: a full-colour app icon (rounded square on a dark violet ground) and a transparent mark.

## Files (`docs/design/brand/`)

| File | What | Used for |
| --- | --- | --- |
| `overseer-icon.png` | Full-colour app icon, rounded square, 1024 px or larger | macOS app and helper icon, phone app icon, VS Code Marketplace icon (exported at 256 px) |
| `overseer-mark.png` | Full-colour mark on a transparent ground | Overseer's own views (composer heading, empty states, splash), documentation |
| `overseer-glyph.svg` | Single-colour silhouette of the same mark (three swooshes and the core), no gradients, drawn for 16–24 px | Surfaces that tint one colour: VS Code activity bar and status bar, macOS template images, Android monochrome icon |
| `exports/` | Sizes each platform asks for (Marketplace 256, macOS `.icns` set, iOS and Android icon sets) | Generated from the two sources above |

Status: the owner's two images are to be added to `docs/design/brand/` (they were shared in a chat, not as files). The single-colour glyph is drawn from them; the owner approves it before it replaces the current eye icon.

## Rules

- Full colour wherever a surface allows colour; the single-colour glyph only where a surface tints one colour.
- Do not recolour, stretch, add effects or put the full-colour mark on a busy background; on light grounds use the transparent mark as is.
- The old eye glyph (`extension/media/overseer.svg`) is retired once AC-142 lands.
- One exception (owner, 2026-09-27, [AC-177](../overseer-rfc.md)): in Voice Mode the mark is animated and moves with the voice. Only there, only the animation the owner picks, and never stretched or recoloured. Design: [Voice Mode RFC](../rfcs/voice-mode.md#the-mark-in-the-middle).
