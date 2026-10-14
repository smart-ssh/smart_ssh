# Changelog

All notable changes to Smart SSH are documented here. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/).

Features that are only available in a paid edition are marked with
**(Pro)**.

## [Unreleased]

## [0.6.0] — 2026-10-09

### Highlights
- The local database is now fully encrypted, and all credentials move from the OS keychain into it.
- Optional master password with an unlock screen, as an alternative to the OS keychain.
- MCP requests run in their own session and tab per server and tool, with at most one pending confirmation.
- The AI can search the web and read pages, showing every search and the cited sources in the chat.
- Step-by-step connection log for connection tests and failed connections, per hop for jump hosts.
- Servers get an optional start directory and can be moved between groups by drag and drop.

### Added
- The local database is now **fully encrypted** (SQLCipher) — including server,
  host and user names, filter rules and provider headers.
- An existing database is converted automatically, with verification, on first
  start; if a step fails, the previous file is left unchanged.
- **All credentials** (passwords, keys, passphrases, certificates, sudo
  passwords, API keys, provider headers) move from the keychain into the
  encrypted database on first start. An old keychain entry is only deleted once
  the secret in the database is verifiably identical.
- **Master password** as an alternative to the keychain, e.g. for Linux without
  a Secret Service: it can be set up, changed and removed again in Settings;
  the section shows where the database key is stored.
- **Unlock screen at startup:** with a master password, nothing is reachable
  until you unlock — no server, no setting, no MCP server.
- After three failed attempts, the unlock screen offers “Start over”. Your
  existing data is lost in the process (the files are only renamed), and you
  are asked once more beforehand.
- Servers can have an optional start directory; the terminal and the file
  browser start there. If it is missing, both start in the home directory with
  a notice. AI commands still run in the home directory.
- Servers and groups can be moved to another group or to the top level by
  drag and drop, in the server list and in “Manage”.
- “+ Server” and “+ Group” create the new item in the selected group; in the
  server list, every group row has a “+” for a new server.
- The group picker in the forms shows the hierarchy as an indented path (e.g.
  “Prod / Web”).
