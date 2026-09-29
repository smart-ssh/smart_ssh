# Spec 0093 — Windows-CI grün: zwei rote Ziele, drei wackelige Tests

Status: freigegeben · Backlog: BL-0281 · Gate: —
Zweck: `cargo test --workspace --no-fail-fast` wird unter `windows-latest` grün und bleibt es.
Review-Priorität: NORMAL

## 1. Ist-Stand (origin/main fd7dacf)

1. CI-Lauf `Community` auf `42ac8b4` (Job `Test (windows-latest)`, aus dem Log
   gelesen): `error: 2 targets failed: -p app-shell --lib`,
   `-p ssh-transport --test integration`. Alle übrigen Ziele sind grün, auch
   unter Ubuntu und macOS.
2. **`ssh-transport`, `test_sftp_set_permissions`:** erwartet nach
   `set_permissions(…, 0o640)` bei `stat()` den Wert `0o640` und bekommt unter
   Windows `511` (`0o777`). Der Test-SFTP-Server
   (`tests/fixtures/test_server.rs`, `setstat`) wendet die Rechte nur unter
   `#[cfg(unix)]` an und ignoriert sie unter Windows bewusst. `stat` liefert
   die Rechte aus den Dateisystem-Metadaten (gelesen, nicht ausgeführt). Der
   Test prüft deshalb unter Windows eine Eigenschaft, die der Testserver dort
   nicht hat.
3. **`app-shell --lib`:** Das Test-Binary startet nicht:
   `exit code: 0xc0000139, STATUS_ENTRYPOINT_NOT_FOUND`. Kein Test läuft an.
   Gemessen per Cross-Build (Docker `rust:trixie`, Target
   `x86_64-pc-windows-gnu`,
   `cargo test --locked --no-run -p app-shell --lib`, danach
   `x86_64-w64-mingw32-objdump`):
   - Das Binary importiert `TaskDialogIndirect` aus `comctl32.dll`.
   - Es hat keinen `.rsrc`-Abschnitt, also kein eingebettetes Manifest
     (`objdump -h | grep -c .rsrc` → 0).

   `TaskDialogIndirect` gibt es nur in comctl32 v6, und die lädt Windows
   nur für Programme mit Manifest. `app-shell` hat kein `build.rs`. Die App
   selbst bekommt ihr Manifest über `tauri_build::build()` in
   `apps/smart-ssh-community/build.rs`.

4. **Wege für die Einbettung**, gemessen per Probe-Crate (reine Lib mit
   einem Unit-Test, gleiches Cross-Build-Setup):
   - Die Crate `embed-manifest` 1.x gibt `cargo:rustc-link-arg-bins` aus.
     Bei einer Lib ohne Bin-Ziel bricht der Build ab (`invalid instruction
     cargo:rustc-link-arg-bins … does not have a bin target`). Sie ist
     damit ungeeignet.
   - Ein eigenes `build.rs` wirkt dagegen. Die Plattform liest es aus
     `CARGO_CFG_TARGET_OS`/`CARGO_CFG_TARGET_ENV`. Das Manifest übersetzt es
     per `x86_64-w64-mingw32-windres` (Paket `binutils-mingw-w64-x86-64`)
     zu einem COFF-Objekt und gibt es per `cargo:rustc-link-arg`
     aus. Danach hat das Unit-Test-Binary der Lib einen `.rsrc`-Abschnitt
     (`grep -c .rsrc` → 1) mit `Microsoft.Windows.Common-Controls`.
     Gemessen ist das nur für GNU. Für MSVC übernimmt der Linker selbst
     (`/MANIFEST:EMBED`, `/MANIFESTDEPENDENCY:…`); das ist nicht gemessen.

   **Nicht gemessen:** das MSVC-Target, mit dem die CI baut. Der Import
   kommt aus derselben Abhängigkeit, der Befund ist daher
   übertragbar. Bestätigen kann ihn aber nur der CI-Lauf, s. Teil 0.

