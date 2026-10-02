# Spec 0101 — Ganze Datenbank verschlüsseln, Secrets in die Datenbank, Master-Passwort

Status: Vorschlag (Architekt) · Backlog: BL-0314, BL-0297, BL-0117, BL-0203 · Gate: release-1.0/C
Zweck: Die Datenbankdatei ist vollständig verschlüsselt (SQLCipher), alle Secrets liegen darin, und ihr Schlüssel kommt entweder aus dem OS-Schlüsselbund oder aus einem Master-Passwort.
Review-Priorität: ERHÖHT (Verschlüsselung, Credentials, Migration, Start)

## Getroffene Entscheidungen

- **E1** Der Datenbankschlüssel wird aus dem vorhandenen Zufallsschlüssel
  `app:chat_content_encryption_key` abgeleitet (im Folgenden **Wurzelschlüssel K**).
  Kein zusätzlicher Schlüsselbund-Eintrag.
- **E2** Alle Secrets ziehen in die verschlüsselte Datenbank: Server-Passwörter,
  Private Keys, Passphrasen, Zertifikate, Sudo-Passwörter, API-Keys, das
  MCP-Bearer-Token. Im Schlüsselbund bleibt höchstens K.
- **E3** Schlüsselbund beim Start nicht erreichbar → Startdialog mit
  „Erneut versuchen“ / „Beenden“; nichts wird angefasst.
- **E4** Datenbank verschlüsselt, K nicht auffindbar → nie still ein neuer
  Schlüssel. „Neu anfangen“ nur als bewusste Wahl; die alte Datei wird
  umbenannt, nicht gelöscht.
- **E5** Eine bestehende Klartext-Datenbank wird beim ersten Start automatisch
  umgewandelt; das Klartext-Original wird nach erfolgreicher Prüfung entfernt.
- **E6** Ältere Versionen können die Datei danach nicht mehr öffnen
  (akzeptiert, Changelog).
- **E7** Alte Schlüsselbund-Einträge der Secrets werden gelöscht, sobald jedes
  Secret aus der Datenbank zurückgelesen und gleich ist.
- **E8** Master-Passwort für alle wählbar. Standard bleibt der Schlüsselbund;
  Wechsel in beide Richtungen in den Einstellungen. Fehlt beim Start ein
  Schlüsselbund, bietet der Startdialog das Einrichten an.
- **E9** Im Passwort-Modus liegt K mit einem aus dem Passwort abgeleiteten
  Schlüssel verpackt in einer Datei neben der Datenbank. Moduswechsel und
  Passwortänderung verpacken K neu; die Datenbank bleibt unberührt.
- **E10** Passwort vergessen: keine Wiederherstellung. Beim Einrichten wird
  das deutlich gesagt und muss bestätigt werden.
- **E11** Feldweise Verschlüsselung (Chat, Ledger, Eingabe-Historie,
  Zusammenfassungen) bleibt vorerst bestehen.

## 1. Ist-Stand (Stand `4d1c307`)

**Datenbank.** Eine Datei `smart-ssh.db` im Datenverzeichnis
(`persistence_sqlite::default_db_path`, Override `SMART_SSH_DATA_DIR`).
`SqliteProfileStore::connect_with` öffnet sie mit `create_if_missing(true)`,
`foreign_keys(true)`, einem Pool mit `max_connections(1)` und führt direkt
danach `sqlx::migrate!` aus. Einziger Produktivaufruf: `build_app_state` in
`app-shell/src/lib.rs`. Alle Stores teilen diesen Pool. WAL setzt Migration
`0001`. Kein anderer Prozess öffnet die Datei; `mcp-server` hängt nicht an
`persistence-sqlite` (gelesen, nicht ausgeführt).

**Startreihenfolge** (`build_app_state`): Log → Datenbank öffnen und migrieren
→ `probe_keychain_availability` → `resolve_or_generate_key` → Cipher und
verschlüsselnde Stores → Host-Key-Store. Der Zustand entsteht **vor**
`tauri::Builder::setup`, also bevor ein Fenster existiert.

**Wurzelschlüssel.** `crypto::key::resolve_or_generate_key` liest
`app:chat_content_encryption_key` (32 Byte, Base64). Bei `NotFound`
**erzeugt es still einen neuen Schlüssel** und schreibt ihn; bei Backend-Fehler
`KeyStoreAccessFailed`. Im Fehlerfall startet die App eingeschränkt: Chat,
Eingabe-Historie und Ledger sind `None`, dazu `startup_dialog::show_warning`
(Spec 0059 Fall 3, Spec 0071 A12). Das widerspricht E4 und gibt es mit einer
verschlüsselten Datenbank nicht mehr.

