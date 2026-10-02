# Spec 0101 — Ganze Datenbank verschlüsseln, Secrets in die Datenbank, Master-Passwort

Status: freigegeben · Backlog: BL-0314, BL-0297, BL-0117, BL-0203 · Gate: release-1.0/C
Zweck: Die Datenbankdatei ist vollständig verschlüsselt (SQLCipher), alle Secrets liegen darin, und ihr Schlüssel kommt entweder aus dem OS-Schlüsselbund oder aus einem Master-Passwort.
Review-Priorität: ERHÖHT (Verschlüsselung, Credentials, Migration, Start)

## Getroffene Entscheidungen

- **E1** DB-Schlüssel aus dem vorhandenen Zufallsschlüssel
  `app:chat_content_encryption_key` (**Wurzelschlüssel K**), kein neuer Eintrag.
- **E2** Alle Secrets (Server-Passwörter, Private Keys, Passphrasen,
  Zertifikate, Sudo-Passwörter, API-Keys, MCP-Token) in die Datenbank; im
  Schlüsselbund bleibt höchstens K.
- **E3** Schlüsselbund beim Start nicht erreichbar → „Erneut versuchen“ /
  „Beenden“, nichts wird angefasst.
- **E4** Datei verschlüsselt, K fehlt → nie still ein neuer K; „Neu anfangen“
  nur bewusst, alte Datei umbenannt.
- **E5** Klartext-Datenbank beim ersten Start automatisch umwandeln, Original
  nach erfolgreicher Prüfung entfernen.
- **E6** Ältere Versionen können die Datei nicht mehr öffnen (Changelog).
- **E7** Alte Schlüsselbund-Einträge löschen, sobald jedes Secret aus der
  Datenbank zurückgelesen und gleich ist.
- **E8** Master-Passwort für alle wählbar, Standard Schlüsselbund, Wechsel in
  beide Richtungen; ohne Schlüsselbund bietet der Startdialog es an.
- **E9** Passwort-Modus: K mit Argon2id(Passwort) verpackt in einer Datei
  neben der Datenbank; Wechsel und Passwortänderung verpacken nur neu.
- **E10** Keine Wiederherstellung bei vergessenem Passwort; Warnung und
  Bestätigung beim Einrichten.
- **E11** Feldweise Verschlüsselung (Chat, Ledger, Historie,
  Zusammenfassungen) bleibt in dieser Spec; der Rückbau folgt als eigenes
  Item (BL-0318).

## 1. Ist-Stand (Stand `4d1c307`)

Gelesen, nicht ausgeführt, soweit nicht als Messung gekennzeichnet.

**Datenbank.** Eine Datei `smart-ssh.db` im Datenverzeichnis
(`persistence_sqlite::default_db_path`, Override `SMART_SSH_DATA_DIR`).
`SqliteProfileStore::connect` legt sie an (`create_if_missing`) und ruft
`connect_with`; dort `foreign_keys(true)`, Pool `max_connections(1)`, direkt
danach `sqlx::migrate!`. Einziger Produktivaufruf: `build_app_state` in
`app-shell/src/lib.rs`. Alle Stores teilen diesen Pool. WAL setzt nur
Migration `0001`. Kein anderer Prozess öffnet die Datei; `mcp-server` hängt
nicht an `persistence-sqlite`.

**Startreihenfolge** (`build_app_state`): Log → Datenbank öffnen und migrieren
→ `probe_keychain_availability` → `resolve_or_generate_key` → Cipher und
verschlüsselnde Stores → Host-Key-Store. Der Zustand entsteht **vor**
`tauri::Builder::setup`, also bevor ein Fenster existiert.

**Wurzelschlüssel.** `crypto::key::resolve_or_generate_key` liest
`app:chat_content_encryption_key` (32 Byte, Base64). Bei `NotFound`
**erzeugt es still einen neuen Schlüssel**; bei Backend-Fehler
`KeyStoreAccessFailed`; bei ungültigem Inhalt `InvalidKey`. Im Fehlerfall
startet die App eingeschränkt: Chat, Eingabe-Historie und Ledger sind `None`,
dazu `startup_dialog::show_warning` (Spec 0059 Fall 3, Spec 0071 A12).

**Secrets.** Trait `CredentialStore` (`core/src/profiles/credentials.rs`,
synchron: `get`/`set`/`delete`, Fehler `NotFound`/`Backend`). Produktiv nur
`KeyringCredentialStore` (Dienst `"Smart SSH"`, Account = `CredentialRef`),
ohne Cache. Einträge `server:{id}:{slot}` mit den Slots `password`,
`private_key`, `passphrase`, `certificate`, `certificate_key`,
`sudo_password` sowie `ai-provider:{id}`. Die Referenzen stehen auch in der
Datenbank: `ai_provider_configs.credential_ref` und im `auth_method`-JSON der
Server. `keychain_aware_credential_error` (`app-logic/src/error.rs`) macht aus
jedem `Backend` die Codes `KEYCHAIN_ACCESS_FAILED`/`KEYCHAIN_UNAVAILABLE`
(Spec 0098). Rund 18 DE-Texte sagen, Secrets lägen im Schlüsselbund (z. B.
`serverForm.convertToKeychain.*`, `secretWillBeDeleted`). Zweiter Weg zum
Frontend: `SshError::CredentialStoreFailed` (`core/src/ssh/error.rs`) trägt
fest `KEYCHAIN_ACCESS_FAILED` und den Text „Zugriff auf den Schlüsselbund
fehlgeschlagen“; `keychain_aware_ssh_error_code` und `test_connection.rs`
bauen darauf auf.

