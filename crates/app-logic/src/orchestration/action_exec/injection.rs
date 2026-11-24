//! Split out of `action_exec.rs` (issue #859, pure move).

use uuid::Uuid;

use ssh_manager_core::ai::{
    truncate_for_second_opinion, OutputRedactor, DEFAULT_SECOND_OPINION_MAX_LEN,
};

use crate::events::{
    emit_chat_error, emit_chat_injection_check, EventEmitter, InjectionCheckState,
};
use crate::session::Session;
use crate::state::SessionId;

use super::super::chat_turn::{wait_for_ai_request_slot, wait_for_rate_limit_budget};

/// Issue #693: separator between the confirmation text and the checker's
/// reason; the frontend splits on the last occurrence. The reason itself
/// never contains a newline (see [`sanitize_injection_reason`]).
pub(crate) const INJECTION_REASON_MARKER: &str = "\nReason: ";

/// Issue #693: the checker's reason is derived from untrusted content, so
/// before it is stored or shown it is redacted, stripped of control
/// characters (newlines become spaces) and cut to
/// `INJECTION_REASON_MAX_CHARS`. Redaction runs first so a cut can never
/// split a secret into an unrecognisable fragment.
pub(crate) fn sanitize_injection_reason(session: &Session, raw: &str) -> String {
    let redacted = session.redactor.redact_text(raw);
    let cleaned: String = redacted
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    cleaned
        .trim()
        .chars()
        .take(crate::events::INJECTION_REASON_MAX_CHARS)
        .collect()
}

/// Issue #693: current suspicion reason, without consuming it.
pub(super) fn peek_injection_reason(session: &Session) -> Option<String> {
    session
        .safety
        .injection_suspected_reason
        .lock()
        .ok()
        .and_then(|g| g.clone())
}

/// Issue #693: takes the suspicion reason out together with the flag.
pub(super) fn take_injection_reason(session: &Session) -> Option<String> {
    session
        .safety
        .injection_suspected_reason
        .lock()
        .ok()
        .and_then(|mut g| g.take())
}

