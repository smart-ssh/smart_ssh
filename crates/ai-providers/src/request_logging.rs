//! Gemeinsame Logging-Helfer für Spec 0016 ("Strukturiertes Logging &
//! Diagnose"), Abschnitt 4, Punkte 1–3 — geteilt zwischen
//! `AnthropicProvider` und `OpenAiCompatibleProvider`, damit das Format
//! nicht zwischen beiden auseinanderläuft (analog zu `crate::error`).
//!
//! `request_id` korreliert alle Log-Zeilen *eines* `AiProvider::send()`-
//! Aufrufs (s. `crate::anthropic::AnthropicProvider::send`-Kommentar zur
//! Design-Entscheidung, warum diese ID lokal pro Aufruf erzeugt wird statt
//! ein `SessionContext`-Feld zu sein).

use ssh_manager_core::ai::{MessageContent, RejectionReason, SessionContext};
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
