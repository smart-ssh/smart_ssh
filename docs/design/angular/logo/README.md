# Logo – direction B "Trace"

The logo sheet (`../screens/Logo.html`, `../screens/Logo.png`) shows three directions: A "Gate",
B "Trace" and C "Bracket". **B "Trace" is the chosen one**; only B is exported here.

> An S drawn as one orthogonal connection trace between two terminals: client and server. It is a
> monogram for *Smart SSH*, it does not depend on the generic `>_` prompt every terminal app uses,
> and it reads at 16 px because it has only three strokes and two solid squares. The accent squares
> are the only colour, so the mark survives one-colour use (print, tray icon, monochrome macOS menu
> bar template).

## Files

| File | Content |
|---|---|
| `app-icon-dark.svg` | 64 × 64 square tile, background `#0e1116`, stroke `#f2f4f7`, accent `#2cc6e8` |
| `app-icon-light.svg` | 64 × 64 square tile, background `#f2f4f7`, stroke `#12161c`, accent `#0a7fa0` |
| `glyph.svg` | Glyph without tile, 64-unit box, stroke 7 in `currentColor`, accent squares `#2cc6e8` (use `#0a7fa0` on light backgrounds) |
| `favicon.svg` | Glyph only with stroke 9 for 16 px; switches between light (`#12161c` / `#0a7fa0`) and dark (`#f2f4f7` / `#2cc6e8`) with `prefers-color-scheme` |
| `wordmark-dark.svg` | For dark backgrounds: glyph + "SMART" in `#f2f4f7` + "SSH" in `#2cc6e8` |
| `wordmark-light.svg` | For light backgrounds: glyph + "SMART" in `#12161c` + "SSH" in `#0a7fa0` |

The wordmark text is a live `<text>` element set in **Barlow Condensed 700**, uppercase,
letter-spacing .12em (3.6 px at 30 px). It only renders correctly where that font is available (the
app bundles it). For use outside the app, convert the text to outlines first. As on the sheet,
"SMART" and "SSH" are set without a space; the colour change separates the words.

## Geometry

All marks are drawn on a **64-unit grid** with square caps and mitred joins, one stroke weight and
one accent colour.

- Stroke path: `M48 14H16v18h32v18H16` (stroke width 7 u, no fill).
- Accent squares: `M42 8h12v12H42z M10 44h12v12H10z` (12 × 12 u, filled with the accent).
- In the tile the glyph group is scaled to 80 % and centred:
  `<g transform="translate(6.4 6.4) scale(.8)">` inside a 64 × 64 square.
- Tiles are square with radius 0; macOS, iOS and Android apply their own mask on top.

## Grid, clear space, minimum size

- **Grid**: 8-unit modules (the construction grid on the sheet has lines every 8 u).
- **Clear space**: 8 units on every side (the dashed lines at 8 and 56 on the sheet). Keep this
  space free of other elements; scale it with the mark.
- **Stroke**: 7 u in tiles and at normal sizes; **9 u for the bare 16 px favicon**.
- **Minimum size**: 16 px for the glyph alone, 24 px for the tile.
- The sheet shows the tile at 64, 32, 24 and 16 px and the favicon at 16 and 32 px in browser-tab
  style on dark and light backgrounds.

## Wordmark layout

On the sheet the wordmark is the glyph at 34 px followed after a 12 px gap by the text at 30 px.
The screens use two lock-ups:

- **Full wordmark** ("SMART" + accent "SSH", no space, letter-spacing .12–.14em): unlock screens
  (22 px, glyph in a 40 px tile) and the mobile navigation menu (17 px).
- **Compact header** (glyph without tile + "SMART SSH" with a space, all in `--fg-strong`, 15–16 px,
  letter-spacing .1em): desktop app header, settings header and the mobile server list header.
