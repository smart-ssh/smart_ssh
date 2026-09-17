# 0052 — Fataler Startfehler-Dialog (Spec 0059): Mechanismus & offen gelassene Entscheidungen

## Status

Angenommen

## Kontext

Spec 0059 verlangt einen nativen Fehlerdialog für vier Startup-Fehlerfälle,
die alle **vor** `tauri::Builder::default()` auftreten — also bevor eine
Tauri-Runtime oder ein Fenster existiert. Teil 0 der Spec verlangt
ausdrücklich, den Mechanismus "im echten Crate-/Tauri-Kontext" zu prüfen,
nicht aus der Doku zu vermuten, und mehrere Implementierungsentscheidungen
wurden während der Umsetzung getroffen, die hier festgehalten werden
(CLAUDE.md: ADR "besonders [bei] einer Scope-Reduktion" oder einer
nicht-offensichtlichen Umsetzungsentscheidung).

## Entscheidungen

### 1. Mechanismus: `rfd::MessageDialog` direkt, nicht `tauri_plugin_dialog`

`tauri-plugin-dialog` (bereits eine reguläre `app-shell`-Abhängigkeit für
In-App-Dialoge) wickelt seine Dialoge intern über einen laufenden
`tauri::AppHandle` — genau die Voraussetzung, die vor
`tauri::Builder::default()` noch nicht existiert. `rfd` selbst (Version
0.16) ist bereits **transitiv** über `tauri-plugin-dialog` im
Dependency-Baum (per `Cargo.lock` verifiziert) und lässt sich davon
unabhängig aufrufen: `rfd::MessageDialog::new()....show()` baut/zeigt den
Dialog vollständig selbst (macOS: `NSAlert::runModal()`; Linux/GTK3: ein
eigener GTK-Thread, denselben Mechanismus nutzt `tauri-plugin-dialog`
bereits standardmäßig; Windows: `MessageBox`/`TaskDialogIndirect`) —
blockiert synchron, ganz ohne Fenster oder laufende Tauri-Runtime. Damit
reicht eine einzige plattformübergreifende Crate; keine dritte,
plattformspezifische `#[cfg(...)]`-Lösung nötig (die von Spec 0059 Teil 0
bevorzugte Option). `rfd` wurde als direkte `app-shell`-Abhängigkeit
hochgestuft, mit `default-features = false, features = ["gtk3",
"common-controls-v6"]` — explizit passend zu dem Backend, das
`tauri-plugin-dialog` selbst schon standardmäßig einsetzt (`gtk3`), statt
`rfd`s eigenem Default (`xdg-portal`), der auf dieser Linux-Installation
bisher ungetestet wäre.

### 2. Fehlerklassifizierung bleibt in `persistence-sqlite`, `sqlx`-frei nach außen

`PersistenceError` (`Connect(sqlx::Error)`/`Migrate(MigrateError)`) bleibt
absichtlich `sqlx`-typisiert und nicht direkt an `app-shell` durchgereicht
— stattdessen klassifiziert `PersistenceError::classify()` grob in
`ConnectFailureKind` (`SchemaTooNew`, `PermissionDenied`, `Other`), einen
neuen, `sqlx`-freien Typ, den `app-shell` ohne eigene `sqlx`-Abhängigkeit
konsumiert. Die drei Fälle wurden empirisch gegen den echten
`SqliteProfileStore::connect`-Codepfad verifiziert (temporärer,
anschließend entfernter Testcode): Downgrade →
`Migrate(VersionMissing(n))`; nicht beschreibbares Verzeichnis →
`Connect(Io(PermissionDenied))`; korrupte DB → tatsächlich
`Migrate(Execute(Database(...)))`, nicht `Connect(...)` (SQLite öffnet die
Datei erst beim ersten echten Query, hier: dem Migrationslauf) — fällt
korrekt auf `Other` zurück, kein eigener Fall dafür nötig.

### 3. Fall 3 (Keychain) bleibt nicht-fatal — Spec-0040-Design bewusst bewahrt

Spec 0059 benennt den Keychain-Fall als einen der vier "fatalen"
Startfehler und verlangt wörtlich, den `.expect()`-Panic aus Spec 0040
Teil 6 zu entfernen. Bei der Umsetzung stellte sich heraus: dieser Panic
war zum Start dieser Arbeit **bereits** entfernt — Spec 0040 hatte
bewusst entschieden, dass ein Keychain-Zugriffsproblem nur eine optionale
Komfortfunktion betrifft (Chat-Persistenz/Notiz-Zusammenfassungen/
Prompt-Historie), nicht den App-Kern (SSH-Verbindungen, KI-Chat,
Filter-Engine funktionieren unverändert), und degradiert seither still
mit `tracing::warn!` statt abzustürzen.