**Klartext in der Datei heute:** `servers` (Host, Benutzer), Notizen,
Filterregeln, Sitzungstitel, `ai_provider_configs.extra_headers` (JSON).

**MCP-Token.** `mcp_settings::load_or_init_token` hält das Token im
Tauri-Store `settings.json` im **Konfigurationsverzeichnis** (Schlüssel
`mcpServerToken`), Rechte 0600 nur unter Unix.

**Startdialoge.** `startup_dialog` nutzt rfd 0.16 nur mit „OK“; rfd kann bis
zu drei eigene Knöpfe (`common-controls-v6` ist an), keine Texteingabe. DB-Fehlerarten
`ConnectFailureKind::{SchemaTooNew, PermissionDenied, Other}`; der `Other`-Text
empfiehlt, ein Backup einzuspielen.

**Erststart-Hinweis.** `firstRunNotice.encryptionText` (DE/EN): „Die lokale
Datenbank ist nicht zusätzlich verschlüsselt …“, geprüft in
`FirstRunNoticeScreen.test.tsx`.

**Logs.** Nur Datei (`app-logic/src/logging.rs`); stderr wird nirgends ins
Log umgeleitet. Diagnosepaket mit Allowlist `SAFE_LOG_MESSAGES`.

**Messungen** (Protokoll in der Beilage, macOS; Linux wo genannt):
- `libsqlite3-sys = "=0.37.0"` mit `bundled-sqlcipher-vendored-openssl` neben
  `sqlx` 0.9: SQLCipher 4.10.0, 14 Migrationen, WAL mitverschlüsselt, 78/78
  Tests, `cargo audit` ohne Funde (macOS, Linux). **Windows nicht gemessen.**
- Roher Schlüssel `"x'<64 Hex>'"` über `pragma("key", …)` funktioniert.
- Falscher oder fehlender Schlüssel: Pool öffnet, erste Abfrage `code 26 „file
  is not a database“`; läuft zuerst `migrate!`, kommt `code 7 „out of memory“`.
  Falscher Schlüssel und beschädigte Datei sind so nicht unterscheidbar.
- `cipher_log_level` Standard `WARN`, `NONE` angenommen (Linux schreibt sonst
  auf stderr).
- `ATTACH … KEY "x'…'"` + `sqlcipher_export` wandelt um, **überträgt aber
  weder `journal_mode` (danach `delete`) noch `user_version` (danach 0)**.
- SQLite sinkt von 3.51.3 auf 3.50.4.
- Vorhanden: `hkdf` 0.13, `sha2` 0.11, `argon2` 0.6, `chacha20poly1305`, `zeroize`.

## 2. Teil 0

1. **Baut die Abhängigkeit auf Windows in der CI?** Nur nach einem Push
   messbar. Lauf 1 endet nach Commit 2. Weiter erst, wenn die CI auf allen
   drei Plattformen grün ist; sonst Log-Auszug melden, Klarstellung folgt.
2. **Synchroner Store auf asynchronem Pool** (vor Commit 6): Wie der
   synchrone `CredentialStore` aus asynchronen Kommandos die Datenbank
   erreicht, ohne `block_on` in der Laufzeit und ohne Deadlock mit dem Pool
   der Größe 1. Der Coder klärt das selbst und hält es im Bericht fest; er
   hält nur an und fragt, wenn dafür ein zweiter Pool, eine zweite Verbindung
   oder eine Änderung am Trait nötig wäre.
3. **Startablauf im Passwort-Modus** (zu Beginn von Lauf 3 melden): wie die
   App bis zur Entsperrung ohne `AppState` läuft (A16); wo Einrichten aus dem
   Startdialog (A13) und der Dialog aus A11/A11.1 im Passwort-Modus erscheinen
   (rfd hat keine Texteingabe); ob Tauri 2 ein Kommando mit nicht verwaltetem
   `State` mit Fehler oder Panic beantwortet. Blockiert A11.1, A13–A17.

## 3. Ziel und Nicht-Ziele

Ziel: Keine Datei im Daten- und im Konfigurationsverzeichnis enthält
Hostnamen, Benutzernamen, Header-Werte, Secrets oder das MCP-Token im
Klartext. Bestehende Installationen wandeln ohne Datenverlust um. Ohne
Schlüssel startet nichts halb, und nichts wird still neu erzeugt.

Nicht-Ziele:
- Rückbau der feldweisen Verschlüsselung (E11).
- Sicheres Überschreiben des alten Klartexts auf dem Datenträger, in Backups
  oder Schnappschüssen (Changelog).