**Secrets.** Trait `CredentialStore` (`core/src/profiles/credentials.rs`,
synchron: `get`/`set`/`delete`, Fehler `NotFound`/`Backend`). Produktiv nur
`KeyringCredentialStore` (Dienst `"Smart SSH"`, Account = `CredentialRef`),
ohne Cache. Einträge: `server:{id}:{slot}` mit den Slots `password`,
`private_key`, `passphrase`, `certificate`, `certificate_key`,
`sudo_password` (`server_credentials.rs`, `identity_file.rs`) sowie
`ai-provider:{id}` (`dto.rs`, `commands/ai_providers.rs`).
`AppState.credential_store` ist ein `Arc<dyn CredentialStore>`.
Daneben `EphemeralCredentialStore` (nur Verbindungstest) und
`RecordingCredentialStore`.

**Klartext in der Datei heute** (Spec 0096 §1): `servers.*` inkl. Host und
Benutzer, `ai_provider_configs.extra_headers` (JSON, BL-0297), Notizen,
Filterregeln, Sitzungstitel.

**MCP-Token.** `mcp_settings::load_or_init_token` hält das Token im
Tauri-Store `settings.json` (Schlüssel `mcpServerToken`), Rechte 0600 nur
unter Unix. `regenerate_mcp_server_token` schreibt es neu.

**Startdialoge.** `startup_dialog::show_fatal_error_and_exit` und
`show_warning` (rfd 0.16) haben nur „OK“. Texte in
`app-logic/src/startup_error_messages.rs`; DB-Fehlerarten
`ConnectFailureKind::{SchemaTooNew, PermissionDenied, Other}`. Schlüsselbund-Zustände
`KeychainAvailability::{Available, Unavailable(reason)}` mit `NoSessionBus`,
`NoSecretServiceProvider`, `Locked`, `Unknown`.

**Erststart-Hinweis.** `firstRunNotice.encryptionText` (DE/EN) sagt „Die lokale
Datenbank ist nicht zusätzlich verschlüsselt …“; der Test
`FirstRunNoticeScreen.test.tsx` prüft diesen Satz.

**Logs.** Nur Datei (`app-logic/src/logging.rs`, JSON); stderr wird nirgends
ins Log umgeleitet. Diagnosepaket mit Allowlist `SAFE_LOG_MESSAGES`.

**Messungen** (Protokoll in der Beilage):
- `libsqlite3-sys = "=0.37.0"` mit Feature `bundled-sqlcipher-vendored-openssl`
  vereinigt sich mit `sqlx` 0.9 (`sqlite-bundled`). SQLCipher 4.10.0.
  Gemessen auf macOS und Linux; **Windows nicht gemessen**.
- Mit dem echten Code (Kopie von `4d1c307`, nur `Cargo.toml` ergänzt): alle
  14 Migrationen laufen auf der verschlüsselten Datei, `journal_mode=wal`,
  `foreign_keys=1`; `-wal` ist mitverschlüsselt; 78/78 Tests von
  `persistence-sqlite` grün.
- Schlüssel über `SqliteConnectOptions::pragma("key", …)`. Ein roher
  Schlüssel `"x'<64 Hex>'"` funktioniert (Probe macOS: Rundlauf ok, kein
  Klartext-Header, Marker nicht in der Datei).
- Falscher oder fehlender Schlüssel: Der Pool öffnet trotzdem; erst die erste
  Abfrage scheitert mit `code 26 „file is not a database“`. Läuft zuerst
  `migrate!`, kommt dagegen `code 7 „out of memory“` — irreführend.
- `PRAGMA cipher_log_level` steht auf `WARN`, `NONE` wird angenommen. Unter
  Linux schreibt SQLCipher bei falschem Schlüssel `ERROR CORE … hmac check
  failed` auf stderr; unter macOS kam nichts.
- Klartext → verschlüsselt mit `ATTACH … KEY …` und
  `SELECT sqlcipher_export(…)` funktioniert, `_sqlx_migrations` bleibt erhalten.
- SQLite sinkt von 3.51.3 auf 3.50.4 (Dateiformat kompatibel).
- Im Abhängigkeitsbaum vorhanden: `hkdf` 0.13, `sha2` 0.11 (direkt in
  `app-logic`), `argon2` 0.6, `chacha20poly1305` 0.10, `zeroize`.
