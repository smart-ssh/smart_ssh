# Spec 0094 — Keine Inhalte in der Logdatei auf dem Standard-Level

Status: freigegeben · Backlog: BL-0029 · Gate: release-1.0/C
Zweck: Die Logdatei enthält auf dem Standard-Level (`info`) keinen Kommandotext, keine Kommando-Ausgabe, keinen Chat-, Notiz- oder Prompt-Inhalt und keine Werkzeug-Argumente mehr — unabhängig davon, welche Muster der Redactor kennt.
Review-Priorität: ERHÖHT (Redactor, Log als Datensenke)

## Getroffene Entscheidungen

- **E1 — Standard-Level ohne Inhalt:** Log-Zeilen auf `info`, `warn` und
  `error` tragen nur inhaltsfreie Angaben (IDs, Entscheidung, gegriffene
  Regel, Längen, Exit-Code, Fehlercodes, Zählwerte). Der Inhalt, den diese
  Zeilen heute tragen, erscheint nur noch auf `debug`, und dort durch den
  Redactor gelaufen. `debug` ist ohne `RUST_LOG` aus.
- **E2 — Nur die Logdatei:** Die Datenbank (Ausführungsprotokoll,
  Chatverlauf, Eingabe-Historie) ist nicht Teil dieser Spec (BL-0295).

Diese Spec ändert damit Spec 0016 §4 (Punkte 1–5) für die Level ab `info`:
Kommando, Kontext und Rohantwort stehen dort künftig auf `debug`.

## 1. Ist-Stand (origin/main d5b9342)

1. **Logging-Aufbau** (gelesen): `app_logic::logging::init_logging` setzt
   einen JSON-`fmt`-Subscriber mit `EnvFilter::try_from_default_env()`,
   sonst `EnvFilter::new("info")`; Ziel ist eine täglich rotierende Datei.
   Kein Layer filtert oder redigiert Inhalte. Beim Start löscht
   `cleanup_old_logs` Dateien älter als `MAX_LOG_AGE` (14 Tage).
2. **Stellen mit Inhalt auf `info` oder höher** (gelesen):
   - `FilterEngine::evaluate_explained` — `info`, `command` roh.
   - `request_logging::log_outgoing_context` — `info`, `system_context` und
     `history` roh (bei Kommando-Ergebnissen das Kommando roh plus Längen).
   - `request_logging::log_tool_call_fragment` — `info`, `raw_arguments`.
   - `request_logging::log_tool_call_parsed` — `info`, `?action`
     (bei `ExecuteCommand` mit Kommando).
   - `request_logging::log_tool_call_parse_error` — `error`,
     `raw_arguments` und `error`.
   - `request_logging::log_provider_error_response` und
     `log_provider_rate_limited_retry` — `warn`, `body` der
     Provider-Antwort, ersetzt werden nur eigene Schlüssel
     (`redact_secrets`). Aufrufer: `anthropic.rs`, `openai_compatible.rs`,
     `discovery.rs`.
   - `action_exec::log_command_execution` — `info`, `command`,
     `stdout`/`stderr` (bis 4096 Zeichen), redigiert.
   - `action_exec::log_command_execution_failed` — `warn`, `command`
     redigiert, `error = %err`.
   - MCP-Server, Zeile „mcp tool call completed" — `info`,
     `outcome = ?outcome`. Das `ActionOutcome` trägt bei Erfolg die
     Zusammenfassung mit Kommando-Ausgabe bzw. Dateiinhalt, beim Fehlschlag
     die Chat-Fehlermeldung, die das Kommando **unredigiert** enthält
     (`action_exec`: „Kommando '{command}' konnte nicht ausgeführt werden …",
     übernommen in `app_shell::mcp_backend`).
   - Fehlerwerte vom Typ `AiError` und `SshError` als `error = %err` auf
     `warn`/`error` an sieben Stellen (gezählt, Q-BL-0029-01; maßgeblich
     bleibt das funktionale Kriterium A1.7): `compaction.rs`, `notes.rs`,
     zwei in `app_shell::commands::connect` (eine davon über `CommandError`,
     dessen `message` aus `SshError` stammt),
     `action_exec::log_command_execution_failed`,
     `request_logging::log_provider_transport_error` (über einen String) und
     `log_tool_call_parse_error` (als `dyn Display`, dort auch
     `serde_json::Error`). Ihr `Display` kann Inhalt tragen: `AiError::ModelNotFound`
     enthält den vollen Antworttext des Providers, `AiError::InvalidResponse`
     gibt Argumentwerte wieder, `SshError`-Varianten tragen freien Text.
     Beide Typen haben `code()`.
