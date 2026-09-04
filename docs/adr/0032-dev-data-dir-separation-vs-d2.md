# 0032-dev-data-dir-separation-vs-d2

## Status
Vorgeschlagen

## Kontext

`persistence_sqlite::default_db_path` (und darüber der Host-Key-Speicher,
der als `.parent()` des DB-Pfads lebt, s. `app-tauri::lib::
build_app_state`) lieferte bislang für jeden Build denselben Pfad
(`~/Library/Application Support/Smart SSH/smart-ssh.db` auf macOS,
analog auf Windows/Linux) — unabhängig davon, ob es sich um
`cargo tauri dev` oder ein tatsächliches Release-Binary handelte.

Konkret aufgetretener Konflikt: eine lokale `cargo tauri dev`-Instanz
lief bereits auf einem neueren Migrationsstand (Migration 6 angewendet),
danach wurde ein lokal gebautes Release-Binary von einem älteren Commit
(kennt Migration 6 noch nicht) gegen dieselbe DB-Datei gestartet. `sqlx`s
Migrationsprüfung erkennt eine bereits angewendete, dem Binary unbekannte
Migrationsnummer nicht als "kann ignoriert werden", sondern als
Inkonsistenz — die App panicte beim Start mit `Migrate(VersionMissing(6))`
(kein Fenster, nur kurzes Aufblitzen).

**Spannung mit dem Architektur-Brief, Abschnitt D2:** D2 verlangt
identische Datenpfade für Community- und Official-Edition, explizit
damit ein Nutzer zwischen beiden wechseln kann, ohne Daten zu verlieren.
Eine Verzeichnis-Trennung könnte oberflächlich wie ein Verstoß gegen
dieses Prinzip aussehen.

## Entscheidung

**Der Debug-Build (`cargo tauri dev`, `#[cfg(debug_assertions)]`) bekommt
ein eigenes, klar erkennbar gekennzeichnetes Datenverzeichnis
("Smart SSH (dev)" / `smart-ssh-dev` auf Linux) — Community- und
Official-Edition (beide Release-Builds) bleiben davon unberührt und
teilen sich weiterhin exakt denselben Pfad wie zuvor.**

Begründung, warum das **kein** D2-Verstoß ist: D2 beschreibt ein
**Nutzer**-Szenario — ein Mensch, der zwischen der kostenlosen und der
bezahlten Edition derselben installierten App wechselt, und dabei seine
Server-Profile, Notizen, Chat-Historie etc. behalten will. Ein
Debug-Build ist kein drittes Nutzer-Szenario in diesem Sinn — niemand
"nutzt" `cargo tauri dev` als Endanwender-App, es ist ausschließlich
Entwickler-Innensicht (dieselbe Unterscheidung, die z. B. auch
`docs/adr/0022-stable-dev-code-signature.md` für die Signierung trifft:
eine eigene Dev-Behandlung, die den Release-Pfad unverändert lässt). D2s
Garantie — Community ↔ Official ohne Datenverlust wechselbar — bleibt
vollständig intakt, weil beide weiterhin identisch bleiben; nur ein
dritter, von D2 nicht gemeinter Fall bekommt eine eigene Behandlung.

**Warum eine Verzeichnis-, keine reine Fehlerbehandlungs-Lösung:** die
naheliegende Alternative — eine sichtbare Fehlermeldung statt eines
Panics bei einer unbekannten Migrationsnummer zu zeigen — behebt nicht
die eigentliche Ursache (geteiltes Verzeichnis zwischen zwei Builds mit
unterschiedlichem Kenntnisstand), sondern würde nur denselben Konflikt
freundlicher melden. Beides sind unterschiedliche, sich ergänzende
Verbesserungen — die generelle Startup-Fehlerbehandlung (ein sichtbares
Fehler-Fenster statt Panic bei DB-/Keychain-/Verzeichnisfehlern jeder
Art) ist bewusst **nicht** Teil dieses Schritts, sondern eine eigene,
größere, noch ausstehende Spec.