- **Eine verständliche Meldung in älteren Versionen** (Akzeptanz des Items):
  entfällt wegen E6, alte Versionen sind nicht änderbar. Ersatz: A22.
- Maskierte Anzeige der Provider-Header in der Oberfläche: Das ist eine Frage
  der Anzeige, nicht der Ablage; sie bleibt als Rest von BL-0297 offen.
- Export/Import, Sperren nach Leerlauf, Biometrie, `license.key`,
  Threat-Model-Dokument.

## 4. Anforderungen

### Etappe 1 — SQLCipher, Schlüssel, Umwandlung

- **A1 MUSS** SQLite wird mit SQLCipher gebaut (§1). `cargo deny` und
  `cargo audit` bleiben grün; die erzeugten Drittlizenzen enthalten die
  Lizenztexte von SQLCipher und OpenSSL.
- **A2 MUSS** Datenbankschlüssel = HKDF-SHA256(K, `info` =
  `smart-ssh/db-key/v1`), 32 Byte, als roher Schlüssel `x'…'` (kein
  PBKDF2). Weder K noch der abgeleitete Schlüssel erscheinen in Log,
  Fehlertext, DTO oder Diagnosepaket.
- **A3 MUSS** Der Start entscheidet nach dieser Tabelle, **bevor** eine
  Migration läuft. Dateizustand wird am Header erkannt, ohne die Datei zu
  ändern: *fehlt* (keine Datei, oder 0 Byte ohne nichtleere `-wal`),
  *Klartext* (`SQLite format 3\0`), *sonst*. K-Zustand: *da*, *NotFound*,
  *nicht erreichbar*, *ungültig* (`InvalidKey`; Verpackungsdatei, deren
  Format oder Version nicht lesbar ist). Maßgeblich ist das Ergebnis des
  Lesens von K, nicht die Probe beim Start: *nicht erreichbar* heißt, das
  `get` scheitert mit `Backend` oder wird wegen `Unavailable` gar nicht
  versucht. Im Passwort-Modus ist K erst nach der Entsperrung (A16) *da*;
  falsches Passwort ist kein Tabellenfall.

  | Datei \ K | da | NotFound | nicht erreichbar | ungültig |
  |---|---|---|---|---|
  | fehlt | neu anlegen | K erzeugen, neu anlegen | Dialog D1 | Dialog D3 |
  | Klartext | umwandeln (A6) | K erzeugen, umwandeln | Dialog D1 | Dialog D4 |
  | sonst | öffnen; nicht lesbar → Dialog D2 | Dialog D2 | Dialog D1 ohne Einrichten | Dialog D3 |

  - **D1** „Erneut versuchen“ / „Beenden“; ab Etappe 3 zusätzlich
    „Master-Passwort einrichten“ — **nur** bei Datei *fehlt* oder
    *Klartext* **und** Grund `NoSecretServiceProvider` oder `NoSessionBus`
    (dort kann kein erreichbarer K existieren; bei `Locked`, `Unknown` oder
    einem Backend-Fehler würde ein neuer K den feldweise verschlüsselten
    Verlauf unlesbar machen). Bei *Klartext* nennt der Text den Verlust des
    bisherigen Verlaufs wie D4. Ursache bei `Unavailable` wie Spec 0071 A12.
    „Erneut versuchen“ prüft ohne Neustart erneut.
  - **D2** eigener Code (z. B. `DB_KEY_MISMATCH`): Datei mit dem vorhandenen
    Schlüssel nicht lesbar bzw. kein Schlüssel zur verschlüsselten Datei.
    Text: beschädigt oder zu einem anderen Schlüssel gehörig, **kein**
    Hinweis auf „Backup einspielen“ als erste Wahl. Knöpfe „Beenden“ /
    „Neu anfangen“ (A5).
  - **D3** „Schlüssel unbrauchbar“: „Beenden“ / „Neu anfangen“ (A5).
    Der vorhandene Eintrag bzw. die Datei wird erst nach dieser Wahl ersetzt.
  - **D4** wie D3, aber statt „Neu anfangen“ „Neuen Schlüssel erzeugen“: Die
    Klartext-Datei bleibt lesbar und wird mit neuem K umgewandelt (A6); nur
    der feldweise verschlüsselte Verlauf geht verloren, das sagt der Text.
    Zweite Bestätigung wie A5; im Passwort-Modus Reihenfolge wie A5 (neues
    Passwort zuerst, alte Verpackungsdatei umbenennen, nie überschreiben).

  In keinem Dialog-Fall wird die Datenbank geöffnet, verändert oder
  umbenannt, solange der Nutzer nicht gewählt hat. **Ein neuer K entsteht
  nur** in den beiden Feldern „K erzeugen“ und nach einer ausdrücklichen Wahl
  „Neu anfangen“ (D2, D3, A16), „Neuen Schlüssel erzeugen“ (D4) oder
  „Master-Passwort einrichten“ (D1).