- When answering, the AI can search the web and read web pages (text only, via
  the provider's web tools, currently Anthropic). The chat shows every search
  and page along with its sources; the “Web research” setting is on by default
  for each provider. Your computer opens no additional connection for this.
- “Test Connection” and connection errors in the server list offer an
  expandable step-by-step log (DNS, TCP, handshake, host key, login, per
  jump-host hop) with the failed step highlighted and a “Copy” button. It is
  not stored anywhere.
- Settings → Diagnostics shows where Smart SSH keeps its data (database, logs,
  host keys, key file, MCP settings), copyable and with “Open folder”.
- The MCP example configuration in Settings has a “Copy configuration” button
  and is now displayed in full.

### Changed
- **Important:** Older versions can no longer open the database after the
  switch and report it as “possibly corrupted” — this is not true. Do not
  restore a backup; use the new version instead.
- **Important:** Without the database key (or with a forgotten master
  password), the data cannot be recovered, not even from a backup. This is
  confirmed when you set up a master password.
- **Important:** The old, unencrypted file is removed; however, older copies
  may still exist in backups, snapshots or an earlier keychain backup.
- The database key is derived from the existing key in the keychain; no second
  entry is added.
- Without this key, Smart SSH no longer starts in a restricted mode but asks
  “Try again” or “Quit”; your data is not changed.
- If the key is missing or does not match, a new one is **never** created
  silently. If you deliberately start over, the old file is kept under a new
  name.
- Chat history, execution log, input history and summaries are now protected
  solely by the encrypted file. Old entries that can no longer be read are
  removed during the conversion; a notice states how many.
- The MCP token is now stored in the encrypted database instead of in plain
  text in `settings.json`; existing MCP clients keep working.
- Actions of an external tool via MCP run in their own session, with their own
  connection and their own tab per server and tool (“Claude Code @ web-01”).
  Your own tab, terminal and chat are left untouched; the MCP tab does not take
  focus and shows a pending confirmation as a badge.
- If the provider account has web search disabled, the chat shows a notice and
  sends requests without web research for the rest of the session, instead of
  failing on every message.
- The AI system prompt exists in German and English; it follows the UI
  language, as do the notices when long sessions are shortened.
- The AI second opinion and the check for injected instructions no longer
  trigger an unnecessary prompt because of quoted configuration lines (e.g.
  `PermitRootLogin yes`).
- If no AI provider is active yet, a newly created provider is set active
  right away. An already active provider is never replaced, even if the
  provider list was not loaded when saving.
- The SSH connection setup has its own time limits for each server in the
  chain: 10 s for connection and handshake, 60 s for login — enough for
  hardware keys requiring a touch or agent confirmation.
- If a host-key prompt times out in the background (e.g. for a connection
  started via MCP), it closes, and a notice says that the connection was not
  established and the key was not trusted.
- If a server's sudo password cannot be read from the credential store, a
  warning appears in the log (without the secret).
- Groups and servers in the Manage sidebar can be selected with the keyboard;
  empty column headers and the command field in the confirmation dialog now
  have names for screen readers.

### Fixed
- The host-key prompt now also appears while “Manage” or “Filter Rules” is
  open; previously, the connection there ran into a rejection without asking.
- A damaged or foreign key file no longer locks you out permanently: “Start
  over” is then offered before unlocking, but only if no password can possibly
  match anymore.
- If the key file cannot be read at the moment (permissions, read error, in
  use), nothing is changed and no “Start over” is offered.
- When changing the master password, the old password stays in effect if an
  error occurs; switching back to the keychain does not overwrite a foreign key
  without confirmation.
- While the app is locked, settings files and system services are also
  unreachable.
- Startup error messages name the state and the affected file correctly; an
  invisible prompt ends with a message instead of hanging.
- Smart SSH can no longer be started twice with the same data directory; a
  second start brings the open window to the front. Instances with different
  data directories run side by side.
- If startup fails because of a database from a newer version, no `-wal`/`-shm`
  files are left behind, and on Windows the file can be renamed immediately.
- A server whose login method this version cannot read (e.g. after a
  downgrade) no longer empties the server list: it appears as “unusable” with a
  reason and can be deleted.
- If a running SSH session breaks (command, shell or file browser), the app
  reports “The connection to the server was interrupted” instead of a
  misleading setup or SFTP channel error.
- The file browser of the local pseudo-server and local commands start in the
  home directory instead of the app's working directory.
- A drag-and-drop upload always uses the channel and the directory shown at the
  time of the drop, even right after elevated mode.
- Moving by drag and drop no longer discards unsaved input in an open server or
  group form.
- When the active tab is closed, one of your own tabs (otherwise the overview)
  becomes active next — never an MCP tab automatically.
- With the English UI, the file browser, chat notices, the note field and its
  suggestions, the session header and other dialogs no longer show German text.
- The window can be dragged by the title bar and zoomed or maximized by
  double-click even while a dialog is open and on the unlock screen.
- Technical input fields (host, user name, paths, patterns …) turn off
  autocorrect and automatic capitalization; `root` stays `root`.
- A resumed chat shows the real exit code again for command results instead of
  “—”.
- If the AI summary of a large note is truncated, the note suggestion clearly
  warns that it is incomplete.
- An error in an extension of the notice shown before the first connection only
  hides that part instead of the whole interface.
- If a command is too long or too deeply nested for the check, the confirmation
  dialog says exactly that instead of claiming a secret path.

### Security
- The filter engine detects more evasion attempts and requires confirmation for
  them despite an allow rule: invisible and special Unicode characters,
  homoglyphs, `$'\x..'` quoting, variables or globs as the command name,
  subshells and shell constructs, shells reading from standard input, and
  `eval`.
- Deny rules now also apply behind `eval`, here-strings (`bash <<<`), `-c` of
  further shells and shell variants (`ksh`, `fish`, `bash5`, `zsh-5.9` …), `-c`
  after other options, in later chain elements, behind wrappers with a path or
  different spelling, and behind `exec`/`builtin`.
- Unknown programs on `sh` that are invoked with `-c`/`-s` (e.g. `pwsh -c …`)
  require at least a confirmation.
- The risk indicator rates the code behind `-c` of every recognized shell (also
  behind `sudo`/`env`): `bash -c 'shutdown -h now'` now shows red.
- `trap` handlers and alias definitions always require confirmation, even with
  an allow rule; if the stored code matches a deny rule, it is blocked.
- The command length limit is now measured in bytes everywhere; long commands
  with multibyte characters no longer pass through an allow rule.
- MCP output does not reach the AI history of your own session, and your own
  chat does not reach the MCP session.
- In an MCP session, at most one action waits for confirmation; further ones
  are rejected immediately with a reference to the open confirmation.
- External tools (MCP) can open at most 4 sessions of their own per server;
  tool names are shortened to 64 characters in the tab and notification.
- If a tab with a pending confirmation is closed — also an MCP tab or after a
  reload — the action is rejected immediately.
- Command output, files read and server notes that an MCP client receives are
  redacted or fenced as untrusted, as with the built-in AI. Clients that parse
  the text literally will see a changed format.
- An empty MCP token is no longer accepted and is replaced at startup; tools
  set up with it need the new token from Settings.
- Linux: the MCP settings file `settings.json` now actually gets the
  permissions `0600` (existing ones on the next save of an MCP setting).
- Web content from web research is treated as untrusted: it is redacted,
  fenced, and turns on escalation after being read.
- `ssh_config` import: imported tags that match a tag allow rule (also via
  pattern) are deselected by default in the preview — unless they also match a
  deny rule.
- The file browser only reads local files that were chosen for the session or
  dragged into the window; other paths (also via `..` or symlinks) are
  rejected.
- Copies of server files left behind by “Open locally” are removed on the next
  start instead of remaining in the cache in plain text.

## [0.5.2] — 2026-10-01

### Added
- Server profiles can be exported to an OpenSSH `ssh_config` — for switching to
  another tool or simply as a readable backup. The generated file is accepted
  by `ssh` itself (verified with `ssh -G`). After the export, a message shows
  the path, any renamed server names (if a name did not yield a valid `Host`
  alias) and the line to include the file in your own `~/.ssh/config`.
- What cannot be mapped — groups, tags, notes, filter rules, security settings
  and the login method — does not disappear silently: for every affected
  server, a comment sits directly above its entry in the file.
- Server profiles can be imported from an OpenSSH `ssh_config`. A preview first
  shows in full what would be created — fields with their origin, the files
  read with their full path, the resulting folder structure, name conflicts and
  every line that is **not** imported. Individual entries can be deselected;
  if you cancel, nothing is created. A second import of the same file creates
  no duplicates.
- `Include` is read as well, and every included file becomes a subfolder.
  `ProxyJump` is mapped to the jump-host chain; a chain that loops back on
  itself is rejected at import time instead of only being noticed when
  connecting.
- For an `IdentityFile` line, the user decides what should happen: adopt the
  path as a key file (the default — the file is not opened in the process),
  read the key and store it in the keychain, or skip the line. If a path points
  to nothing, only that one entry falls back; the import continues and says
  why.
- New setting “Always ask on red risk” in the risk classifier section, **on by
  default**. When on, every AI suggestion that the risk indicator rates red on
  the server or the data axis always requires a confirmation — even against an
  allow rule. Takes effect from the next connection.
- The About section of Settings now has a “Third-party licenses” entry that
  shows the license texts of all Rust and npm dependencies, of build tools with
  shipped code and of the bundled fonts in a scrollable view. The list is
  regenerated with every release build and readable offline; a development
  build shows a notice instead that it is only included in release builds.

### Changed
- “Open locally” in the file browser rejects files over 50 MB and points to
  “Download”. No local copy is created in the process.
- The automatically generated title of a chat session is redacted before it is
  stored. The title is the only part of a session stored in plain text in the
  database — if the AI had taken a password from the conversation into the
  title, it was previously readable there. If the title then consists only of
  placeholders, no title is stored at all.
- When the AI proposes a note, its proposal is redacted **before** it appears
  in the comparison dialog — in the chat, when disconnecting, and for a
  proposal via MCP. The same version is displayed, confirmed and stored. If the
  proposal consisted exclusively of detected credentials, no proposal is
  created, and the app says so.
- “Add to note” is unchanged: what you yourself take from the chat into a note
  is not touched.

### Fixed
- When switching the login method, the previous credentials were lost if saving
  failed. If you switched a server from password to “Certificate” and left the
  fields empty, you correctly got an error message — but the password had been
  removed from the keychain afterwards, while the server was still stored as a
  password login. It could no longer connect, and nothing indicated why. The
  same applied if the keychain, the sudo password or the saving itself failed.
  Credentials of the previous login method are now only removed after the new
  ones have been saved successfully.
- A self-hosted AI server with a small context window previously returned an
  error instead of an answer when the response budget was too large. The app
  now recognizes this case from the server's error message and repeats the
  request once with a smaller budget.
- The error message “Context too large for the AI provider” now names the
  remedy: lower the “Max. response length (tokens)” setting or start a new
  chat.
- With Anthropic, the automatic retry after a truncated response no longer
  goes below a “Max. response length” you set yourself.
- The `ssh_config` import on Windows now follows every `Include`. Previously,
  every included file was silently skipped because the prefix that Windows
  prepends to canonical paths was mistaken for a wildcard character.
- If access to the system keychain fails although it was reachable at startup
  — because it is locked or access was denied — an explanatory message now
  appears in the app's language instead of the raw English text of the keychain
  library (“No default store has been set …”). This affects saving and reading
  API keys, server passwords, passphrases and sudo passwords.
- When connecting and when testing a connection, such an error is no longer
  shown as “Network error” or “Credentials could not be resolved” but names the
  keychain as the cause. This also applies to a chain via jump hosts.
- A single failed access no longer counts as “keychain unavailable”. The
  message no longer claims that, and the next access succeeds again as soon as
  the keychain responds. If the entry is simply missing from the keychain, the
  previous message “no entry found” remains — that is not a keychain failure.

### Security
- No export ever writes a password, a passphrase or key contents — only, for
  the matching login method, the path to a key file. A server whose key is in
  the keychain does not silently lose any information: the comment above its
  entry says that the key is missing and why.
- The export rejects your own `~/.ssh/config` as a target — even if it is
  explicitly chosen in the save dialog and an overwrite is confirmed. That file
  is never touched.
- The local pseudo-server never appears in the exported file.
- The import by itself opens only `ssh_config` files. A key file is only read
  if you explicitly ask for it, only when confirming, and only the files that
  the preview named beforehand.
- No content from a file that was read reaches the screen: an included file
  that is not an `ssh_config` is reported with its path and skipped; for a line
  that was not imported, only the name of the directive with its line number
  appears, never its value.
- An imported file cannot set any security setting of a profile — escalation
  based on server content and the AI check always stay at the product
  defaults.
- Excessively large, deeply nested or self-including configurations lead to an
  understandable message instead of a hanging or crashing program.
- If you turn off the elevated file browser mode during a running action (e.g.
  recursive delete or changing permissions), that action now aborts
  immediately.
- The 256 KB limit of “Copy file content” in the file browser now also holds
  if the file grows between the size check and reading, or if its size cannot
  be determined.
- The TLS library for connections to AI providers (`rustls`) is updated to
  0.23.45, fixing RUSTSEC-2026-0285 (TLS 1.3 handshake messages were accepted
  across encryption levels).
- If the optional AI second opinion raises the data risk to red afterwards —
  that is, only after a suggestion was already announced as “runs
  automatically” —, the action no longer runs automatically but requires a
  click like any other confirmation.
- At the default level, the log file no longer contains any content: no
  command, no command output, no chat, note or prompt text, no tool arguments
  and no error message from an AI or SSH connection. Only content-free details
  remain — IDs, decision, rule that applied, lengths, exit code, error codes.
  This holds regardless of whether a password is written in a form that the
  automatic suppression recognizes. If you need the contents for
  troubleshooting, start the app with `RUST_LOG=debug`; there, they still pass
  through the suppression. The only exception remains an AI provider's response
  to an error — it is needed to detect a misconfiguration, and appears in the
  log suppressed and truncated to 512 characters.
- Passwords passed as an argument to a command-line program are now redacted
  before a command or its output goes into the AI context, the log, the chat or
  to an MCP client: `mysql`/`mariadb`/`mysqldump`/`mysqladmin` with an attached
  `-p`, `sshpass -p`, `curl -u user:password`, `htpasswd -b`, `redis-cli -a`,
  `smbclient -U user%password`, and `openssl -pass pass:…` and `openssl -k`.
  Program, switch and user name remain readable — only the value disappears.
- Additionally detected: Twilio API keys, SendGrid keys, the keys in Azure
  connection strings (`AccountKey=`, `SharedAccessKey=`), Argon2 and phpass
  password hashes, and JSON Web Tokens without a `Bearer` prefix.
- The prompt for an unknown or changed host key can now be operated entirely
  with the keyboard: on opening, focus is on the reject button, Tab no longer
  leaves the dialog, and Escape rejects the connection (never “trust”). An
  Enter key held down from a previous dialog no longer decides anything
  automatically. Screen readers now announce the dialog as modal.

## [0.5.1] — 2026-09-25

### Added
- File browser with elevated rights: an “Elevated rights” switch opens the
  file browser as root (or another user) by starting the SFTP server through
  `sudo -n` — like WinSCP. This requires a passwordless sudo rule for
  `sftp-server` on the server; the app never asks for a sudo password. If the
  rule is missing, the app shows the matching sudoers line to copy, along with
  a note on what it means. The mode is never on by default, is not saved, is
  impossible to miss thanks to a frame and a banner, and confirmations name it
  (“Delete … as root?”). The AI and MCP clients have no access to this
  channel. The app detects the path to `sftp-server` automatically; it can be
  overridden in the server profile under “Advanced”.
- Every file browser action now reports its result: success is shown briefly
  (“datei.conf” downloaded to ~/Downloads”, with “Show in Finder”), errors
  stay visible with the reason. Folder, recursive and multi-item actions
  summarize the result in a single message with a count.
- The About dialog now also shows whether a dev or a release build is running
  (e.g. “0.5.0 (d887019) · Community · Dev build”); the title bar marks dev
  builds with “· Dev”. Both use separate data directories — so it is
  immediately clear which data you are looking at. “Copy” takes the whole line
  for bug reports.
- The chat can now be used at any time, even while the AI is working: a message
  written in the meantime interrupts nothing, but is queued and sent to the AI
  with the next request (or right afterwards, if no further round follows) —
  handy for making corrections without waiting for the answer to finish. The
  “Stop” button appears for every running answer and immediately aborts the
  running request to the AI provider, instead of only preventing further
  automatic rounds. A command that is currently running on the server runs to
  completion; an open confirmation dialog stays open.
- The AI is now instructed to avoid reading passwords, private keys, tokens and
  similar secrets where possible: existence is checked via metadata (e.g. file
  size), and such files are copied directly on the server (`cp`, pipe) instead
  of being read and rewritten — so secrets never end up in the chat history in
  the first place.
- The app now uses Anthropic's prompt caching for the system prompt, tool
  definitions and server note — these barely change within a session, but so
  far were re-sent with **every** request and counted fully against the
  input-token rate limit. For most Claude models, cached tokens do **not**
  count against the rate limit, so this noticeably reduces costs and
  **rate-limit load** (Anthropic gives 80% cache hit rate = effectively 5× more
  input throughput per minute as an example). The `uname` system banner of the
  connected server, previously embedded directly in the system prompt, moves
  into a separate message marked like command output (it could otherwise have
  invalidated the cache in every session).
- New “Create diagnostics package” button in Settings (Diagnostics) next to
  “Open log path”: creates a single, redacted text package (version + build
  hash, operating system/architecture, data paths, configured AI provider
  **types**, server **count**, the last up to 500 log lines) to pass on to
  support or attach to a GitHub issue. Contains **no** credentials, passwords,
  host keys, server addresses or note/chat contents; embedded log lines
  additionally go through the same redaction as elsewhere in the app. It is
  shown as a preview before saving — nothing is saved or sent automatically.
- The app now reads the AI provider's rate-limit headers (Anthropic:
  `anthropic-ratelimit-{requests,input-tokens,output-tokens,tokens}-
  {remaining,reset}`) and throttles proactively instead of running blindly into
  the limit: if the known remaining budget drops below ~15% (or an estimated
  request would exceed the remaining token budget), the app waits until the
  known reset time before sending the request — the chat shows “Waiting for AI
  budget — next attempt in Xs…” in the meantime, and the request is sent
  automatically afterwards. All AI calls (main chat, optional second
  opinion/injection check, summary, auto-title, note suggestion/shortening)
  that use the same API key/endpoint/model share one common budget. Providers
  without these headers (e.g. a local Ollama endpoint) remain unchanged — no
  proactive waiting, only the existing reactive retry after a 429.
