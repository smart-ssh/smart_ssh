//! Diagnose-Export (Spec 0063): ein einzelnes, redigiertes Text-Paket
//! (Version/Hash, OS, Datenpfade, App-Zustand, letzte Log-Zeilen), das ein
//! Nutzer an den Support hängen oder in ein — oft **öffentliches** —
//! GitHub-Issue pasten kann, statt bei jedem "geht nicht" Version/OS/Log
//! einzeln erfragen zu müssen.
//!
//! Bewusst reine, IO-freie Zusammenstellung ([`build_diagnostics_bundle`])
//! getrennt von der eigentlichen Sammlung der Eingabedaten (liest DB/Log-
//! Ordner, Provider-/Server-Zahlen — passiert im `generate_diagnostics_
//! bundle`-Tauri-Command in `commands.rs`) — dasselbe Muster wie
//! `document_export.rs`: die reine Textzusammenstellung lässt sich ohne
//! Tauri-Laufzeit/echtes Dateisystem testen.
//!
//! **Sicherheitsrelevant (Spec 0063 §2/§3, PFLICHT)**: zwei unabhängige
//! Schutzschichten für die eingebetteten Log-Zeilen, in dieser Reihenfolge.
//! (1) [`SAFE_LOG_MESSAGES`] — eine Positivliste rein struktureller/
//! Lifecycle-/Fehler-Metadaten-Log-Zeilen; alles andere (insbesondere
//! Chat-History, Notiz-Inhalte, Kommando-Text/-Output, Server-Adressen —
//! Spec 0063 §2 "KEINE") fliegt vollständig aus dem Paket, nicht nur
//! redigiert. Das ist die eigentliche Antwort auf §2, nicht der Redactor
//! (2. spec-reviewer-Runde: der ursprüngliche Stand redigierte zwar Secret-
//! *Muster*, ließ aber Nutzer*inhalte* wie die volle Chat-History
//! unverändert durch — genau die von §2 ausgeschlossene Kategorie). (2)
//! Was die Positivliste durchlässt, läuft zusätzlich durch den
//! [`OutputRedactor`] — Defense in Depth für den Fall, dass eine der
//! zugelassenen Zeilen (z. B. eine Fehlermeldung mit einer URL) doch ein
//! Secret-Muster trägt. Ebenso sicherheitsrelevant: [`DiagnosticsInput`]
//! hat schlicht **keine Felder** für Keys/Passwörter/Host-Keys/Server-
//! Adressen/Notiz-/Chat-Inhalte — was nicht im Eingabe-Typ steckt, kann
//! `build_diagnostics_bundle` strukturell nicht ausgeben, unabhängig von
//! einer Laufzeit-Filterung.

use ssh_manager_core::ai::OutputRedactor;

