# Theming — semantic colour tokens (issue #805, Spec 0055)

Colour values live in the two theme files
[`src/theme/dark.css`](src/theme/dark.css) and `light.css`. Both follow the "Angular" design
(`docs/design/angular/tokens.md`): neutral graphite surfaces, signal-cyan accent.
The palette primitives are `--palette-graphite-*` (was slate) and `--palette-cyan-*` (was indigo). Components use **semantic Tailwind
classes only** (`bg-surface`, `text-fg-muted`, `border-border`, `ring-focus`,
opacity modifiers such as `bg-accent/16` work). `src/index.css` maps each token
to a Tailwind utility (`@theme inline { --color-surface: var(--surface); … }`).
The guard test `src/theme/colourGuard.test.ts` fails on palette classes
(`text-slate-400`), `bg-black/white`, hex and `rgb()/hsl()` colours in
non-test `src/**/*.{ts,tsx}`. There is no allowlist: every component is migrated.

## Naming scheme

`<role>[-<variant>]`, used as the Tailwind colour name.

| Group | Tokens |
|---|---|
| Neutral surfaces | `surface-sunken` (inputs, window ground), `surface`, `surface-raised`, `surface-hover` |
| Lines | `border`, `border-strong` |
| Text | `fg-strong`, `fg`, `fg-soft`, `fg-muted`, `fg-subtle`, `fg-faint`, `fg-bright` (pure white in dark, near-black in light) |
| Roles `accent`, `danger`, `warning`, `success` | `<role>-subtle` (tinted fill), `-subtle-hover`, `-border`, `-border-strong`, `<role>` (solid), `-strong` (hover/brighter solid), `-fg` (text), `-fg-soft`, `-fg-softer`, `-fg-faint`, `-fg-faintest` (`danger` has `-fg-faintest` for text on a solid danger fill) |
| Info | `info`, `info-fg` |
| Local session | `local-border`, `local`, `local-fg` (badge and header of a local-shell session) |
| Misc | `on-accent` / `on-solid` (text on a solid accent fill: dark in the dark theme, white in light), `focus` (focus ring, = accent), `shadow-offset` (colour of the hard offset shadow), `grid-line` (1 px grid of the start screens, `bg-grid-line`), `selection`, `overlay` (modal scrim base, use `bg-overlay/70`), `web-border` / `web-fg` (web activity card), `mcp-subtle` / `mcp-fg` (external-tool origin badge) |
| Terminal | `term-bg`, `term-fg`, `term-cursor`, `term-cursor-accent`, `term-selection`, `term-ansi-{black,red,green,yellow,blue,magenta,cyan,white,bright-*}` (read by the terminal as CSS variables; not Tailwind utilities) |

Shape and depth (Angular design): all `--radius-*` values are `0`, including
`rounded-full` (status markers are `StatusSquare`s; the spinner and the typing
pings are square, too). All blurred `shadow-*` values are `none`; depth comes
from 1 px `border`/`border-strong` rules and surface steps. Floating layers use
the hard offset shadows `shadow-offset-sm` (4 px: menu, tooltip, drag ghost),
`shadow-offset` (6 px: popovers) and `shadow-offset-lg` (8 px: dialogs), coloured by
`--shadow-offset`.

Focus ring: use the utilities defined once in `src/index.css`, never a literal
`focus-visible:ring-2` chain (guarded by `colourGuard.test.ts`): `focus-ring`
and `focus-ring-flush` (2 px `focus` ring, no offset) and `focus-ring-inset`
(drawn inside the element). `FIELD_CONTROL_CLASS` uses the same look.

Type scale utilities (`src/index.css`): `text-display` (Barlow Condensed
32/700, +0.06em, uppercase), `text-title` (20/600, +0.05em), `text-label`
(11.5/600, +0.14em, `fg-faint`), plus the `label-condensed` class for buttons,
tabs and badges (600, uppercase, +0.08em). For Japanese, Chinese, Korean and
Hindi the letter spacing is `normal`.

Primitives (`src/components/ui/`): `Button` (`primary`, `secondary`, `danger`,
`ghost`, `bare`, `iconOnly`), `Field`, `Tabs`, `DialogFrame` (`icon`, `ticks`,
`tone`), `Badge`, `StatusSquare`, `CornerTicks`, `Menu`, `Tooltip`, `TagInput`.

