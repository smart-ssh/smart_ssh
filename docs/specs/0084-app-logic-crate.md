# Spec 0084 — Tauri-freie Anwendungslogik in eigenen Crate `app-logic`

Status: **freigegeben** · Backlog: BL-0269 · Gate: release-1.0/E
Repo: **öffentlich** `smart-ssh` — `crates/app-shell/`, neu `crates/app-logic/`,
`.github/workflows/community.yml`, `docs/architecture.md`, `CLAUDE.md`
Review-Priorität: **ERHÖHT** (Teil 1 ändert, wie der erhöhte SFTP-Kanal
gehalten wird; Spec 0067 A)
Zweck: `app-shell` enthält danach nur noch Tauri-Anbindung; die
Anwendungslogik liegt in einem Crate, der Tauri nachweislich nicht kennt.

## 1. Ausgangslage (gemessen, Stand `1286046`)

- `app-shell` mischt Tauri-Anbindung und Anwendungslogik. Die `CLAUDE.md`
  des Repos nennt ihn „thin wrapper“; der Compiler prüft das nicht.
- Direkt an Tauri (`tauri::`, `tauri_plugin_*::`, `AppHandle`, `rfd::`,
  `#[tauri::command]`) hängen: `chat_retention`, `commands`, `events`,
  `first_run_notice`, `lib`, `local_server`, `mcp_backend`, `mcp_settings`,
  `risk_second_opinion`, `ssh_config_apply`, `ssh_config_export`,
  `startup_dialog`, `wiring`. Alle anderen Module sind selbst Tauri-frei,
  17 davon aber nur über folgende Kanten an Tauri-Module gebunden:
  - `events`: bis auf `impl EventEmitter for tauri::AppHandle` Tauri-frei.
    Liegt `EventEmitter` in einem anderen Crate als `app-shell`, verbietet
    die Orphan-Regel diese Impl in `app-shell`.
  - `state` → `ssh_config_apply::PendingImport` (Datentyp).
  - `dto` → `local_server::is_local` (reine Funktion).
  - `orchestration` → `risk_second_opinion::{fetch_second_opinion,
    fetch_injection_check}`; das Modul liest Einstellungen über
    `tauri_plugin_store`.
  - `test_connection` → `commands::SSH_CONNECT_TIMEOUT` (Konstante).
  - `elevated_sftp` → `commands::BrowserAccess`, und `Session` trägt
    `elevated_sftp: ElevatedSftpSlot`.
- `BrowserAccess(())` hat ein privates Feld und ist nur in
  `commands::elevation` konstruierbar. `ElevatedSftpSlot::lock` verlangt
  ihn. So kommen KI (`orchestration`) und MCP (`mcp_backend`) nie an den
  erhöhten Kanal (Spec 0067 A; Regressionstest
  `test_ai_and_mcp_file_actions_never_use_the_elevated_channel`). Über
  Crate-Grenzen gibt es keine vergleichbare Sichtbarkeit.
- Eine Sitzung wird nur über `SessionManager::remove` entfernt, aufgerufen
  vom Befehl `disconnect`. Bei einem Verbindungsabbruch bleibt sie mit
  Status `Disconnected` stehen, ihr erhöhter Kanal ebenfalls. Der Kanal
  endet heute mit dem `Session`-Wert. Sitzungskennungen werden bei jedem
  Verbindungsaufbau neu erzeugt.
- `sftp_elevation_enable` holt die Sitzung und trägt den Kanal erst nach
  mehreren Proben ein (`elevated_sftp::enable`). Endet die Sitzung
  dazwischen, verschwindet der Kanal heute mit ihr.
- Testhilfen (`test_support`, `TestEmitter` in `events`, die Hilfen in
  `orchestration`) sind `#[cfg(test)]` und werden auch von Tests in Modulen
  genutzt, die in `app-shell` bleiben (u. a. `commands/*`, `mcp_backend`,
  `mcp_settings`, `local_server`).