Das ist ein echter Konflikt zwischen der neuen Spec 0059 (die Fall 3
implizit als fatal wie die anderen drei behandelt) und der bestehenden,
bewussten Spec-0040-Entscheidung. Mit dem Nutzer abgestimmt (explizite
Rückfrage): **Fall 3 bleibt nicht-fatal.** Statt `show_fatal_error_and_exit`
+ `std::process::exit` zeigt `startup_dialog::show_warning` jetzt eine
SICHTBARE, nicht-blockierende Warnung (`MessageLevel::Warning`, kein
Exit) — die App startet danach unverändert degradiert weiter, exakt wie
vor diesem Schritt, nur dass das bisher rein ins Log geschriebene
`tracing::warn!` jetzt zusätzlich für den Nutzer sichtbar ist. Auf Linux
nennt der Text zusätzlich, welches Paket typischerweise fehlt
(gnome-keyring, KWallet, oder KeePassXC mit aktivierter
Secret-Service-Integration), wie von Spec 0059 Fall 3 wörtlich verlangt.

### 4. Zusätzlich behoben: Host-Key-Speicher — kein eigener Spec-0059-Fall, aber derselbe Pfad

`FileHostKeyStore::load(...)` in `crate::run` hatte ein eigenes,
unbehandeltes `.expect(...)` auf demselben kritischen Startpfad wie Fall 4
(Datenverzeichnis-Zugriffsproblem) und teilt sich dasselbe Datenverzeichnis
wie die SQLite-Datenbank. Spec 0059 benennt diesen Fall nicht namentlich,
aber ihn unbehandelt zu lassen hätte denselben undiagnostizierbaren
Absturz direkt neben den vier behobenen Fällen übrig gelassen — mit
demselben Mechanismus (`show_fatal_error_and_exit`) geschlossen.

### 5. Bewusst außerhalb des Scopes: `BaseDirs::new()` und `db_path.parent()`

Zwei verwandte `.expect()`-Aufrufe bleiben unverändert:

- `resolve_data_dir()`s `BaseDirs::new().expect(...)`
  (`persistence-sqlite/src/paths.rs`) und `default_log_dir()`s
  Äquivalent (`app-shell/src/logging.rs`) — der "kein Home-Verzeichnis"-
  Fall. Passiert **vor** dem Logging-Setup selbst (Spec 0047 B1 läuft
  danach), ist auf allen drei Zielplattformen praktisch nicht
  herbeiführbar, und ein Dialog an dieser Stelle hätte dasselbe
  Henne-Ei-Problem, das dieser Schritt gerade für die vier
  benannten Fälle löst — ohne dass Spec 0059 diesen Fall
  einschließt.
- `db_path.parent().expect("db_path hat immer ein Elternverzeichnis...")`
  — ein strukturelles Invariante (der Pfad wird immer aus
  `resolve_data_dir()` plus einem festen Dateinamen zusammengesetzt),
  kein I/O-Fehlerfall im Sinne der vier Spec-0059-Fälle.

Beide bleiben unverändert; keiner ist ein vom Nutzer beim Kickoff
genanntes Fall, und beide sind strukturell verschieden von den vier
tatsächlich behobenen I/O-Fehlerfällen.

### 6. spec-reviewer-Review (ERHÖHT, Commit 2dd3f81): behoben vs. bewusst nicht behoben

Der Pflicht-Review (s. CLAUDE.md, Verbindlicher Review-Workflow) fand einen
echten Korrektheitsfehler und mehrere kleinere Härtungen — behoben in
Commit `ee0add2`:

- **Fall 4 wurde im realistischen Testfall nicht erkannt.** `create_dir_all`
  gibt für ein bereits existierendes Verzeichnis `Ok` zurück, unabhängig
  von dessen Schreibrechten — der Spec-Testfall "Rechte entziehen" trifft
  aber genau diesen Fall. `SqliteProfileStore::connect` prüft jetzt aktiv
  per Schreib-Probe (Testdatei anlegen + löschen), statt sich auf einen
  mehrdeutigen `sqlx`-Fehlercode zu verlassen (der reale Bug hätte sonst zu
  einer irreführenden, potenziell schädlichen "Backup einspielen"-Empfehlung
  für eine intakte Datenbank geführt).
- `Other`-Fallback-Text abgeschwächt (deckt auch gesperrte DB durch eine
  zweite Instanz ab, nicht nur Korruption) und verweist jetzt zusätzlich
  auf das Log-Verzeichnis.