/// Spec 0063 §2 ("KEINE ... Notiz-Inhalte, Chat-Inhalte") — spec-reviewer-
/// Fund (2. Runde, ERHÖHT): der Log-Tail selbst trägt genau diese
/// Kategorien, unabhängig vom `OutputRedactor` (der nur Secret-*Muster*
/// erkennt, keine Nutzerinhalte). Betroffene Log-Zeilen aus `tracing`-
/// Aufrufen, deren `fields.message` NICHT in dieser Liste steht:
/// `ai_providers::request_logging::log_outgoing_context` ("outgoing
/// session context to AI provider" — System-Kontext + volle Chat-History),
/// `log_tool_call_fragment`/`log_tool_call_parsed` (rohe Tool-Argumente,
/// i. d. R. das vorgeschlagene Kommando im Klartext),
/// `ssh_manager_core::filter::engine` ("filter engine decision" — voller
/// Kommandotext), `app_shell::orchestration::log_command_execution`(_failed)
/// ("ssh command executed"/"ssh command execution failed" — stdout/stderr),
/// `app_shell::commands` ("host key trusted", "connection attempt failed",
/// "resolving the connection target (jump host chain) failed" — alle drei
/// tragen Host+Port, eine Server-Adresse).
///
/// Statt jede aktuelle UND künftige sensible Log-Zeile einzeln
/// auszuschließen (Denylist — bricht offen, sobald jemand eine neue
/// `tracing::info!` mit Nutzerinhalt hinzufügt, ohne an dieses Modul zu
/// denken), eine **Positivliste** rein struktureller/Lifecycle-/
/// Fehler-Metadaten-Zeilen: jede Zeile, deren `message` hier NICHT
/// aufgeführt ist, fliegt aus dem Diagnosepaket — auch wenn sie
/// tatsächlich harmlos wäre. Fail-closed: die Kosten eines fälschlich
/// ausgeschlossenen, harmlosen Log-Eintrags (etwas weniger Diagnose-
/// Kontext) sind hier bewusst in Kauf genommen gegen die Kosten eines
/// fälschlich eingeschlossenen, sensiblen (ein öffentlicher Leak).
const SAFE_LOG_MESSAGES: &[&str] = &[
    // app_shell::lib / Startup-Lifecycle
    "Smart SSH startet",
    "connecting to SQLite database",
    "running database migrations",
    "database migrations complete",
    "SQLite database connected",
    "resolving chat-content encryption key from OS keychain",
    "chat-content encryption key resolved",
    "loading host-key store",
    "host-key store loaded",
    "app setup complete, creating main window",
    "mcp server started",
    "mcp notification could not be shown",
    "fatal: SQLite database connect/migrate failed",
    "fatal: host-key store failed to load",
    "showing fatal startup error dialog",
    "showing non-fatal startup warning dialog",
    "custom titlebar decoration activation failed, restored native titlebar",
    "macOS traffic-light inset failed, keeping the custom titlebar active",
    "app panicked",
    // Sitzungs-Lifecycle (nur session_id/server_id — UUIDs, keine
    // Adressen/Namen)
    "session connected",
    "session disconnected",
    "old chat sessions cleaned up",
    // Persistenz-/Hintergrund-Fehler (nur `error`-Feld, kein Nutzerinhalt)
    "chat message persistence failed",
    "chat session auto-titling failed",
    "chat session creation failed",
    "chat session mark_ended failed",
    "chat session retention cleanup failed",
    "ledger entry persistence failed",
    "legacy plaintext prompt_history rows encrypted",
    "prompt_history encryption migration failed",
    "note shrink persistence failed",
    "note shrink summarization failed",
    "note shrink summarization timed out",
    "note target resolution failed",
    "session summary could not be loaded on resume",
    "session summary generation timed out, falling back to plain round truncation",
    "session summary persistence failed",
    "session summary generation failed, falling back to plain round truncation",
    "skipping note-update suggestion on disconnect: context compaction shortened the \
     note for this request, a proposal based on it could drop content",
    "policy source unavailable, skipped",
    // ai_providers — nur request_id/Status-Metadaten, keine Prompt-/
    // Tool-Argument-Inhalte
    "AI request future started executing",
    "about to send HTTP request to AI provider",
    "about to poll AI provider stream for the first time",
    "received text delta stream (summarized)",
    "AI response turn ended",
    // Spec 0080, A4: Nachfolger von "received text delta stream
    // (summarized)" für den OpenAI-kompatiblen Provider — trägt dieselbe
    // Art Inhalt (nur `request_id` + zwei Längen, kein Prompt-/Antworttext),
    // s. `ai_providers::request_logging::log_openai_round_summary`-
    // Doc-Kommentar. Ohne diesen Eintrag würde das Diagnosepaket für diesen
    // Provider genau die Information verlieren, die Spec 0080 §1 fehlte.
    "AI response round ended (text/reasoning length)",
    "AI provider returned an error response",
    "AI provider rate-limited the request (429) — retrying with backoff",
    "AI provider transport/connection error",
];

/// s. [`SAFE_LOG_MESSAGES`]. `false` für alles, was kein valides JSON-Lines-
/// Objekt mit einem erkannten `fields.message` ist — dieselbe Fail-closed-
/// Richtung wie oben (ein nicht parsbares Log-Format wird ausgeschlossen,
/// nicht durchgereicht).
fn is_safe_diagnostic_log_line(line: &str) -> bool {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    let Some(message) = parsed
        .get("fields")
        .and_then(|f| f.get("message"))
        .and_then(|m| m.as_str())
    else {
        return false;
    };
    SAFE_LOG_MESSAGES.contains(&message)
}