- Eine Test-Kante: `orchestration/chat_turn/tests_rounds.rs` nutzt
  `commands::sanitize_uname_output` (definiert in
  `commands/diagnostics_export.rs`).
- Nachgelagerte Nutzer von `app-shell` verwenden nur `app_shell::run`,
  `app_shell::Wiring` und `app_shell::Edition`.

## 2. Ziel und Nicht-Ziele

Ziel: neuer Crate `crates/app-logic` (Paketname `app-logic`) mit der
Anwendungslogik; `app-shell` hängt von ihm ab, nie umgekehrt; die Grenze
prüft CI.

Nicht-Ziele: keine Verhaltensänderung für Nutzer, keine Änderung an `core`
oder den Implementierungs-Crates, kein Umbau der Logik auf neue Traits
über das in §4 Genannte hinaus, keine Umbenennung von Tests oder
Tauri-Befehlen, kein Changelog-Fragment (keine Nutzerwirkung).

## 3. Anforderungen

**A1 — Erhöhter Kanal bleibt in `app-shell`.** `Session` enthält keinen
erhöhten SFTP-Kanal mehr. Kanal, Zugangsnachweis, Zuordnung Sitzung → Kanal
und Aktivieren/Deaktivieren liegen in `app-shell`; die Zuordnung ist
eigener, von Tauri verwalteter Zustand von `app-shell`. Nur die
Browser-Befehle erreichen ihn; `app-logic` enthält keinen Typ, keine
Funktion und kein Feld, über das der erhöhte Kanal erreichbar ist.

**A2 — Kein erhöhter Kanal überlebt seine Sitzung.**
1. Das Entfernen einer Sitzung und ihres Kanals geschieht in **einer**
   Funktion von `app-shell`, die `disconnect` aufruft; außer ihr und Tests
   ruft nichts `SessionManager::remove` auf. Die Funktion sperrt den
   Transport der Sitzung nicht; das Trennen des Transports bleibt danach
   in `disconnect`. `SessionManager::remove` läuft vor dem Löschen des
   Zuordnungs-Eintrags oder unter derselben Sperre.
2. Aktivieren trägt den Kanal nur ein, wenn die Sitzung zu diesem Zeitpunkt
   noch im `SessionManager` steht; Prüfung und Eintrag geschehen unter
   derselben Sperre, unter der auch das Entfernen (A2.1) den Eintrag
   löscht. Sonst endet der Befehl mit dem bestehenden Fehler
   `Err("Session nicht gefunden")` (kein neuer Fehlertyp im DTO), und der
   geöffnete Kanal wird verworfen.
3. Ein Kanal ist nur für die Sitzung erreichbar, für die er aktiviert
   wurde, nicht für eine andere Sitzung zum selben Server.
4. Bei Verbindungsabbruch (Sitzung bleibt `Disconnected` stehen) verhält
   sich der Kanal wie bisher.

**A3 — Grenze.** `app-logic` hat in seinem gesamten Abhängigkeitsbaum
(normale, Build- und Dev-Abhängigkeiten, alle Features) kein Paket `tauri`,
`tauri-*`, `wry`, `tao` oder `rfd`. Ein CI-Schritt in `community.yml`
scheitert, wenn das verletzt ist.

**A4 — Umfang.** Jedes Modul, das nach der Umsetzung in `app-shell`
bleibt, braucht Tauri selbst (Kriterium aus §1) oder ist in
`docs/architecture.md` mit Grund als Ausnahme genannt.

**A5 — Testhilfen.** Testhilfen, die Tests beider Crates brauchen, liegen
in `app-logic` hinter einem Feature `test-support`. `app-shell` aktiviert
es nur als Dev-Abhängigkeit (Muster: die Dev-Abhängigkeit auf
`ssh-manager-core` in `app-shell/Cargo.toml`). Kein Produktivbau
aktiviert es.