- The app now shows an understandable message on startup errors instead of a
  silent crash: if the database cannot be opened (e.g. created by a newer
  program version, corrupted, or the data directory is not writable) or the
  host key store cannot be loaded, a dialog with the cause now appears instead
  of an unexplained crash before the first window. If the system keychain is
  locked or unavailable at startup, the app still starts as before (chat
  history and note summaries are then disabled for this program run), but now
  also shows this visibly instead of only logging it silently.
- If a server's saved note is very large, the app now offers to summarize it
  with the AI (or shorten it yourself) when you disconnect — as with every AI
  note suggestion, the summary is only actually saved after confirmation in the
  diff comparison (old/new), never automatically. This suggestion now also
  appears for the local pseudo-server (localhost).
- When editing a very large note, the note editor now shows an unobtrusive
  hint with a direct link to have it summarized by the AI right away (same
  confirmation flow as the suggestion on disconnect).

- The AI's maximum response length now depends on the model instead of being
  fixed at ~4000 tokens (up to 128K for current Claude models) — a limit that
  was too tight occasionally caused the response to break off in the middle of
  a sentence/command. If a response still breaks off at the length limit, the
  text so far stays visible and a “Continue” button resumes the response
  exactly where it ended. In addition, the AI provider form has an optional
  “Max. response length (tokens)” field under “Advanced” for providers with an
  unknown output maximum (e.g. a self-hosted model) — the default is still
  “Automatic”.