- `openssl-src` baut unter MSVC mit Perl; ohne `nasm` mit `no-asm`. Der
  Windows-Job der CI hat keinen eigenen Perl- oder NASM-Schritt.

## 2. Teil 0

1. **Baut die Abhängigkeit auf Windows in der CI?** Nur nach einem Push
   messbar. Erster Lauf endet nach Commit 2 (Umsetzung); erst wenn die CI auf
   allen drei Plattformen grün ist, geht es weiter. Scheitert Windows, meldet
   der Coder den Log-Auszug, und die Spec bekommt eine Klarstellung (z. B.
   Perl-Pfad, `OPENSSL_RUST_USE_NASM`). Unabhängig davon: nichts.
2. **Startablauf im Passwort-Modus.** Der Zustand entsteht vor dem Fenster
   (§1). Der Coder klärt zu Beginn von Etappe 3 und meldet vor dem Bau, wie
   die App bis zur Entsperrung ohne `AppState` startet, ohne dass ein
   Kommando vorher einen halben Zustand sieht (A15). Blockiert A15–A17,
   nicht Etappe 1 und 2.

## 3. Ziel und Nicht-Ziele

Ziel: Keine Datei im Datenverzeichnis enthält Hostnamen, Benutzernamen,
Header-Werte, Secrets oder das MCP-Token im Klartext. Bestehende
Installationen wandeln ohne Datenverlust um. Ohne Schlüssel startet nichts
halb, und nichts wird still neu erzeugt.

Nicht-Ziele:
- Rückbau der feldweisen Verschlüsselung (E11).
- Sicheres Überschreiben des alten Klartexts auf dem Datenträger, in Backups
  oder Schnappschüssen. Wird im Changelog genannt.
- Lesbarkeit für ältere Versionen (E6).
- Export/Import (BL-0081), Sperren nach Leerlauf, Biometrie.
- `license.key` und andere Dateien neben der Datenbank.
- Ein Threat-Model-Dokument (BL-0049); A21 liefert nur den Stoff dafür.

## 4. Anforderungen

### Etappe 1 — SQLCipher, Schlüssel, Umwandlung

- **A1 MUSS** SQLite wird mit SQLCipher gebaut (§1, Messung). `cargo deny`
  bleibt grün; die erzeugten Drittlizenzen (Spec 0099) enthalten die
  Lizenztexte von SQLCipher und OpenSSL.
- **A2 MUSS** Der Datenbankschlüssel ist HKDF-SHA256 aus K mit einem festen,
  versionierten `info` (z. B. `smart-ssh/db-key/v1`), 32 Byte, übergeben als
  roher Schlüssel `x'…'` (kein PBKDF2 von SQLCipher). Weder K noch der
  abgeleitete Schlüssel erscheinen in Log, Fehlertext, DTO oder Diagnosepaket.
- **A3 MUSS** Vor jeder Migration prüft der Start den Schlüssel mit einer
  Leseabfrage. Ein falscher Schlüssel ergibt einen eigenen Fehler
  (`ConnectFailureKind` o. ä.), nie „out of memory“ und nie einen
  Migrationsfehler.
- **A4 MUSS** K wird nur noch neu erzeugt, wenn **keine** Datenbankdatei
  existiert oder die vorhandene Datei Klartext ist (dann waren die feldweise
  verschlüsselten Spalten ohnehin unlesbar — Verhalten wie heute). Bei
  verschlüsselter Datei und fehlendem K gilt A6.
- **A5 MUSS** Schlüsselbund nicht erreichbar (Backend-Fehler oder
  `Unavailable`) und kein Passwort-Modus: Startdialog mit „Erneut versuchen“
  und „Beenden“. Die Datenbank wird nicht geöffnet, nicht verändert, nicht
  umbenannt. „Erneut versuchen“ wiederholt die Prüfung ohne Neustart. Bei
  `Unavailable` nennt der Text die Ursache wie heute (Spec 0071 A12) und ab
  Etappe 3 zusätzlich „Master-Passwort einrichten“ (A16).
- **A6 MUSS** Datei verschlüsselt, K fehlt (`NotFound`, keine Verpackungsdatei):
  Dialog mit „Beenden“ und „Neu anfangen“. „Neu anfangen“ verlangt eine
  zweite Bestätigung, benennt die Datei um in
  `smart-ssh.db.unreadable-<UTC-Zeitstempel>` (samt `-wal`/`-shm` mit
  gleichem Suffix), löscht nichts und startet frisch. Der Dialog nennt den
  neuen Dateinamen.
