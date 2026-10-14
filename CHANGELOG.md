# Changelog

Alle nennenswerten Änderungen an Smart SSH werden hier dokumentiert. Das
Format folgt [Keep a Changelog](https://keepachangelog.com/de/1.1.0/),
dieses Projekt hält sich an [Semantic Versioning](https://semver.org/lang/de/).

Funktionen, die nur in einer kostenpflichtigen Edition verfügbar sind,
sind mit **(Pro)** markiert.

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
- Dateibrowser mit erhöhten Rechten: Ein Umschalter „Erhöhte Rechte“ öffnet
  den Dateibrowser als root (oder einen anderen Nutzer), indem der
  SFTP-Server per `sudo -n` gestartet wird — wie bei WinSCP. Voraussetzung
  ist eine sudo-Regel ohne Passwort für `sftp-server` auf dem Server; die
  App fragt nie nach einem sudo-Passwort. Fehlt die Regel, zeigt die App
  die passende sudoers-Zeile zum Kopieren samt Hinweis, was sie bedeutet.
  Der Modus ist nie standardmäßig an, wird nicht gespeichert, ist durch
  Rahmen und Banner unübersehbar, und Bestätigungen nennen ihn („…als root
  löschen?“). KI und MCP-Clients haben auf diesen Kanal keinen Zugriff.
  Den Pfad zu `sftp-server` erkennt die App automatisch; im Server-Profil
  lässt er sich unter „Erweitert“ überschreiben.
- Jede Dateibrowser-Aktion meldet jetzt ihr Ergebnis: Erfolg kurz
  eingeblendet („„datei.conf“ heruntergeladen nach ~/Downloads“, mit „Im
  Finder zeigen“), Fehler bleiben mit Grund stehen. Ordner-, rekursive und
  Mehrfach-Aktionen fassen das Ergebnis in einer Meldung mit Anzahl
  zusammen.
- Der Über-Dialog zeigt jetzt auch, ob ein Dev- oder Release-Build läuft
  (z. B. „0.5.0 (d887019) · Community · Dev-Build“); die Titelzeile
  markiert Dev-Builds mit „· Dev“. Beide nutzen getrennte
  Datenverzeichnisse — so ist sofort klar, welche Daten man gerade sieht.
  „Kopieren“ übernimmt die ganze Zeile für Fehlerberichte.
- Der Chat ist jetzt jederzeit bedienbar, auch während die KI arbeitet:
  Eine währenddessen geschriebene Nachricht unterbricht nichts, sondern
  wird eingereiht und mit der nächsten Anfrage an die KI mitgeschickt
  (bzw. direkt danach, falls keine weitere Runde folgt) — gut zum
  Korrigieren, ohne auf das Ende der Antwort warten zu müssen. Der
  „Stopp"-Knopf erscheint bei jeder laufenden Antwort und bricht die
  laufende Anfrage an den KI-Anbieter sofort ab, statt nur weitere
  automatische Runden zu verhindern. Ein gerade laufendes Kommando auf dem
  Server läuft dabei zu Ende; ein offener Bestätigungsdialog bleibt stehen.
- Die KI wird jetzt angewiesen, Passwörter, private Schlüssel, Tokens und
  ähnliche Geheimnisse möglichst nicht zu lesen: Existenz wird über
  Metadaten (z. B. Dateigröße) geprüft, und solche Dateien werden direkt
  auf dem Server kopiert (`cp`, Pipe) statt gelesen und neu geschrieben —
  so landen Geheimnisse gar nicht erst im Chat-Verlauf.
- App nutzt jetzt Anthropics Prompt-Caching für System-Prompt,
  Werkzeug-Definitionen und Server-Notiz — diese ändern sich innerhalb
  einer Sitzung kaum, werden bislang aber bei **jedem** Request neu
  gesendet und voll gegen das Input-Token-Rate-Limit gezählt. Gecachte
  Tokens zählen bei den meisten Claude-Modellen **nicht** gegen das
  Rate-Limit, senkt also Kosten und **Rate-Limit-Last** spürbar (Anthropic
  nennt als Beispiel 80% Cache-Trefferquote = effektiv 5× mehr
  Input-Durchsatz pro Minute). Der bisher direkt im System-Prompt
  eingebettete `uname`-Systembanner des verbundenen Servers wandert dabei
  in eine eigene, wie Kommando-Ausgaben gekennzeichnete Nachricht (hätte
  sonst bei jeder Sitzung den Cache ungültig machen können).
- Neuer "Diagnosepaket erzeugen"-Knopf in den Einstellungen (Diagnose)
  neben "Logpfad öffnen": erzeugt ein einzelnes, redigiertes Text-Paket
  (Version + Build-Hash, Betriebssystem/Architektur, Datenpfade,
  konfigurierte KI-Provider-**Typen**, Server-**Anzahl**, die letzten bis
  zu 500 Log-Zeilen) zur Weitergabe an den Support oder zum Anhängen an
  ein GitHub-Issue. Enthält **keine** Zugangsdaten, Passwörter, Host-Keys,
  Server-Adressen oder Notiz-/Chat-Inhalte; eingebettete Log-Zeilen laufen
  zusätzlich durch dieselbe Redaction wie sonst in der App. Wird vor dem
  Speichern erst als Vorschau angezeigt — nichts wird automatisch
  gespeichert oder versendet.
- App liest jetzt die Rate-Limit-Header des KI-Providers (Anthropic:
  `anthropic-ratelimit-{requests,input-tokens,output-tokens,tokens}-
  {remaining,reset}`) und drosselt proaktiv, statt blind ins Limit zu
  laufen: sinkt das bekannte Restbudget unter ~15% (oder würde eine
  geschätzte Anfrage das verbleibende Token-Budget überschreiten), wartet
  die App bis zum bekannten Reset-Zeitpunkt, bevor sie die Anfrage
  abschickt — der Chat zeigt dabei "Warte auf KI-Budget — nächster
  Versuch in Xs…" an, der Versand erfolgt danach automatisch. Alle
  KI-Aufrufe (Haupt-Chat, optionale Zweitmeinung/Einschleusungs-Check,
  Zusammenfassung, Auto-Titel, Notiz-Vorschlag/-Kürzung), die denselben
  API-Key/Endpunkt/Modell nutzen, teilen sich dabei ein gemeinsames
  Budget. Provider ohne diese Header (z. B. ein lokaler Ollama-Endpunkt)
  bleiben unverändert — kein proaktives Warten, nur das bestehende
  reaktive Wiederholen nach einem 429.
- App zeigt jetzt bei Startfehlern eine verständliche Meldung statt
  stillem Absturz: kann die Datenbank nicht geöffnet werden (z. B. von
  einer neueren Programmversion angelegt, beschädigt, oder das
  Datenverzeichnis nicht beschreibbar) oder der Host-Key-Speicher nicht
  geladen werden, erscheint jetzt ein Dialog mit der Fehlerursache statt
  eines unerklärten Absturzes vor dem ersten Fenster. Ist der
  Systemschlüsselbund beim Start gesperrt oder nicht verfügbar, startet
  die App wie bisher trotzdem (Chat-Verlauf und Notiz-Zusammenfassungen
  sind dann für diesen Programmlauf deaktiviert), zeigt das aber jetzt
  ebenfalls sichtbar an statt es nur stillschweigend zu protokollieren.
- Ist die gespeicherte Notiz eines Servers sehr groß, bietet die App beim
  Trennen der Verbindung jetzt an, sie per KI zusammenzufassen (oder
  selbst zu kürzen) — wie bei jedem KI-Notiz-Vorschlag wird die
  Zusammenfassung erst nach Bestätigung im Diff-Vergleich (alt/neu)
  tatsächlich gespeichert, nie automatisch. Dieser Vorschlag erscheint
  jetzt auch für den lokalen Pseudo-Server (Localhost).
- Der Notiz-Editor zeigt jetzt beim Bearbeiten einer sehr großen Notiz
  einen dezenten Hinweis mit direktem Link, sie sofort per KI
  zusammenfassen zu lassen (derselbe Bestätigungs-Ablauf wie beim
  Trennen-Vorschlag).

- Die maximale Antwortlänge der KI ist jetzt modellabhängig statt fest
  auf ~4000 Tokens begrenzt (bei aktuellen Claude-Modellen bis zu 128K)
  — ein zu knappes Limit führte bislang gelegentlich dazu, dass die
  Antwort mitten im Satz/Kommando abbrach. Bricht eine Antwort trotzdem
  am Längenlimit ab, bleibt der bisherige Text sichtbar und ein
  "Weiter"-Knopf setzt die Antwort exakt dort fort, wo sie endete.
  Zusätzlich im KI-Provider-Formular unter "Erweitert" ein optionales
  Feld "Max. Antwortlänge (Tokens)" für Provider mit unbekanntem
  Output-Maximum (z. B. ein selbstgehostetes Modell) — Standard weiterhin
  "Automatisch".
- Öffnest du die KI-Provider-Einstellungen ohne konfigurierten
  Ollama-Provider, sucht die App einmalig ein lokal laufendes Ollama
  (`127.0.0.1:11434`) und bietet an, es zu übernehmen — inklusive
  Modellauswahl. Ohne gefundenes Ollama zeigt eine kompakte Anleitung, wie
  man es installiert. Kein Netzwerkaufruf ohne diese Aktion oder einen
  Klick auf „Erneut suchen".
- Ist noch kein eigener Server angelegt, führt die Serverliste jetzt aktiv
  mit „Ersten Server anlegen" in den Anlage-Dialog, statt nur einen
  Entwicklertext zu zeigen.
- Beim Anlegen eines Servers mit Schlüssel-Anmeldung empfiehlt ein Hinweis
  Ed25519-Schlüssel (`ssh-keygen -t ed25519`) — RSA bleibt unterstützt.
- Bei Ollama ist der API-Key im Formular nicht mehr erforderlich.
- Die Diagnose-Ansicht zeigt eine Zeile „Systemschlüsselbund: verfügbar /
  nicht verfügbar (Grund)" samt nächstem Schritt — nachschlagbar auch dann,
  wenn der Startdialog bereits weggeklickt wurde.
- Der Button „Modelle laden" ist jetzt auch für Anthropic-Provider
  verfügbar — bisher fehlte er dort ganz.
- Beim Anlegen und Bearbeiten eines Servers steht neben Passwort, Private
  Key, SSH-Agent und Zertifikat jetzt eine fünfte Anmeldeart zur Wahl:
  „Schlüsseldatei“ — ein Pfad statt eines im Schlüsselbund gespeicherten
  Inhalts, wie `IdentityFile` in `ssh_config`. Vor dem Speichern zeigt die
  Oberfläche, ob die Datei existiert, ob die Rechte passen, ob sie wie ein
  OpenSSH-Schlüssel aussieht und ob sie verschlüsselt ist. Server-Liste und
  -Details zeigen den Pfad, an dem die Anmeldung hängt.
- Ein Knopf „In den Schlüsselbund übernehmen“ legt den Inhalt einer solchen
  Schlüsseldatei einmalig in den Systemschlüsselbund und stellt die
  Anmeldeart auf „Private Key“ um — die Ursprungsdatei bleibt dabei
  unverändert liegen. Vorher zeigt ein Dialog, welche Datei gelesen wird
  und was sich ändert.

### Changed
- Smart SSH ist jetzt Open Source unter der **Apache License 2.0**
  (bisher Functional Source License 1.1, FSL-1.1-MIT). Das gilt
  rückwirkend: Auch alle bereits veröffentlichten Versionen dürfen unter
  der Apache License 2.0 genutzt werden.
- Der System-Prompt weist die KI jetzt ausdrücklich an, ein angekündigtes
  Kommando auch tatsächlich über `suggest_command` vorzuschlagen, statt es
  nur im Fließtext anzukündigen und dort aufzuhören — beobachtetes Muster,
  bei dem die automatische Fortsetzung dadurch ohne erkennbaren Grund
  stehen blieb. Kurze Erklärungen vor einem Kommando bleiben ausdrücklich
  erwünscht.
- Fehlermeldungen auf dem Weg „Provider einrichten → Server anlegen →
  Verbindung testen → verbinden" nennen jetzt durchgängig Ursache **und**
  nächsten Schritt, auf Deutsch und Englisch — u. a. für falschen API-Key,
  unbekanntes Modell, nicht erreichbaren lokalen KI-Dienst, abgelehnte
  SSH-Verbindung, unbekannten Host, nicht erreichbaren Host und
  abgelehnten/nicht rechtzeitig bestätigten Host-Key.
- Der SSH-Verbindungsaufbau bricht jetzt nach 10 Sekunden ohne Antwort mit
  einer klaren Meldung ab, statt minutenlang auf das Betriebssystem zu
  warten.
- Die Startdialoge (Datenbank-, Host-Key- und Schlüsselbund-Fehler) sprechen
  jetzt Deutsch oder Englisch, abhängig von `LC_ALL`/`LC_MESSAGES`/`LANG`.
  Vorgabe bleibt Deutsch, wenn keine brauchbare Spracheinstellung gesetzt
  ist.
- Ein vertippter Modellname zeigt bei Anthropic jetzt korrekt „Modell nicht
  gefunden" statt „Provider nicht erreichbar".
- Der Platzhaltertext in der Verwalten-Ansicht ohne ausgewählte
  Gruppe/Server ist jetzt auch auf Englisch übersetzt (erschien zuvor auch
  in der englischen Oberfläche auf Deutsch).
- API-Key, Server-Passwort, Passphrase und Sudo-Passwort werden an den
  Rändern nach derselben Regel bereinigt. Nur die Ränder: Was innerhalb
  eines Wertes steht, bleibt unangetastet — auch ein ungewöhnliches
  Zeichen, das dort hingehören könnte.
- Beim selbst gesetzten Pfad zum `sftp-server` (erhöhter Dateibrowser)
  gilt dieselbe Regel: Ein unsichtbares Zeichen am Rand wird entfernt,
  statt den Pfad als ungültig abzulehnen. Ein solches Zeichen
  **innerhalb** des Pfades führt unverändert zur Ablehnung.
- Als Folge davon kann die Prüfung auf eingeschleuste Anweisungen jetzt
  häufiger anschlagen — etwa wenn das Modell in seiner Begründung eine
  gewöhnliche Konfigurationszeile wie `PermitRootLogin yes` zitiert. Die
  nächste Aktion verlangt dann eine Bestätigung, statt automatisch zu
  laufen. Das ist die sichere Richtung: Eine Nachfrage zu viel ist sichtbar
  und mit einem Klick erledigt, eine verschluckte Warnung nicht.
- Eine bereits gespeicherte Regel mit einem solchen Muster — etwa aus einer
  älteren Programmfassung — wird in der Regelliste sichtbar markiert, mit
  dem Hinweis, dass das Muster ungültig ist, und der Fundstelle darin.
  Bearbeiten und Löschen bleiben möglich; Speichern verlangt dann ein
  gültiges Muster. Zusätzlich wird eine solche Regel bei jeder Auswertung
  im Protokoll vermerkt (mit Regel-Kennung, nie mit dem Kommando).
- Meldet das Regel-Formular ein ungültiges Muster, steht dort jetzt ein
  verständlicher Satz in der eingestellten Sprache und darunter die genaue
  Fundstelle im Muster — bisher nur der englische Text der zugrunde
  liegenden Bibliothek.
- An einer so markierten Regel lässt sich die Priorität nicht mehr über die
  Pfeiltasten verschieben; die Pfeile sind deaktiviert und nennen den Grund.
- Der Vorschlag, eine sehr große Notiz per KI zusammenzufassen, hat jetzt
  einen ✕-Knopf zum Schließen sowie einen Knopf „Später“, nach dem er für
  diesen Server bis zum nächsten Start der App nicht wieder erscheint.
- Dieser Vorschlag sowie der Hinweis beim Bearbeiten einer großen Notiz im
  Notiz-Editor erscheinen jetzt erst ab 10 000 Zeichen statt bisher 8 000
  Byte.
- KI-Antworten ohne Text verschwinden nicht mehr still. Bricht die
  Antwort eines Reasoning-Modells wegen des Ausgabe-Limits leer ab, wird
  sie einmal automatisch mit mehr Budget wiederholt; bleibt eine Antwort
  auf eine Nachricht trotzdem ganz ohne Text und ohne Vorschlag, zeigt der
  Chat jetzt einen Hinweis mit dem Tipp, das Ausgabe-Limit in den
  Provider-Einstellungen zu erhöhen.

### Fixed
- "Mache ich selbst" im Kürzungs-Vorschlag scrollt jetzt direkt zum
  Notizfeld und fokussiert es, statt nur das Server-Formular zu öffnen.
- Ein KI-Provider (oder ein zwischengeschalteter Proxy), der einen
  Fehlerstatus zwar sofort beantwortete, den Antworttext danach aber
  hängen ließ, konnte einen Chat-Turn unbegrenzt und ohne jede
  Fehlermeldung blockieren (Chat antwortet nicht mehr, kein Log-Eintrag).
  Der Aufruf bricht jetzt spätestens nach 90 Sekunden mit einer
  sichtbaren Fehlermeldung ab.
- Eine sehr lange Chat-Sitzung oder eine sehr große Server-Notiz konnte
  eine KI-Anfrage so groß werden lassen, dass sie das Kontextfenster des
  Modells sprengte — der Chat blieb dann ohne Antwort/Fehlermeldung
  hängen. Die App schätzt jetzt vor jeder Anfrage die tatsächliche Größe
  und kürzt bei Bedarf automatisch: ältere Gesprächsrunden werden durch
  eine von der KI erstellte, laufend aktualisierte Zusammenfassung
  ersetzt (schlägt dieser Zusammenfassungs-Aufruf fehl, schneidet die App
  stattdessen wie gehabt ab — der Chat bleibt in jedem Fall funktionsfähig),
  danach ggf. überlange einzelne Kommando-Ausgaben und zuletzt die Notiz
  gekürzt — die gespeicherte Notiz und der vollständige Sitzungsverlauf
  bleiben dabei unangetastet.
- Ein KI-Provider-Aufruf konnte nach einem kurzen Netzwerk-Aussetzer ohne
  jede Fehlermeldung unbegrenzt hängen bleiben (der Chat antwortete dann
  einfach nicht mehr). Die Verbindung zum KI-Provider nutzt jetzt aktives
  TCP-Keepalive, damit eine durch den Aussetzer "leise gestorbene"
  Verbindung schneller erkannt wird.
- Linux ohne Systemschlüsselbund: Statt des englischen Bibliothekstexts
  „No default store has been set, so cannot search or create entries" nennt
  Smart SSH jetzt den tatsächlichen Zustand, die dadurch blockierten
  Funktionen und das Paket, das ihn behebt — je nachdem, ob kein
  Secret-Service-Anbieter läuft (`sudo apt install gnome-keyring`, auch
  unter KDE), kein D-Bus-Session-Bus erreichbar ist
  (`sudo apt install dbus-user-session`) oder der Schlüsselbund nur
  gesperrt ist. Ein gesperrter Schlüsselbund führt dabei nie zu einem
  Installationsvorschlag. Wer KWallet oder KeePassXC ohnehin nutzt, wird
  darauf hingewiesen, dass deren Secret-Service-Integration laufen bzw.
  eingeschaltet sein muss — Nachinstallieren allein genügt dort nicht.
- Der Startdialog behauptete bisher, ohne Schlüsselbund funktioniere alles
  außer dem Chat-Verlauf normal. Das stimmte nicht: Ohne Schlüsselbund
  lassen sich weder KI-Provider noch Server-Passwörter, Passphrasen oder
  Sudo-Passwörter speichern oder lesen. Der Dialog zählt jetzt auf, was
  wirklich blockiert ist — und was weiterhin geht (SSH-Agent, Schlüssel
  ohne Passphrase).
- Das Server-Formular meldete „kein Sudo-Passwort hinterlegt", wenn der
  Schlüsselbund gar nicht antworten konnte. Es zeigt jetzt einen neutralen
  Zustand, statt etwas zu behaupten, das es nicht wissen kann.
- „Hinterlegtes Sudo-Passwort entfernen" meldete Erfolg, auch wenn der
  Schlüsselbund den Eintrag gar nicht löschen konnte — das Passwort wäre
  beim nächsten `sudo` weiter eingespeist worden. Der Vorgang schlägt jetzt
  sichtbar fehl.
- Einen Server zu löschen funktioniert weiterhin auch dann, wenn der
  Schlüsselbund klemmt. Neu ist: Smart SSH sagt danach ausdrücklich, welche
  Einträge im Schlüsselbund zurückgeblieben sind, dass sie dort jetzt
  verwaist sind und wie sie sich von Hand entfernen lassen.
- „Verbindung testen" bewertet leere und eingefügte Zugangsdaten jetzt
  genau wie „Speichern". Legt man einen neuen Server an und lässt das
  Passwort-, Schlüssel- oder Zertifikatsfeld leer, meldet der Test die
  Fehlermeldung, statt sich mit einem leeren Wert anzumelden — auf einem
  Server, der leere Passwörter erlaubt, konnte er dafür bisher Erfolg
  melden, obwohl sich derselbe Server anschließend nicht speichern ließ.
  Beim Bearbeiten bedeutet ein leeres Feld weiterhin „das hinterlegte
  Zugangsdatum verwenden".
  Die Meldung ist dieselbe (und in der eingestellten Sprache) wie beim
  Speichern, etwa „Passwort ist erforderlich".
- „Verbindung testen" behandelt die Passphrase einer Schlüsseldatei oder
  eines privaten Schlüssels jetzt genauso wie „Speichern". Bisher konnte
  derselbe eingefügte Wert den Verbindungstest scheitern lassen und
  danach trotzdem richtig gespeichert werden — der Test sagte damit
  etwas anderes aus als der Server, der daraus entstand.
- Ein eingefügter API-Key oder ein eingefügtes Passwort funktioniert jetzt
  auch dann, wenn beim Kopieren ein unsichtbares Zeichen an den Rand
  geraten ist — etwa ein BOM aus einer Textdatei oder ein Zero-Width-Space
  aus einer Webseite. Bisher wurde ein solcher Wert unverändert
  gespeichert, die Anmeldung schlug fehl, und der Key sah in der
  Oberfläche trotzdem richtig aus.
- Eine Filterregel, deren Muster sich nicht übersetzen lässt, wird beim
  Anlegen und beim Ändern jetzt abgewiesen, statt gespeichert zu werden und
  anschließend wirkungslos in der Liste zu stehen. Das galt bisher für jede
  Aktion — auch für eine Deny- oder Bestätigen-Regel, die dadurch stillschweigend
  nichts tat. Das gilt für das Regel-Formular und für die Schnellregel aus
  dem Bestätigungsdialog.
- Der Bestätigungsdialog schlägt keine Schnellregel mehr vor, deren Muster
  sich nicht übersetzen lässt. Bisher entstand ein solcher Vorschlag aus
  einem Kommando mit einer Klammer im Argument und ließ sich anwählen,
  führte aber zu keiner wirksamen Regel.
- Zugangsdaten in einer Verbindungs-URL werden jetzt auch dann vollständig
  geschwärzt, wenn Passwort oder Benutzername ein unkodiertes `@` enthalten
  (etwa `postgres://app:Xy9@kLm2@db/prod` oder die bei mehreren gehosteten
  Datenbanken vorgeschriebene Schreibweise `benutzer@mandant`). Das gilt für
  alles, was an das KI-Modell geht, und für alles, was in der
  Gesprächshistorie gespeichert wird.
- Ein Passwort-Parameter im Query-String einer Verbindungs-URL ohne Pfad
  (`redis://cache:6379?password=…`) wird ebenfalls vollständig geschwärzt.
- „Zugangsdaten testen“ für einen Anthropic-Provider schlug fälschlich mit
  „Provider nicht erreichbar“ fehl, obwohl der Schlüssel gültig war — die
  Probe schickte keinen System-Prompt, und Anthropic lehnt einen leeren
  System-Textblock mit Caching-Markierung ab.

### Security
- Glob-`*` in pfadförmigen Allow-/Deny-Regeln (Muster mit einem
  Dateipfad-artigen Argument, z. B. `Allow: cat /var/log/*`) überquert
  keine Verzeichnisgrenzen mehr, auch nicht über Shell-Quoting/-Escaping
  (`\..`, `".."`, `'..'`, Brace-/Bracket-Tricks) versteckt. Bisher konnte
  eine harmlos erteilte Allow-Regel wie `cat /var/log/*` auch
  `cat /var/log/../../../etc/shadow` automatisch erlauben, weil der
  Glob-`*` über `/`-Grenzen hinwegging — die Filter-Engine "erlaubte"
  damit etwas, das nie freigegeben werden sollte. **Verhaltensänderung**:
  eine bestehende pfadförmige Regel mit einem einzelnen `*` deckt jetzt
  nur noch eine Verzeichnisebene ab (z. B. matcht `/etc/*` weiterhin
  `/etc/passwd`, aber nicht mehr `/etc/nginx/nginx.conf`). Für
  **Deny/Confirm**-Regeln mit mehrstufigem Schutzbedarf `**` verwenden
  (`/etc/**`) — bestehende Deny-Regeln verlieren dabei ohnehin nie
  Schutzumfang (altes und neues Matching gelten zusätzlich). Für
  **Allow**-Regeln wird `**` NICHT empfohlen: `**` kann in einer
  Allow-Regel auch über das eigentlich gemeinte Argument hinweg
  zusätzliche, unbeabsichtigte Kommando-Teile mit erlauben — für
  mehrstufigen Allow-Bedarf stattdessen mehrere spezifische,
  einstufige Allow-Regeln anlegen. Kommando-Argument-Globs ohne
  Pfad-Charakter (z. B. über eine URL) sind unverändert.
- Ein von der KI vorgeschlagenes Kommando, dessen Antwort mitten im
  Vorschlag durch das Längenlimit abgeschnitten wurde, konnte in einem
  seltenen Fall trotzdem — mit zufällig vollständig aussehendem, aber
  in Wahrheit gekürztem Inhalt — bis zum Bestätigungsdialog durchkommen.
  Ein solcher abgeschnittener Vorschlag wird jetzt nie mehr angezeigt
  oder ausgeführt; die App versucht stattdessen einmalig automatisch
  erneut mit mehr Platz, bevor sie im Ausnahmefall einen sichtbaren
  Fehler statt eines unvollständigen Kommandos zeigt.
- API-Schlüssel von KI-Anbietern und Code-Hostern werden jetzt auch ohne
  erkennbares Stichwort davor geschwärzt, bevor Text an die KI geht oder
  gespeichert wird: Anthropic (`sk-ant-…`), OpenAI (`sk-proj-…` u. a.),
  OpenRouter (`sk-or-v1-…`), GitLab (`glpat-…`, `glrt-…`, `gldt-…`),
  Hugging Face (`hf_…`), Groq (`gsk_…`) und xAI (`xai-…`). Ebenso Header
  wie `x-api-key`, `api-key`, `x-goog-api-key`, `Authorization: Basic …`
  und `Authorization: Token …`, Passwörter in beliebigen Adressen der Form
  `schema://nutzer:passwort@host` (der Nutzername bleibt lesbar) sowie die
  Zugangsdaten aus `.netrc`, `.pgpass`, Docker- und kubeconfig-Dateien.
- Liest ein KI-Vorschlag eine typische Geheimnis-Datei (private SSH- und
  Host-Schlüssel, `*.pem`, `*.key`, `*.p12`/`*.pfx`/`*.jks`, `.env`/`.envrc`,
  `/etc/shadow`, `~/.aws/credentials`, `~/.docker/config.json`,
  `~/.kube/config`, `.netrc`, `.pgpass`, `.git-credentials`, `~/.gnupg`,
  `/etc/ssl/private`, `~/.npmrc`, `~/.pypirc`, `~/.my.cnf`, Shell-Historien,
  Prozess-Umgebungen, `wp-config.php`, Kubernetes-Admin-Konfigurationen
  u. a.), fragt die
  App jetzt immer nach — auch wenn eine Allow-Regel das Kommando sonst
  automatisch freigeben würde. Das gilt auch für Anfragen externer
  MCP-Clients und für das direkte Lesen einer Datei, und es lässt sich
  nicht über Pfad-Tricks, Verzeichniswechsel, Platzhalter oder
  vorangestellte Befehle umgehen. **Verhaltensänderung:** rekursives
  Durchsuchen (`grep -r`, `rg`) und Lesen per `find -exec`/`xargs` fragt
  ebenfalls immer nach, weil der Inhalt vorab nicht prüfbar ist.
- Schlägt die KI einen Aufruf von `sftp-server` vor (etwa per `sudo -n`),
  zeigt die Risikoeinschätzung jetzt Rot und die App fragt immer nach —
  auch wenn eine Allow-Regel das Kommando sonst freigeben würde: Mit der
  sudo-Regel für den erhöhten Dateibrowser bedeutet das Dateizugriff mit
  Root-Rechten.
- Lehnt man eine von mehreren Aktionen einer KI-Antwort ab (oder blockiert
  eine Regel sie), laufen die übrigen Aktionen derselben Antwort nicht mehr
  automatisch, sondern fragen ebenfalls nach — sie könnten auf der
  abgelehnten aufbauen.
- Der Bestätigungsdialog für das Schreiben einer Datei sagt jetzt vorab,
  wenn die App bei fehlenden Rechten mit dem hinterlegten Sudo-Passwort
  schreiben würde. Bisher war das erst im Ergebnis zu sehen.
- Ein Host-Key-Dialog, der nie beantwortet wird (z. B. weil das Fenster
  neu geladen wurde), lässt den Verbindungsaufbau nicht mehr ewig warten.
  Nach einer Stunde bricht die App ab und gilt die Abfrage als abgelehnt;
  ein Host-Key wird durch Zeitablauf nie vertraut. Auch „Modelle laden“
  und die Attestierungsabfrage hängen bei einem nicht antwortenden
  Anbieter nicht mehr, sondern melden nach 90 Sekunden einen Fehler.
- Die KI-Zweitmeinung zum Datenrisiko und die Prüfung auf eingeschleuste
  Anweisungen lesen ihr Urteil jetzt zuverlässig aus der Antwort: Enthält
  eine Antwort mehrere Urteilswörter, zählt das warnende. Bisher zählte das
  zuerst genannte — eine Antwort, die den geprüften Text zitierte und darin
  ein „nein"/„none" enthielt, konnte so eine danach ausgesprochene Warnung
  verschlucken. Bei der Prüfung auf eingeschleuste Anweisungen ließ sich das
  gezielt ausnutzen, weil der geprüfte Text dort aus einer nicht
  vertrauenswürdigen Quelle stammt. Eine Warnung kann jetzt nicht mehr durch
  die Wortstellung verlorengehen.

## [0.5.0] — 2026-09-11

### Added
- Chat-Eingabefeld erlaubt jetzt mehrzeilige Eingaben: Enter sendet,
  Shift+Enter fügt einen Zeilenumbruch ein, das Feld wächst mit dem Inhalt
  bis zu einer Maximalhöhe und scrollt danach intern.
- Chat: "In Notiz übernehmen"/"Als Markdown exportieren" sind bei jeder
  Antwort weiterhin vorhanden (auch per Tastatur erreichbar), blenden aber
  erst bei Hover/Tastaturfokus ein statt dauerhaft sichtbar zu sein — ein
  laufender/fehlgeschlagener/gerade erfolgreicher Export bzw. eine
  Notiz-Übernahme bleibt dabei immer sichtbar, unabhängig von Hover/Fokus.
- Einstellungen: der KI-Provider-Bereich ist überarbeitet — konfigurierte
  Provider, die optionale Risiko-Zweitmeinung und das Formular für einen
  neuen Provider stehen jetzt in klar umrandeten, gruppierten Karten statt
  einer kontrastarmen, ungegliederten Liste; alle Formularfelder haben
  einen sichtbaren Fokus-Rahmen, der Format-Hinweis für den API-Key und
  das Testen-Ergebnis erscheinen als eigene, klar erkennbare Hinweisboxen.

### Fixed
- Chat-Eingabefeld: Enter während einer laufenden IME-Komposition (z. B.
  Kandidatenauswahl bei ostasiatischer Eingabe, Akzent-Eingabe über
  Tottasten) sendet nicht mehr vorzeitig eine noch unfertige Eingabe.
- Einstellungen: einige Kategorien ("Anzeige & Sprache", "Sitzungen &
  Daten", "MCP-Server") zeigten ihren Titel doppelt (einmal von der
  Einstellungen-Navigation, einmal als eigene Überschrift im Inhalt).
- Einstellungen: die Navigationseinträge "Sitzungen & Daten" und
  "MCP-Server" standen unabhängig von der UI-Sprache immer auf Deutsch da
  — erscheinen jetzt in Englisch übersetzt, wenn die UI-Sprache Englisch
  ist.

### Security
- Kommando-Ausgaben mit Unix-Passwort-Hashes (z. B. `/etc/shadow`,
  Apache-`.htpasswd`) gingen bislang unredigiert an den KI-Anbieter bzw.
  einen verbundenen MCP-Client weiter, wenn der Nutzer das zugrunde
  liegende Kommando (nach dem üblichen Bestätigungsdialog) ausführen
  ließ — die Redaction kannte deren Hash-Format schlicht nicht. Erkennt
  jetzt gängige Crypt-Hash-Formate (MD5-, SHA-256-, SHA-512-, yescrypt-,
  scrypt- und bcrypt-Hashes) und ersetzt nur den Hash-Anteil, Nutzername
  und übrige Felder bleiben lesbar.
- Aus demselben Grund gingen bislang auch DB-Connection-Strings
  (Postgres/MySQL/MariaDB/MongoDB/Redis/AMQP, Passwort zwischen `:` und
  `@`) sowie Provider-Tokens mit eindeutigem Präfix (Slack, Stripe,
  Google-API, npm) unredigiert weiter. Erkennt jetzt auch diese — bei
  Connection-Strings bleibt nur das Passwort-Segment ersetzt, Schema/
  Nutzername/Host/Datenbank bleiben lesbar.

## [0.4.6] — 2026-09-10

### Added
- Dateimanager: Spaltenbreiten (Name, Größe, Rechte, Geändert) lassen sich
  jetzt per Ziehen anpassen (Mindestbreiten, Name bleibt der flexible
  Rest).
- Die Aufteilung zwischen KI-Bereich und SSH-/SFTP-Bereich (Terminal/
  Dateimanager) lässt sich per Ziehen am Trenner anpassen (Mindestgrößen
  für beide Bereiche, Fallback auf die Standardaufteilung bei sehr kleinen
  Fenstern). Beide Einstellungen überleben einen Neustart.
- Dateimanager: Rechtsklick öffnet jetzt dasselbe Kontextmenü wie das
  Drei-Punkte-Symbol, mit neuen Aktionen — Herunterladen direkt ins
  Standard-Downloadverzeichnis oder per Dialog an einen gewählten Ort
  (auch für Ordner, rekursiv), Dateiinhalt kopieren, Pfad kopieren,
  Eigenschaften (Größe, Rechte numerisch + symbolisch, Besitzer/Gruppe,
  Änderungsdatum) und Aktualisieren.
- Dateimanager: server-verändernde Aktionen — Rechte bearbeiten (chmod,
  Checkbox-Matrix + numerisch, optional rekursiv für Ordner), Löschen jetzt
  auch für Ordner (rekursiv, mit Datei-/Ordner-Anzahl in der Bestätigung),
  Umbenennen mit Kollisionswarnung, Verschieben per Ausschneiden/Einfügen
  (ebenfalls mit Kollisionswarnung) und Hochladen mit Diff-Vorschau beim
  Überschreiben einer bestehenden Datei. Keine Bestätigung außer bei
  irreversiblen/überschreibenden Aktionen — manuelle Dateibrowser-Aktionen
  laufen (wie das Terminal) nicht durch die Filter-Engine.
- Einstellungen: neue Kategorie "Dateien" — pro Dateiendung ein lokales
  Standardprogramm festlegen (Fallback: Betriebssystem-Standard). Rein
  lokal, ohne Server-Bezug.
- Dateimanager: "Lokal öffnen…" lädt eine Datei in ein kontrolliertes
  Temp-Verzeichnis herunter, öffnet sie mit dem festgelegten (oder dem
  Betriebssystem-Standard-)Programm, und bietet nach einer erkannten
  lokalen Änderung automatisch den Upload an — inklusive Diff-Vorschau und
  einer Warnung, falls sich die Datei auf dem Server seit dem Download
  ebenfalls geändert hat. Die Bearbeitungskopie wird beim Beenden der
  Bearbeitung bzw. spätestens beim Trennen der Verbindung aufgeräumt.
- Datei-Diff-Vorschauen (Notiz-/Dateischreibvorgänge, Datei-Überschreiben
  beim Hochladen) zeigen jetzt die Zeilennummer vor jeder hinzugefügten
  oder entfernten Zeile.

### Fixed
- Dateimanager: das Drei-Punkte-Menü an einem Eintrag ließ sich oft nicht
  öffnen bzw. schloss ein gerade erst geöffnetes Menü eines anderen
  Eintrags sofort wieder — verursacht durch einen mit jedem weiteren Klick
  kollidierenden internen "Klick-außerhalb-schließt"-Mechanismus.

## [0.4.5] — 2026-09-09

### Fixed
- **Kritische Regression aus 0.4.4 behoben**: 0.4.4 startete auf Windows
  überhaupt nicht mehr (`Migrate(VersionMismatch(1))`-Absturz bei jedem
  Start, per Tester-Log bestätigt). Ursache war ein Seiteneffekt des
  CI-Fixes aus 0.4.4 selbst (`.gitattributes`) — die Windows-CI checkte
  dadurch die SQL-Migrationsdateien mit anderen Zeilenenden aus als bei
  0.4.1–0.4.3, was die zur Compile-Zeit eingebettete Prüfsumme der ersten
  Migration änderte und sie gegen bestehende Datenbanken bestehender
  Windows-Installationen ungültig machte. Eingegrenzt auf das, was
  tatsächlich betroffen war (Rust-Quelldateien) — SQL-Migrationen
  behalten ihr ursprüngliches Checkout-Verhalten.

## [0.4.4] — 2026-09-09

### Fixed
- Titelleisten-Buttons auf Windows/Linux waren die ganze Zeit sichtbar,
  aber mit sehr schlechtem Kontrast (kaum sichtbare graue Icons, nur
  "Schließen" reagierte erkennbar auf Hover): `tauri-plugin-decoration`
  färbt seine Controls standardmäßig für eine **helle** Titelleiste ein
  und wechselt nur bei einem im Betriebssystem eingestellten dunklen
  Modus auf helle Icons — unsere App ist aber immer dunkel, unabhängig
  vom Windows-Theme. Erzwingt das dunkle Farbschema jetzt bedingungslos
  — auf Windows 11 bestätigt behoben.

## [0.4.3] — 2026-09-09

### Fixed
- Zwei von `tauri-plugin-decoration` laut eigener Doku verlangte
  CSS-Variablen (`--tauri-plugin-decoration-titlebar-height`/`-z-index`)
  waren nie gesetzt — jetzt gesetzt. War letztlich nicht die Ursache der
  gemeldeten Sichtbarkeitsprobleme (s. 0.4.4), aber eine für sich
  genommen korrekte Ergänzung laut Plugin-Dokumentation.

## [0.4.2] — 2026-09-09

### Added
- Version + Commit-Hash (`0.4.2 (a5b3e01)`) sind jetzt auf einen Blick
  sichtbar: in der ersten Log-Zeile, kopierbar in einer neuen "Über"-
  Kategorie der Einstellungen, und (nur während der 0.x-Testphase, leicht
  abschaltbar) zusätzlich in der Titelzeile — identifiziert einen Bug-
  Report/ein Log jetzt eindeutig, auch wenn mehrere Builds dieselbe
  Version tragen.

## [0.4.1] — 2026-09-09

### Added
- Einstellungen neu strukturiert: Navigation links, Inhalt rechts (statt
  einer langen, ungegliederten Liste) — Kategorien für KI-Provider, Anzeige
  & Sprache, Diagnose, Sitzungen & Daten und MCP-Server.
- KI-Provider-Formular: sofortiger, rein lokaler Hinweis, falls ein
  eingegebener API-Key nicht zum erwarteten Format des gewählten Providers
  passt (nur ein Hinweis, blockiert nie das Speichern).
- KI-Provider-Formular: "Zugangsdaten testen"-Button prüft die gerade
  eingegebenen, noch nicht gespeicherten Zugangsdaten mit einem echten
  Mini-Request und zeigt, ob sie gültig sind, die Authentifizierung
  fehlschlägt, oder der Provider nicht erreichbar ist.

### Fixed
- Ein eingefügter API-Key, ein Server-Passwort/Sudo-Passwort oder eine
  Key-Passphrase mit einem angehängten Zeilenumbruch/Leerzeichen (z. B. von
  einem Copy-Paste unter Windows) wird jetzt beim Speichern getrimmt, statt
  die Authentifizierung mit "Credentials ungültig" scheitern zu lassen.
- Windows: die Titelleiste fällt bei einem Aktivierungsfehler jetzt sauber
  auf die volle native Titelleiste zurück (inkl. funktionierender
  Minimieren-/Maximieren-/Schließen-Controls und Fenster-Ziehen), statt in
  einem kaputten Zwischenzustand hängen zu bleiben.
- Ein vom KI-Provider zurückgemeldetes Rate-Limit (HTTP 429) — bislang der
  häufigste Grund, warum die KI mitten in einer Sitzung ohne jede Meldung
  aufhörte zu antworten — wird jetzt automatisch mit Backoff und
  `Retry-After`-Berücksichtigung wiederholt; die mehreren KI-Anfragen pro
  Nachricht (Hauptantwort, Risiko-Zweitmeinung, Einschleusungs-Check) werden
  zeitlich entzerrt statt als Burst abgeschickt. Scheitert es trotzdem, zeigt
  der Chat jetzt eine eigene, handlungsanleitende Meldung ("bitte kurz warten
  und erneut senden") statt der generischen "Provider-Konfiguration prüfen"-
  Meldung.

### Security
- Bei einem Fehler eines KI-Providers (falscher API-Key, Rate-Limit,
  Netzwerkfehler, Modell nicht gefunden) landet die Fehlerantwort des
  Providers jetzt redigiert im Log, ergänzend zur bestehenden
  UI-Meldung — erleichtert die Diagnose, ohne je ein Secret preiszugeben.

## [0.4.0] — 2026-09-07

Erste öffentliche Testversion.

### Added
- Word-Export als erstes Pro-Modul **(Pro)**.
- Lizenz-Eingabe-UI mit Live-Aktivierung **(Pro)**.

### Security
- Ressourcen-Caps gegen feindliche/fehlerhafte Server: ein Output-Cap, der
  vorher erst nach vollständigem Puffern griff, begrenzt jetzt bereits
  während des Streamings; ein expliziter Rekursions-Cap gegen
  verschachtelte Command-Substitution.
- Integrität der Fencing-Markierungen für nicht vertrauenswürdigen Inhalt
  im KI-Kontext abgesichert.

### Fixed
- Härtungsrunde für die Testphase: verwaiste Keychain-Einträge nach einem
  fehlgeschlagenen Server-Anlegen werden jetzt zuverlässig zurückgerollt,
  Fehlermeldungen (falscher API-Key, Host nicht erreichbar, Provider nicht
  gestartet, …) nennen jetzt den nächsten Schritt statt roher Technik, und
  ein Absturz beim Start landet jetzt garantiert in der Logdatei statt
  spurlos zu verschwinden.

## [0.3.0] — Initial Early Access

Erste zusammenhängende Version: SSH-Client mit KI-Copilot und
Filter-/Policy-Engine mit Bestätigungs-Workflow für jedes vorgeschlagene
Kommando, Server-/Gruppen-/Regel-/Notizverwaltung, MCP-Server-Anbindung,
persistente Chat-Sessions, Risiko-Indikatoren, Multi-Provider-KI-Anbindung
(Anthropic, OpenAI, Ollama, generisch OpenAI-kompatibel) mit Redaction
sensibler Inhalte, macOS-Build signiert und notarisiert.