- If you open the AI provider settings without a configured Ollama provider,
  the app searches once for a locally running Ollama (`127.0.0.1:11434`) and
  offers to adopt it — including model selection. If no Ollama is found, a
  compact guide shows how to install it. No network call without this action or
  a click on “Search again”.
- If no server of your own has been created yet, the server list now actively
  leads to the creation dialog with “Create first server”, instead of only
  showing a developer text.
- When creating a server with key login, a hint recommends Ed25519 keys
  (`ssh-keygen -t ed25519`) — RSA remains supported.
- For Ollama, the API key is no longer required in the form.
- The diagnostics view shows a line “System keychain: available / unavailable
  (reason)” along with the next step — available for lookup even after the
  startup dialog has already been dismissed.
- The “Load models” button is now also available for Anthropic providers — it
  was missing there entirely before.
- When creating and editing a server, a fifth login method is now available
  alongside password, private key, SSH agent and certificate: “Key file” — a
  path instead of content stored in the keychain, like `IdentityFile` in
  `ssh_config`. Before saving, the interface shows whether the file exists,
  whether the permissions are right, whether it looks like an OpenSSH key and
  whether it is encrypted. The server list and details show the path the login
  depends on.
- A button “Move to keychain” stores the content of such a key file in the
  system keychain once and switches the login method to “Private key” — the
  original file is left unchanged. Beforehand, a dialog shows which file will
  be read and what changes.

