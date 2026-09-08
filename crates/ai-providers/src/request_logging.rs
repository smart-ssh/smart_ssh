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
/// gar nicht auftauchen. Trotzdem wird defensiv geprüft, ob einer der
/// `secrets` (API-Key, s. Aufrufstellen — bei `OpenAiCompatibleProvider`
/// zusätzlich jeder `extra_headers`-Wert, s. Spec 0025 Abschnitt 3: dort
/// landen in der Praxis zusätzliche Auth-Token, die ein Gateway/Proxy in
/// einer Fehlermeldung spiegeln könnte) wörtlich in `body` vorkommt und in
/// diesem Fall ersetzt — dieselbe "Logs sind kein Schlupfloch für
/// Secrets"-Regel wie bei `app_shell::orchestration::log_command_execution`.
///
/// Spec-Reviewer-Fund (Spec 0049, Review dieses Schritts): ursprünglich
/// nahm diese Funktion nur den API-Key entgegen — `extra_headers`-Werte
/// blieben ungeprüft.
pub(crate) fn log_provider_error_response(
    request_id: Uuid,
    status: u16,
    body: &str,
    error: &AiError,
    secrets: &[&str],
) {
    tracing::warn!(
        request_id = %request_id,
        status,
        code = error.code(),
        body = %redact_secrets(body, secrets),
        "AI provider returned an error response",
    );
}

/// Gegenstück zu [`log_provider_error_response`] für einen Transport-
/// Fehler (Verbindungsaufbau, Timeout, TLS, ...) — hier existiert kein
/// HTTP-Status/Response-Body, nur die über `Display` lesbare Fehlermeldung.
///
/// Spec-Reviewer-Fund (Spec 0049, Review dieses Schritts): der ursprüngliche
/// Doc-Kommentar behauptete, diese Meldung könne "nie den API-Key
/// enthalten" — das galt zwar für den API-Key selbst (der nie Teil einer
/// URL ist), aber `reqwest::Error`s `Display` hängt bei einem
/// Verbindungsfehler die Ziel-URL an, und `base_url` ist ein vom Nutzer
/// editierbares Feld (Spec 0049, Fund 1 trimmt es explizit) — ein Nutzer
/// könnte dort z. B. `https://user:token@proxy.intern/v1` eintragen, dessen
/// Userinfo dann unredigiert im Log gelandet wäre. Dieselben `secrets` wie
/// bei [`log_provider_error_response`] werden deshalb auch hier angewendet.
pub(crate) fn log_provider_transport_error(request_id: Uuid, error: &AiError, secrets: &[&str]) {
    let message = redact_secrets(&error.to_string(), secrets);
    tracing::warn!(
        request_id = %request_id,
        code = error.code(),
        error = %message,
        "AI provider transport/connection error",
    );
}

/// Ersetzt jedes wörtliche Vorkommen jedes nicht-leeren Eintrags aus
/// `secrets` in `text` durch `[REDACTED]` (ein leerer Eintrag würde sonst
/// via `str::replace` jedes Zeichen "ersetzen").
fn redact_secrets(text: &str, secrets: &[&str]) -> String {
    let mut redacted = text.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            redacted = redacted.replace(secret, "[REDACTED]");
        }
    }
    redacted
}

#[cfg(test)]
mod error_logging_tests {
    use super::*;
    use crate::test_support::{clear_log_buffer, install_test_subscriber_once, log_buffer_text};

    #[test]
    fn test_provider_error_response_logs_status_and_code_redacted() {
        install_test_subscriber_once();
        clear_log_buffer();

        let error = AiError::AuthenticationFailed;
        log_provider_error_response(
            Uuid::new_v4(),
            401,
            "invalid x-api-key",
            &error,
            &["sk-ant-real-secret-key"],
        );

        let log_text = log_buffer_text();
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
        clear_log_buffer();

        let api_key = "sk-ant-real-secret-key";
        let error = AiError::AuthenticationFailed;
        log_provider_error_response(
            Uuid::new_v4(),
            401,
            &format!("invalid credentials for key {api_key}"),
            &error,
            &[api_key],
        );

        let log_text = log_buffer_text();
        assert!(
            !log_text.contains(api_key),
            "der API-Key darf unter keinen Umständen im Log-Output auftauchen: {log_text}"
        );
        assert!(
            log_text.contains("REDACTED"),
            "der Redaction-Platzhalter muss stattdessen im Log stehen: {log_text}"
        );
    }

    /// Spec-Reviewer-Fund (Spec 0049, Review dieses Schritts): `extra_headers`
    /// (Spec 0025, Abschnitt 3) können ein zweites Auth-Token tragen, nicht
    /// nur den API-Key — muss ebenfalls redigiert werden, wenn es als
    /// zusätzliches Secret übergeben wird.
    #[test]
    fn test_provider_error_response_redacts_every_given_secret_not_just_the_first() {
        install_test_subscriber_once();
        clear_log_buffer();

        let api_key = "sk-ant-real-secret-key";
        let extra_header_token = "gateway-token-xyz";
        let error = AiError::AuthenticationFailed;
        log_provider_error_response(
            Uuid::new_v4(),
            401,
            &format!("rejected: key={api_key} token={extra_header_token}"),
            &error,
            &[api_key, extra_header_token],
        );

        let log_text = log_buffer_text();
        assert!(
            !log_text.contains(api_key),
            "API-Key darf nicht im Log stehen: {log_text}"
        );
        assert!(
            !log_text.contains(extra_header_token),
            "extra_headers-Wert darf nicht im Log stehen: {log_text}"
        );
    }

    #[test]
    fn test_provider_transport_error_logs_code_and_message() {
        install_test_subscriber_once();
        clear_log_buffer();

        let error = AiError::NetworkError("connection refused".to_string());
        log_provider_transport_error(Uuid::new_v4(), &error, &[]);

        let log_text = log_buffer_text();
        assert!(log_text.contains("AI_NETWORK_ERROR"));
        assert!(log_text.contains("connection refused"));
    }

    /// Spec-Reviewer-Fund (Spec 0049, Review dieses Schritts): `reqwest`s
    /// `Display` für einen Transport-Fehler hängt die Ziel-URL an — steht
    /// dort (weil der Nutzer sie z. B. mit eingebetteter Proxy-Auth als
    /// `base_url` eingetragen hat) ein Secret drin, muss auch das hier
    /// redigiert werden, nicht nur beim HTTP-Fehler-Body.
    #[test]
    fn test_provider_transport_error_redacts_secret_embedded_in_the_message() {
        install_test_subscriber_once();
        clear_log_buffer();

        let embedded_secret = "user:proxy-token-123";
        let error = AiError::NetworkError(format!(
            "error sending request for url (https://{embedded_secret}@proxy.intern/v1)"
        ));
        log_provider_transport_error(Uuid::new_v4(), &error, &[embedded_secret]);

        let log_text = log_buffer_text();
        assert!(
            !log_text.contains(embedded_secret),
            "in der URL eingebettetes Secret darf nicht im Log stehen: {log_text}"
        );
    }
}