Preferences when migrating: focus rings/outlines → `ring-focus`/`outline-focus`
(not `accent-fg`); text on a solid accent button → `text-on-accent`; hover on a
raised surface → `bg-surface-hover`.

## Palette class → token (mechanical migration)

Applies to every utility prefix (`bg-`, `text-`, `border-`, `ring-`,
`ring-offset-`, `outline-`, `divide-`, `placeholder:` …) and keeps modifiers
(`/55`).

| Palette class suffix | Token suffix |
|---|---|
| `graphite-950` | `surface-sunken` |
| `graphite-900` | `surface` |
| `graphite-800` | `surface-raised` |
| `graphite-700` | `border` |
| `graphite-600` | `border-strong` |
| `graphite-500` | `fg-faint` |
| `graphite-400` | `fg-subtle` |
| `graphite-300` | `fg-muted` |
| `graphite-200` | `fg-soft` |
| `graphite-100` | `fg` |
| `graphite-50` | `fg-strong` |
| `cyan-950` | `accent-subtle` |
| `cyan-900` | `accent-subtle-hover` |
| `cyan-800` | `accent-border` |
| `cyan-700` | `accent-border-strong` |
| `cyan-600` | `accent` |
| `cyan-500` | `accent-strong` |
| `cyan-400` | `accent-fg` |
| `cyan-300` | `accent-fg-soft` |
| `cyan-200` | `accent-fg-softer` |
| `cyan-100` | `accent-fg-faint` |
| `cyan-50` | `accent-fg-faintest` |
| `red-950` | `danger-subtle` |
| `red-900` | `danger-subtle-hover` |
| `red-800` | `danger-border` |
| `red-700` | `danger-border-strong` |
| `red-600` | `danger` |
| `red-500` | `danger-strong` |
| `red-400` | `danger-fg` |
| `red-300` | `danger-fg-soft` |
| `red-200` | `danger-fg-softer` |
| `red-100` | `danger-fg-faint` |
| `red-50` | `danger-fg-faintest` |
| `amber-950` | `warning-subtle` |
| `amber-900` | `warning-subtle-hover` |
| `amber-800` | `warning-border` |
| `amber-700` | `warning-border-strong` |
| `amber-600` | `warning` |
| `amber-500` | `warning-strong` |
| `amber-400` | `warning-fg` |
| `amber-300` | `warning-fg-soft` |
| `amber-200` | `warning-fg-softer` |
| `amber-100` | `warning-fg-faint` |
| `emerald-950` | `success-subtle` |
| `emerald-900` | `success-subtle-hover` |
| `emerald-800` | `success-border` |
| `emerald-700` | `success-border-strong` |
| `emerald-600` | `success` |
| `emerald-500` | `success-strong` |
| `emerald-400` | `success-fg` |
| `emerald-300` | `success-fg-soft` |
| `emerald-200` | `success-fg-softer` |
| `emerald-100` | `success-fg-faint` |
| `fuchsia-900` | `local-border` |
| `fuchsia-500` | `local` |
| `fuchsia-300` | `local-fg` |
| `steel-600` | `info` |
| `steel-400` | `info-fg` |
| `white` | `fg-bright` |
| `black` | `overlay` |

Example: `text-graphite-400` → `text-fg-subtle`; `border-red-800` →
`border-danger-border`; `bg-cyan-600/16` → `bg-accent/16`.

## Add a token

1. Add `--<name>: …;` to `src/theme/dark.css` (reference a `--palette-*`
   primitive or give a value).
2. Add `--color-<name>: var(--<name>);` to the `@theme inline` block in
   `src/index.css`.
3. Add a row to the tables above.

## Add a theme

Light (`src/theme/light.css`) is the worked example.

1. Create `src/theme/<name>.css` that overrides the same variables as
   `dark.css` under `html[data-theme="<name>"] { … }` (palette primitives,
   tokens with literal values, `--term-*`, window controls), and import it
   from `index.css`.
2. Add the name to `ResolvedTheme` and `THEME_CHOICES` in `src/lib/theme.ts`
   (resolution rules) and a label `appearance.<name>` in every locale. A test
   fails when a theme file misses a variable that has a literal value in
   `dark.css`.
3. Extend the contrast test (`src/themeContrast.test.ts`) to the new theme.

No component changes are needed — as long as no colour is hard-coded (the
guard enforces this).