- Host-Key-Lösch-Empfehlung warnt jetzt ausdrücklich, dass sie keinen
  Schutz vor einem seither untergeschobenen Server bietet (TOFU-Pinning-
  Historie geht beim Löschen verloren).
- `show_fatal_error_and_exit`: `WorkerGuard` wird jetzt explizit vor
  `process::exit` gedroppt (blockierender Flush), da `process::exit` keine
  Destruktoren ausführt und die fatale Log-Zeile sonst im Puffer des
  nicht-blockierenden Writers verloren gehen könnte.
- Pfad-Anzeige bereinigt Steuerzeichen (defense in depth).
- `should_warn_about_keychain()` als eigene, testbare Funktion extrahiert
  statt eines `matches!(...)` direkt an der (ohne echte Keychain/DB nicht
  testbaren) Aufrufstelle.

**Bewusst nicht in dieser Runde behoben** (dem Nutzer explizit gemeldet,
nicht stillschweigend fallen gelassen):

- **GTK-Reinitialisierungsrisiko unter Linux (Fall 3, der einzige
  nicht-fatale `rfd`-Pfad, der APP-lauf danach fortsetzt)** und ein
  analoges, geringeres Risiko für `NSAlert`/AppKit unter macOS (Case 3 vor
  Keychain-Ablehnung) — technisch nur auf einem echten Gerät prüfbar, kein
  Code-Fix möglich ohne dieses Risiko zu verifizieren. Teil des von Stefan
  ohnehin geforderten manuellen Testablaufs.
- **`logging.rs`s `tracing_appender::rolling::daily(...)`-Panic** und
  **`lib.rs`s `.run(context).expect(...)`** (z. B. fehlendes WebKitGTK
  unter Linux) bleiben unbehandelte, undiagnostizierbare Absturzpfade —
  beide liegen außerhalb der vier in Spec 0059 namentlich benannten Fälle
  (dieselbe Kategorie wie `BaseDirs::new()`/`db_path.parent()` oben), eine
  künftige Spec müsste sie explizit aufgreifen.
- **Keine Lokalisierung** der neuen Dialogtexte (hart Deutsch) — Spec 0059
  verlangt es nicht, aber ein englischsprachiger Nutzer bekäme beim
  allerersten Fehlerfall einen deutschen Dialog.
- **Kein "nicht mehr anzeigen" für die wiederkehrende Fall-3-Warnung** — bei
  jedem Start ohne funktionierenden Secret-Service erscheint der Dialog
  erneut. UX-Komfort, keine Korrektheits- oder Sicherheitsfrage.
- Der bewusst tautologische `test_no_generated_text_leaks_a_placeholder_
  secret_value`-Test (kann laut Funktionssignaturen nicht fehlschlagen)
  bleibt unverändert — er dokumentiert die Invariante ausführbar, ersetzt
  aber keinen Architektur-Beweis (den liefert bereits, dass keine der
  Builder-Funktionen einen rohen Fehler entgegennimmt).
- Ein `chmod`-basierter echter End-zu-Ende-Test für den neuen Schreib-Probe
  (Verzeichnis schreibgeschützt machen → `connect()` → `PermissionDenied`
  erwarten) ließ sich in der Sandbox dieser Entwicklungsumgebung NICHT
  zuverlässig reproduzieren (ein `chmod 500` auf ein selbst besessenes
  Testverzeichnis blieb dort wirkungslos, vermutlich ACL-Overrides unter
  macOS-`/var/folders`). Die neuen Tests decken den Probe-Mechanismus
  stattdessen über einen zuverlässig reproduzierbaren Fehlerfall
  (nicht-existierendes Verzeichnis) ab — **der eigentliche
  `PermissionDenied`-Fall muss von Stefan manuell auf einem echten Gerät
  verifiziert werden**, ohnehin Teil des geforderten Testablaufs.

## Konsequenzen

- Kein plattformspezifischer `#[cfg(...)]`-Code für den Dialog-Mechanismus
  nötig — `rfd` deckt alle drei Zielplattformen aus einer einzigen
  Codebasis ab.
- Fall 3 (Keychain) bleibt architektonisch nicht-fatal — künftige Texte zu
  diesem Fall müssen weiterhin klarstellen, dass die App normal
  weiterläuft, nicht dass sie abbricht.
- `BaseDirs::new()`/`db_path.parent()` bleiben als bekannte, bewusst nicht
  behobene Restrisiken dokumentiert — sollte einer davon in der Praxis
  auftreten, zeigt sich weiterhin das alte, undiagnostizierbare Verhalten;
  eine künftige Spec müsste das explizit aufgreifen.
