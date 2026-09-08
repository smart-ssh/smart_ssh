//! Gemeinsame Logging-Helfer für Spec 0016 ("Strukturiertes Logging &
//! Diagnose"), Abschnitt 4, Punkte 1–3 — geteilt zwischen
//! `AnthropicProvider` und `OpenAiCompatibleProvider`, damit das Format
//! nicht zwischen beiden auseinanderläuft (analog zu `crate::error`).
//!
//! `request_id` korreliert alle Log-Zeilen *eines* `AiProvider::send()`-
//! Aufrufs (s. `crate::anthropic::AnthropicProvider::send`-Kommentar zur
//! Design-Entscheidung, warum diese ID lokal pro Aufruf erzeugt wird statt
//! ein `SessionContext`-Feld zu sein).

use ssh_manager_core::ai::{AiError, MessageContent, RejectionReason, SessionContext};
use ssh_manager_core::profiles::AiAction;
use uuid::Uuid;

/// Spec 0016, Abschnitt 4, Punkt 1: der tatsächlich an den Provider
/// gesendete `SessionContext` — **nach** Redaction. Der hier ankommende
/// `context` wurde bereits in `app-shell::orchestration` redigiert, bevor
/// ein Kommando-Ergebnis überhaupt in `context.history` landete (s.
/// `OutputRedactor`, Spec 0006 Abschnitt 5) — diese Funktion loggt also nie
/// rohen, unredigierten Kommando-Output. `CommandResult`-Einträge werden
/// hier bewusst nur als Kommando + Längen zusammengefasst (nicht der volle
/// Text): der volle, redigierte Output steht bereits in einem eigenen
/// Log-Eintrag pro Ausführung (Spec 0016, Abschnitt 4, Punkt 5, s.
/// `app-shell::orchestration::log_command_execution`) — ihn hier zusätzlich
/// vollständig zu wiederholen würde Logs nur unnötig aufblähen, ohne neue
/// Information zu liefern.
pub(crate) fn log_outgoing_context(request_id: Uuid, context: &SessionContext) {
    let history: Vec<String> = context
        .history
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::CommandResult { command, output, cancelled } => format!(
                "[command_result] {command} (exit={:?}, stdout_len={}, stderr_len={}, cancelled={cancelled})",
                output.exit_code,
                output.stdout.len(),
                output.stderr.len()
            ),
            MessageContent::ActionRejected { command, reason } => format!(
                "[action_rejected] {command} ({})",
                match reason {
                    RejectionReason::User => "user".to_string(),
                    RejectionReason::Blocked(reason) => format!("blocked: {reason}"),
                    RejectionReason::Timeout => "timeout".to_string(),
                }
            ),
        })
        .collect();
    let action_names: Vec<&str> = context
        .available_actions
        .iter()
        .map(|a| a.name.as_str())
        .collect();

    tracing::info!(
        request_id = %request_id,
        system_context = %context.system_context,
        history_len = history.len(),
        history = ?history,
        available_actions = ?action_names,
        "outgoing session context to AI provider",
    );
}

/// Spec 0016, Abschnitt 4, Punkt 2: Text-Deltas zusammengefasst geloggt
/// (Gesamtlänge des Streams), nicht zeichenweise — die Spec erlaubt das
/// explizit ("reine Text-Deltas ggf. zusammengefasst statt Zeichen für
/// Zeichen").
pub(crate) fn log_text_delta_summary(request_id: Uuid, total_len: usize) {
    if total_len == 0 {
        return;
    }
    tracing::debug!(
        request_id = %request_id,
        text_len = total_len,
        "received text delta stream (summarized)",
    );
}