/// Spec 0063 §2: exakt die Felder, die ins Paket dürfen — bewusst als
/// eigener Typ statt z. B. `AppState`/`AiProviderConfig` direkt
/// durchzureichen, damit der Compiler mit erzwingt, dass niemand
/// versehentlich ein sensibles Feld (`credential_ref`, `base_url`,
/// `extra_headers`, Server-Adressen, Notiz-/Chat-Inhalte) mit einschleust.
pub struct DiagnosticsInput {
    pub version_display: String,
    /// `"Dev"`/`"Release"` (`crate::version::BuildType::as_str`).
    pub build_type: &'static str,
    pub edition: String,
    pub os: &'static str,
    pub arch: &'static str,
    /// Best-effort (Spec 0063 §2: "OS, Version") — `None`, falls die
    /// plattformspezifische Abfrage fehlschlägt (kein neues Cargo-
    /// Dependency nötig, s. `os_version`-Doc-Kommentar in `commands.rs`).
    pub os_version: Option<String>,
    pub db_path: String,
    pub log_dir: String,
    pub host_key_path: String,
    /// Spec 0063 §2: "Provider-**Typen** (nicht Keys!)" — je konfiguriertem
    /// Provider nur `ProviderType::as_db_str()`, z. B. `["anthropic",
    /// "openai"]`. Absichtlich `Vec<String>`, kein `Vec<AiProviderConfig>`
    /// (s. Modul-Doc). `None`, falls der Store-Zugriff selbst fehlschlug —
    /// spec-reviewer-Fund (Follow-up-Review): `commands.rs` bildete das
    /// zuvor über `unwrap_or_default()` auf "keine Provider konfiguriert"
    /// ab, was für ein Diagnose-Artefakt aktiv irreführend ist (ein
    /// Lese-/DB-Fehler sieht dann wie eine leere, aber intakte
    /// Konfiguration aus — für die Fehlersuche der eigentlich interessante
    /// Unterschied).
    pub provider_types: Option<Vec<String>>,
    /// Wie `provider_types`: `None` nur bei einem tatsächlichen Lesefehler,
    /// nicht bei null konfigurierten Servern (das ist `Some(0)`).
    pub server_count: Option<usize>,
}

/// Spec 0063 §2: "die letzten N Log-Zeilen ... redigiert". `max_lines` wird
/// vom Aufrufer (`commands::generate_diagnostics_bundle`) über
/// `logging::read_last_log_lines` durchgereicht — hier nur noch die
/// Redaction + Formatierung.
pub const MAX_LOG_LINES: usize = 500;