5. **Wackelige Tests** (auf `fd7dacf`, CI-Lauf 36562465215, Windows: zusätzlich
   rot, im Lauf davor grün; lokal macOS einmal rot in einem von zwei vollen
   Gate-Läufen):
   - `ssh-transport`, `test_connect_to_closed_local_port_yields_connection_refused`:
     Meldung „ein geschlossener Port darf keine Verbindung liefern“. Der Test
     bindet `127.0.0.1:0`, liest den Port, gibt ihn frei und verbindet
     danach. Parallel laufende Tests desselben Binaries starten Testserver
     auf `127.0.0.1:0`. Ein solcher Server kann den gerade freigegebenen
     Port erhalten (Wettlauf, gelesen, nicht ausgeführt).
   - `core`, `risk::tests::test_secret_check_stays_fast_on_adversarial_long_input`:
     Limit fest 3 s, gemessen 3,01 s bei Länge 3608 (Debug-Build, Windows-Runner).
   - `ssh-transport`, `test_sftp_upload_download_roundtrip`: lokal unter
     macOS einmal rot, `on_disk` war leer (die Datei direkt nach
     `write_file` von der Platte gelesen). Einzeln 20 von 20 grün
     (`cargo test -p ssh-transport --test integration
     test_sftp_upload_download_roundtrip`, 20×). Ursache laut Code (gelesen,
     nicht ausgeführt): Der `write`-Handler des Testservers schreibt per
     `tokio::fs::File::write_all` und antwortet sofort. Es gibt kein
     `flush`, und `close` entfernt den Handle nur aus der Map. Tokio puffert
     die Daten und schreibt sie später. Der Client wartet nach `write_all`
     per `shutdown()` auf die Bestätigungen (`ssh-transport/src/sftp.rs`,
     `write_file`).

## 2. Teil 0

1. **Startet das `app-shell`-Test-Binary unter `windows-latest` mit der
   Änderung aus A2?** Das beantwortet nur ein CI-Lauf nach dem Push. Lokal
   gilt als Nachweis: Das per Cross-Build erzeugte Binary hat danach einen
   `.rsrc`-Abschnitt, und das Manifest enthält die Abhängigkeit
   `Microsoft.Windows.Common-Controls` Version 6. Der Architekt fährt diese
   Prüfung nach dem Lauf selbst, der Coder hat kein Docker. Scheitert der
   CI-Lauf trotzdem, bleibt A1 davon unberührt.

## 3. Ziel und Nicht-Ziele

Ziel: Beide Ziele sind unter Windows grün, ohne Tests abzuschwächen, die
unter Unix etwas prüfen.

Nicht-Ziele:
- Kein `#[ignore]` und kein Abschalten des gesamten `app-shell`-Test-Binarys
  unter Windows.
- Keine Änderung am Manifest oder an `build.rs` der App.
- Keine Rechte-Emulation, weder im Produktcode noch im Testserver (A1).

## 4. Anforderungen

- A1 MUSS: `test_sftp_set_permissions` prüft auf **allen** Plattformen, dass
  der Client den Modus `0o640` per SFTP-`setstat` an den Server übermittelt.
  Dazu hält der Testserver das empfangene `permissions`-Feld fest und macht
  es dem Test zugänglich, nach dem Muster, mit dem er schon andere
  Beobachtungen weiterreicht. `stat` liest weiterhin das echte
  Dateisystem. Der Testserver emuliert keine Rechte.
  Unter Unix prüft er **zusätzlich** wie heute, dass `stat()` danach `0o640`
  meldet. Unter Windows darf der Test die zweite Prüfung auslassen, die erste
  aber nicht.
- A2 MUSS: Das Unit-Test-Binary von `app-shell` startet unter Windows. Es
  trägt dazu ein Manifest mit der Abhängigkeit auf Common Controls v6.
  - Die Einbettung muss das **Test-Ziel der Lib** erreichen, also
    `cargo:rustc-link-arg` und nicht `-bins` (Ist-Stand 4; `embed-manifest`
    scheidet aus).
  - Sie gilt für `target_env = "msvc"` (die CI) **und** `"gnu"` (der lokale
    Nachweis T4; dort über `windres`, wie in Ist-Stand 4 gemessen).
  - Auf anderen Betriebssystemen tut sie nichts, und das App-Binary bleibt
    unverändert.
  - Die Plattformwahl im `build.rs` liest `CARGO_CFG_TARGET_OS`/
    `CARGO_CFG_TARGET_ENV`, nicht `#[cfg]` oder `cfg!` — Letztere gelten
    für den Build-Rechner, nicht für das Ziel.
- A3 MUSS: Unter macOS und Linux bleiben Build und Tests unverändert grün.
  Ein neues `build.rs` tut dort nichts.
- A5 MUSS: Der Test zum geschlossenen Port kann nicht mehr an einen
  fremden Dienst geraten: Der Port bleibt bis nach dem Verbindungsversuch
  für diesen Test belegt, nimmt aber keine Verbindung an (etwa ein
  gebundener Socket ohne `listen`). Er prüft weiterhin `ConnectionRefused`,
  auf allen drei Plattformen.
- A6 MUSS: Der Zeittest erkennt weiterhin einen Rückfall in exponentielle
  Laufzeit, schlägt aber nicht mehr wegen eines langsamen Runners an. Das
  Limit ist begründet: gemessene Laufzeit auf dem eigenen Rechner im
  Debug-Build und Abstand zu einer exponentiellen Laufzeit. Beides steht
  im Kommentar.