- **A7 MUSS** Klartext-Datenbank (Header `SQLite format 3\0`) wird beim Start
  vor den Migrationen umgewandelt:
  1. WAL des Originals vollständig einspielen und schließen;
  2. verschlüsselte Kopie in eine Zwischendatei schreiben;
  3. Zwischendatei mit dem Schlüssel öffnen und prüfen: `integrity_check` =
     `ok`, je Tabelle gleiche Zeilenzahl, `_sqlx_migrations` gleich,
     `user_version` gleich;
  4. erst dann Zwischendatei an die Stelle des Originals (atomares Umbenennen),
     alte `-wal`/`-shm` vorher entfernt, sodass sie nie auf die neue Datei
     angewandt werden;
  5. danach Migrationen wie gewohnt.

  Scheitert ein Schritt vor 4, bleibt das Original unverändert, die
  Zwischendatei wird entfernt, und der Start meldet einen Fehler mit Code
  (kein stiller Weiterlauf im Klartext). Eine liegengebliebene Zwischendatei
  aus einem Abbruch wird beim nächsten Start verworfen und neu erzeugt.
- **A8 MUSS** Die Umwandlung ist auch an einer Datei korrekt, die die heutige
  Version (SQLite 3.51.3) mit allen 14 Migrationen geschrieben hat.
- **A9 MUSS** Unbekannte Dateien (weder Klartext-Header noch mit K lesbar,
  z. B. abgeschnitten) werden nicht als „verschlüsselt, K fehlt“ behandelt,
  wenn K vorhanden ist: Fehlerdialog wie heute bei `Other`, Datei unverändert.
- **A10 SOLL** `cipher_log_level` wird auf `NONE` gesetzt. Es entsteht keine
  Umleitung von stderr ins App-Log.

### Etappe 2 — Secrets in der Datenbank

- **A11 MUSS** Ein neuer produktiver `CredentialStore` speichert Secrets in
  einer Tabelle der verschlüsselten Datenbank; `AppState.credential_store`
  zeigt darauf. Gleiche Semantik wie der Schlüsselbund-Store: `NotFound`
  bleibt `NotFound`, `delete` ist idempotent, Fehler als `Backend` ohne
  Secret im Text. Der Schlüsselbund dient danach nur noch K.
- **A12 MUSS** Umzug beim ersten Start mit dem neuen Store: Für jede
  bekannte Referenz (alle Server × Slots, alle Provider) das Secret aus dem
  Schlüsselbund lesen, in die Datenbank schreiben, zurücklesen, vergleichen. `NotFound` im
  Schlüsselbund → übersprungen. Erst wenn **alle** Referenzen erfolgreich
  geschrieben und gleich sind, wird der Umzug als erledigt vermerkt; dann
  (E7) werden die Schlüsselbund-Einträge gelöscht.
- **A13 MUSS** Scheitert beim Umzug ein Lesen mit Backend-Fehler: Startdialog
  wie A5 („Erneut versuchen“/„Beenden“), nichts im Schlüsselbund gelöscht.
  Scheitert nur das Löschen nach erfolgreichem Umzug: Warnung ins Log ohne
  Secret, App startet, das Löschen wird bei jedem Start wiederholt, bis alle
  Einträge weg (oder `NotFound`) sind.
- **A13.1 MUSS** Ist der Umzug noch offen und der Schlüsselbund `Unavailable`
  (K kommt aus dem Master-Passwort, Etappe 3), bietet der Dialog zusätzlich
  „Ohne Übernahme fortfahren“ mit Hinweis, dass im Schlüsselbund gespeicherte
  Secrets dann neu eingegeben werden müssen. Diese Wahl vermerkt den Umzug als
  erledigt und löscht nichts im Schlüsselbund. Ohne diese Wahl bleibt es bei A13.
- **A14 MUSS** Das MCP-Token liegt in der Datenbank (gleicher Store). Ein
  vorhandenes Token aus `settings.json` wird übernommen (gleicher Wert, MCP-
  Clients bleiben gültig) und danach aus `settings.json` entfernt. Erzeugen und
  Erneuern schreiben nur noch in die Datenbank.

### Etappe 3 — Master-Passwort