/// Setzt das redigierte Text-Paket zusammen (Markdown — Spec 0063 §4:
/// "leicht in ein Issue zu pasten", eine einzelne Datei reicht, s.
/// `commands.rs`-Doc-Kommentar zur Formatwahl).
pub fn build_diagnostics_bundle(
    input: &DiagnosticsInput,
    log_lines: &[String],
    redactor: &dyn OutputRedactor,
) -> String {
    let mut out = String::new();

    out.push_str("# Smart SSH – Diagnosepaket\n\n");
    out.push_str(
        "Dieses Paket wurde lokal auf diesem Rechner zusammengestellt und **nicht** automatisch \
         versendet. Bitte kurz durchsehen, bevor du es an den Support gibst oder in ein \
         (ggf. öffentliches) Issue einfügst.\n\n",
    );

    out.push_str("## Version\n");
    out.push_str(&format!(
        "{} · {} · {}-Build\n\n",
        input.version_display, input.edition, input.build_type
    ));

    out.push_str("## System\n");
    out.push_str(&format!("OS: {} ({})\n", input.os, input.arch));
    out.push_str(&format!(
        "OS-Version: {}\n\n",
        input.os_version.as_deref().unwrap_or("unbekannt")
    ));

    out.push_str("## Datenpfade\n");
    out.push_str(&format!("Datenbank: {}\n", input.db_path));
    out.push_str(&format!("Logs: {}\n", input.log_dir));
    out.push_str(&format!("Host-Keys: {}\n\n", input.host_key_path));

    out.push_str("## App-Zustand\n");
    out.push_str(&format!(
        "Konfigurierte KI-Provider-Typen: {}\n",
        match &input.provider_types {
            None => "unbekannt (Fehler beim Lesen)".to_string(),
            Some(types) if types.is_empty() => "keine".to_string(),
            Some(types) => types.join(", "),
        }
    ));
    out.push_str(&format!(
        "Server: {}\n\n",
        match input.server_count {
            None => "unbekannt (Fehler beim Lesen)".to_string(),
            Some(count) => count.to_string(),
        }
    ));

    // Spec 0063 §2 ("KEINE ... Notiz-Inhalte, Chat-Inhalte") — s.
    // `SAFE_LOG_MESSAGES`-Doc-Kommentar: erst auf strukturelle/Lifecycle-
    // Zeilen eingrenzen, DANACH zusätzlich redigieren (Defense in Depth
    // bleibt für das, was durch die Positivliste kommt, z. B. eine
    // versehentlich in einer Fehlermeldung mitgelieferte URL mit
    // eingebetteten Zugangsdaten).
    let safe_lines: Vec<&String> = log_lines
        .iter()
        .filter(|line| is_safe_diagnostic_log_line(line))
        .collect();

    out.push_str(&format!(
        "## Letzte Log-Zeilen (auf unkritische Diagnose-Ereignisse beschränkt und redigiert, \
         bis zu {MAX_LOG_LINES})\n```\n"
    ));
    if safe_lines.is_empty() {
        out.push_str("(keine Log-Zeilen verfügbar)\n");
    } else {
        for line in safe_lines {
            out.push_str(&redactor.redact_text(line));
            out.push('\n');
        }
    }
    out.push_str("```\n");

    out
}

#[cfg(test)]
mod tests {
    use ssh_manager_core::ai::DefaultOutputRedactor;

    use super::*;

    fn base_input() -> DiagnosticsInput {
        DiagnosticsInput {
            version_display: "0.5.0 (892001f)".to_string(),
            build_type: "Dev",
            edition: "Community".to_string(),
            os: "macos",
            arch: "aarch64",
            os_version: Some("15.1".to_string()),
            db_path: "/Users/test/Library/Application Support/Smart SSH/smart-ssh.db".to_string(),
            log_dir: "/Users/test/Library/Logs/Smart SSH".to_string(),
            host_key_path: "/Users/test/Library/Application Support/Smart SSH/host_keys.json"
                .to_string(),
            provider_types: Some(vec!["anthropic".to_string()]),
            server_count: Some(3),
        }
    }

    #[test]
    fn test_bundle_contains_version_os_paths_and_state() {
        let bundle = build_diagnostics_bundle(&base_input(), &[], &DefaultOutputRedactor::new());

        assert!(bundle.contains("0.5.0 (892001f) · Community · Dev-Build"));
        assert!(bundle.contains("macos"));
        assert!(bundle.contains("aarch64"));
        assert!(bundle.contains("15.1"));
        assert!(bundle.contains("smart-ssh.db"));
        assert!(bundle.contains("Smart SSH/host_keys.json") || bundle.contains("host_keys.json"));
        assert!(bundle.contains("anthropic"));
        assert!(bundle.contains("Server: 3"));
    }