### Changed
- Smart SSH is now open source under the **Apache License 2.0** (previously
  Functional Source License 1.1, FSL-1.1-MIT). This applies retroactively: all
  already published versions may also be used under the Apache License 2.0.
- The system prompt now explicitly instructs the AI to actually propose an
  announced command via `suggest_command`, instead of only announcing it in
  running text and stopping there — an observed pattern in which automatic
  continuation stalled for no apparent reason. Short explanations before a
  command remain explicitly welcome.
- Error messages on the path “set up provider → create server → test
  connection → connect” now consistently name the cause **and** the next step,
  in German and English — among others for a wrong API key, unknown model,
  unreachable local AI service, rejected SSH connection, unknown host,
  unreachable host and rejected/not confirmed in time host key.
- Establishing the SSH connection now aborts after 10 seconds without a
  response with a clear message, instead of waiting for the operating system
  for minutes.
- The startup dialogs (database, host key and keychain errors) now speak German
  or English, depending on `LC_ALL`/`LC_MESSAGES`/`LANG`. The default remains
  German if no usable language setting is set.
- A mistyped model name for Anthropic now correctly shows “Model not found”
  instead of “Provider unreachable”.
- The placeholder text in the Manage view without a selected group/server is
  now also translated into English (previously it appeared in German in the
  English interface as well).
- API key, server password, passphrase and sudo password are now trimmed at
  the edges by the same rule. Only the edges: what is inside a value stays
  untouched — even an unusual character that might belong there.
- The same rule applies to a self-set path to `sftp-server` (elevated file
  browser): an invisible character at the edge is removed instead of rejecting
  the path as invalid. Such a character **inside** the path still leads to
  rejection, unchanged.
- As a consequence, the check for injected instructions can now trigger more
  often — for example when the model quotes an ordinary configuration line
  such as `PermitRootLogin yes` in its reasoning. The next action then
  requires a confirmation instead of running automatically. This is the safe
  direction: one question too many is visible and settled with a click, a
  swallowed warning is not.
- An already saved rule with such a pattern — for example from an older
  program version — is visibly marked in the rule list, with a note that the
  pattern is invalid and the location within it. Editing and deleting remain
  possible; saving then requires a valid pattern. In addition, such a rule is
  noted in the log on every evaluation (with the rule ID, never with the
  command).
- If the rule form reports an invalid pattern, it now shows an understandable
  sentence in the selected language and, below it, the exact location in the
  pattern — previously only the English text of the underlying library.
- The priority of a rule marked this way can no longer be moved with the arrow
  keys; the arrows are disabled and state the reason.
- The suggestion to summarize a very large note with the AI now has a ✕ button
  to close it and a “Later” button, after which it does not appear again for
  this server until the app is next started.
- This suggestion and the hint when editing a large note in the note editor
  now only appear from 10,000 characters instead of the previous 8,000 bytes.
- AI responses without text no longer disappear silently. If a reasoning
  model's response breaks off empty because of the output limit, it is
  automatically retried once with more budget; if a response to a message
  still ends up with no text and no suggestion at all, the chat now shows a
  hint with the tip to raise the output limit in the provider settings.

### Fixed
- “I'll do it myself” in the shortening suggestion now scrolls straight to the
  note field and focuses it, instead of only opening the server form.
- An AI provider (or an intermediate proxy) that answered an error status
  immediately but then left the response body hanging could block a chat turn
  indefinitely and without any error message (chat no longer responds, no log
  entry). The call now aborts after 90 seconds at the latest with a visible
  error message.
- A very long chat session or a very large server note could make an AI request
  so large that it exceeded the model's context window — the chat then hung
  without a response/error message. Before every request, the app now
  estimates the actual size and shortens automatically if needed: older
  conversation rounds are replaced by a continuously updated summary created
  by the AI (if this summarization call fails, the app truncates instead as
  before — the chat remains functional in any case), then, if necessary,
  overly long individual command outputs and finally the note are shortened —
  the saved note and the complete session history remain untouched.
