# Design reference – direction "Angular"

Static reference package of the Smart SSH redesign "Angular": brand, icon set, colour and type
tokens, and the main views of the desktop (1440 × 900) and mobile (390 × 844) app.

## The direction

> Direction "Angular": radius 0 everywhere (buttons, inputs, cards, dialogs, badges, chips, tabs,
> app tile, avatars). Signature: 1 px rules, tree connector lines, uppercase Barlow Condensed labels,
> square status markers, corner ticks on dialogs.
>
> Fonts are the ones the app already bundles (Barlow, Barlow Condensed, JetBrains Mono). Colours are
> mapped 1:1 onto the existing semantic token names in theme/dark.css and theme/light.css, so the
> switch is a palette change, not a component rewrite.

## Contents

| Path | What it is |
|---|---|
| `screens/<Name>.png` | Screenshot of each artboard at its design size (device scale 1). Use it for the look. |
| `screens/<Name>.html` | The same artboard as static, self-contained HTML. Open it in a browser and use the developer tools to read exact CSS values (sizes, spacing, colours, fonts). |
| `tokens.md` | Colour tokens for dark and light, typography scale, component rules. |
| `icons/` | The icon set as one SVG per icon, plus `icons/README.md` (name → intended use). |
| `logo/` | The chosen logo, direction B "Trace": app icon tiles, glyph, favicon, wordmarks, plus `logo/README.md` (grid, clear space, minimum size). |

## Artboards

### Brand and foundations

| File | Title | Size | Shows |
|---|---|---|---|
| `screens/Logo` | Logo – 3 directions | 1440 × 1100 | The three logo directions A "Gate", B "Trace" (recommended, chosen) and C "Bracket" as tiles, size ramps, favicons and wordmarks; construction grid and rationale of B |
| `screens/Icons` | Icon set | 1440 × 1100 | The 54 icons in three groups and an "In use" panel (server tree, drag ghost, Move to… dialog, file browser, ssh_config import) |
| `screens/Tokens` | Colour & type tokens | 1440 × 1100 | Dark and light token tables with contrast, type scale, sample components, rules of the system |

### Desktop · 1440 × 900

| File | Title | Size | Shows |
|---|---|---|---|
| `screens/Main` | Desktop – Main window (dark) | 1440 × 900 | Connect view: app header, session tabs, server tree sidebar, terminal pane and AI chat pane side by side, an automatically run command |
| `screens/MainLight` | Desktop – Main window (light) | 1440 × 900 | The same main window in the light theme |
| `screens/DeskChat` | Desktop – Chat with confirmation card | 1440 × 900 | AI pane with a suggested command waiting for confirmation: editable command, covered parts, to-decide rows with scope and rule level, Save rules / Deny / Execute |
| `screens/DeskFiles` | Desktop – File browser | 1440 × 900 | Files pane: path bar, toolbar, file table with symlinks, detail panel of the selected file, transfers |
| `screens/DeskRules` | Desktop – Filter rules | 1440 × 900 | Filter Rules view: rule table (priority, type, pattern, action, scope, invalid pattern), hard blacklist, rule tester with per-segment decision |
| `screens/DeskSettings` | Desktop – Settings | 1440 × 900 | Settings with category list; AI Provider section (configured providers, AI second opinion) |
| `screens/DeskServerForm` | Desktop – Server form | 1440 × 900 | Manage view: server form, Connection tab (host, port, user, jump host, authentication with key file checks, connection test, sudo password, advanced) |
| `screens/DeskActivity` | Desktop – Activity center | 1440 × 900 | Activities popover: running download, speech model verification, interrupted, failed and done transfers |
| `screens/DeskUnlock` | Desktop – Master password / startup | 1440 × 900 | Unlock card on a grid background with corner ticks, failed-attempt hint, Start over |
| `screens/DeskDialogs` | Desktop – Dialogs sheet | 1440 × 1000 | Host key changed warning, Move to… dialog, tree drag ghost with drop hint, quit while activities are running, delete folder, ssh_config import |

### Mobile · 390 × 844