    /// Baut eine realistische, JSON-Lines-formatierte Log-Zeile (exakt das
    /// Format aus `logging::init_logging`s `tracing_subscriber::fmt().json()`)
    /// mit `message` als `fields.message` — die Form, die
    /// `is_safe_diagnostic_log_line` tatsächlich parst.
    fn json_log_line(message: &str, extra_fields: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-09-19T12:00:00Z","level":"INFO","fields":{{"message":"{message}"{extra_fields}}},"target":"test"}}"#
        )
    }

    /// Der eigentliche Testfall aus der Spec: ein Fake-Secret in einer
    /// Log-Zeile darf NICHT im Klartext im Paket landen — hier auf einer
    /// zugelassenen (`SAFE_LOG_MESSAGES`) Zeile, damit der Test wirklich
    /// die Redaction prüft, nicht (versehentlich) nur die Positivliste.
    #[test]
    fn test_fake_secret_in_log_line_is_redacted() {
        let log_lines = vec![json_log_line(
            "AI provider transport/connection error",
            r#","error":"token=sk-ant-FAKE-SECRET-VALUE-123""#,
        )];

        let bundle =
            build_diagnostics_bundle(&base_input(), &log_lines, &DefaultOutputRedactor::new());

        assert!(
            !bundle.contains("sk-ant-FAKE-SECRET-VALUE-123"),
            "Fake-Secret darf nicht im Klartext im Diagnosepaket landen: {bundle}"
        );
        assert!(
            bundle.contains("[REDACTED]"),
            "die Zeile muss als redigiert erkennbar bleiben: {bundle}"
        );
    }

    /// Spec 0063 §2 ("KEINE ... Notiz-Inhalte, Chat-Inhalte") — spec-
    /// reviewer-Fund (2. Runde, ERHÖHT): das war vor dem Follow-up-Fix NICHT
    /// abgedeckt — der Redactor kennt nur Secret-*Muster*, keine
    /// Chat-*Inhalte*. Eine `log_outgoing_context`-Zeile (System-Kontext +
    /// volle Chat-History) muss deshalb VOLLSTÄNDIG ausgeschlossen werden,
    /// nicht nur redigiert.
    #[test]
    fn test_chat_history_log_line_is_excluded_entirely_not_just_redacted() {
        let sensitive_chat_text = "mein geheimer Plan ist XYZ-ACME-42";
        let log_lines = vec![json_log_line(
            "outgoing session context to AI provider",
            &format!(r#","history":"[{{\"text\":\"{sensitive_chat_text}\"}}]""#),
        )];

        let bundle =
            build_diagnostics_bundle(&base_input(), &log_lines, &DefaultOutputRedactor::new());

        assert!(
            !bundle.contains(sensitive_chat_text),
            "Chat-Inhalt darf gar nicht erst ins Paket gelangen: {bundle}"
        );
        assert!(
            !bundle.contains("outgoing session context"),
            "die ganze Zeile muss verschwinden, nicht nur der sensible Teil: {bundle}"
        );
        assert!(bundle.contains("(keine Log-Zeilen verfügbar)"));
    }

    /// Ebenso: Kommando-Output (`ssh command executed`) und eine
    /// Server-Adresse (`host key trusted`) müssen komplett rausfallen.
    #[test]
    fn test_command_output_and_host_address_log_lines_are_excluded() {
        let log_lines = vec![
            json_log_line(
                "ssh command executed",
                r#","command":"cat /etc/nginx/nginx.conf","stdout":"server_name internal.example.corp;""#,
            ),
            json_log_line("host key trusted", r#","host":"192.168.1.42","port":22"#),
        ];

        let bundle =
            build_diagnostics_bundle(&base_input(), &log_lines, &DefaultOutputRedactor::new());

        assert!(!bundle.contains("nginx.conf"));
        assert!(!bundle.contains("internal.example.corp"));
        assert!(!bundle.contains("192.168.1.42"));
    }

    /// Fail-closed-Gegenprobe: eine unbekannte/nicht gelistete `message`
    /// (z. B. eine künftig hinzugefügte `tracing::info!`-Zeile, an die
    /// niemand hier gedacht hat) fliegt ebenfalls raus, statt im Zweifel
    /// durchgelassen zu werden.
    #[test]
    fn test_unrecognized_log_message_is_excluded_by_default() {
        let log_lines = vec![json_log_line(
            "some brand new log line nobody vetted yet",
            "",
        )];

        let bundle =
            build_diagnostics_bundle(&base_input(), &log_lines, &DefaultOutputRedactor::new());

        assert!(bundle.contains("(keine Log-Zeilen verfügbar)"));
    }

    /// Ebenso fail-closed: nicht-JSON-Zeilen (z. B. ein alter
    /// Klartext-Log-Rest, ein beschädigter Frame).
    #[test]
    fn test_non_json_log_line_is_excluded() {
        let log_lines = vec!["not a json line at all".to_string()];

        let bundle =
            build_diagnostics_bundle(&base_input(), &log_lines, &DefaultOutputRedactor::new());

        assert!(bundle.contains("(keine Log-Zeilen verfügbar)"));
    }

    /// Gegenprobe: eine tatsächlich zugelassene Lifecycle-Zeile bleibt
    /// erhalten — die Positivliste schließt nicht schlicht alles aus.
    #[test]
    fn test_allowlisted_lifecycle_log_line_is_included() {
        let log_lines = vec![json_log_line("session connected", "")];

        let bundle =
            build_diagnostics_bundle(&base_input(), &log_lines, &DefaultOutputRedactor::new());

        assert!(bundle.contains("session connected"));
        assert!(!bundle.contains("(keine Log-Zeilen verfügbar)"));
    }

    /// Spec-reviewer-Fund (Spec 0080, Review dieses Schritts): die neue A4-
    /// Log-Zeile des OpenAI-kompatiblen Providers (`text_len`/
    /// `reasoning_len`, kein Prompt-/Antworttext) muss im Diagnosepaket
    /// erhalten bleiben — ohne diesen Allowlist-Eintrag würde sie (wie jede
    /// unbekannte Zeile) fail-closed ausgeschlossen, und für diesen
    /// Provider verschwände genau die Information wieder, die Spec 0080
    /// §1 ursprünglich fehlte.
    #[test]
    fn test_a4_round_summary_log_line_is_allowlisted() {
        let log_lines = vec![json_log_line(
            "AI response round ended (text/reasoning length)",
            r#","text_len":0,"reasoning_len":42"#,
        )];

        let bundle =
            build_diagnostics_bundle(&base_input(), &log_lines, &DefaultOutputRedactor::new());

        assert!(bundle.contains("AI response round ended (text/reasoning length)"));
        assert!(!bundle.contains("(keine Log-Zeilen verfügbar)"));
    }

    /// Gegenprobe zum Spec-Testfall: ein konfigurierter (Fake-)API-Key darf
    /// gar nicht erst ins Paket gelangen — hier strukturell erzwungen, weil
    /// `DiagnosticsInput` überhaupt kein Feld für Keys/Credentials hat.
    /// `build_diagnostics_bundle` bekommt den Key also nie zu Gesicht; der
    /// Test dokumentiert diese Invariante trotzdem explizit, statt sie nur
    /// implizit aus der Typdefinition abzuleiten.
    #[test]
    fn test_provider_api_key_never_appears_only_the_type_does() {
        let mut input = base_input();
        input.provider_types = Some(vec!["anthropic".to_string()]);
        let fake_api_key = "sk-ant-FAKE-CONFIGURED-KEY-456";

        let bundle = build_diagnostics_bundle(&input, &[], &DefaultOutputRedactor::new());

        assert!(bundle.contains("anthropic"));
        assert!(!bundle.contains(fake_api_key));
    }

    #[test]
    fn test_no_configured_providers_or_log_lines_degrades_gracefully() {
        let mut input = base_input();
        input.provider_types = Some(Vec::new());

        let bundle = build_diagnostics_bundle(&input, &[], &DefaultOutputRedactor::new());

        assert!(bundle.contains("keine"));
        assert!(bundle.contains("(keine Log-Zeilen verfügbar)"));
    }

    /// Spec-reviewer-Fund (Follow-up-Review): ein Store-Lesefehler muss im
    /// Paket klar von "null konfiguriert" unterscheidbar sein — sonst
    /// sieht eine kaputte DB wie "Nutzer hat nichts eingerichtet" aus,
    /// genau das Gegenteil dessen, was ein Diagnose-Artefakt leisten soll.
    #[test]
    fn test_store_read_failure_is_shown_as_unknown_not_as_zero() {
        let mut input = base_input();
        input.provider_types = None;
        input.server_count = None;

        let bundle = build_diagnostics_bundle(&input, &[], &DefaultOutputRedactor::new());

        assert!(bundle.contains("unbekannt (Fehler beim Lesen)"));
        assert!(!bundle.contains("Server: 0"));
        assert!(!bundle.contains("Konfigurierte KI-Provider-Typen: keine"));
    }
}