- An AI provider call could hang indefinitely without any error message after
  a brief network dropout (the chat then simply stopped responding). The
  connection to the AI provider now uses active TCP keepalive, so that a
  connection that “silently died” through the dropout is detected faster.
- Linux without a system keychain: instead of the English library text “No
  default store has been set, so cannot search or create entries”, Smart SSH
  now names the actual state, the features blocked by it and the package that
  fixes it — depending on whether no Secret Service provider is running
  (`sudo apt install gnome-keyring`, also under KDE), no D-Bus session bus is
  reachable (`sudo apt install dbus-user-session`) or the keychain is merely
  locked. A locked keychain never leads to an installation suggestion. Users
  who use KWallet or KeePassXC anyway are told that their Secret Service
  integration must be running or enabled — installing additional packages
  alone is not enough there.
- The startup dialog used to claim that without a keychain everything except
  the chat history works normally. That was not true: without a keychain, AI
  providers, server passwords, passphrases and sudo passwords can neither be
  saved nor read. The dialog now lists what is actually blocked — and what
  still works (SSH agent, keys without a passphrase).
- The server form reported “no sudo password stored” when the keychain could
  not respond at all. It now shows a neutral state instead of claiming
  something it cannot know.
- “Remove stored sudo password” reported success even if the keychain could not
  delete the entry — the password would have continued to be fed in at the
  next `sudo`. The operation now visibly fails.
- Deleting a server still works even when the keychain is stuck. New: Smart SSH
  then states explicitly which entries were left behind in the keychain, that
  they are now orphaned there and how to remove them by hand.
- “Test connection” now evaluates empty and pasted credentials exactly like
  “Save”. When creating a new server and leaving the password, key or
  certificate field empty, the test reports the error message instead of
  logging in with an empty value — on a server that allows empty passwords, it
  could previously report success even though the same server could not be
  saved afterwards. When editing, an empty field still means “use the stored
  credential”. The message is the same as when saving (and in the selected
  language), such as “Password is required”.
- “Test connection” now treats the passphrase of a key file or private key
  exactly like “Save”. Previously, the same pasted value could make the
  connection test fail and then still be saved correctly — so the test said
  something different from the server that resulted from it.
- A pasted API key or password now also works when an invisible character
  ended up at the edge while copying — such as a BOM from a text file or a
  zero-width space from a web page. Previously, such a value was saved
  unchanged, the login failed, and the key still looked correct in the
  interface.
- A filter rule whose pattern cannot be compiled is now rejected on creation
  and on change, instead of being saved and then sitting uselessly in the
  list. Previously this applied to every action — including a deny or confirm
  rule that silently did nothing as a result. This applies to the rule form and
  to the quick rule from the confirmation dialog.
- The confirmation dialog no longer suggests a quick rule whose pattern cannot
  be compiled. Previously, such a suggestion arose from a command with a
  bracket in an argument and could be selected, but did not result in an
  effective rule.
- Credentials in a connection URL are now fully redacted even if the password
  or user name contains an unencoded `@` (such as
  `postgres://app:Xy9@kLm2@db/prod` or the `user@tenant` notation required by
  several hosted databases). This applies to everything sent to the AI model
  and to everything stored in the conversation history.
- A password parameter in the query string of a connection URL without a path
  (`redis://cache:6379?password=…`) is likewise fully redacted.
- “Test credentials” for an Anthropic provider wrongly failed with “Provider
  unreachable” although the key was valid — the probe sent no system prompt,
  and Anthropic rejects an empty system text block with a caching marker.

### Security
- Glob `*` in path-shaped allow/deny rules (patterns with a file-path-like
  argument, e.g. `Allow: cat /var/log/*`) no longer crosses directory
  boundaries, not even when hidden via shell quoting/escaping (`\..`, `".."`,
  `'..'`, brace/bracket tricks). Previously, a harmlessly granted allow rule
  such as `cat /var/log/*` could also automatically allow
  `cat /var/log/../../../etc/shadow`, because the glob `*` went across `/`
  boundaries — the filter engine thereby “allowed” something that was never
  meant to be approved. **Behavior change**: an existing path-shaped rule with
  a single `*` now covers only one directory level (e.g. `/etc/*` still
  matches `/etc/passwd`, but no longer `/etc/nginx/nginx.conf`). For
  **deny/confirm** rules needing multi-level protection, use `**`
  (`/etc/**`) — existing deny rules never lose protection scope in the process
  (old and new matching both apply). For **allow** rules, `**` is NOT
  recommended: in an allow rule, `**` can also allow additional, unintended
  command parts beyond the argument actually meant — for multi-level allow
  needs, create several specific, single-level allow rules instead. Command
  argument globs without path character (e.g. via a URL) are unchanged.
- A command proposed by the AI whose response was cut off mid-proposal by the
  length limit could, in a rare case, still get through to the confirmation
  dialog — with content that by chance looked complete but was in fact
  truncated. Such a truncated proposal is now never shown or executed again;
  the app instead retries once automatically with more room before, in the
  exceptional case, showing a visible error instead of an incomplete command.
- API keys of AI providers and code hosts are now redacted even without a
  recognizable keyword in front of them, before text goes to the AI or is
  stored: Anthropic (`sk-ant-…`), OpenAI (`sk-proj-…` and others), OpenRouter
  (`sk-or-v1-…`), GitLab (`glpat-…`, `glrt-…`, `gldt-…`), Hugging Face
  (`hf_…`), Groq (`gsk_…`) and xAI (`xai-…`). The same applies to headers such
  as `x-api-key`, `api-key`, `x-goog-api-key`, `Authorization: Basic …` and
  `Authorization: Token …`, passwords in arbitrary addresses of the form
  `scheme://user:password@host` (the user name stays readable) and the
  credentials from `.netrc`, `.pgpass`, Docker and kubeconfig files.