- **A4 MUSS** Nie „out of memory“ oder ein Migrationsfehler für einen
  Schlüssel-Fall: Die Lesbarkeit wird vor `migrate!` geprüft.
- **A5 MUSS** „Neu anfangen“ verlangt eine zweite Bestätigung, benennt Datei,
  `-wal`, `-shm` und (Passwort-Modus) die Verpackungsdatei um in
  `<name>.unreadable-<UTC-Zeitstempel>`, löscht nichts, startet frisch und
  nennt im Dialog den neuen Dateinamen. Im Passwort-Modus bleibt der Modus:
  das neue Master-Passwort wird **zuerst** eingerichtet (A13), die neue
  Verpackungsdatei geschrieben, erst dann werden die alten Dateien umbenannt.
  Bricht der Nutzer das Einrichten ab, bleibt alles unverändert.
- **A6 MUSS** Umwandlung einer Klartext-Datei, vor den Migrationen:
  1. Original öffnen, WAL vollständig einspielen, schließen;
  2. verschlüsselte Kopie in eine Zwischendatei schreiben; die Zwischendatei
     mit dem Schlüssel öffnen, `user_version` übernehmen und
     `journal_mode=WAL` setzen (der Export überträgt beides nicht; auf der
     geöffneten Datei gesetzt, bleibt beides erhalten — gemessen), schließen;
  3. Zwischendatei neu öffnen und prüfen: `integrity_check` =
     `ok`, je Tabelle gleiche Zeilenzahl, `_sqlx_migrations` gleich,
     `user_version` gleich, `journal_mode` = `wal`; schließen;
  4. alte `-wal`/`-shm` entfernen, dann Zwischendatei atomar an die Stelle des
     Originals umbenennen;
  5. Migrationen wie gewohnt.

  Scheitert ein Schritt vor 4: Original inhaltsgleich, Zwischendatei
  entfernt, Startfehler mit Code (kein Weiterlauf im Klartext). Eine
  liegengebliebene Zwischendatei wird beim nächsten Start verworfen, nie
  übernommen. Ist `smart-ssh.db` ein Symlink, wird nicht umgewandelt:
  Startfehler mit Meldung, nichts verändert.
- **A7 MUSS** Die Umwandlung gelingt auch an einer Datei, die der heutige
  Build (SQLite 3.51.3) mit allen 14 Migrationen geschrieben hat.
- **A8 SOLL** `cipher_log_level` = `NONE`. Keine Umleitung von stderr ins Log.

### Etappe 2 — Secrets in der Datenbank

- **A9 MUSS** Ein neuer produktiver `CredentialStore` speichert Secrets in
  einer Tabelle der verschlüsselten Datenbank; `AppState.credential_store`
  zeigt darauf. Semantik wie bisher: `NotFound` bleibt `NotFound`, `delete`
  idempotent, `Backend` ohne Secret im Text. Der Schlüsselbund dient danach
  nur noch K.
- **A9.1 MUSS** Fehler dieses Stores erreichen das Frontend mit einem eigenen
  Code (z. B. `SECRET_STORE_FAILED`) und ohne „Schlüsselbund“ im Text, nicht
  mit `KEYCHAIN_*`, und hängen nicht von `AppState.keychain` ab — auf beiden
  Wegen aus §1, auch beim Verbindungsaufbau und im Verbindungstest auf jedem
  Hop. Für das Lesen und Schreiben von K gilt Spec 0098
  unverändert.
- **A10 MUSS** Umzug beim ersten Start mit dem neuen Store: Referenzen aus der
  Datenbank (`credential_ref`, `auth_method`, plus die festen Sudo-Slots), je
  Referenz das Secret aus dem Schlüsselbund lesen, schreiben, zurücklesen,
  vergleichen; `NotFound` → ausgelassen. Zustand in der Datenbank:
  *offen* → *umgezogen, Löschen ausstehend* (erst wenn **alle** Referenzen
  gleich sind) → *erledigt* (alle Einträge gelöscht oder `NotFound`). Beim
  Übergang nach *umgezogen* wird die Liste der zu löschenden Referenzen
  festgehalten; gelöscht wird nach dieser Liste, nicht nach dem späteren
  Datenbankstand.
- **A11 MUSS** Lesefehler (Backend) beim Umzug: Dialog D1 ohne Einrichten
  (im Passwort-Modus plus A11.1), nichts gelöscht,
  Zustand bleibt *offen*. Löschfehler im Zustand *umgezogen*: Warnung ins Log
  ohne Secret, App startet, Löschen bei jedem Start erneut.
- **A11.1 MUSS** Zustand *offen*, K aus dem Master-Passwort, Lesen beim
  Umzug scheitert (`Backend` oder `Unavailable`): Der Dialog bietet zusätzlich „Ohne Übernahme fortfahren“
  (Hinweis: gespeicherte Passwörter usw. neu eingeben). Diese Wahl setzt den
  Zustand *übersprungen*. In *übersprungen* wird **nie** ein
  Schlüsselbund-Eintrag gelöscht, auch wenn der Schlüsselbund später
  erreichbar ist.
