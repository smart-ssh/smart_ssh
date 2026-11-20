# Tokens – direction "Angular"

Source: artboard `screens/Tokens.html` (plus the `.ss { … }` token blocks at the top of every
screen in `screens/`, which use the same values).

The design keeps the app's semantic token names (`--surface*`, `--fg*`, `--accent*`, `--danger*`,
`--warning*`, `--success*`, `--info*`, `--local*`, `--term-*`) from the frontend's `theme/dark.css` and
`theme/light.css`. Only the values change: indigo becomes signal cyan, slate becomes a neutral
graphite.

## Colour – tokens on the sheet

Contrast is measured against `--surface` of the same theme (WCAG ratio, as printed on the sheet).
"—" means the token is a surface or border colour, not a text colour.

| Token | Dark (`dark.css`) | Contrast on `--surface` | Light (`light.css`) | Contrast on `--surface` |
|---|---|---|---|---|
| `--surface-sunken` | `#080a0d` | — | `#f2f4f7` | — |
| `--surface` | `#0e1116` | — | `#ffffff` | — |
| `--surface-raised` | `#151a21` | — | `#ebeef2` | — |
| `--surface-hover` | `#1e252f` | — | `#dfe3e9` | — |
| `--border` | `#242b35` | — | `#d5dae1` | — |
| `--border-strong` | `#3a4452` | — | `#a9b2bf` | — |
| `--fg-faint` | `#7d8796` | 5.2:1 | `#5b6472` | 6.0:1 |
| `--fg-muted` | `#aeb6c2` | 9.2:1 | `#3f4753` | 9.4:1 |
| `--fg` | `#e3e7ed` | 15.2:1 | `#12161c` | 18.1:1 |
| `--fg-strong` | `#f2f4f7` | 17.2:1 | `#06080b` | 20.1:1 |
| `--accent` | `#2cc6e8` | 9.3:1 | `#0a7fa0` | 4.6:1 |
| `--accent-fg` | `#6fdcf2` | 11.9:1 | `#086a86` | 6.1:1 |
| `--accent-subtle` | `#0a2630` | — | `#e3f4f9` | — |
| `--accent-border` | `#134a59` | — | `#9fd6e6` | — |
| `--danger` | `#f04848` | 5.2:1 | `#c42b2b` | 5.6:1 |
| `--danger-fg` | `#ff8a85` | 8.3:1 | `#a32020` | 7.5:1 |
| `--warning` | `#f5a524` | 9.3:1 | `#9a5b00` | 5.4:1 |
| `--warning-fg` | `#ffc266` | 11.9:1 | `#7f4a00` | 7.3:1 |
| `--success` | `#3dd68c` | 10.1:1 | `#157a47` | 5.4:1 |
| `--success-fg` | `#7be5ae` | 12.3:1 | `#11653b` | 7.1:1 |
| `--info-fg` | `#b5c2d8` | 10.5:1 | `#3c4c66` | 8.7:1 |
| `--local-fg` | `#e9a6f7` | 10.1:1 | `#8a2aa0` | 7.2:1 |
| `--term-bg` | `#07090c` | — | `#f7f8fa` | — |
| `--term-fg` | `#d8dee6` | 14.0:1 | `#1a1f27` | 16.5:1 |

## Colour – further tokens used in the screens

These are not on the token sheet but are defined in the screens (dark values from every dark
screen, light values from `screens/MainLight.html`). They are part of the palette as well.

| Token | Dark | Light | Used for |
|---|---|---|---|
| `--fg-subtle` | `#96a0ae` | `#4b5361` | inactive tab text, secondary meta text |
| `--fg-soft` | `#c9cfd8` | `#2a313b` | default button text, tree rows |
| `--accent-strong` | `#5ad4ee` | `#086a86` | link hover |
| `--on-accent` | `#041015` | `#ffffff` | text on `--accent` fills (primary button, active chip) |
| `--danger-subtle` | `#2a0d0e` | `#fdecec` | danger panel background (hard blacklist, host key warning) |
| `--danger-border` | `#5c1c1d` | `#f0b4b4` | danger button / badge border |
| `--warning-subtle` | `#2a1d06` | `#fdf4e0` | confirmation card header, interrupted activity |
| `--warning-border` | `#5a3e0e` | `#e8c98a` | warning badge / card border |
| `--success-subtle` | `#0a2418` | `#e4f5ec` | success panel background |
| `--success-border` | `#175536` | `#a6dcbf` | success badge border |
| `--info` | `#8a9bb8` | `#566a8a` | info marker |
| `--local` | `#d86cf0` | `#a23bbb` | local-shell status square |
| `--local-border` | `#5a2366` | `#e2b6ec` | "Local" badge border |