/// Spec 0016, Abschnitt 4, Punkt 2: ein vollständig akkumuliertes
/// Tool-Call-JSON-Fragment, sobald ein Block abgeschlossen ist — "vollständig"
/// bezieht sich auf den fertigen Block, nicht auf jedes einzelne
/// Zwischen-Chunk (die läppern sich oft zu keinem gültigen JSON für sich
/// genommen).
pub(crate) fn log_tool_call_fragment(request_id: Uuid, tool_name: &str, raw_arguments: &str) {
    tracing::info!(
        request_id = %request_id,
        tool_name,
        raw_arguments,
        "received tool call fragment",
    );
}

/// Spec 0016, Abschnitt 4, Punkt 3, Erfolgsfall.
pub(crate) fn log_tool_call_parsed(request_id: Uuid, action: &AiAction) {
    tracing::info!(
        request_id = %request_id,
        action = ?action,
        "tool call parsed successfully",
    );
}

/// Spec 0016, Abschnitt 4, Punkt 3, Fehlerfall: die **vollständige
/// Rohantwort** plus die genaue Fehlermeldung — genau das, was im
/// beobachteten `target_id ist keine gültige UUID`-Bugfall (Spec 0016,
/// Abschnitt 1/6) gefehlt hätte, um sofort zu sehen, was die KI tatsächlich
/// geschickt hat.
pub(crate) fn log_tool_call_parse_error(
    request_id: Uuid,
    tool_name: &str,
    raw_arguments: &str,
    error: &dyn std::fmt::Display,
) {
    tracing::error!(
        request_id = %request_id,
        tool_name,
        raw_arguments,
        error = %error,
        "tool call parsing/validation failed",
    );
}

/// Spec 0049, Fund 2: bislang wurde nur die ausgehende Anfrage geloggt
/// (`log_outgoing_context` oben), nie die **Fehlerantwort** des Providers
/// — bei der Diagnose eines Tester-Problems musste geraten werden. Ergänzt
/// die verständliche UI-Meldung (Spec 0047, Fund D2), ersetzt sie nicht.
///
/// Für einen nicht-erfolgreichen HTTP-Status, **bevor** `body` von
/// `crate::error::map_http_status` auf eine der u. U. detailärmeren
/// `AiError`-Varianten (`AuthenticationFailed`/`RateLimited` sind Unit-
/// Varianten ohne Status/Body) reduziert wird — deshalb wird hier an den
/// eigentlichen Aufrufstellen geloggt (mit dem noch vollständigen
/// `status`/`body`), nicht erst am gemeinsamen App-Shell-Chokepoint, wo
/// diese Information für 401/429 bereits verloren wäre.
///
/// Redaction-Invariante (Spec 0016/gilt auch hier laut Spec 0049): `body`
/// ist strikt der Response-Body des Providers, nie die ausgehenden
/// Request-Header — der eigene API-Key kann dort unter normalen Umständen
/// gar nicht auftauchen. Trotzdem wird defensiv geprüft, ob `api_key`
/// wörtlich in `body` vorkommt (z. B. ein Provider, der die Anfrage in
/// einer Fehlermeldung spiegelt) und in diesem Fall ersetzt — dieselbe
/// "Logs sind kein Schlupfloch für Secrets"-Regel wie bei
/// `app_shell::orchestration::log_command_execution`.
pub(crate) fn log_provider_error_response(
    request_id: Uuid,
    status: u16,
    body: &str,
    error: &AiError,
    api_key: &str,
) {
    tracing::warn!(
        request_id = %request_id,
        status,
        code = error.code(),
        body = %redact_secret(body, api_key),
        "AI provider returned an error response",
    );
}

/// Gegenstück zu [`log_provider_error_response`] für einen Transport-
/// Fehler (Verbindungsaufbau, Timeout, TLS, ...) — hier existiert kein
/// HTTP-Status/Response-Body, nur die (bereits über `Display` lesbare)
/// Fehlermeldung, die nie den API-Key enthalten kann (sie beschreibt einen
/// gescheiterten Verbindungsaufbau, nicht dessen Inhalt).
pub(crate) fn log_provider_transport_error(request_id: Uuid, error: &AiError) {
    tracing::warn!(
        request_id = %request_id,
        code = error.code(),
        error = %error,
        "AI provider transport/connection error",
    );
}