- If an AI suggestion reads a typical secrets file (private SSH and host keys,
  `*.pem`, `*.key`, `*.p12`/`*.pfx`/`*.jks`, `.env`/`.envrc`, `/etc/shadow`,
  `~/.aws/credentials`, `~/.docker/config.json`, `~/.kube/config`, `.netrc`,
  `.pgpass`, `.git-credentials`, `~/.gnupg`, `/etc/ssl/private`, `~/.npmrc`,
  `~/.pypirc`, `~/.my.cnf`, shell histories, process environments,
  `wp-config.php`, Kubernetes admin configurations and others), the app now
  always asks — even if an allow rule would otherwise approve the command
  automatically. This also applies to requests from external MCP clients and
  to reading a file directly, and it cannot be bypassed via path tricks,
  directory changes, wildcards or prepended commands. **Behavior change:**
  recursive searching (`grep -r`, `rg`) and reading via `find -exec`/`xargs`
  also always asks, because the content cannot be checked beforehand.
- If the AI proposes a call to `sftp-server` (for example via `sudo -n`), the
  risk assessment now shows red and the app always asks — even if an allow
  rule would otherwise approve the command: with the sudo rule for the
  elevated file browser, this means file access with root rights.
- If you reject one of several actions of an AI response (or a rule blocks it),
  the remaining actions of the same response no longer run automatically but
  ask as well — they could build on the rejected one.
- The confirmation dialog for writing a file now says up front if the app would
  write with the stored sudo password when permissions are missing. Previously
  this was only visible in the result.
- A host key dialog that is never answered (e.g. because the window was
  reloaded) no longer makes the connection setup wait forever. After one hour
  the app aborts and the prompt counts as rejected; a host key is never trusted
  through the passage of time. “Load models” and the attestation query also
  no longer hang on a provider that does not respond, but report an error
  after 90 seconds.
- The AI second opinion on data risk and the check for injected instructions
  now read their verdict reliably from the response: if a response contains
  several verdict words, the warning one counts. Previously the one named first
  counted — a response that quoted the checked text and contained a “no”/
  “none” in it could thereby swallow a warning given afterwards. For the check
  for injected instructions, this could be exploited deliberately, because the
  checked text there comes from an untrusted source. A warning can no longer
  get lost through word order.

## [0.5.0] — 2026-09-11

### Added
- The chat input field now allows multi-line input: Enter sends, Shift+Enter
  inserts a line break, and the field grows with its content up to a maximum
  height and then scrolls internally.
- Chat: "Add to note"/"Export as Markdown" are still available on every
  answer (also reachable by keyboard), but now appear only on hover/keyboard
  focus instead of being permanently visible — an export or note transfer that
  is running, has failed or has just succeeded always stays visible,
  regardless of hover/focus.
- Settings: the AI provider section has been reworked — configured providers,
  the optional risk second opinion and the form for a new provider now sit in
  clearly outlined, grouped cards instead of a low-contrast, unstructured
  list; all form fields have a visible focus outline, and the format hint for
  the API key and the test result appear as separate, clearly recognizable
  notice boxes.

### Fixed
- Chat input field: pressing Enter during an ongoing IME composition (e.g.
  candidate selection with East Asian input, accent input via dead keys) no
  longer sends an unfinished input prematurely.
- Settings: some categories ("Display & Language", "Sessions & Data", "MCP
  Server") showed their title twice (once from the settings navigation, once
  as a heading in the content).
- Settings: the navigation entries "Sessions & Data" and "MCP Server" were
  always shown in German regardless of the UI language — they now appear
  translated into English when the UI language is English.

### Security
- Command output containing Unix password hashes (e.g. `/etc/shadow`, Apache
  `.htpasswd`) was previously passed on unredacted to the AI provider or a
  connected MCP client if the user let the underlying command run (after the
  usual confirmation dialog) — the redaction simply did not know their hash
  format. It now detects common crypt hash formats (MD5, SHA-256, SHA-512,
  yescrypt, scrypt and bcrypt hashes) and replaces only the hash part; the
  user name and other fields remain readable.
- For the same reason, DB connection strings (Postgres/MySQL/MariaDB/MongoDB/
  Redis/AMQP, password between `:` and `@`) as well as provider tokens with a
  distinctive prefix (Slack, Stripe, Google API, npm) were previously passed
  on unredacted. These are now detected as well — for connection strings only
  the password segment is replaced; scheme/user name/host/database remain
  readable.

## [0.4.6] — 2026-09-10

### Added
- File manager: column widths (Name, Size, Permissions, Modified) can now be
  adjusted by dragging (minimum widths; Name remains the flexible remainder).
- The split between the AI area and the SSH/SFTP area (terminal/file manager)
  can be adjusted by dragging the divider (minimum sizes for both areas,
  fallback to the default split in very small windows). Both settings survive
  a restart.
- File manager: right-click now opens the same context menu as the three-dot
  icon, with new actions — download directly to the default download
  directory or via dialog to a chosen location (also for folders,
  recursively), copy file content, copy path, properties (size, permissions
  numeric + symbolic, owner/group, modification date) and refresh.