**A6 — Nichts ändert sich nach außen.**
1. Öffentliche Schnittstelle von `app-shell` unverändert (`run`, `Wiring`,
   `Edition`), `apps/` unverändert.
2. Registrierte Tauri-Befehlsnamen unverändert.
3. Jeder bestehende Test existiert danach mit identischem Namen
   (Modulpfad und Crate dürfen sich ändern). Kein Test entfällt oder wird
   `#[ignore]`. Hinzu kommen nur die Tests T5b–T9, T8b, T10–T10c und die
   beiden Widerrufs-Regressionstests aus §9 (Nachtrag Review Teil 1,
   Runde 2).
4. Elemente, die `app-shell` aus `app-logic` braucht, dürfen dort `pub`
   werden, Testhilfen hinter `test-support` ebenso; sonst keine
   Sichtbarkeitserweiterung über `pub(crate)` hinaus.

**A7 — Verschieben, nicht abschreiben.** Module ziehen als Datei um
(`git mv`), ihr Inhalt ändert sich nur bei `use`-Zeilen, Pfadpräfixen,
Sichtbarkeiten und `cfg`-Attributen für A5, außer an den in §4 genannten
Schnitten.

**A8 — Dokumentation.** `docs/architecture.md` (neu) beschreibt die Crates
und ihre Abhängigkeitsrichtung, die Regel „was gehört wohin“, den
CI-Schritt aus A3, das Feature `test-support` und warum der erhöhte Kanal
in `app-shell` liegt. Der Architekturbaum in `CLAUDE.md` nennt `app-logic`.
Ein ADR hält die verworfenen Wege aus §4 fest.

**A9 — Gate grün** (Repo-`CLAUDE.md`), keine neuen `#[allow(…)]`.

## 4. Design

- **Schnitte**, jeweils ohne Verhaltensänderung:
  `EventEmitter`-Impl für `AppHandle` → Newtype in `app-shell`;
  `PendingImport`, `is_local`, `SSH_CONNECT_TIMEOUT` → dorthin, wo ihre
  Tauri-freien Nutzer liegen; aus `risk_second_opinion` zieht die
  Abruf-Logik nach `app-logic`, das Lesen der Einstellungen bleibt in
  `app-shell` und wird als Wert übergeben; `sanitize_uname_output` zieht
  nach `app-logic`, wenn es Tauri-frei ist, sonst wandert der Test, der es
  nutzt, nach `app-shell`.
- **Erhöhter Kanal (A1/A2):** Zuordnung Sitzungskennung → Kanal als eigener
  verwalteter Zustand von `app-shell`. Die Sperre der Zuordnung gilt nur für
  Nachschlagen, Eintragen und Entfernen; der Kanal selbst behält eine eigene
  Sperre je Sitzung (wie heute `ElevatedSftpSlot`), damit ein laufender
  Vorgang in einer Sitzung das Trennen einer anderen nicht aufhält.
- A2.1 ist nach dem Umzug nur per Suche gesichert (`SessionManager::remove`
  wird in `app-logic` `pub`); das ADR sagt das ausdrücklich.
- `app-logic` behält die Modulnamen, damit `crate::…`-Pfade in den
  verschobenen Dateien gültig bleiben.
- Verworfen: Zugangsnachweis nach `app-logic` mit öffentlichem Konstruktor
  plus CI-Prüfung (die Garantie hinge an einem Skript statt am Compiler);
  nur die freien Module auslagern (Ziel verfehlt).

## 5. Sicherheits-Invarianten

- **Spec 0067 A (erhöhter Kanal nur für Browser-Befehle):** wird strenger:
  Die KI- und MCP-Logik liegt danach in einem Crate, der den Kanal nicht
  kennt. Neu entstehen die Risiken aus A2 (Kanal überlebt die Sitzung,
  gehört zur falschen Sitzung, Wettlauf mit dem Trennen); T5–T9 decken sie
  ab.
