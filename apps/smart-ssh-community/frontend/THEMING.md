# Theming — semantic colour tokens (issue #805, Spec 0055)

Colour values live in exactly one place: the theme file
[`src/theme/dark.css`](src/theme/dark.css). Components use **semantic Tailwind
classes only** (`bg-surface`, `text-fg-muted`, `border-border`, `ring-focus`,
opacity modifiers such as `bg-accent/16` work). `src/index.css` maps each token
to a Tailwind utility (`@theme inline { --color-surface: var(--surface); … }`).
The guard test `src/theme/colourGuard.test.ts` fails on palette classes
(`text-slate-400`), `bg-black/white`, hex and `rgb()/hsl()` colours in
non-test `src/**/*.{ts,tsx}`, except for files in
`src/theme/colourAllowlist.json` (not yet migrated). The allowlist may only
shrink: the test also fails when a listed file is already clean — remove it.

## Naming scheme

`<role>[-<variant>]`, used as the Tailwind colour name.

| Group | Tokens |
|---|---|
| Neutral surfaces | `surface-sunken` (inputs, window ground), `surface`, `surface-raised`, `surface-hover` |
| Lines | `border`, `border-strong` |
| Text | `fg-strong`, `fg`, `fg-soft`, `fg-muted`, `fg-subtle`, `fg-faint`, `fg-bright` (pure white) |
| Roles `accent`, `danger`, `warning`, `success` | `<role>-subtle` (tinted fill), `-subtle-hover`, `-border`, `-border-strong`, `<role>` (solid), `-strong` (hover/brighter solid), `-fg` (text), `-fg-soft`, `-fg-softer`, `-fg-faint`, `-fg-faintest` |
| Info | `info`, `info-fg` |
| Misc | `on-accent` (text on a solid accent fill), `focus` (focus ring/outline), `selection`, `overlay` (modal scrim base, use `bg-overlay/70`) |
| Terminal | `term-bg`, `term-fg`, `term-cursor`, `term-cursor-accent`, `term-selection`, `term-ansi-{black,red,green,yellow,blue,magenta,cyan,white,bright-*}` |

Preferences when migrating: focus rings/outlines → `ring-focus`/`outline-focus`
(not `accent-fg`); text on a solid accent button → `text-on-accent`; hover on a
raised surface → `bg-surface-hover`.

## Palette class → token (mechanical migration)

Applies to every utility prefix (`bg-`, `text-`, `border-`, `ring-`,
`ring-offset-`, `outline-`, `divide-`, `placeholder:` …) and keeps modifiers
(`/55`).

| Palette class suffix | Token suffix |
|---|---|
| `slate-950` | `surface-sunken` |
| `slate-900` | `surface` |
| `slate-800` | `surface-raised` |
| `slate-700` | `border` |
| `slate-600` | `border-strong` |
| `slate-500` | `fg-faint` |
| `slate-400` | `fg-subtle` |
| `slate-300` | `fg-muted` |
| `slate-200` | `fg-soft` |
| `slate-100` | `fg` |
| `slate-50` | `fg-strong` |
| `indigo-950` | `accent-subtle` |
| `indigo-900` | `accent-subtle-hover` |
| `indigo-800` | `accent-border` |
| `indigo-700` | `accent-border-strong` |
| `indigo-600` | `accent` |
| `indigo-500` | `accent-strong` |
| `indigo-400` | `accent-fg` |
| `indigo-300` | `accent-fg-soft` |
| `indigo-200` | `accent-fg-softer` |
| `indigo-100` | `accent-fg-faint` |
| `indigo-50` | `accent-fg-faintest` |
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
| `steel-600` | `info` |
| `steel-400` | `info-fg` || `white` | `fg-bright` |
| `black` | `overlay` |

Example: `text-slate-400` → `text-fg-subtle`; `border-red-800` →
`border-danger-border`; `bg-indigo-600/16` → `bg-accent/16`.
Until a file is migrated its palette classes keep working through a legacy
`--color-<palette>-<shade>` alias block in `index.css` (values still from
`dark.css`); that block is deleted with the last allowlisted file.

## Add a token

1. Add `--<name>: …;` to `src/theme/dark.css` (reference a `--palette-*`
   primitive or give a value).
2. Add `--color-<name>: var(--<name>);` to the `@theme inline` block in
   `src/index.css`.
3. Add a row to the tables above.

## Add a theme

Create `src/theme/<name>.css` that overrides the same layer-2 variables under
`[data-theme="<name>"] { … }`, import it from `index.css`, and set
`data-theme` on `<html>`. No component changes are needed — as long as no
colour is hard-coded (the guard enforces this).
