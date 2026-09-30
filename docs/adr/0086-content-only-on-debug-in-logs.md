# ADR 0086 — Inhalt in der Logdatei nur noch auf `debug`

Status: akzeptiert · Spec: [0094](../specs/0094-no-content-in-default-logs.md) · Backlog: BL-0029

## Kontext

Spec 0094 nimmt jeder Log-Zeile ab `info` den Inhalt: Kommandotext,
Kommando-Ausgabe, Dateiinhalt, Chat-/Notiz-/Prompt-Text, `system_context`,
Werkzeug-Argumente und das `Display` eines `AiError`/`SshError`. Der Inhalt
steht seitdem nur auf `debug`, und `debug` ist ohne `RUST_LOG` aus.

Die Spec lässt bewusst offen, **wie** die einzelnen Stellen umgebaut werden
(§5: „Wie die Stellen umgebaut werden, entscheidet der Coder"). Die
Entscheidungen, die dabei fielen, stehen hier. Dazu die Funde des Reviews,
die bewusst nicht behoben wurden.

## Entscheidungen

### 1. Art statt Inhalt über ausgeschriebene `match`-Ausdrücke

`AiAction::kind()` (`crates/core/src/profiles/types.rs`) und
`ActionOutcome::kind()`/`text_len()` (`crates/mcp-server/src/backend.rs`)
sind neu. Beide geben einen `&'static str` aus einem ausgeschriebenen
`match` über alle Varianten zurück — nicht `std::mem::discriminant`, nicht
ein abgeschnittenes `Debug`.

Grund: Jede Variante dieser beiden Typen trägt Inhalt (`command`,
`new_content`, `content_markdown`, `path`, `content` bzw. `summary`,
`reason`, `message`). Ein ausgeschriebenes `match` erzwingt bei einer neuen
Variante eine Entscheidung an genau dieser Stelle. Ein abgeschnittenes
`Debug` hätte stattdessen stillschweigend den Anfang des nächsten
Feldwertes mitgenommen.

### 2. Ein Redactor je Prozess für die `debug`-Zeilen

`ssh_manager_core::ai::default_log_redactor()` hält genau eine
`DefaultOutputRedactor`-Instanz (`OnceLock`) für die Stellen, an denen kein
Session-Redactor erreichbar ist (`filter::engine`,
`ai_providers::request_logging`, `mcp_server::tool_server`).

Grund: `DefaultOutputRedactor::new()` übersetzt bei jedem Aufruf mehrere
Dutzend `Regex`. Die Filter-Entscheidung ist der heißeste Pfad der
Anwendung; einmal je Entscheidung wäre spürbar. A2 verlangt es außerdem
ausdrücklich.

**Bekannte Grenze** (Review, Runde 1): Dieser Redactor ist *schwächer* als
der einer Sitzung. Der Session-Redactor wird mit
`with_extra_patterns` gebaut und kennt zusätzlich das Sudo-Passwort des
jeweiligen Servers. Ein Kommando, das genau dieses Passwort enthält, bleibt
auf einer `debug`-Zeile, die über den prozessweiten Redactor läuft, im
Klartext stehen. Gegenüber dem Stand vor dieser Spec ist das trotzdem eine
Verbesserung — dort stand derselbe Text roh auf `info`. Vermerkt am
Doc-Kommentar der Funktion, damit eine neue Inhaltszeile den
Session-Redactor nimmt, wo er erreichbar ist.

### 3. Redaction läuft nur, wenn `debug` aufgezeichnet wird

A2 verlangt das. Umgesetzt nicht über eine eigene `if tracing::enabled!`-
Abfrage, sondern indem der `redact_text`-Aufruf im **Feldausdruck** des
`tracing::debug!`-Makros steht. Dessen Expansion wertet Feldausdrücke erst
innerhalb des `if enabled`-Zweigs aus, den es selbst erzeugt — im
Standardbetrieb ohne `debug` kostet die Redaction damit nichts. Eine
zusätzliche Abfrage wäre eine zweite, redundante Prüfung derselben
Bedingung.

### 4. Reihenfolge: redigieren, dann kürzen

An beiden Stellen, die kürzen (`truncate_logged_body` in
`request_logging.rs`, `truncate_for_log` in `action_exec.rs`), läuft die
Redaction **vor** der Kürzung. Umgekehrt schneidet die Kürzung ein
Secret-Muster entzwei, danach greift kein Muster mehr, und der Anfang des
Geheimnisses bleibt im Klartext stehen.

In `action_exec.rs` stand zunächst die falsche Reihenfolge (Review,
Runde 1). Am heutigen einzigen Aufrufer war das folgenlos, weil der den
vollen Text schon vorher redigiert — aber der zweite Durchlauf dort
existiert gerade für einen künftigen Aufrufer, der das nicht tut.

### 5. Der Hinweistext zählt zur Längengrenze

A1.5 erlaubt dem Provider-Antwortkörper höchstens 512 Zeichen. Der
Hinweis „… (gekürzt, vollständig nur auf debug)" wird **in** diese 512
Zeichen hineingerechnet, nicht angehängt (Review, Runde 1). Sonst wäre das
Feld ~549 Zeichen lang, und die geprüfte Grenze wäre nicht die, die in der
Spec steht.

### 6. Prozessweiter Mitschnitt in `mcp-server`, thread-lokal in den anderen

Die neue Testhilfe `crates/mcp-server/src/test_support.rs` nutzt einen
prozessweiten Puffer unter `Mutex` plus eine zweite Sperre, die die
aufzeichnenden Tests gegeneinander serialisiert — anders als die
thread-lokalen Puffer in `core`, `ai-providers` und `app-logic`.

Grund: Die geprüfte Zeile entsteht in `tool_server.rs` innerhalb eines
`tokio::spawn`, und die Tests laufen auf `rt-multi-thread`. Ein
thread-lokaler Puffer hätte diese Zeile nie zu sehen bekommen — und eine
Abwesenheits-Aussage („steht nicht auf `info`") wäre über einen leeren
Puffer trivial wahr und damit wertlos geworden.

### 7. Ab `info` nur die Gesamtlänge

Längenangaben stehen je Log-Zeile bzw. je History-Eintrag, nie je Wort oder
je Teilkommando. Eine Längenreihe verrät die Struktur eines Kommandos und
damit mittelbar seinen Inhalt. Aus demselben Grund steht ab `info` auch kein
Hash und kein Programmname (Spec §5): Passwörter haben wenig Entropie, und
das erste Wort kann selbst ein Geheimnis sein (`PGPASSWORD=… psql`).

## Bewusst nicht behoben

Alle drei sind Funde des Reviews, die **außerhalb** von §1.2 der Spec
liegen. Die Spec sagt dazu: „jede weitere Log-Stelle mit Inhalt, die nicht
in §1.2 steht — melden, nicht mitreparieren."

1. **Remote-Pfade auf `info`.** `app_shell::commands::elevation` (Zeile
   „file browser change with elevated rights") und
   `app_shell::elevated_sftp` (Zeile „file browser elevated rights
   enabled") loggen einen Pfad auf dem Server und den Zielnutzer. Ein Pfad
   ist keine der Kategorien aus A1, und beide Zeilen sind
   Audit-Einträge manueller Aktionen (ADR 0045). Eigener Backlog-Punkt.
2. **`log_stop_reason`** loggt den vom Provider gelieferten
   `stop_reason`-String roh auf `info`, und die Zeile steht in
   `SAFE_LOG_MESSAGES`. Provider-kontrolliert, in der Praxis ein Enum-Wert
   (`end_turn`, `max_tokens`), kein Nutzerinhalt. Eigener Backlog-Punkt.
3. **`CHANGELOG.md`** bleibt unverändert. Nicht weil der Eintrag fehlen
   soll, sondern weil dieses Repo Changelog-Einträge als Fragment unter
   `changelog.d/` sammelt und erst beim Versionssprung zusammenführt. Das
   Fragment zu dieser Spec liegt als `changelog.d/0094-log-ohne-inhalt.md`.

## Offen

§8.2 der Spec (Q-BL-0029-02): Über `SAFE_LOG_MESSAGES` erreicht der
Provider-Antwortkörper aus der A1.5-Ausnahme nicht nur die Logdatei,
sondern auch das Diagnosepaket, das Nutzer in ein oft öffentliches Issue
einfügen. Der Code setzt die Empfehlung der Spec um (unverändert lassen);
`SAFE_LOG_MESSAGES` ist in diesem Schritt nicht angefasst (§3). Die
Entscheidung selbst steht noch aus.
