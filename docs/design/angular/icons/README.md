# Icons – flat angular set

54 icons from the icon sheet (`../screens/Icons.html`, `../screens/Icons.png`). They replace the
folder / computer emoji used today in the server tree, the file browser and the dialogs.

Construction: 24 × 24 grid with a 2-unit keyline, stroke 2, square caps, mitre joins, only 0°, 45°
and 90° angles, colour `currentColor`. Every file has the same root attributes:

```html
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24"
     fill="none" stroke="currentColor" stroke-width="2"
     stroke-linecap="square" stroke-linejoin="miter">
```

Display size: 16 px on desktop (tree, buttons, tabs), 18 px on mobile, 12 px for tree chevrons,
14 px for inline status checks. The screens keep stroke 2 at 16 px (the sheet header mentions 1.5 at
16 px; the screens do not use that).

The file name is the icon key from the sheet in kebab-case (`folderOpen` → `folder-open.svg`).

## Tree · servers · files

| File | Intended use |
|---|---|
| `folder.svg` | Tree group, collapsed; folder in the file browser; group in the Move to… dialog and drop target |
| `folder-open.svg` | Tree group, expanded |
| `folder-link.svg` | Symlink to a directory in the file browser (`ssl → /etc/letsencrypt/…`) |
| `server.svg` | Server entry in the tree, tabs, server form header, drag ghost |
| `local.svg` | Local shell ("Localhost"), shown in `--local-fg` |
| `file.svg` | Plain file in the file browser; "Files" settings category |
| `file-text.svg` | Text / config file (`nginx.conf`, `README.md`, `composer.lock`) |
| `file-code.svg` | Code or script file (`artisan`, `composer.json`) |
| `symlink.svg` | Symlink to a file (`default → sites-available/default`, `.env → …`) |
| `file-check.svg` | ssh_config import: file read successfully |
| `file-x.svg` | ssh_config import: file skipped (not an ssh_config) |
| `root.svg` | "(top level)" target in the Move to… dialog |
| `move.svg` | Move action; drop hint on the drag ghost; Move to… dialog title |
| `import.svg` | Import… (server list footer, ssh_config import dialog) |
| `export.svg` | Export… (server list footer) |
| `upload.svg` | Upload / upload folder in the file browser, upload transfers |
| `download.svg` | Download in the file browser, download activities, "Data and backup" settings |
| `parent.svg` | Parent directory button in the file browser path bar |

## Actions

| File | Intended use |
|---|---|
| `plus.svg` | New session tab, attach in the chat composer, new session in the mobile menu |
| `x.svg` | Close tab / panel / dialog, dismiss activity, Deny |
| `check.svg` | Covered part of a command, picked rule, done transfer, key file checks |
| `chev-r.svg` | Collapsed tree group or to-decide row; mobile list rows (drill-in) |
| `chev-d.svg` | Expanded tree group or to-decide row; sort indicator; select |
| `more.svg` | Row / session overflow menu (…) |
| `menu.svg` | Mobile burger menu |
| `edit.svg` | Edit / open locally |
| `copy.svg` | Copy (no screen uses it yet) |
| `trash.svg` | Delete (file browser, delete folder dialog) |
| `search.svg` | Search / filter field (no screen uses it yet) |
| `refresh.svg` | Reload a listing (no screen uses it yet) |
| `grip.svg` | Drag handle for reorderable rows (no screen uses it yet) |
| `play.svg` | Execute a confirmed command; Test in the rule tester |
| `stop.svg` | Stop a running command or activity (no screen uses it yet) |
| `link.svg` | Link / jump host reference (no screen uses it yet) |
| `split.svg` | Split view, two panes (no screen uses it yet) |
| `send.svg` | Send a chat message |

## App · status

| File | Intended use |
|---|---|
| `terminal.svg` | Terminal pane switch |
| `spark.svg` | AI pane switch; AI Provider settings |
| `chat.svg` | Chat; "Sessions & Data" settings |
| `notes.svg` | Server notes (no screen uses it yet) |
| `shield.svg` | Filter Rules; confirmation card header; Save rules |
| `sliders.svg` | Settings (header button, mobile menu) |
| `activity.svg` | Activities centre button (with running count); Diagnostics settings |
| `lock.svg` | Unlock / master password; elevated rights; hard blacklist |
| `key.svg` | Host keys (server form, Security) |
| `tag.svg` | Tag scope of a rule |
| `bolt.svg` | "Ran automatically" by a rule; "next time this runs automatically" hint |
| `clock.svg` | Interrupted / waiting activity |
| `warn.svg` | Warnings: failed attempts, failed activity, host key changed, quit while running |
| `info.svg` | Inline information notes; About settings |
| `mic.svg` | Voice input; speech model download; Voice input settings |
| `user.svg` | User / account (no screen uses it yet) |
| `plug.svg` | Connect; MCP Server settings |
| `fingerprint.svg` | Host key fingerprint (no screen uses it yet) |

## Not on the sheet, but used in the screens

These paths appear in the screens without being part of the sheet. Draw them with the same root
attributes if they are needed.

| Suggested name | Path `d` | Where |
|---|---|---|
| `chev-u` | `M6 15l6-6 6 6` | rule priority up, terminal key row ↑ |
| `chev-l` | `M15 6l-6 6 6 6` | mobile back button, terminal key row ← |
| `server-plain` | `M3 4h18v7H3z M3 13h18v7H3z M7 7.5h2 M7 16.5h2` | "Server" scope of a rule, Manage in the mobile menu |
| `display` | `M3 4h18v13H3z M8 21h8 M12 17v4` | "Display & Language" settings (mobile) |
| `grid` | `M3 3h18v18H3z M3 12h18 M12 3v18` | "Global" scope of a rule |
| `home` | `M3 11l9-8 9 8v10H3z M9 21v-6h6v6` | Home directory button in the file browser |
| `columns` | `M3 4h18v16H3z M9 4v16 M15 4v16` | Toggle the file browser detail panel |