3. **Der Redactor erkennt gängige Passwort-Argumente von
   Kommandozeilenprogrammen nicht** (gemessen mit
   `DefaultOutputRedactor::new().redact_text`, Protokoll in der Beilage).
   Die schon redigierten Stellen aus Punkt 2 sind deshalb ebenfalls
   betroffen. Die Session nutzt denselben Redactor (`Session::redactor`).
4. **Diagnose-Export:** `build_diagnostics_bundle` übernimmt nur Zeilen,
   deren Nachricht in `SAFE_LOG_MESSAGES` steht, und lässt sie zusätzlich
   durch den `OutputRedactor`. Drei Nachrichten aus Punkt 2 stehen darin
   (`log_provider_error_response`, `log_provider_rate_limited_retry`,
   `log_provider_transport_error`) — genau die Gruppe, für die A1.5 die
   Ausnahme macht (gemessen, Q-BL-0029-01; §8.2). Keine andere Zeile aus
   Punkt 2 steht darin.
5. **Test-Aufzeichnung ist je Crate verschieden** (gelesen): je Testbinary
   ein globaler Subscriber, einmal gesetzt.
   - `core` (`filter/tests.rs`, Mitschnitt für T-A10): nur `ERROR`.
   - `ai-providers` (`test_support`): `TRACE`.
   - `app-logic` (`test_support::log_capture`): ohne `with_max_level`, also
     Standard `INFO` von `tracing-subscriber` (der Kommentar „kein
     Level-Filter" dort ist falsch).
   - `mcp-server`: keine Aufzeichnung; `tracing-subscriber` ist dort keine
     Abhängigkeit, die Tests laufen auf `rt-multi-thread`.
   - `test_log_command_execution_never_logs_unredacted_secret` sucht
     „REDACTED" in der `info`-Zeile der Ausführung.

Messung und Recherche: Beilage `0094-no-content-in-default-logs.md`.

## 2. Teil 0

Teil 0: entfällt. Welche Felder auf welchem Level stehen, ist statisch
prüfbar; dass der Standard ohne `RUST_LOG` kein `debug` schreibt, belegt
T10 an der Filterkonfiguration selbst.

## 3. Ziel und Nicht-Ziele

Ziel: Ein Kommando wie `mysql -p'geheim'` — vom Nutzer getippt, von der KI
vorgeschlagen, ausgeführt, abgelehnt, fehlgeschlagen oder über MCP
ausgeführt — hinterlässt `geheim` in keiner Zeile der Logdatei, solange
`RUST_LOG` nicht gesetzt ist. **Einzige Ausnahme** ist A1.5 (Antworttext
eines Providers bei einem Fehler).

Nicht-Ziele:
- Datenbank (E2).
- Neue Redactor-Muster (BL-0118).
- Bestehende Logdateien (§8).
- Fehlerwerte anderer Typen (Datenbank, Dateisystem, Schlüsselbund) —
  nur `AiError` und `SshError` sowie Typen, die sie im `Display` enthalten.
- Die Panic-Zeile „app panicked" (`app_shell`) bleibt unverändert.
- Kein neuer Subscriber-Layer, keine Änderung an `SAFE_LOG_MESSAGES`.
- Die „Testen"-Ansicht (`EvaluationTrace` an die Oberfläche) bleibt
  unverändert.

## 4. Anforderungen

**A1 — Kein Inhalt ab `info`.** MUSS: Keine Log-Zeile auf `info`, `warn`
oder `error` enthält, weder roh noch redigiert: Kommandotext;
Kommando-Ausgabe; Dateiinhalt; Chat-, Notiz- oder Prompt-Text;
`system_context`; Argumente eines Werkzeugaufrufs (roh oder geparst); das
`Display` eines `AiError` oder `SshError`. Was jede Zeile behält:
- A1.1 Filter-Entscheidung: `decision`, gegriffene Regel bzw.
  Hard-Blacklist-Eintrag, Länge des Kommandos.
- A1.2 Ausgehende KI-Anfrage: `request_id`, Anzahl History-Einträge, Namen
  der Aktionen; je History-Eintrag nur Art und Länge (bei Ergebnissen
  Exit-Code und Ausgabelängen).
- A1.3 Werkzeugaufruf: `request_id`, Werkzeugname, Länge der Argumente,
  Art der Aktion; beim Parse-Fehler der Fehlercode.
- A1.4 Ausführung: `session_id`, Exit-Code, Ausgabelängen, Kommandolänge;
  beim Fehlschlag der Fehlercode.
- A1.5 **Ausnahme** Provider-Fehlerantwort: `body` bleibt auf `warn`,
  **erst** durch den Redactor gelaufen, **dann** auf höchstens 512 Zeichen
  gekürzt. Grund: Die Antwort stammt vom Provider und wird zur Diagnose
  von Fehlkonfigurationen gebraucht. Spiegelt ein Provider Teile der
  Anfrage, bleibt das auf 512 Zeichen begrenzt (Restrisiko).
- A1.6 MCP-Aufruf: Werkzeug, Server, Art des Ergebnisses
  (angenommen/abgelehnt/fehlgeschlagen …), Länge der Zusammenfassung bzw.
  Meldung.
- A1.7 Jede Stelle, die heute das `Display` eines `AiError` oder `SshError`
  ab `info` loggt — direkt per `%`, über einen String oder über einen Typ,
  der es enthält (z. B. `CommandError` in `app_shell::commands::connect`) —,
  loggt stattdessen `code()`.

**A2 — Inhalt nur auf `debug`, redigiert.** MUSS: Für jede Stelle, deren
Inhalt A1 entfernt, gibt es eine `debug`-Zeile mit dem bisherigen Inhalt,
der vorher durch den Redactor läuft. Längenbegrenzungen von heute bleiben.
Wo kein Session-Redactor erreichbar ist, wird ein `DefaultOutputRedactor`
genutzt (gleicher Musterstand). Er wird nicht bei jedem Aufruf neu gebaut,
und Redaction für `debug` läuft nur, wenn `debug` für diese Stelle aktiv ist.

**A3 — Standard bleibt `info`.** MUSS: Ohne `RUST_LOG` ist `debug` für
keines der Targets aktiv. Der Standardfilter (der Fallback, ohne
`try_from_default_env`) ist als eigene Funktion testbar (T10).

**A4 — Nachrichtentexte.** SOLL: Die `message` der geänderten Zeilen bleibt
gleich; die neuen `debug`-Zeilen haben eine eigene `message`.

## 5. Design

- Wie die Stellen umgebaut werden, entscheidet der Coder.
- Kein Hash des Kommandos und kein Programmname ab `info`: Passwörter haben
  wenig Entropie, und das erste Wort kann selbst ein Geheimnis sein
  (`PGPASSWORD=… psql`).

## 6. Sicherheits-Invarianten

- **Redaction vor jeder Datensenke:** verschärft — die Logdatei ist ab
  `info` keine Senke für Inhalte mehr; auf `debug` gilt die bisherige Regel.
- **Filter-Entscheidung:** unberührt; nur die Log-Zeile ändert sich.
- **Diagnose-Export:** unberührt.
- **Transparenz gegenüber Nutzer, KI und MCP-Client** (Event, Chat,
  Ledger, Werkzeug-Ergebnis): unberührt — nur die Log-Zeile ändert sich,
  nicht das `ActionOutcome` oder die Chat-Meldung.

## 7. Tests

**Aufzeichnung:** Jede betroffene Crate zeichnet in Tests auf `TRACE` auf.
Tests werten Zeilen nach dem Feld `level` aus („ab `info`" bzw. „`debug`").
Dafür darf die Testhilfe in `core`, `ai-providers` und `app-logic`
angepasst und in `mcp-server` neu angelegt werden (`tracing-subscriber` als
dev-dependency, globaler Subscriber wie in den anderen Crates); T-A10 in `core` wertet danach nur `ERROR`-Zeilen aus und
bleibt inhaltlich unverändert. Der Bestandstest
`test_log_command_execution_never_logs_unredacted_secret` darf sein
„REDACTED" auf der `debug`-Zeile suchen und muss zusätzlich prüfen, dass
die `info`-Zeile keinen Inhalt trägt.

Geheimnis in allen Tests: `geheim-0094`, in einer Form, die der Redactor
**nicht** erkennt (`-p'geheim-0094'`, `sshpass -p geheim-0094`), außer wo
anders angegeben. T1–T9 müssen gegen den heutigen Stand scheitern; der
Coder belegt das je Test. T10 und T11 sind Wächter: Für T10 belegt der
Coder, dass der Test rot wird, wenn der Standard testweise auf `debug`
steht.

- **T1 Filter-Entscheidung:** `mysql -p'geheim-0094' -e 'select 1'` →
  ab `info` kein Treffer; `decision` und Kommandolänge vorhanden.
- **T2 Verkettung:** `true && mysql -pgeheim-0094; echo ok` → ab `info` kein
  Treffer.
- **T3 Mehrzeilig:** Heredoc mit `set password='geheim-0094';` → ab `info`
  kein Treffer in keiner Zeile.
- **T4 KI-Anfrage:** Kontext mit Text, `CommandResult`, `ActionRejected` und
  `system_context`, je mit `geheim-0094` → ab `info` kein Treffer; Art und
  Länge je Eintrag vorhanden.
- **T5 Werkzeugaufruf:** Fragment, geparste `ExecuteCommand`-Aktion und
  Parse-Fehler, dessen **Fehlertext** `geheim-0094` enthält (z. B.
  unbekannter `target`-Wert) → ab `info` kein Treffer; Fehlercode vorhanden.
- **T6 Ausführung:** Ausführung mit `sshpass -p geheim-0094 …`, Ausgabe mit
  `geheim-0094`; Fehlschlag mit einem `SshError`, dessen Text
  `geheim-0094` enthält → ab `info` kein Treffer.
- **T7 MCP:** MCP-Aufruf eines Kommandos mit `geheim-0094`, einmal
  erfolgreich (Ausgabe mit `geheim-0094`), einmal mit Ausführungsfehler →
  ab `info` kein Treffer; Art des Ergebnisses vorhanden.
- **T8 Provider-Fehler:** (a) Body mit `--password=geheim-0094` in den
  ersten 100 Zeichen → Zeile auf `warn`, kein Treffer (prüft den Redactor);
  (b) Body mit 2000 Zeichen → `body` ≤ 512 Zeichen (prüft die Kürzung);
  (c) `AiError::ModelNotFound` mit `geheim-0094` im Text, geloggt an einer
  Stelle aus A1.7 → kein Treffer.
- **T9 `debug`-Zeilen:** Auf `debug` erscheinen die Inhaltszeilen aus A2;
  ein Geheimnis in einem Muster, das der Redactor kennt
  (`--password=geheim-0094`), ist dort ersetzt. Je ein Fall in `core`,
  `ai-providers`, `app-logic` und `mcp-server`.
- **T10 Standardfilter:** Der Standardfilter aus `init_logging` (ohne
  `RUST_LOG`) lässt `debug` für die Targets von `core`, `ai-providers`,
  `app-logic` und `mcp-server` nicht durch, `info` schon. Scheitert, wenn der
  Standard auf `debug` geändert wird.
- **T11 Filter unverändert:** Alle bisherigen Tests in `core/src/filter`
  bleiben unverändert grün (Regressionswächter, muss heute nicht scheitern).

**Manueller Ablauf (Abnahme BL-0029, Logdatei):** App ohne `RUST_LOG`
starten; `mysql -p'geheim-0094'` einmal als Chat-Vorschlag ausführen, einmal
ablehnen, einmal über MCP ausführen; danach im Log-Verzeichnis nach
`geheim-0094` suchen → kein Treffer.

## 8. Offene Punkte

1. **Alte Logdateien.** Dateien älterer Versionen enthalten weiter Inhalte.
   Optionen: (a) nicht anfassen — sie werden nach 14 Tagen beim Start
   gelöscht (`MAX_LOG_AGE`); (b) beim ersten Start der neuen Version alle
   älteren Logdateien löschen. Empfehlung: (a).
2. **Provider-`body` auch im Diagnosepaket** (Q-BL-0029-02, nicht
   blockierend). Über §1.4 erreicht der `body` aus A1.5 nicht nur die
   Logdatei, sondern auch das Diagnosepaket. Optionen: (a) unverändert —
   nach A1.5/A1.7 trägt die Gruppe weniger als heute, und der Export
   redigiert ein zweites Mal; (b) `body` im Export weglassen; (c) A1.5
   fallen lassen. Empfehlung: (a). Bis zur Entscheidung gilt A1.5 wie
   geschrieben; `SAFE_LOG_MESSAGES` wird in keinem Fall in diesem Schritt
   angefasst (§3).

## 9. Klarstellungen

- §8.1 alte Logdateien: Option (a) — nicht anfassen. Nichts umzusetzen.
- §1.2/§1.4 (Q-BL-0029-01): Zählung und Aussage zum Diagnose-Export
  korrigiert. An den Anforderungen ändert sich nichts; A1.7 gilt funktional,
  nicht über eine Anzahl.

## Umsetzung

**Teil 0:** entfällt (§2).

**Reihenfolge:**
1. `test: record tracing output at all levels in core, ai-providers, app-logic and mcp-server tests [BL-0029]` — Testhilfe (§7 Aufzeichnung).
2. `fix(core): log filter decisions without command text [BL-0029]` — A1.1, A2, T1–T3, T11.
3. `fix(ai-providers): keep prompts, history, tool arguments and error text out of default-level logs [BL-0029]` — A1.2, A1.3, A1.5, A1.7, A2, T4, T5, T8.
4. `fix(app-logic): log command executions and errors without content at info [BL-0029]` — A1.4, A1.7 (inkl. `app-shell`), A2, A3, T6, T9, T10.
5. `fix(mcp-server): log tool call outcomes without content [BL-0029]` — A1.6, A2, T7, T9 (mcp-server).

**Priorität:** ERHÖHT. Angriffsrichtungen für den Review:
- Ein Inhaltsfeld wandert in ein anderes Feld oder in `message`.
- `?`-Ausgabe eines Typs mit Inhalt (`AiAction`, `EvaluationTrace`,
  `SessionContext`, `ActionOutcome`, `AiError`, `SshError`) ab `info`.
- Fehlertexte, die Eingaben wiedergeben (serde-Ausschnitte, `SshError`,
  `AiError::InvalidResponse`/`ModelNotFound`).
- Kürzen vor Redigieren (schneidet Muster an).
- Längenangaben, die den Inhalt verraten (Länge je Wort statt gesamt).
- `#[instrument]` ohne `skip_all` auf Funktionen mit Inhaltsparametern.
- Zweiter und dritter Aufrufer derselben Log-Funktion
  (`openai_compatible.rs`, `discovery.rs`).

**Aufteilung:** ein Lauf, Opus — alle Schritte berühren Redaction und
Datensenken und teilen die Testhilfe.

**Berührte Module:** `crates/core/src/filter/`,
`crates/ai-providers/src/` (`request_logging.rs`, Aufrufer),
`crates/app-logic/src/` (`orchestration/action_exec.rs`, `logging.rs`,
`test_support.rs`, Stellen aus A1.7), `crates/app-shell/src/commands/connect.rs`
(A1.7), `crates/mcp-server/` (`tool_server.rs`, Testhilfe, `Cargo.toml`).

**Melde zurück:** je Test den Beleg „scheitert gegen den alten Stand"; die
Liste der Stellen aus A1.7; das Ergebnis des manuellen Ablaufs, falls du
die App starten kannst, sonst den Ablauf zur Übergabe; jede weitere
Log-Stelle mit Inhalt, die nicht in §1.2 steht — melden, nicht
mitreparieren.