- **A15 MUSS** Im Passwort-Modus liegt K nur in einer Verpackungsdatei neben der
  Datenbank, nicht im Schlüsselbund. Format (versioniert, z. B. JSON):
  Version, KDF `argon2id` mit Parametern, Salt (≥ 16 Byte, zufällig je
  Verpacken), Nonce, Chiffrat von K mit ChaCha20-Poly1305 unter
  Argon2id(Passwort, Salt); Kopf als zusätzliche authentifizierte Daten.
  Startparameter mindestens m = 64 MiB, t = 3, p = 1. Die App lehnt
  Parameter unter diesem Mindestmaß beim Entsperren ab. Bis zur Entsperrung
  ist kein Kommando außer Entsperren/Beenden/„Neu anfangen“ erreichbar, der
  MCP-Server läuft nicht, keine Verbindung wird aufgebaut.
- **A16 MUSS** Einrichten (Einstellungen oder Startdialog ohne Schlüsselbund):
  Passwort zweimal, Mindestlänge 12 Zeichen, Warnung „ohne Passwort sind alle
  Daten verloren, es gibt keine Wiederherstellung“ mit ausdrücklicher
  Bestätigung (E10). Reihenfolge beim Wechsel Schlüsselbund → Passwort:
  Verpackungsdatei schreiben (atomar), mit dem Passwort entpacken und mit K
  vergleichen, dann erst K aus dem Schlüsselbund löschen.
- **A17 MUSS** Wechsel Passwort → Schlüsselbund: aktuelles Passwort verlangen,
  K in den Schlüsselbund schreiben, zurücklesen, vergleichen, dann
  Verpackungsdatei entfernen. Passwort ändern: altes Passwort verlangen, neu
  verpacken (neues Salt), atomar ersetzen.
- **A18 MUSS** Start im Passwort-Modus zeigt eine Entsperrmaske. Falsches
  Passwort (Authentifizierung der Verpackung scheitert) → sichtbare Meldung,
  erneute Eingabe; nie ein leerer oder neuer Schlüssel, nie Zugriff auf die
  Datenbank. „Neu anfangen“ wie A6, zusätzlich wird die Verpackungsdatei mit
  demselben Suffix umbenannt.
- **A19 MUSS** Liegen Verpackungsdatei **und** Schlüsselbund-Eintrag vor
  (abgebrochener Wechsel), gilt die Verpackungsdatei. Nach erfolgreicher
  Entsperrung wird der Schlüsselbund-Eintrag gelöscht, wenn er gleich K ist;
  ist er verschieden, wird nichts gelöscht und eine Warnung geloggt.
- **A20 MUSS** Die Einstellungen zeigen, welcher Modus aktiv ist.
- **A21 SOLL** Passwort, K und abgeleitete Schlüssel liegen im Speicher in
  Typen, die beim Freigeben überschrieben werden (`zeroize`/`secrecy`), und
  verlassen das Backend nicht. Das Passwort geht vom Frontend genau einmal je
  Vorgang ans Backend.

### Etappe 4 — Texte

- **A22 MUSS** Alle neuen Dialoge und Meldungen in DE und EN; neue Fehlercodes
  in `KNOWN_ERROR_CODES`.
- **A23 MUSS** Erststart-Hinweis: Text nach §8 Punkt 1; der Test zum alten
  Satz wird auf den neuen umgestellt.
- **A24 MUSS** Changelog-Fragment: Datei wird verschlüsselt, ältere Versionen
  können sie danach nicht öffnen, Secrets ziehen aus dem Schlüsselbund in die
  Datenbank, alter Klartext kann in Backups liegen, Master-Passwort ohne
  Wiederherstellung. README: Linux braucht Secret Service **oder** ein
  Master-Passwort.

## 5. Design

- **Schlüsselkette:** K (32 Byte Zufall) → HKDF → Datenbankschlüssel. Der
  Chat-Cipher benutzt K weiterhin direkt, damit vorhandener Inhalt lesbar
  bleibt. K liegt **entweder** im Schlüsselbund **oder** verpackt in der
  Datei — der Modus ergibt sich aus der Existenz der Verpackungsdatei.
- Zwischendatei, Verpackungsdatei und umbenannte Dateien liegen im
  Datenverzeichnis; Unix-Rechte 0600. Namen wählt der Coder, sie stehen im
  Bericht.
- Der neue Store ist synchron wie der Trait (§1); wie er den asynchronen Pool
  anspricht, entscheidet der Coder, ohne Deadlock mit dem Pool der Größe 1.