- **A12 MUSS** Das MCP-Token liegt in der Datenbank. Ein Token aus
  `settings.json` wird mit gleichem Wert übernommen, zurückgelesen, dann dort
  entfernt. Erzeugen und Erneuern schreiben nur in die Datenbank.

### Etappe 3 — Master-Passwort

- **A13 MUSS** Einrichten — aus den Einstellungen, aus D1 (nur Datei
  *fehlt*/*Klartext*) oder im Passwort-Modus aus A5/D4: Passwort zweimal, mindestens 12 Zeichen, Warnung „ohne
  Passwort sind alle Daten verloren, keine Wiederherstellung“ mit
  ausdrücklicher Bestätigung (E10). Schreibreihenfolge: K (vorhanden oder
  neu nach A3) verpacken, Verpackungsdatei atomar schreiben, entpacken und mit
  K vergleichen, dann erst K aus dem Schlüsselbund löschen bzw. umwandeln.
  Aus D1 gibt es keinen K im Schlüsselbund; dort wird K neu erzeugt (A3).
- **A14 MUSS** Verpackungsdatei: versioniert; KDF `argon2id` mit Parametern,
  Salt ≥ 16 Byte zufällig je Verpacken, Nonce, Chiffrat von K mit
  ChaCha20-Poly1305 unter Argon2id(Passwort, Salt), Kopf als AAD.
  Parameter beim Schreiben m = 64 MiB, t = 3, p = 1; beim Entpacken werden
  Parameter darunter abgelehnt. Unix-Rechte 0600.
- **A15 MUSS** Wechsel Passwort → Schlüsselbund: aktuelles Passwort, K in den
  Schlüsselbund schreiben, zurücklesen, vergleichen, dann Verpackungsdatei
  entfernen. Passwort ändern: altes Passwort, neu verpacken (neues Salt),
  atomar ersetzen.
- **A16 MUSS** Start im Passwort-Modus: Entsperrmaske. Bis zur Entsperrung ist
  kein Kommando außer Entsperren / Beenden / „Neu anfangen“ erreichbar, der
  MCP-Server läuft nicht, keine Verbindung wird aufgebaut. Falsches Passwort →
  sichtbare Meldung, erneute Eingabe, kein Datenbankzugriff, nie ein neuer
  Schlüssel.
- **A17 MUSS** Verpackungsdatei **und** Schlüsselbund-Eintrag (abgebrochener
  Wechsel): Die Verpackungsdatei gilt. Nach Entsperrung wird der Eintrag
  gelöscht, wenn er gleich K ist; sonst nichts gelöscht, Warnung ins Log.
  Ist der Schlüsselbund nicht erreichbar: nichts tun, Warnung ins Log.
  Scheitert die Authentifizierung der Verpackung, lautet die Meldung
  „Passwort falsch oder Datei beschädigt“ — beides ist nicht unterscheidbar.
- **A18 MUSS** Die Einstellungen zeigen den aktiven Modus.
- **A19 SOLL** Passwort, K und abgeleitete Schlüssel liegen in Typen, die beim
  Freigeben überschrieben werden, und verlassen das Backend nicht.

### Etappe 4 — Texte

- **A20 MUSS** Alle neuen Dialoge, Masken und Codes in DE und EN; Codes in
  `KNOWN_ERROR_CODES`. Bestehende Texte, die Secrets im Schlüsselbund
  verorten, sagen künftig „in der verschlüsselten Datenbank“; die Liste der
  geänderten Schlüssel steht im Bericht.
- **A21 MUSS** Erststart-Hinweis nach §9 Punkt 1; Test auf den neuen Satz.
- **A22 MUSS** Changelog-Fragment: Datei verschlüsselt; ältere Versionen
  melden danach „möglicherweise beschädigt … Backup einspielen“ — **das
  stimmt dann nicht, kein Backup einspielen**, sondern die neue Version
  nutzen; Secrets ziehen in die Datenbank; alter Klartext kann in Backups
  liegen; Master-Passwort ohne Wiederherstellung. README: Linux braucht
  Secret Service **oder** ein Master-Passwort.

## 5. Design

**Startablauf (Ziel):** 1. Log. 2. Dateizustand (A3). 3. K beschaffen:
Verpackungsdatei vorhanden → Entsperren (A16), sonst Schlüsselbund.
4. Tabelle A3. 5. Ggf. Umwandlung (A6). 6. Öffnen, Lesbarkeit,
Migrationen. 7. Secrets-Umzug (A10–A11.1). 8. MCP-Token (A12). 9. Übriger
Zustand, dann MCP-Server.

- Der Chat-Cipher nutzt K weiter direkt. Der Modus ergibt sich aus der
  Existenz der Verpackungsdatei.
- Zwischen-, Verpackungs- und umbenannte Dateien im Datenverzeichnis,
  Unix-Rechte 0600; Namen im Bericht.
- Secrets-Tabelle und Umzugszustand per neuer Migration; Secrets dort nicht
  zusätzlich feldweise verschlüsselt.
- Etappe 1 darf native Zwei- und Drei-Knopf-Dialoge nutzen; ob Etappe 3 die
  Startdialoge ins Fenster verlegt, folgt aus Teil 0 Frage 3.
- Herleitungen (HKDF, Argon2-Parameter, Umwandlungsreihenfolge) in der Beilage.

## 6. Sicherheits-Invarianten

- **`CredentialStore`:** neue Implementierung, gleiche Semantik (A9);
  Spec 0071 A14/I4 gilt für beide Stores; Spec 0098 für den Zugriff auf K.
- **Fehlerpfade im UI** (Spec 0059): jeder Fall aus A3, A6, A11, A11.1, A16, A17 hat einen
  Dialog oder eine Meldung, keiner endet still.
- **Keine stillen Rückfälle:** kein Weiterlauf im Klartext, kein neuer K außer
  in den Fällen aus A3 („Ein neuer K entsteht nur …“), kein Wechsel des Modus ohne Nutzerhandlung.
- **Log/Redaction** (Spec 0094): keine Schlüssel, Passwörter, Secrets, kein
  Token in Log, Diagnosepaket oder DTO; neue Log-Zeilen mit festen Texten.
- **Neue Datensenken:** Secrets-Tabelle (nur in der verschlüsselten Datei),
  Verpackungsdatei (nur Chiffrat), Zwischendatei (verschlüsselt).
- **MCP:** Token-Wert bleibt bei der Übernahme gleich; vor der Entsperrung
  nicht erreichbar.

## 7. Tests

Grundlage: der Rohdatei-Test aus Spec 0096 (Datei, `-wal`, `-journal`).
Marker je Test eindeutig: `host-0101.example`, `user-0101`, `Header-0101`,
`Secret-0101`, `Token-0101`.

- **T0 (Fixture, vor A1):** Datenbankdatei vom heutigen Build, 14 Migrationen,
  Beispielzeilen mit allen Markern, feldweise verschlüsselter Chatinhalt unter
  einem festen Test-K. Grundlage für T4–T6.
- **T1 (A1–A2, Rohdatei, nach Commit 8):** Neue Installation; Server, Provider
  mit Header, Secret, MCP-Token anlegen, Pool offen → kein Marker, kein
  Klartext-Header in Daten- und Konfigurationsverzeichnis. Teil ohne Secret
  und Token schon in Commit 4. Scheitert heute (Hostname im Klartext).
- **T2 (A2):** Known-Answer: festes K → fester, im Test hinterlegter
  Hex-Schlüssel; dazu verschiedene K → verschiedene Schlüssel, Schlüssel ≠ K.
  Scheitert bei jeder Änderung an `info` oder Verfahren.
- **T3 (A3/A4, Tabelle):** je Feld der Tabelle ein Fall mit Test-Store, der
  Aufrufe zählt: erwarteter Ausgang, `set` auf K ohne Nutzerwahl nur in den
  zwei „K erzeugen“-Feldern, Datei in allen Dialog-Fällen byte-gleich, solange
  nichts gewählt ist, nie Code 7.
  Scheitert heute (`resolve_or_generate_key` erzeugt bei verschlüsselter Datei).
- **T4 (A6, A7):** Fixture umwandeln → Zeilen je Tabelle gleich,
  `user_version` gleich, `journal_mode=wal`, Migrationen vollständig, kein
  Marker, kein Klartext-Original, Chatinhalt mit Test-K lesbar.
- **T5 (A6, Abbruch):** Fehlerinjektion nach Schritt 2 und nach Schritt 3 →
  Original inhaltsgleich, keine Zwischendatei, nächster Start wandelt um.
- **T6 (A6, WAL):** Zeile nur im WAL des Originals → nach der Umwandlung
  vorhanden; keine alte `-wal` neben der neuen Datei.
- **T7 (A5, D4):** „Neu anfangen“ aus D2 und D3 → Dateien umbenannt,
  byte-gleich, Dialogtext nennt den Namen, neue leere Datenbank; ohne zweite
  Bestätigung passiert nichts. D4 → Klartext-Datei mit neuem K umgewandelt,
  Zeilen erhalten. Variante Passwort-Modus (Commit 10) für A5 und D4: Einrichten
  abgebrochen → alle Dateien unverändert; durchgeführt → alte
  Verpackungsdatei umbenannt (nicht überschrieben), kein `set` auf den
  Schlüsselbund.
- **T8 (D1):** K nicht erreichbar → Datenbank nicht geöffnet (Inhalt und
  mtime gleich); „Erneut versuchen“ mit danach funktionierendem Store startet.
- **T9 (A1):** Drittlizenz-Ausgabe enthält SQLCipher und OpenSSL.
- **T10 (A9, A9.1):** Vertragstests des neuen Stores (`get` nach `set`,
  `NotFound`, idempotentes `delete`, Überschreiben); Fehler ergibt
  `SECRET_STORE_FAILED` ohne Secret, auch bei `KeychainAvailability::Unavailable`.
  Dazu Verbindungstest mit Jump-Host, dessen Secret-Lesen am neuen Store
  scheitert → `SECRET_STORE_FAILED`, kein `KEYCHAIN_*`, kein „Schlüsselbund“.
- **T11 (A10–A11.1):** Test-Schlüsselbund mit zwei Servern (alle Slots) und
  einem Provider, ein Slot fehlt → alles umgezogen, Slot bleibt `NotFound`,
  alle Einträge gelöscht. Varianten: ein `get` scheitert → nichts gelöscht,
  Zustand *offen*; `delete` scheitert → Start, nächster Start löscht;
  Server gelöscht, während Löschen aussteht → seine Einträge werden trotzdem
  gelöscht;
  *übersprungen* (nach Commit 10), danach Schlüsselbund erreichbar → kein
  einziges `delete`; Schlüsselbund-Modus mit scheiterndem `get` → keine
  Option „Ohne Übernahme“.
- **T12 (A12):** `settings.json` mit Token → Token in der Datenbank, gleicher
  Wert, Schlüssel aus `settings.json` entfernt; Erneuern schreibt nicht dorthin.
- **T13 (A13–A15):** Einrichten → Schlüsselbund ohne K, Verpackungsdatei da;
  Passwort mit 11 Zeichen und ohne Bestätigung abgelehnt; Fehlerinjektion
  nach dem Schreiben der Verpackung → K bleibt im Schlüsselbund. Neustart mit
  richtigem Passwort öffnet; Passwort ändern → altes scheitert, neues gelingt,
  Datenbank byte-gleich; zurück auf Schlüsselbund → Verpackungsdatei weg.
  Einrichten aus D1 bei Klartext-Datei → umgewandelt, K nur verpackt.
  Verpackungsdatei unter Unix mit Rechten 0600. D1 bietet **kein** Einrichten
  bei `Locked`/`Unknown`/Backend-Fehler und bei Datei *sonst* ×
  `NoSecretServiceProvider`/`NoSessionBus`; Umzugs-Dialog (A11) nie.
- **T14 (A16, A18, A20):** Frontend: Entsperrmaske, falsches Passwort zeigt
  Meldung, Modusanzeige, Einrichten mit Bestätigung, die Codes der Etappe 3
  übersetzt (übrige Codes in Commit 12).

Adversarial (ERHÖHT):
- **T15 Verpackung manipuliert:** ein Byte in Chiffrat, Salt oder Kopf
  geändert → Entsperren scheitert, kein Zugriff, nichts gelöscht. Gültige
  Verpackung mit m = 8 KiB → abgelehnt.
- **T16 Halb abgebrochener Wechsel (A17):** Verpackung und Eintrag, einmal
  gleich, einmal verschieden → Verpackung gilt; gelöscht nur im Gleich-Fall.
- **T17 Schlüssel in Ausgaben (nach Commit 10):** Log und Diagnosepaket nach
  T1, T3, T11, T13 ohne K (Hex/Base64), Datenbankschlüssel, Passwort, Marker.
- **T18 Kommando vor Entsperrung:** Daten-Kommando und MCP-Anfrage im
  gesperrten Zustand → Fehler, kein Panic, keine Daten.
- **T19 Fremde Zwischendatei:** fremde Datei am Ort der Zwischendatei →
  verworfen, nicht als fertige Umwandlung übernommen.
- **T20 Symlink:** `smart-ssh.db` ist Symlink auf eine Klartext-Datei →
  Startfehler, Ziel und Symlink unverändert, keine verschlüsselte Datei
  angelegt.

## 8. Offene Punkte

Keine.

## 9. Klarstellungen

1. **Erststart-Hinweis (A21), entschieden:** DE wörtlich: „Die lokale
   Datenbank ist verschlüsselt. Den Schlüssel verwahrt der Schlüsselbund
   deines Betriebssystems oder – wenn du es einrichtest – dein
   Master-Passwort. Wer Zugriff auf dein entsperrtes Benutzerkonto hat, kann
   die Daten lesen.“ EN sinngemäß, gleicher Inhalt.
2. **T0-Fixture unter Windows (Teil 0 Frage 1, K2):** Windows-Builds checken
   die Migrationen mit CRLF aus (`.gitattributes`), die Prüfsummen in
   `_sqlx_migrations` sind dort andere als in der unter LF geschriebenen
   Fixture (`Migrate(VersionMismatch(1))` in der CI). Tests, die die Fixture
   öffnen (T0, T4–T6), setzen in ihrer **Kopie** vor dem Öffnen die
   Prüfsummen auf die des laufenden Builds, je Version, nur für die 14
   vorhandenen Einträge; die eingecheckte Datei bleibt unverändert. Im Feld
   tritt der Fall nicht auf (Datenbank und Build stammen von derselben
   Plattform). Der Produktivcode bekommt dafür keine Sonderbehandlung.
3. **Teil 0 Frage 1 beantwortet:** Die CI baut und testet mit SQLCipher
   (vendored OpenSSL) auf Ubuntu, Windows und macOS grün, einschließlich
   `cargo build`, Drittlizenzen und Frontend (Stand `3417819`). Kein
   Workflow-Schritt nötig. Ungemessen bleiben die Release-Ziele
   `universal-apple-darwin` und `ubuntu-22.04`; sie sind vor dem nächsten
   Release zu prüfen.

## Umsetzung

**Teil 0** — §2. Lauf 1 endet nach Commit 2. Frage 2 vor Commit 6, Frage 3
zu Beginn von Lauf 3 melden.

**Kein Release, bevor Etappe 3 fertig ist:** Ab Commit 5 startet die App ohne
Schlüsselbund nicht mehr; den Ausweg bringt erst Etappe 3.

**Reihenfolge:**
1. `test(persistence): add a database fixture written by the current SQLite build [BL-0314]` — T0
2. `build(persistence): build SQLite with SQLCipher [BL-0314]` — A1, T9, bestehende Tests grün. **Ende Lauf 1.**
3. `feat(core): derive the database key from the root key [BL-0314]` — A2, T2
4. `feat(persistence): open the database with its key and convert a plaintext database [BL-0314]` — A4, A6–A8, T1 (Teil), T4–T6, T19, T20
5. `feat(app-shell): decide at startup by database state and key state [BL-0314]` — A3, A5, T3, T7, T8
6. `feat(persistence): keep credentials in the encrypted database [BL-0314]` — A9, A9.1, T10
7. `feat(app-shell): move secrets from the OS keychain into the database [BL-0314]` — A10, A11, T11 (ohne *übersprungen*)
8. `feat(app-shell): keep the MCP server token in the database [BL-0117]` — A12, T12, T1 (Rest)
9. `feat(core): wrap the root key with a master password [BL-0203]` — A14, A19, T15
10. `feat(app-shell): unlock with a master password at startup [BL-0203]` — A13, A15–A17, A11.1, D1-Einrichten, A5 im Passwort-Modus, T7 (Variante), T11 (*übersprungen*), T13, T16–T18
11. `feat(frontend): unlock screen and master password settings [BL-0203]` — Oberfläche zu A13, A15, A16, A18, Texte und Codes dieser Etappe (A20 Teil), T14
12. `docs: describe database encryption in the first-run notice, README and changelog [BL-0314]` — A20 (Rest: Codes aus Etappe 1–2, geänderte Schlüsselbund-Texte), A21, A22

**Priorität:** ERHÖHT. Angriffsrichtungen für den Review:
- Ein Weg, der doch einen neuen K erzeugt: Fehlerart verwechselt (`Backend`
  als `NotFound`), Dateizustand falsch erkannt, Einrichten bei
  verschlüsselter Datei (T3, T13).
- Alte `-wal` wird auf die neue Datei angewandt, eine halbe Umwandlung als
  fertig übernommen, ein Symlink-Ziel bleibt im Klartext (T5, T6, T19, T20).
- Schlüssel, Passwort oder Secret über `Debug`, `format!("{e}")`, einen
  SQL-Fehlertext mit dem `PRAGMA key`-Wert oder ein `tracing`-Feld (T17).
- Schlüsselbund-Einträge gelöscht, obwohl nicht gleich umgezogen oder
  *übersprungen* (T11).
- Kommando oder MCP-Server vor der Entsperrung (T18).
- Manipulierte Verpackung mit schwachen Parametern oder vertauschtem Kopf (T15).
- Abgebrochener Moduswechsel löscht die einzige Kopie von K (T13, T16).

**Aufteilung:**
- Lauf 1, **Sonnet**: Commits 1–2. Danach Push und CI (Teil 0 Frage 1).
- Lauf 2, **Opus**: Commits 3–8 (Kern; gemeinsame Module Start und Store).
  Teil 0 Frage 2 klärt der Coder selbst (§2).
- Lauf 3, **Opus**: Commits 9–11, beginnt mit Teil 0 Frage 3.
- Lauf 4, **Sonnet**: Commit 12; „Kern ist fertig und geprüft“.

**Berührte Module:** `crates/persistence-sqlite`, `crates/core/src/crypto`,
`crates/core/src/ssh` (`error.rs`, Fehlercode),
`crates/credentials-keyring`, `crates/app-logic` (`error.rs`,
Startfehler-Texte, `server_credentials.rs`, `identity_file.rs`,
Log-Allowlist), `crates/app-shell` (`lib.rs`, `startup_dialog.rs`,
`mcp_settings.rs`, `commands/ai_providers.rs`), Frontend (Einstellungen,
Entsperrmaske, `errorCodes.ts`, Locales, `FirstRunNoticeScreen`),
`README.md`, `changelog.d/`, Workflows nur nach Teil 0 Frage 1.

**Melde zurück:** Teil-0-Befunde; Namen und Format von Verpackungs- und
Zwischendatei; Bauzeit- und Größenzuwachs; Beleg, dass T1 und T3 am alten
Stand scheitern; manuelle Testabläufe: echte 0.5.2-Datenbank umwandeln,
macOS-Schlüsselbund-Zugriff ablehnen, Linux ohne Secret Service mit
Master-Passwort, falsches Passwort, „Neu anfangen“.