/// Ersetzt jedes wörtliche Vorkommen von `secret` in `text` durch
/// `[REDACTED]` — no-op für ein leeres `secret` (sonst würde
/// `str::replace` jedes Zeichen "ersetzen").
fn redact_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        return text.to_string();
    }
    text.replace(secret, "[REDACTED]")
}

#[cfg(test)]
mod error_logging_tests {
    use super::*;

    thread_local! {
        static TEST_LOG_BUFFER: std::cell::RefCell<Vec<u8>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    #[derive(Clone, Default)]
    struct ThreadLocalTestWriter;

    impl std::io::Write for ThreadLocalTestWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            TEST_LOG_BUFFER.with(|b| b.borrow_mut().extend_from_slice(buf));
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for ThreadLocalTestWriter {
        type Writer = ThreadLocalTestWriter;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// Dasselbe Muster wie `app_shell::orchestration`s
    /// `install_test_subscriber_once` (dortiger Kommentar erklärt, warum
    /// ein einmaliger **globaler** Default nötig ist statt
    /// `tracing::subscriber::with_default`).
    fn install_test_subscriber_once() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .json()
                .with_writer(ThreadLocalTestWriter)
                .finish();
            let _ = tracing::subscriber::set_global_default(subscriber);
        });
    }

    #[test]
    fn test_provider_error_response_logs_status_and_code_redacted() {
        install_test_subscriber_once();
        TEST_LOG_BUFFER.with(|b| b.borrow_mut().clear());

        let error = AiError::AuthenticationFailed;
        log_provider_error_response(
            Uuid::new_v4(),
            401,
            "invalid x-api-key",
            &error,
            "sk-ant-real-secret-key",
        );

        let log_text = TEST_LOG_BUFFER.with(|b| String::from_utf8(b.borrow().clone()).unwrap());
        assert!(log_text.contains("401"));
        assert!(log_text.contains("AI_AUTH_FAILED"));
        assert!(log_text.contains("invalid x-api-key"));
    }

    /// Der eigentliche Testfall aus der Spec: "ein simulierter Provider-401
    /// landet redigiert im Log (Key nicht)" — konstruiert absichtlich einen
    /// Body, der den API-Key wörtlich enthält (der worst case, den die
    /// Redaction abfangen soll, auch wenn ein echter Provider das nicht
    /// tut), und prüft, dass der Key selbst nirgends im Log-Output landet.
    #[test]
    fn test_provider_error_response_never_logs_the_api_key_verbatim() {
        install_test_subscriber_once();
        TEST_LOG_BUFFER.with(|b| b.borrow_mut().clear());

        let api_key = "sk-ant-real-secret-key";
        let error = AiError::AuthenticationFailed;
        log_provider_error_response(
            Uuid::new_v4(),
            401,
            &format!("invalid credentials for key {api_key}"),
            &error,
            api_key,
        );

        let log_text = TEST_LOG_BUFFER.with(|b| String::from_utf8(b.borrow().clone()).unwrap());
        assert!(
            !log_text.contains(api_key),
            "der API-Key darf unter keinen Umständen im Log-Output auftauchen: {log_text}"
        );
        assert!(
            log_text.contains("REDACTED"),
            "der Redaction-Platzhalter muss stattdessen im Log stehen: {log_text}"
        );
    }

    #[test]
    fn test_provider_transport_error_logs_code_and_message() {
        install_test_subscriber_once();
        TEST_LOG_BUFFER.with(|b| b.borrow_mut().clear());

        let error = AiError::NetworkError("connection refused".to_string());
        log_provider_transport_error(Uuid::new_v4(), &error);

        let log_text = TEST_LOG_BUFFER.with(|b| String::from_utf8(b.borrow().clone()).unwrap());
        assert!(log_text.contains("AI_NETWORK_ERROR"));
        assert!(log_text.contains("connection refused"));
    }
}