- Die Tabelle für Secrets und der Vermerk „Umzug erledigt“ kommen per neuer
  Migration. Secrets werden darin nicht zusätzlich feldweise verschlüsselt.
- Nicht-native Startdialoge sind erlaubt, wenn A15 dadurch erst möglich wird;
  ob Etappe 1 native Zwei-Knopf-Dialoge nutzt, entscheidet der Coder.
- Herleitung (Wahl HKDF statt PBKDF2, Argon2-Parameter, Reihenfolge der
  Umwandlung) in der Beilage.

## 6. Sicherheits-Invarianten

- **`CredentialStore`:** neue Implementierung, gleiche Semantik (A11);
  Spec 0071 A14/I4 (`NotFound` ≠ Fehler) und Spec 0098 (kein Bibliothekstext
  im Frontend, `KEYCHAIN_ACCESS_FAILED`) gelten für den Schlüsselbund-Zugriff
  auf K weiter.
- **Fehlerpfade erscheinen im UI** (Spec 0059): A3, A5, A6, A7, A13, A18 —
  keiner endet still oder nur im Log.
- **Keine stillen Rückfälle:** kein Weiterlauf im Klartext, kein neuer
  Schlüssel bei verschlüsselter Datei, kein Rückfall vom Passwort auf den
  Schlüsselbund ohne Nutzerhandlung.
- **Redaction/Log** (Spec 0094): keine Schlüssel, Passwörter, Secrets, kein
  MCP-Token in Log, Diagnosepaket oder DTO; neue Log-Zeilen nur mit festen
  Texten.
- **Neue Datensenken:** Tabelle für Secrets (nur in der verschlüsselten
  Datei), Verpackungsdatei (nur Chiffrat), Zwischendatei (verschlüsselt).
- **MCP:** Token-Wert bleibt bei der Übernahme gleich; kein Kommando ist vor der
  Entsperrung erreichbar (A15).

## 7. Tests

Grundlage ist der Rohdatei-Test aus Spec 0096 (durchsucht Datei, `-wal`,
`-journal`). Marker in jedem Test eindeutig, etwa `host-0101.example`,
`user-0101`, `Header-0101`, `Secret-0101`, `Token-0101`.

- **T0 (Fixture, vor A1):** Eine Datenbankdatei, geschrieben vom heutigen
  Build mit allen 14 Migrationen und Beispielzeilen inkl. aller Marker,
  liegt als Test-Fixture im Repo. Grundlage für T4, T5, T13.
- **T1 (A1–A2, Rohdatei):** Neue Installation, Server, Provider mit Header,
  Secret, MCP-Token anlegen, Pool offen lassen → kein Marker und kein
  Klartext-Header in irgendeiner Datei des Datenverzeichnisses. Scheitert am
  heutigen Stand (Hostname im Klartext).
- **T2 (A2):** Zwei verschiedene K ergeben verschiedene Datenbankschlüssel;
  Datenbankschlüssel ≠ K. Gleiches K ergibt denselben Schlüssel.
- **T3 (A3):** Datei mit K1 anlegen, mit K2 öffnen → eigener Fehlercode, nicht
  „out of memory“, Datei byteweise unverändert, `_sqlx_migrations` unberührt.
- **T4 (A7, A8):** Fixture aus T0 umwandeln → alle Zeilen gleich (je Tabelle),
  Migrationen danach vollständig, kein Marker in den Dateien, kein
  Klartext-Original mehr da, feldweise verschlüsselter Chatinhalt mit K lesbar.
- **T5 (A7, Abbruch):** Umwandlung nach Schritt 2 bzw. 3 abbrechen
  (Fehlerinjektion) → Original byte-gleich, keine Zwischendatei, nächster
  Start wandelt erfolgreich um.
- **T6 (A7, WAL):** Klartext-Datei mit nicht eingespieltem WAL (Zeile nur im
  WAL) → Zeile ist nach der Umwandlung vorhanden; nach der Umwandlung liegt
  keine alte `-wal`-Datei neben der neuen.
- **T7 (A4, A6):** Verschlüsselte Datei, K `NotFound` → kein `set` auf dem
  Store (Test-Store zählt Aufrufe), Datei unverändert. „Neu anfangen“ → Datei
  umbenannt, Inhalt byte-gleich, neue leere Datenbank. Scheitert am heutigen
  Stand (`resolve_or_generate_key` erzeugt still).