Hard-coded colours that are not tokens: the destructive solid button in the dialogs uses white text
(`#ffffff`) on `--danger`; offset shadows are pure black (`#000000`).

Tokens that exist in the app theme but are **not specified** by this design (for example `--focus`,
`--overlay`, `--selection`, the `*-strong`, `*-fg-soft…`/`*-fg-faint…`, `*-subtle-hover` and
`*-border-strong` variants, `--term-cursor*`, `--term-selection` and the `--term-ansi-*` colours)
should be derived from the palette above: focus and selection use the accent, terminal ANSI colours
stay readable on `--term-bg`.

In the app, the light theme overrides the palette primitives under `html[data-theme="light"]` and the
semantic tokens reference them; the table gives the resulting semantic values the design expects.

## Typography

Fonts the app already bundles: **Barlow** (UI text), **Barlow Condensed** (headings, labels,
buttons, tabs, badges, always uppercase), **JetBrains Mono** (commands, paths, hosts, terminal).

| Role | Font | Size / weight | Letter-spacing | Case | Example |
|---|---|---|---|---|---|
| display | Barlow Condensed | 32 px / 700 | .06em | uppercase | "Unlock" |
| title | Barlow Condensed | 20 px / 600 | .05em | uppercase | "Server: web-01" |
| label | Barlow Condensed | 11.5 px / 600, colour `--fg-faint` | .14em | uppercase | "Parts of this command" |
| body | Barlow | 14 px / 400, line-height 1.45 | normal | sentence | chat text, descriptions |
| mono | JetBrains Mono | 12.5 px / 400 | normal | as typed | `journalctl -u php8.3-fpm -n 200` |

Component text used in the screens:

| Element | Desktop | Mobile |
|---|---|---|
| Button (`.btn`) | Barlow Condensed 600, 13 px, .08em, uppercase | 14 px, .08em |
| App / session tab (`.tab`) | Barlow Condensed 600, 13.5 px, .06em | pane switch 14 px, .08em, uppercase |
| Badge (`.badge`) | Barlow Condensed 600, 11 px, .1em, uppercase, height 20 px | same |
| Tree row (`.tr`) | Barlow 13.5 px, height 30 px, server names in mono 12.5 px | list rows min. 52 px |
| Terminal (`.t`) | JetBrains Mono 12.5 px, line-height 1.6 | JetBrains Mono |

## Component rules

From the "Rules of the system" panel on the token sheet, with the values the screens use:

- **Radius 0** on every element: buttons, inputs, cards, dialogs, badges, chips, tabs, app tile,
  avatars. The screens enforce it with `*, *::before, *::after { border-radius: 0 !important }`.
- **Focus**: 1 px accent border plus a 1 px accent ring, no glow –
  `border: 1px solid var(--accent); box-shadow: 0 0 0 1px var(--accent);`.
  Selected rows use an inset ring instead: `background: var(--accent-subtle); box-shadow: inset 0 0 0 1px var(--accent-border);`.
- **Depth** comes from 1 px rules and surface steps (`--surface-sunken` → `--surface` →
  `--surface-raised` → `--surface-hover`), never from soft shadows. The only shadow is a hard
  offset shadow without blur (sheet: 4 px, for drag ghosts and menus). The screens use
  `4px 4px 0 #000` for the drag ghost, `6px 6px 0 #000` for the activities popover and
  `8px 8px 0 #000` for dialogs.
- **Status = 8 px solid square, never a dot** (`.sq { width: 8px; height: 8px }`, background
  `--success` / `--warning` / `--danger` / `--local`). Inside 20 px badges the square is 7 px.
- **Spacing** on a 4 px grid.
- **Control heights**: desktop 32 px (buttons, icon buttons 32 × 32; inputs 34 px in the screens),
  mobile 44 px (touch targets, buttons, pane switch; scope chips in the mobile confirmation card are 36 px). App header and session tabs are
  44 px high on desktop as well.
- **Text contrast** ≥ 4.5 : 1 against `--surface` for every text colour pair.
- **Tabs**: active tab gets `--surface` background, `--fg-strong` text and a 2 px accent rule
  (top edge on desktop tabs, bottom edge on the mobile pane switch).
- **Tree connector lines**: child lists have a 1 px `--border-strong` left rule
  (`margin-left: 17px`) and an 8 px horizontal tick per row.
- **Corner ticks** on dialogs and the unlock card: L-shaped 2 px marks, 12 px (dialogs, mobile
  unlock) or 14 px (desktop unlock), offset 7–8 px outside the corners; accent colour, danger colour
  on the host-key warning (which shows only the two top ticks).
- **Icons**: 24 × 24 grid, stroke 2, square caps, mitre joins, `currentColor`; shown at 16 px on
  desktop and 18 px on mobile. See `icons/`.