- Filter-Engine, Risiko-Klassifizierung, Redaktion, Bestätigung,
  Credentials: verschoben, nicht verändert (A7, T2). Eine inhaltliche
  Abweichung dort ist ein Blocker.

## 6. Tests

Prüfungen:
- **T1 — Testnamen.** `cargo test --workspace -- --list` vor/nach; nach
  Abschneiden des Pfads sind die Listen mit Duplikaten identisch, bis auf
  die neuen Tests T5b–T9 und T8b (namentlich im Bericht).
- **T2 — Verschiebung.** `git diff -M` zwischen Ausgangs- und Endstand: Für
  jede umbenannte Datei die Inhaltsdifferenz; zulässig nur, was A7 nennt,
  jede andere Zeile einzeln begründet.
- **T3 — Grenze wirkt.** Der CI-Schritt aus A3 ist grün, und er scheitert
  nachweislich: einmal lokal mit einer testweise eingetragenen
  `tauri`-Abhängigkeit in `app-logic` gefahren (nicht committet), Ausgabe
  im Bericht.
- **T4 — Umfang und Feature.** Liste der in `app-shell` verbliebenen Module
  mit Kriterium (A4); `cargo tree -p smart-ssh-community -e features`
  zeigt `test-support` nicht (A5). Der CI-Schritt aus A3 fragt den Baum
  mit `-p app-logic` ab, nicht an der Workspace-Wurzel.

Tests (adversarial, A1/A2):
- **T5** Bestehender Regressionstest
  `test_ai_and_mcp_file_actions_never_use_the_elevated_channel` bleibt
  unter seinem Namen und liegt dort, wo er den Kanal aufbauen kann.
  **T5b** (neu): wie T5, aber eine MCP-**Schreib**aktion; scheitert, wenn
  sie über den erhöhten Kanal läuft.
- **T6** Kanal aktiv, Sitzung wird über die Funktion aus A2.1 entfernt:
  Die Zuordnung enthält danach keinen Eintrag für diese Kennung.
  Scheitert, wenn die Funktion den Eintrag nicht löscht.
- **T7** Zwei gleichzeitige Sitzungen **zum selben Server**, Kanal nur in A
  aktiv: Browser-Befehle von B laufen über den normalen Kanal, Deaktivieren
  in B lässt A aktiv. Scheitert bei Zuordnung über den Server statt die
  Sitzung.
- **T8** Wettlauf: Aktivieren ist nach den Proben, vor dem Eintrag
  angehalten (Mock-Transport, der beim Öffnen des erhöhten SFTP-Kanals auf
  ein Signal wartet); die Sitzung wird über A2.1 entfernt; danach läuft das
  Aktivieren weiter. Ergebnis: `Err("Session nicht gefunden")`, kein
  Eintrag in der Zuordnung. Scheitert, wenn A2.2 fehlt. **T8b:** dieselbe
  Verschränkung, aber das Aktivieren läuft weiter, während A2.1 zwischen
  `SessionManager::remove` und dem Löschen des Eintrags steht (sofern die
  Umsetzung dort eine Lücke hat, sonst im Bericht begründen, warum es sie
  nicht gibt). Scheitert bei falscher Reihenfolge in A2.1.
- **T9** Deaktivieren bei nicht aktivem Kanal und für eine unbekannte
  Sitzungskennung: kein Absturz, Ergebnis wie bisher bei „nicht aktiv“.

Für T6, T7, T8 und T8b im Bericht belegen, dass der Test gegen die jeweils
genannte kaputte Variante scheitert (lokal umgestellt, nicht committet).
Zusätzlich: Außer der Funktion aus A2.1 und Tests ruft nichts
`SessionManager::remove` auf (Suche im Bericht).

## 7. Umsetzungsreihenfolge