| File | Title | Size | Shows |
|---|---|---|---|
| `screens/MobServers` | Mobile – Server list | 390 × 844 | Server tree with user@host sub-lines, add group / server, import / export |
| `screens/MobMenu` | Mobile – Navigation menu | 390 × 844 | Burger sheet: main views, open sessions, new session, Activities, Settings |
| `screens/MobTerminal` | Mobile – Terminal + key row | 390 × 844 | Session header, pane switch, terminal output, two-row special key bar |
| `screens/MobChat` | Mobile – Chat + confirmation | 390 × 844 | Confirmation card in the AI pane with full-width to-decide rows |
| `screens/MobFiles` | Mobile – Files | 390 × 844 | File list with meta lines and symlink targets, path bar, New folder / Upload |
| `screens/MobRules` | Mobile – Filter rules | 390 × 844 | Rule list with priority, action, scope and type; hard blacklist summary |
| `screens/MobSettings` | Mobile – Settings | 390 × 844 | Settings category list with current values |
| `screens/MobServerForm` | Mobile – Server form | 390 × 844 | Server form, Security tab: confirmation after server access, AI injection check, host keys |
| `screens/MobUnlock` | Mobile – Unlock | 390 × 844 | Unlock screen with logo, password field and Start over |

Each file exists as `.png` and `.html`.

## Notes from the design

Items marked **PROPOSAL** are design proposals that go beyond the current app; everything else
follows today's features and texts.

### Desktop notes

- Layout and features follow the current app (AppHeader with Connect / Manage / Filter Rules, session tabs, sidebar tree, AI / Terminal / Files panes).
- **PROPOSAL:** tree connector lines and item counts on groups in the sidebar.
- **PROPOSAL:** segmented context meter in the chat header (today a text indicator "Context used / window").
- Confirmation card follows spec 0011 (parts list: covered on top, to-decide rows expand inline with scope + suggested rule level; picked rule shows a check in place; separate Save rules (N)).
- **PROPOSAL:** "Save rules (N)" as its own button is the planned design, not today's "Accept + Rule" dropdown.
- Interactive: in Play, expand/collapse the to-decide rows, switch scope and pick or unpick a rule level; the Save rules count follows.

The static `DeskChat` files show the default state of that interaction: the `journalctl` row is
expanded with scope "This Server" and the suggested rule level picked, the `grep` row is collapsed
with its suggested rule picked (Global), so the button reads "Save rules (2)".

### Desktop notes (row 2)

- Settings shows only the existing categories (AI Provider, Display & Language, Diagnostics, Files, Master password, Data and backup, Voice input, About, plus the registered Sessions & Data and MCP Server).
- Server form: fields per tab as in serverForm/ (General: name, group, tags; Connection: host, port, user, jump host, authentication, test, sudo, advanced; Security: confirmation after server access, AI injection check, host keys; Notes).
- **PROPOSAL:** file browser detail panel on the right (today: entry menu + properties dialog).
- **PROPOSAL:** unlock card with corner ticks and grid background; texts are the existing ones.
- Hard blacklist chips summarise the built-in patterns (rm -rf, dd of=/dev, mkfs, fork bomb, /etc/shadow writes, shutdown/reboot, systemctl reboot…, init 0/6).

### Mobile notes

- Navigation keeps the current NarrowHeaderNav concept: burger opens a sheet with the main views, open sessions and settings; the header shows the active session.
- In a session, the pane switch AI / Terminal / Files sits under the header (as today on narrow screens).
- Terminal key row: Esc, Tab, Ctrl, Alt, arrows, Home/End, PgUp/PgDn, | ~ / - (spec 0114).
- Touch targets 44 px.
- **PROPOSAL:** on mobile the confirmation card's to-decide rows open as full-width blocks; scope chips wrap.
- **PROPOSAL:** key row as two fixed rows of 8 keys (no horizontal scrolling).
- **PROPOSAL:** user@host sub-line in the mobile server list, current value next to each settings section, Activities entry in the burger menu.

## How to use this package

1. **Look** at the PNG of the view you are changing.
2. **Measure** in the HTML: open `screens/<Name>.html` in a browser and inspect the element. All
   styles are inline or in one `<style>` block. In the app screens the tokens are declared as CSS
   custom properties on the root container (`.ss { --surface: …; … }`) and used via `var(--…)`, so
   the names match the app's theme tokens; the brand sheets and the unlock screens use hex values
   directly.
3. **Map** colours to tokens with `tokens.md`; do not copy hex values into components. Light-theme
   values come from `tokens.md` and `screens/MainLight`.
4. **Icons and logo**: use the SVGs in `icons/` and `logo/` instead of redrawing them.
5. **Content is sample data.** Host names, IP addresses, commands, file names, model names and
   numbers in the screens are examples, not requirements. Texts that already exist in the app keep
   their current wording and translations.
6. The HTML loads Barlow, Barlow Condensed and JetBrains Mono from Google Fonts for viewing only;
   the app uses its bundled copies.