- **T8 (A5):** K-Lesen mit Backend-Fehler bzw. `Unavailable` → Datenbank nicht
  geöffnet (Datei-mtime und Inhalt gleich), Ergebnis „Erneut versuchen“
  wiederholt die Prüfung; zweiter Versuch mit funktionierendem Store startet.
- **T9 (A9):** Abgeschnittene Datei, K vorhanden → Fehler `Other`-artig, kein
  „Neu anfangen“-Angebot ohne Nutzerhandlung, Datei unverändert.
- **T10 (A11):** Vertragstests des neuen Stores: `get` nach `set`, `NotFound`,
  idempotentes `delete`, Überschreiben; Fehlertext ohne Secret.
- **T11 (A12, A13):** Test-Schlüsselbund mit Einträgen für zwei Server
  (alle Slots) und einen Provider; ein Slot fehlt → Umzug vollständig, fehlender
  Slot bleibt `NotFound`, alle Schlüsselbund-Einträge gelöscht. Variante: ein
  `get` scheitert mit Backend → nichts gelöscht, kein Vermerk, nächster Start
  zieht um. Variante: `delete` scheitert → App startet, nächster Start löscht.
  Variante (A13.1, nach Etappe 3): Schlüsselbund `Unavailable`, Passwort-Modus →
  ohne Wahl kein Start; mit „Ohne Übernahme fortfahren“ Start, kein `delete`.
- **T12 (A14):** `settings.json` mit Token → Token in der Datenbank mit
  gleichem Wert, Schlüssel aus `settings.json` entfernt; Erneuern schreibt
  nicht in `settings.json`.
- **T13 (A15–A18):** Passwort-Modus einrichten → Schlüsselbund ohne K,
  Verpackungsdatei vorhanden; Neustart mit richtigem Passwort öffnet die
  Datenbank; falsches Passwort → Fehler, keine Datenbankabfrage; Passwort
  ändern → altes Passwort scheitert, neues gelingt, Datenbank byte-gleich;
  zurück auf Schlüsselbund → Verpackungsdatei weg, Start ohne Passwort.
- **T14 (A20, A22):** Frontend: Modusanzeige, Entsperrmaske, Einrichten mit
  Bestätigung, alle neuen Codes übersetzt (DE/EN).

Adversarial (ERHÖHT):
- **T15 Verpackung manipuliert:** ein Byte im Chiffrat, im Salt oder in den
  Parametern geändert → Entsperren scheitert sichtbar, kein Zugriff, nichts
  gelöscht. Parameter unter dem Mindestmaß (m = 8 KiB) bei sonst gültiger
  Verpackung → abgelehnt.
- **T16 Halb abgebrochener Moduswechsel:** Verpackungsdatei und
  Schlüsselbund-Eintrag beide vorhanden (A19), einmal gleich, einmal
  verschieden → richtige Quelle, gelöscht wird nur im Gleich-Fall.
- **T17 Schlüssel in Ausgaben:** Log-Datei und Diagnosepaket nach T1, T3,
  T11, T13 enthalten weder K (Hex/Base64), Datenbankschlüssel, Passwort noch
  einen Marker.
- **T18 Kommando vor Entsperrung:** Im gesperrten Zustand gibt ein
  Daten-Kommando (z. B. Serverliste) und eine MCP-Anfrage einen Fehler, kein
  Panic, keine Daten.
- **T19 Fremde Zwischendatei:** Am Ort der Zwischendatei liegt eine fremde,
  größere Datei → wird verworfen, nicht als fertige Umwandlung übernommen.
- **T20 Symlink:** `smart-ssh.db` ist ein Symlink auf eine Datei außerhalb des
  Datenverzeichnisses → Umwandlung überschreibt nicht das Ziel des Symlinks
  stillschweigend; Verhalten im Bericht (Ablehnung mit Meldung ist erwünscht).

## 8. Offene Punkte

1. **Erststart-Hinweis (rechtliche Wirkung, K3).** Vorschlag DE: „Die lokale
   Datenbank ist verschlüsselt. Den Schlüssel verwahrt der Schlüsselbund des
   Betriebssystems oder – wenn Sie das einrichten – Ihr Master-Passwort. Wer
   Zugriff auf Ihr entsperrtes Benutzerkonto hat, kann die Daten lesen.“ EN
   sinngemäß. Empfehlung: so übernehmen; die Handprüfung beider Sprachen
   läuft wie gehabt über den Erststart-Test.

## 9. Klarstellungen

(wird während der Umsetzung nachgetragen)

## Umsetzung