/// Spec 0039, Abschnitt 5.2: läuft die optionale KI-Prüfung auf
/// eingeschleuste Anweisungen für frisch gelesenen Inhalt (Kommando-
/// Ausgabe, SFTP-Dateiinhalt) — No-op, falls für diese Sitzung kein
/// passender Zweitmeinungs-Provider konfiguriert ist (`Session::
/// injection_check_provider`, s. dortiger Kommentar zu den zwei
/// Voraussetzungen). Inline awaited, nicht `tokio::spawn`, aus demselben
/// Grund wie die Risiko-Zweitmeinung (Spec 0026): ein `Arc<Session>` bis
/// hierher durchzureichen wäre ein unverhältnismäßiger Umbau für einen
/// einzelnen zusätzlichen `.await` (s. `handle_action_proposed`s
/// Kommentar an der entsprechenden Stelle) — "läuft asynchron, blockiert
/// nicht den regulären Ablauf" (Spec-Text) ist hier im selben Sinn wie
/// dort zu lesen: verzögert nicht die bereits gesendeten Events dieser
/// Aktion, nicht "detached vom gesamten Ablauf".
///
/// Setzt bei "ja" `session.injection_suspected` — `handle_action_proposed`
/// konsumiert das beim nächsten vorgeschlagenen Aktionsvorschlag (s.
/// dortiger Kommentar). Ein `AiError`/nicht parsebares Ergebnis (`None`)
/// ändert absichtlich nichts: keine Eskalation, aber auch kein Zurücksetzen
/// eines zuvor schon erkannten Verdachts — "keine Prüfung verfügbar" ist
/// kein "alles in Ordnung".
#[expect(clippy::cognitive_complexity, reason = "split planned")]
pub(crate) async fn check_for_injected_instructions(
    session: &Session,
    session_id: SessionId,
    emitter: &dyn EventEmitter,
    item_id: Uuid,
    content: &str,
    source: &'static str,
) {
    let Some(provider) = session.safety.injection_check_provider.as_deref() else {
        return;
    };
    // Issue #693: progress under the checked result (display only).
    emit_chat_injection_check(emitter, session_id, item_id, InjectionCheckState::Running);
    wait_for_ai_request_slot(session).await;
    if let Some(budget) = session.safety.injection_check_budget.as_deref() {
        // Estimate on the content as it will ACTUALLY be sent, not the raw
        // `content` — `fetch_injection_check` truncates via
        // `truncate_for_second_opinion` before it ever reaches the
        // provider (`build_second_opinion_context`). Estimating on the
        // untruncated content for a large file read massively overshoots
        // the real request size and made the gate wait near-constantly
        // (spec-reviewer finding, Spec 0061 follow-up fix).
        let (truncated_content, _) =
            truncate_for_second_opinion(content, DEFAULT_SECOND_OPINION_MAX_LEN);
        wait_for_rate_limit_budget(
            budget,
            crate::compaction::estimate_tokens(&truncated_content),
            emitter,
            session_id,
        )
        .await;
    }
    tracing::info!(
        session_id = %session_id,
        source,
        content_len = content.len(),
        "injection check started",
    );
    let started = std::time::Instant::now();
    let mut request_usage = None;
    let outcome = crate::second_opinion::fetch_injection_check_with_usage(
        provider,
        content,
        session.redactor.as_ref(),
        &mut request_usage,
    )
    .await;
    if let Some(usage) = request_usage {
        session
            .report_ai_usage(crate::ai_usage::AiUsagePurpose::InjectionCheck, usage)
            .await;
    }
    let verdict = match &outcome {
        crate::second_opinion::SideCallOutcome::Verdict(true, _) => "suspected",
        crate::second_opinion::SideCallOutcome::Verdict(false, _) => "clean",
        crate::second_opinion::SideCallOutcome::NoVerdict => "no_verdict",
        crate::second_opinion::SideCallOutcome::ProviderError { .. } => "provider_error",
    };
    tracing::info!(
        session_id = %session_id,
        source,
        content_len = content.len(),
        verdict,
        duration_ms = started.elapsed().as_millis() as u64,
        "injection check finished",
    );
    if let crate::second_opinion::SideCallOutcome::Verdict(_, reason) = &outcome {
        // Spec 0094 A2: reason text only at debug, through the log redactor.
        tracing::debug!(
            session_id = %session_id,
            reason = %ssh_manager_core::ai::default_log_redactor().redact_text(reason),
            "injection check reason",
        );
    }
    match outcome {
        crate::second_opinion::SideCallOutcome::Verdict(true, reason) => {
            // Issue #693: carry the reason along with the flag (display
            // only; the decision logic is unchanged). Reason first, flag
            // second, so a consumer that sees the flag also sees the reason.
            let reason = sanitize_injection_reason(session, &reason);
            if let Ok(mut slot) = session.safety.injection_suspected_reason.lock() {
                *slot = Some(reason.clone());
            }
            session
                .safety
                .injection_suspected
                .store(true, std::sync::atomic::Ordering::SeqCst);
            emit_chat_injection_check(
                emitter,
                session_id,
                item_id,
                InjectionCheckState::Suspected { reason },
            );
        }
        // Geprüft, unauffällig: ändert nichts (löscht insbesondere einen
        // früheren Verdacht nicht).
        crate::second_opinion::SideCallOutcome::Verdict(false, _) => {
            emit_chat_injection_check(emitter, session_id, item_id, InjectionCheckState::Clean);
        }
        // Issue #102 (Maintainer-Entscheidung: fail closed, pro Aktion): Der
        // Check konnte nicht laufen (Provider-Fehler) oder lieferte kein
        // erkennbares Urteil. `injection_suspected` bleibt unberührt, aber
        // die nächste vorgeschlagene Aktion braucht eine Bestätigung, und
        // der Nutzer sieht den Hinweis. Fester Text: weder Inhalt noch
        // Provider-Fehlertext.
        crate::second_opinion::SideCallOutcome::NoVerdict
        | crate::second_opinion::SideCallOutcome::ProviderError { .. } => {
            session
                .safety
                .injection_check_unavailable
                .store(true, std::sync::atomic::Ordering::SeqCst);
            emit_chat_error(
                emitter,
                session_id,
                "The check for injected instructions could not be run; the next proposed \
                 action requires confirmation."
                    .to_string(),
                Some("AI_INJECTION_CHECK_UNAVAILABLE"),
            );
            emit_chat_injection_check(
                emitter,
                session_id,
                item_id,
                InjectionCheckState::Unavailable,
            );
        }
    }
}

/// Issue #102: einmaliger Sitzungs-Hinweis, wenn die Zweitmeinung in den
/// Einstellungen aktiviert war, beim Verbinden aber nicht eingerichtet
/// werden konnte (Provider gelöscht, ungültige ID, Zugangsdaten nicht
/// auflösbar). Die Sitzung läuft wie bisher ohne Zweitmeinung und
/// Injection-Check, aber nicht mehr stillschweigend. Wird beim Start eines
/// Chat-Turns und bei jedem Aktionsvorschlag aufgerufen (auch MCP) und
/// sendet nur beim ersten Mal.
pub(crate) fn announce_second_opinion_setup_failure(
    session: &Session,
    session_id: SessionId,
    emitter: &dyn EventEmitter,
) {
    if session
        .safety
        .second_opinion_setup_notice_pending
        .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
        emit_chat_error(
            emitter,
            session_id,
            "The AI second opinion is enabled but could not be set up for this session; \
             commands are not being checked by it."
                .to_string(),
            Some("AI_SECOND_OPINION_SETUP_FAILED"),
        );
    }
    // Issue #231: Injection-Prüfung am Server an, aber kein Anbieter.
    if session
        .safety
        .injection_check_inactive_notice_pending
        .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
        emit_chat_error(
            emitter,
            session_id,
            "The check for injected instructions is enabled for this server but is not \
             active in this session (AI second opinion is off or could not be set up)."
                .to_string(),
            Some("AI_INJECTION_CHECK_INACTIVE"),
        );
    }
    // Issue #843: secret values for this server may not be masked.
    if session
        .safety
        .redaction_incomplete_notice_pending
        .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
        emit_chat_error(
            emitter,
            session_id,
            "Some secret values for this server could not be loaded; they may not be masked \
             in what is sent to the AI."
                .to_string(),
            Some("REDACTION_INCOMPLETE"),
        );
    }
}