1. **Teil 1:** A1/A2 innerhalb von `app-shell`, noch ohne neuen Crate;
   T5–T9; Gate. Commit
   `refactor(app-shell): hold elevated sftp channel outside Session [BL-0269]`.
2. **Teil 2**, Schritt a: Schnitte aus §4, Gate, Commit.
3. Teil 2, Schritt b: Crate `app-logic` mit Feature `test-support`, Module
   per `git mv`, Gate, T1/T2/T4, Commit.
4. Teil 2, Schritt c: CI-Schritt (A3, T3), `docs/architecture.md`,
   `CLAUDE.md`, ADR, Commit.

## 8. Offene Punkte

Keine.

## 9. Klarstellungen

(wird während der Umsetzung nachgetragen: Datum · Frage-ID · Antwort)

- 2026-09-28 · Review Teil 1 · **Widerruf wirkt beim Zugriff, nicht beim
  Befehlsbeginn.** Ein Browser-Befehl darf einen erhöhten Kanal nicht über
  das Deaktivieren oder Entfernen hinaus benutzen. Wie bisher gilt: Der
  Kanal wird im Moment der Nutzung unter seiner Sperre geprüft; ist er
  inzwischen deaktiviert, entfernt oder für einen anderen Nutzer neu
  aktiviert, endet der Befehl mit `ELEVATED_CHANNEL_INACTIVE` bzw. dem
  bisherigen Nutzer-Fehler. Deaktivieren wartet wie bisher, bis ein
  laufender Vorgang auf dem Kanal fertig ist. Neue Tests: **T10** Kanal
  von einem Befehl festgehalten, Deaktivieren, danach scheitert der Zugriff;
  **T10b** dasselbe mit Neu-Aktivieren für einen anderen Nutzer; **T10c**
  dasselbe mit Entfernen der Sitzung (A2.1). Jeder scheitert gegen die
  Variante „Kanal bei Befehlsbeginn festhalten“.
- 2026-09-28 · Review Teil 1 · T9: Deaktivieren für eine **unbekannte**
  Sitzungskennung liefert wie bisher `Err("Session nicht gefunden")`;
  „nicht aktiv“ gilt nur für eine bekannte Sitzung ohne Kanal.
- 2026-09-28 · Review Teil 1, Runde 2 · **Der Widerruf wirkt sofort, nicht
  erst an der Kanal-Sperre.** Ein Widerruf, der den Kanal nur unter dessen
  eigener Sperre herausnimmt, greift erst, wenn er in deren Warteschlange an
  der Reihe ist: ein Browser-Befehl, der beim Umschalten schon wartete,
  liefe noch einmal mit den alten Rechten, und das Trennen bliebe hinter
  einem hängenden Transfer stehen. Deshalb trägt der Kanal einen
  Widerrufs-Merker, der ohne Sperre sofort gesetzt und bei **jedem** Zugriff
  geprüft wird. Das Entfernen einer Sitzung (A2.1) setzt nur diesen Merker
  und wartet auf nichts; Deaktivieren nimmt den Kanal zusätzlich unter
  seiner Sperre heraus und wartet dabei wie bisher auf einen laufenden
  Vorgang. Ein widerrufener Kanal meldet auch keinen Ziel-Nutzer mehr, damit
  keine Audit-Zeile eine Erhöhung behauptet, die nicht stattfand. Neue
  Tests: `test_removing_a_session_does_not_wait_for_a_running_elevated_transfer`
  (scheitert gegen ein Entfernen, das auf die Kanal-Sperre wartet) und
  `test_a_command_already_waiting_on_the_channel_fails_after_the_user_switched`
  (scheitert ohne den Merker).
- 2026-09-28 · Review Teil 1 · T5/T5b belegen, dass KI und MCP keinen
  Zugriffsweg zur Zuordnung haben; scheitern können sie nur, wenn ein
  solcher Weg entsteht. Die eigentliche Garantie tragen Signaturen und
  Crate-Grenze.