**Warum `SMART_SSH_DATA_DIR` in beiden Build-Profilen wirkt (nicht nur
Debug):** Die Umgebungsvariable dient gezieltem Testen — sowohl "eine
Dev-Instanz gegen eine bestimmte Test-DB starten" als auch "ein
tatsächliches Release-Artefakt gegen ein sauberes Verzeichnis
verifizieren, ohne die eigene produktive Release-DB zu berühren". Beide
Anwendungsfälle sind unabhängig vom Build-Profil sinnvoll, eine
Beschränkung auf Debug-Builds hätte den zweiten Fall ausgeschlossen, ohne
dass dafür ein Grund ersichtlich wäre.

**Keychain-Service-Name bewusst unverändert.** Nur das Datei-/DB-
Verzeichnis wird getrennt — der Keychain-Service-Name
(`credentials_keyring::SERVICE_NAME`, "Smart SSH") bleibt für Dev und
Release identisch. Eine Dev-Instanz nutzt damit dieselben Secrets
(Sudo-Passwörter, AI-Provider-API-Keys) wie ein Release-Build auf
derselben Maschine — akzeptiert und gewollt: Secrets sind hier nicht das
Problem (sie tragen keine Migrationsversion, ein geteilter Zugriff
verursacht keinen Panic), nur die migrierende SQLite-DB war es.

## Konsequenzen

**Positiv:**
- Behebt die tatsächliche Ursache des berichteten Panics, nicht nur
  dessen Sichtbarkeit — ein Dev-Build und ein Release-Build auf
  derselben Maschine können nie wieder um denselben Migrationsstand
  einer gemeinsamen DB konkurrieren.
- D2s eigentliche Garantie (Community ↔ Official ohne Datenverlust
  wechselbar) bleibt unverändert vollständig erfüllt — beide
  Release-Editionen sind von dieser Änderung überhaupt nicht betroffen
  (`#[cfg(not(debug_assertions))]`-Pfad ist byte-identisch zum
  Vorzustand, s. `test_release_build_default_path_has_no_dev_suffix`).
- `SMART_SSH_DATA_DIR` deckt zusätzlich automatisiertes/gezieltes Testen
  gegen eine definierte DB ab, ohne dass dafür ein eigener Mechanismus
  nötig wäre.

**Negativ / Trade-off:**
- Eine bereits existierende, vor dieser Änderung unter dem
  gemeinsamen Pfad angelegte Dev-Datenbank wird beim nächsten
  `cargo tauri dev` nicht automatisch "mitgenommen" — ein Entwickler mit
  bereits vorhandenen lokalen Testdaten im alten, geteilten Verzeichnis
  startet nach dieser Änderung mit einer frischen, leeren Dev-DB (die
  ursprünglichen Daten bleiben im bisherigen "Smart SSH"-Ordner erhalten,
  gehen also nicht verloren, sind nur nicht mehr die, die der Dev-Build
  sieht). Für reine Testdaten unproblematisch, für einen Entwickler mit
  aufwendig aufgebauten lokalen Testservern/-notizen ein einmaliger
  manueller Umzug (Datei kopieren), falls gewünscht.
- Der Host-Key-Speicher (`host_keys.json`, lebt im selben Verzeichnis wie
  die DB) trennt sich als Nebeneffekt ebenfalls — ein Dev-Build muss
  Host-Keys, die im Release-Build bereits vertraut wurden, erneut
  bestätigen. Bewusst in Kauf genommen: eine partielle Trennung (DB
  getrennt, Host-Keys weiter geteilt) wäre inkonsistenter und schwerer
  nachzuvollziehen als eine vollständige Trennung des gesamten
  Datenverzeichnisses.
- Der Log-Ordner (`app-tauri::logging::default_log_dir`) ist bewusst
  NICHT Teil dieser Trennung — er nutzt ohnehin einen anderen
  Basisordner pro Plattform (z. B. `~/Library/Logs` statt
  `~/Library/Application Support` auf macOS) und war nicht die Ursache
  des berichteten Konflikts (Log-Dateien tragen keine Migrationsversion).
  Dev- und Release-Logs bleiben also weiterhin im selben Ordner —
  außerhalb des Umfangs dieses Schritts.