**Teil 0** — siehe §2. Lauf 1 endet nach Commit 2 und meldet; weiter erst
nach grüner CI auf allen Plattformen. Frage 2 vor dem Bau von Etappe 3 melden.

**Reihenfolge:**
1. `test(persistence): add a database fixture written by the current SQLite build [BL-0314]` — T0
2. `build(persistence): build SQLite with SQLCipher [BL-0314]` — A1, bestehende Tests grün, noch ohne Schlüssel. **Ende Lauf 1.**
3. `feat(core): derive the database key and never create a root key over an encrypted database [BL-0314]` — A2, A4, T2, T7 (Teil)
4. `feat(persistence): open the database with its key and convert a plaintext database [BL-0314]` — A3, A7–A10, T1, T3–T6, T9, T19, T20
5. `feat(app-shell): stop at startup when the key is missing or unreachable [BL-0314]` — A5, A6, T7, T8, T17 (Teil)
6. `feat(persistence): keep credentials in the encrypted database [BL-0314]` — A11, T10
7. `feat(app-shell): move secrets from the OS keychain into the database [BL-0314]` — A12, A13, A13.1, T11
8. `feat(app-shell): keep the MCP server token in the database [BL-0117]` — A14, T12
9. `feat(core): wrap the root key with a master password [BL-0203]` — A15, A21, T15
10. `feat(app-shell): unlock with a master password at startup [BL-0203]` — A15–A19, T13, T16, T18
11. `feat(frontend): choose between keychain and master password [BL-0203]` — A20, A22 (Teil), T14
12. `docs: describe database encryption in the first-run notice, README and changelog [BL-0314]` — A22–A24

**Priorität:** ERHÖHT. Angriffsrichtungen für den Review:
- Ein Pfad, der doch still einen neuen K erzeugt: Fehlerart verwechselt
  (`Backend` als `NotFound`), Klartext-Erkennung an einer verschlüsselten Datei
  falsch (T7, T9).
- Alte `-wal` wird auf die neue Datei angewandt oder ein Abbruch hinterlässt
  eine halbe Datei als „fertig“ (T5, T6, T19).
- Schlüssel, Passwort oder Secret landen über `Debug`, `format!("{e}")`,
  einen SQL-Fehlertext mit dem `PRAGMA key`-Wert oder ein `tracing`-Feld im Log
  (T17).
- Schlüsselbund-Einträge werden gelöscht, obwohl ein Secret nicht gleich in der
  Datenbank liegt (T11).
- Ein Kommando oder der MCP-Server läuft vor der Entsperrung (T18).
- Manipulierte Verpackung mit schwachen Parametern oder vertauschtem Kopf wird
  angenommen (T15).
- Abgebrochener Moduswechsel löscht die einzige Kopie von K (T16).

**Aufteilung:**
- Lauf 1, **Sonnet**: Commits 1–2. Danach Push und CI (Teil 0 Frage 1).
- Lauf 2, **Opus**: Commits 3–8 (Etappe 1 Rest und 2; gemeinsame Module, Kern).
- Lauf 3, **Opus**: Commits 9–11 (Etappe 3), beginnt mit Teil 0 Frage 2.
- Lauf 4, **Sonnet**: Commit 12; Hinweis „Kern ist fertig und geprüft“.

**Berührte Module:** `crates/persistence-sqlite` (Cargo, Öffnen, Migration,
neuer Store), `crates/core/src/crypto`, `crates/credentials-keyring`,
`crates/app-logic` (Startfehler-Texte, `server_credentials.rs`,
`identity_file.rs`, Logging-Allowlist), `crates/app-shell` (`lib.rs`,
`startup_dialog.rs`, `mcp_settings.rs`, `commands/ai_providers.rs`),
Frontend (Einstellungen, Entsperrmaske, `errorCodes.ts`, Locales,
`FirstRunNoticeScreen`), `README.md`, `changelog.d/`, Workflows nur falls
Teil 0 Frage 1 es verlangt.

**Melde zurück:** Teil-0-Befunde (CI-Ergebnis je OS, Startablauf
Passwort-Modus); Dateinamen und Format der Verpackungs- und Zwischendatei;
Bauzeit- und Größenzuwachs der App; Beleg, dass T1 und T7 am alten Stand
scheitern; manuelle Testabläufe: Umwandlung einer echten 0.5.2-Datenbank,
macOS-Schlüsselbund ablehnen, Linux ohne Secret Service mit Master-Passwort,
falsches Passwort, „Neu anfangen“.