- A7 MUSS: Der Testserver bestätigt ein `write` bzw. `close` erst, wenn
  die Daten auf der Platte liegen, also mit einem `flush` vor der Antwort.
  Absicherung: Zeigt sich trotzdem, dass der Client zurückkehrt, bevor der
  Server bestätigt hat, dann **anhalten und melden**. Das wäre ein Fehler
  im Produktcode mit eigener Spec.
- A4 SOLL: Ein kurzer Kommentar an der Stelle aus A2 nennt den Grund
  (`TaskDialogIndirect`, comctl32 v6), ohne Herkunft zu erzählen.

## 5. Design

Keine Vorgabe über A2 hinaus. Die Herleitung steht in der HQ-Beilage.

## 6. Sicherheits-Invarianten

Keine berührt: nur Testserver, Testbinary und Build-Metadaten.

## 7. Tests

- T1: `test_sftp_set_permissions` bleibt unter macOS grün. Er scheitert, wenn
  der Client einen falschen Modus sendet (Gegenprobe: im Test kurz `0o600`
  erwarten, der Test muss rot werden; danach zurück).
- T2: Die Prüfung aus A1 für alle Plattformen scheitert, wenn der Server
  keinen `setstat` oder einen anderen Modus erhält. Die Gegenprobe ist eine
  **eigene**: Nur der erwartete Wert dieser Prüfung wird kurz geändert, der
  `stat`-Assert bleibt unverändert. Der Bericht nennt die Meldung des
  gescheiterten Asserts, damit sichtbar ist, dass die neue Prüfung
  angeschlagen hat.
- T3: `cargo test --workspace` unter macOS ist grün, und
  `cargo clippy --workspace --all-targets -- -D warnings` ist grün.
- T4 (Architekt, Cross-Build): Das `app-shell`-Test-Binary für Windows hat
  einen `.rsrc`-Abschnitt mit Common-Controls-v6-Manifest. Vorher: 0.
- T5 (CI nach dem Push): Der Job `Test (windows-latest)` ist grün.
- T6: Der Socket lebt nachweislich bis nach dem Verbindungsversuch
  (sichtbar im Diff). Der Portfall ist belastet nachgewiesen:
  `cargo test -p ssh-transport --test integration` läuft 20× hintereinander
  grün. Gegenprobe: Lauscht auf dem reservierten Port ein Dienst, wird der
  Test rot.
- T7: Gegenprobe mit kurz ausgehebelter Tiefenbegrenzung, danach
  zurück. Sie läuft **nicht** mit der vollen Eingabe, die dann womöglich
  nie zurückkehrt, sondern mit zwei, drei kleineren Tiefen. Der Bericht
  zeigt, dass die Laufzeit dabei überproportional wächst und das neue
  Limit schon bei der größten dieser Tiefen reißen würde. Die Laufzeit der
  echten Eingaben auf dem eigenen Rechner steht ebenfalls im Bericht.
- T8: `cargo test --workspace` läuft 5× hintereinander grün. Dazu kommt der
  Roundtrip-Test unter Last: das ganze Binary `ssh-transport --test
  integration` 20×. Die Ursache aus A7 steht im Bericht.

## 8. Offene Punkte

Keine.

## 9. Klarstellungen

## Umsetzung

**Teil 0:** s. §2. Der Architekt klärt ihn nach dem Lauf, er blockiert den Coder nicht.

**Reihenfolge:**
1. `test(ssh-transport): check the transmitted mode on all platforms [BL-0281]` — A1, T1, T2.
2. `fix(app-shell): embed a common-controls manifest in the test binary on Windows [BL-0281]` — A2–A4, T3.
3. `test(ssh-transport): keep the closed-port test from racing other test servers [BL-0281]` — A5, T6.
4. `test(core): base the risk timing limit on measured runtime [BL-0281]` — A6, T7.
5. `test(ssh-transport): make the SFTP roundtrip deterministic [BL-0281]` — A7, T8 (entfällt, wenn A7 auf den Client zeigt; dann melden).

**Priorität:** NORMAL. Das gilt bewusst auch für Schritt 4: Dort ändert
sich nur das Limit eines Tests, nicht der Code des Klassifizierers. Das
Limit bleibt so gewählt, dass es einen Rückfall in exponentielle Laufzeit
erkennt (A6, T7).

**Aufteilung:** ein Lauf, Sonnet.

**Berührte Module:** `crates/ssh-transport/tests/`, `crates/core/src/risk/tests.rs`, `crates/app-shell/`
(`Cargo.toml`, neues `build.rs`), `Cargo.lock`.

**Melde zurück:** den gewählten Weg für A2 und warum; die Gegenproben aus
T1/T2 mit Ergebnis; ob eine neue Abhängigkeit dazukam (Name, Version,
Lizenz, `cargo deny check licenses sources bans` grün).