- File manager: actions that modify the server — edit permissions (chmod,
  checkbox matrix + numeric, optionally recursive for folders), delete now
  also for folders (recursive, with the file/folder count in the
  confirmation), rename with collision warning, move via cut/paste (also with
  collision warning) and upload with a diff preview when overwriting an
  existing file. No confirmation except for irreversible/overwriting actions
  — manual file browser actions (like the terminal) do not pass through the
  filter engine.
- Settings: new category "Files" — set a local default program per file
  extension (fallback: operating system default). Purely local, with no
  server involvement.
- File manager: "Open locally…" downloads a file to a controlled temp
  directory, opens it with the configured (or the operating system default)
  program, and automatically offers the upload after a local change is
  detected — including a diff preview and a warning if the file on the server
  has also changed since the download. The editing copy is cleaned up when
  editing ends or, at the latest, when the connection is closed.
- File diff previews (note/file write operations, file overwrite on upload)
  now show the line number before each added or removed line.

### Fixed
- File manager: the three-dot menu on an entry often could not be opened, or
  immediately closed a menu of another entry that had just been opened —
  caused by an internal "click outside closes" mechanism that collided with
  every further click.

## [0.4.5] — 2026-09-09

### Fixed
- **Critical regression from 0.4.4 fixed**: 0.4.4 no longer started at all on
  Windows (`Migrate(VersionMismatch(1))` crash on every start, confirmed by a
  tester log). The cause was a side effect of the CI fix in 0.4.4 itself
  (`.gitattributes`) — as a result, the Windows CI checked out the SQL
  migration files with different line endings than in 0.4.1–0.4.3, which
  changed the checksum of the first migration embedded at compile time and
  made it invalid against the existing databases of existing Windows
  installations. Narrowed down to what was actually affected (Rust source
  files) — SQL migrations keep their original checkout behavior.

## [0.4.4] — 2026-09-09

### Fixed
- Title bar buttons on Windows/Linux were visible the whole time, but with
  very poor contrast (barely visible gray icons, only "Close" visibly reacted
  to hover): `tauri-plugin-decoration` by default tints its controls for a
  **light** title bar and only switches to light icons when the operating
  system is set to dark mode — but our app is always dark, regardless of the
  Windows theme. Now enforces the dark color scheme unconditionally —
  confirmed fixed on Windows 11.

## [0.4.3] — 2026-09-09

### Fixed
- Two CSS variables required by `tauri-plugin-decoration` according to its own
  documentation (`--tauri-plugin-decoration-titlebar-height`/`-z-index`) were
  never set — now set. Ultimately this was not the cause of the reported
  visibility problems (see 0.4.4), but a correct addition in its own right
  according to the plugin documentation.

## [0.4.2] — 2026-09-09

### Added
- Version + commit hash (`0.4.2 (a5b3e01)`) are now visible at a glance: in
  the first log line, copyable in a new "About" category of the settings, and
  (only during the 0.x test phase, easy to switch off) additionally in the
  title bar — identifies a bug report/log unambiguously, even when several
  builds carry the same version.

## [0.4.1] — 2026-09-09

### Added
- Settings restructured: navigation on the left, content on the right
  (instead of one long, unstructured list) — categories for AI providers,
  Display & Language, Diagnostics, Sessions & Data and MCP Server.
- AI provider form: immediate, purely local hint if an entered API key does
  not match the expected format of the selected provider (only a hint, never
  blocks saving).
- AI provider form: "Test credentials" button checks the credentials just
  entered, not yet saved, with a real mini request and shows whether they are
  valid, authentication fails, or the provider is unreachable.

### Fixed
- A pasted API key, server password/sudo password or key passphrase with a
  trailing line break/space (e.g. from a copy-paste on Windows) is now trimmed
  on saving, instead of letting authentication fail with "Invalid
  credentials".
- Windows: the title bar now falls back cleanly to the full native title bar
  (including working minimize/maximize/close controls and window dragging) on
  an activation error, instead of getting stuck in a broken intermediate
  state.
- A rate limit (HTTP 429) reported by the AI provider — previously the most
  common reason why the AI stopped answering in the middle of a session
  without any message — is now retried automatically with backoff and
  `Retry-After` handling; the multiple AI requests per message (main answer,
  risk second opinion, injection check) are spread out over time instead of
  being sent as a burst. If it still fails, the chat now shows a dedicated,
  actionable message ("please wait a moment and send again") instead of the
  generic "Check provider configuration" message.

### Security
- When an AI provider fails (wrong API key, rate limit, network error, model
  not found), the provider's error response is now written to the log in
  redacted form, in addition to the existing UI message — makes diagnosis
  easier without ever revealing a secret.

## [0.4.0] — 2026-09-07

First public test version.

### Added
- Word export as the first Pro module **(Pro)**.
- License entry UI with live activation **(Pro)**.

### Security
- Resource caps against hostile/faulty servers: an output cap that previously
  only took effect after complete buffering now limits already during
  streaming; an explicit recursion cap against nested command substitution.
- Integrity of the fencing markers for untrusted content in the AI context
  secured.

### Fixed
- Hardening round for the test phase: orphaned keychain entries after a failed
  server creation are now reliably rolled back, error messages (wrong API key,
  host unreachable, provider not started, …) now name the next step instead
  of raw technical details, and a crash at startup is now guaranteed to end up
  in the log file instead of disappearing without a trace.

## [0.3.0] — Initial Early Access

First coherent version: SSH client with AI copilot and filter/policy engine
with a confirmation workflow for every suggested command, server/group/rule/
note management, MCP server integration, persistent chat sessions, risk
indicators, multi-provider AI integration (Anthropic, OpenAI, Ollama, generic
OpenAI-compatible) with redaction of sensitive content, macOS build signed
and notarized.
