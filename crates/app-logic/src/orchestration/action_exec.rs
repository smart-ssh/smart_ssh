//! Vorschlag → Bewertung → Ausführung einer `AiAction` (Spec 0007,
//! Abschnitt 6 ff.) — s. Moduldoc in `orchestration.rs` für den
//! vollständigen Kontext (Spec 0083: reine Verschiebung aus
//! `orchestration.rs`, keine Verhaltensänderung).

use uuid::Uuid;

use ssh_manager_core::ai::{
    truncate_for_second_opinion, ChatMessage, MessageContent, OutputRedactor, RejectionReason,
    Role, DEFAULT_SECOND_OPINION_MAX_LEN,
};
use ssh_manager_core::audit::{LedgerDecisionOutcome, LedgerEntryContent, LedgerSource};
use ssh_manager_core::filter::{
    exceeds_command_length_limit, Decision, EvalContext, EvaluationTrace, RuleId, RuleOrigin,
    DEFAULT_MAX_COMMAND_LENGTH,
};
use ssh_manager_core::profiles::{AiAction, PostIngestPolicy, ProfileStore};
use ssh_manager_core::risk::{RiskAssessment, RiskClassifier, RiskLevel, RuleBasedRiskClassifier};
#[cfg(test)]
use ssh_manager_core::ssh::SshError;
use ssh_manager_core::ssh::{CommandOutput, ExecOutcome};

mod rejected_action;
use rejected_action::describe_rejected_action;
mod command_log;
use command_log::{log_command_execution, log_command_execution_failed};
#[cfg(test)]
use ssh_manager_core::profiles::NoteTargetSelector;

use crate::confirmation::ConfirmationRegistry;
use crate::confirmation_settled::emit_settled_for_rejection;
use crate::dto::{ActionOrigin, ActionUserDecision};
use crate::events::{
    emit_action_decision_escalated, emit_chat_action_proposed, emit_chat_action_result,
    emit_chat_error, emit_chat_injection_check, emit_risk_assessment_updated, ActionResultPayload,
    EventEmitter, InjectionCheckState,
};
use crate::session::Session;
use crate::state::{ActionId, SessionId};

use super::approval::{ApprovedAction, ApprovedCommand};
use super::chat_turn::{
    ledger_source_for_origin, push_history_scoped, wait_for_ai_request_slot,
    wait_for_rate_limit_budget, write_ledger_entry, PENDING_ACTION_CONFIRM_TIMEOUT,
};
use super::notes::{execute_note_update, note_target_preview_for_action};
use super::pending_confirmation::{PendingConfirmation, ANOTHER_CONFIRMATION_PENDING_MESSAGE};
use super::remote_files::{
    execute_read_remote_file, execute_write_remote_file, previous_file_content_for_action,
};

// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_core;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_check_unavailable;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_files_and_ledger;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_outgoing;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_pending_confirmation;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_red_risk;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_red_risk_second_opinion;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_not_assessable;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_escalation_log;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_escalation;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_live_output;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_auto_exec_rules;
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_server_tags;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_path_normalisation;

/// Spec 0088, A1.4: Die Entscheidung der Filter-Engine, angereichert um das
/// bereits registrierte Warten.
///
/// Spiegelt [`Decision`] Variante für Variante, trägt im `Confirm`-Zweig aber
/// zusätzlich den [`PendingConfirmation`]-Guard. Vorher lag der Empfänger
/// daneben in einem `Option<oneshot::Receiver<…>>`, das der `Confirm`-Zweig
/// per `expect("confirm_rx muss registriert sein")` auspackte — eine
/// Invariante, die nur im Text stand. Jetzt trägt sie der Typ: Ein
/// `Confirm`-Zweig ohne Empfänger lässt sich nicht konstruieren.
enum PreparedDecision<'a> {
    AutoExec,
    Deny {
        reason: String,
        code: String,
    },
    Confirm {
        reason: String,
        code: String,
        pending: PendingConfirmation<'a>,
    },
}

/// Gibt zurück, ob die Aktion tatsächlich ausgeführt wurde.
///
/// `earlier_rejection` (Spec 0068, Teil 4, Review-Fund): gemeinsam für alle
/// Aktionen EINER KI-Antwort. Wird hier gesetzt, sobald eine Aktion
/// blockiert (`Deny`) oder vom Nutzer abgelehnt wird; ist es gesetzt, wird
/// jede weitere Aktion derselben Antwort von `AutoExec` auf `Confirm`
/// eskaliert — sie kann inhaltlich auf der abgelehnten aufbauen.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_action_proposed(
    session: &Session,
    session_id: SessionId,
    mut action: AiAction,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
    origin: ActionOrigin,
    earlier_rejection: &std::sync::atomic::AtomicBool,
) -> bool {
    let action_id: ActionId = Uuid::new_v4();
    announce_second_opinion_setup_failure(session, session_id, emitter);

    // Spec 0057, §1.1, erster Punkt: "Kommando vorgeschlagen". Bewusst auf
    // `SuggestCommand` beschränkt (das Ledger deckt nur diesen
    // Aktionstyp ab, s. Spec 0057 §9, Grenzen) — `ReadRemoteFile`/
    // `WriteRemoteFile`/`ProposeNoteUpdate` folgen in einer späteren
    // Erweiterung.
    if let AiAction::SuggestCommand { command } = &action {
        write_ledger_entry(
            session,
            ledger_source_for_origin(&origin),
            LedgerEntryContent::CommandProposed {
                command: command.clone(),
            },
        )
        .await;
    }

    // Spec 0040, Abschnitt 4: MCP-Herkunft schreibt nie in die persistierte,
    // wiederaufnehmbare Historie — auch nicht, wenn dieselbe `Session`
    // (samt `chat_session_id`) gerade einen Menschen-Tab bedient (s.
    // `mcp_backend::AppMcpBackend::ensure_session`). Einmal hier aus
    // `origin` abgeleitet und durch die gesamte Ausführungskette gereicht,
    // statt an jeder `push_history`-Stelle einzeln zu entscheiden.
    let persist = !matches!(origin, ActionOrigin::Mcp { .. });

    // Unabhängiger Review-Pass (Spec 0020): Pfad-Normalisierung passiert
    // hier, EINMALIG (via `ssh_manager_core::filter::normalize_lexical_path`,
    // dieselbe Funktion wie in der Glob-Auswertung), bevor irgendein Konsument unten (Filter-Auswertung,
    // Risikoeinschätzung, Vorschau, `execute_action`) den Pfad sieht — alle
    // greifen auf dasselbe `action` zu, das ab hier bereits normalisiert
    // ist.
    if let AiAction::ReadRemoteFile { path } | AiAction::WriteRemoteFile { path, .. } = &mut action
    {
        *path = ssh_manager_core::filter::normalize_lexical_path(path);
    }

    let (evaluation, server_tags_unavailable) =
        evaluate_action(session, &action, profile_store).await;
    let matched_rule = evaluation.matched_rule.clone();
    let matched_rule_origin = evaluation.matched_rule_origin;
    let engine_decision = evaluation.decision.clone();
    let decision = evaluation.decision.clone();

    // Vorgezogen (war vorher erst nach der Eskalationskette berechnet, s.
    // Git-Historie) — die neue Spec-0039-Eskalation unten braucht die
    // Server-Risiko-Achse bereits hier für die `Balanced`-Stufe.
    // `risk_assessment_for_action` ist eine reine, zustandslose Funktion
    // von `action` allein, der Zeitpunkt der Berechnung ändert also nichts
    // an ihrem Ergebnis.
    let risk_assessment = risk_assessment_for_action(&action);

    // The post-engine escalation chain (secret path, `sftp-server`, red risk,
    // post-ingest, injection suspicion/unavailable, MCP origin, sudo password,
    // inserted secret, earlier rejection) lives once in `escalation::escalate`
    // (issue #826): a pure function that only combines via
    // `Decision::stricter`. Here: read the facts, escalate, then consume
    // exactly the one-shot flags the result names.
    let uses_password = uses_stored_sudo_password(session, &action);
    let transform_scope = if session.outgoing_transform.is_some() {
        Some(super::outgoing::outgoing_scope(session, profile_store).await)
    } else {
        None
    };
    let mut facts = escalation_facts(
        session,
        &action,
        risk_assessment.as_ref(),
        transform_scope.as_ref(),
    );
    facts.mcp_origin = matches!(origin, ActionOrigin::Mcp { .. });
    facts.server_tags_unavailable = server_tags_unavailable;
    facts.earlier_rejection = earlier_rejection.load(std::sync::atomic::Ordering::SeqCst);
    let escalation = super::escalation::escalate(decision, &facts);
    if escalation.consume_injection_suspected
        && session
            .injection_suspected
            .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
        let _ = take_injection_reason(session);
    }
    if escalation.consume_injection_check_unavailable {
        session
            .injection_check_unavailable
            .swap(false, std::sync::atomic::Ordering::SeqCst);
    }
    let decision = escalation.decision;

    // Issue #688: one default-level line when the chain above turned the
    // engine's `AutoExec` into something stricter. Logging only; the
    // decision is not touched. Compared once after the chain so a new step
    // cannot forget it; the code is that of the step that set the decision.
    log_filter_escalation(session_id, &action, &engine_decision, &decision);

    // Spec 0088, A1.2/A1.4: Die Registrierung liegt weiterhin VOR den
    // `await`s für Vorschau und Zweitmeinung (s. Kommentar weiter unten:
    // ein Klick währenddessen darf nicht verloren gehen) — aber der
    // Empfänger hängt jetzt am `Confirm`-Zweig des Ergebnistyps statt in
    // einer getrennten `Option`, die der Zweig unten per `expect` hätte
    // auspacken müssen. Dass im `Confirm`-Zweig ein Empfänger vorliegt,
    // garantiert damit der Typ und nicht eine Laufzeitprüfung.
    let prepared = match decision.clone() {
        Decision::AutoExec => PreparedDecision::AutoExec,
        Decision::Deny { reason, code } => PreparedDecision::Deny { reason, code },
        Decision::Confirm { reason, code } => {
            let Ok(pending) = PendingConfirmation::register(
                session,
                action_confirmations,
                action_id,
                matches!(origin, ActionOrigin::Mcp { .. }),
            ) else {
                // Issue #108 / Spec 0104 §3: höchstens eine wartende
                // Bestätigung je MCP-Sitzung; der Vorschlag wird nicht
                // ausgeführt und dem Client als Fehler gemeldet.
                return emit_action_error(
                    session,
                    emitter,
                    session_id,
                    ANOTHER_CONFIRMATION_PENDING_MESSAGE.to_string(),
                    None,
                    persist,
                )
                .await;
            };
            PreparedDecision::Confirm {
                reason,
                code,
                pending,
            }
        }
    };

    // Issue #696: name the rules behind an automatic run (display only; the
    // trace is the one that made the decision, no second evaluation). Only for
    // the final `AutoExec`: an escalation above already turned it into Confirm.
    let auto_exec_rules = if matches!(decision, Decision::AutoExec) {
        auto_exec_rules_for(session, profile_store, &action, &evaluation).await
    } else {
        Vec::new()
    };

    let (previous_note_content, target_name) =
        note_target_preview_for_action(&action, session, profile_store).await;
    let (previous_file_content, previous_file_size) =
        previous_file_content_for_action(&action, session).await;

    let proposed_payload = emit_chat_action_proposed(
        emitter,
        session_id,
        action_id,
        action.clone(),
        decision,
        previous_note_content,
        uses_password,
        previous_file_content,
        previous_file_size,
        target_name,
        risk_assessment.clone(),
        origin.clone(),
        auto_exec_rules,
    );
    session.remember_proposal(action_id, proposed_payload.clone());

    // Spec 0026, Abschnitt 3: läuft NACH der bereits gesendeten
    // regelbasierten Einschätzung (das Event oben ist schon raus, das Badge
    // also bereits sichtbar) — "asynchron" hier bewusst als "verzögert die
    // erste Anzeige nicht" gelesen statt als losgelöster `tokio::spawn`-
    // Task: Letzteres hätte `Arc<Session>`/`Arc<dyn EventEmitter>` bis
    // tief in diese Aufrufkette gebraucht (`run_chat_turn`/`run_one_round`
    // nehmen bewusst `&Session`, s. deren Doc-Kommentare zur
    // Testbarkeit gegen `MockAiProvider`), für einen einzelnen zusätzlichen
    // `.await` unverhältnismäßiger Umbau. Der einzige reale Unterschied:
    // ein bereits registrierter `confirm_rx` (unten) wird trotzdem nicht
    // "verpasst", falls der Nutzer währenddessen schon klickt — der Wert
    // liegt im Kanal bereit, sobald `rx.await` weiter unten drankommt.
    //
    // Spec 0092, A3: Das Ergebnis wird zusätzlich festgehalten — hebt die
    // Zweitmeinung das Daten-Risiko auf Rot, darf die Aktion nicht mehr
    // automatisch laufen (s. direkt darunter).
    let mut second_opinion_data_risk: Option<RiskLevel> = None;
    let mut second_opinion_unavailable = false;
    if let (Some(provider), Some(assessment)) = (
        session.risk_second_opinion_provider.as_deref(),
        risk_assessment,
    ) {
        if let Some(pseudo_command) = pseudo_command_for_risk_classification(&action) {
            wait_for_ai_request_slot(session).await;
            if let Some(budget) = session.risk_second_opinion_budget.as_deref() {
                wait_for_rate_limit_budget(
                    budget,
                    crate::compaction::estimate_tokens(&pseudo_command),
                    emitter,
                    session_id,
                )
                .await;
            }
            let mut request_usage = None;
            let second_opinion = crate::second_opinion::fetch_second_opinion_with_usage(
                provider,
                &pseudo_command,
                &mut request_usage,
            )
            .await;
            if let Some(usage) = request_usage {
                session
                    .report_ai_usage(crate::ai_usage::AiUsagePurpose::RiskSecondOpinion, usage)
                    .await;
            }
            // Issue #102: "konnte nicht geprüft werden" (Provider-Fehler
            // ODER kein erkennbares Urteil) ist vom Ergebnis "geprüft,
            // unauffällig" unterscheidbar. Die Stufe bleibt die
            // regelbasierte (`escalate_data_risk` kennt nur Eskalation),
            // aber die Karte bekommt den Hinweis, und die Aktion wird
            // unten bestätigungspflichtig.
            second_opinion_unavailable = second_opinion.is_unavailable();
            let (data_risk, reason) = escalate_data_risk(
                assessment.data_risk,
                assessment.data_risk_reason,
                match second_opinion {
                    crate::second_opinion::SideCallOutcome::Verdict(level, text) => {
                        Some((level, text))
                    }
                    _ => None,
                },
            );
            session.review_proposal(action_id, data_risk, &reason);
            emit_risk_assessment_updated(
                emitter,
                session_id,
                action_id,
                data_risk,
                reason,
                second_opinion_unavailable,
            );
            // Spec 0092, A3: nur die STUFE festhalten, nicht den Text der
            // Zweitmeinung — s. Kommentar unten am Grundtext.
            second_opinion_data_risk = Some(data_risk);
        }
    }

    // Spec 0092, A3: Die Zweitmeinung hat das Daten-Risiko auf Rot gehoben, nachdem
    // `chat-action-proposed` mit `AutoExec` längst draußen ist. Die Aktion wird ab hier wie jedes
    // andere `Confirm` behandelt — dieselbe Wartezeit, dieselbe Abbruchlogik, derselbe
    // Tab-Indikator, dieselben Ledger-Einträge (A3.1), weil es buchstäblich derselbe
    // `PreparedDecision::Confirm`-Zweig unten ist.
    //
    // Dass `prepared` hier noch `AutoExec` ist, heißt zugleich: Das regelbasierte Rot hat NICHT
    // gegriffen (sonst hätte das Glied oben schon eskaliert) — die Anhebung kommt also tatsächlich
    // von der Zweitmeinung. Nur Eskalation, wie überall: aus `Deny`/`Confirm` wird hier nichts.
    //
    // **Reihenfolge**: `register` VOR dem Ereignis (A3.2) — das Frontend erfährt erst von der
    // wartenden Bestätigung, wenn der Empfänger schon steht, ein sehr schneller Klick geht also
    // nicht ins Leere. Und das Ereignis NACH `risk-assessment-updated` (§5), damit das Badge rot
    // ist, bevor der Dialog erscheint.
    //
    // **A3.3, Stopp hat Vorrang**: hier geprüft, nicht erst unten im `match` — ein während der
    // Zweitmeinung eingetroffener Stopp soll die Aktion überspringen lassen, nicht einen Dialog
    // zeigen. Ohne diese Prüfung eskalierte die Aktion und der `AutoExec`-Stopp-Zweig unten käme
    // nie dran.
    //
    // Restlücke, ehrlich benannt (spec-reviewer-Fund, Runde 1): Ein Stopp, der NACH dieser Zeile
    // eintrifft, sieht einen Dialog für eine gerade gestoppte Aktion — A3.3 verlangt „bekommt
    // keinen Dialog". Das ist eine EIGENE, neue Lücke, nicht die bekannte „Kommando läuft trotz
    // Stopp": Sie ist die sichere Richtung (eine Rückfrage zu viel, nie eine Ausführung zu viel)
    // und lässt sich nicht schließen, solange das Ereignis überhaupt gesendet werden muss, bevor
    // der Nutzer klicken kann. Ausgeführt wird in diesem Rennen nichts ohne Klick.
    let mut prepared = prepared;
    let raised_to_red = second_opinion_data_risk == Some(RiskLevel::Red);
    // Issue #102 (Maintainer-Entscheidung: fail closed, pro Aktion): Eine
    // aktivierte, aber ausgefallene Zweitmeinung lässt eine `AutoExec`-Aktion
    // nicht stillschweigend durch — sie wird wie ein `Confirm` behandelt.
    // Unabhängig von `red_risk_always_confirm`, denn das ist keine rote
    // Einstufung, sondern eine fehlende Schutzschicht. Nur Eskalation.
    let escalate_for_red = session.red_risk_always_confirm && raised_to_red;
    if (escalate_for_red || second_opinion_unavailable)
        && matches!(prepared, PreparedDecision::AutoExec)
        && !(matches!(origin, ActionOrigin::Internal)
            && session
                .auto_continue_stop
                .load(std::sync::atomic::Ordering::SeqCst))
    {
        // spec-reviewer-Fund (Runde 1), Spec 0092, §6 („Keine neue
        // Datensenke … die Musterbegründung des Klassifizierers"): Der Text
        // nennt die rote Achse und ihre Herkunft, aber **nicht** die
        // Formulierung des Zweitmeinungs-Modells. Grund: `reason` landet
        // über `handle_user_decision` im persistierten Ledger, und
        // `redact_ledger_entry_content` lässt `LedgerEntryContent::Decision`
        // bewusst unredigiert durch — mit der ausdrücklichen Begründung, dort
        // stünden nur „von der Filter-Engine selbst erzeugte Texte". Ein
        // freier Modelltext kann dagegen zitieren, was er gerade beurteilt
        // (z. B. ein Passwort aus dem Kommando), und ist über
        // Prompt-Injection mittelbar fremdgesteuert. Den Text vorher durch
        // den Redactor zu schicken wäre Scheinsicherheit: der erkennt
        // bekannte Secret-FORMEN (API-Keys, Private Keys, Hashes), nicht ein
        // beliebiges Ad-hoc-Passwort.
        //
        // Die Begründung der Zweitmeinung geht dem Nutzer nicht verloren —
        // sie steht im `risk-assessment-updated`-Ereignis am Badge (Spec
        // 0092, §4) und ist damit sichtbar, ohne persistiert zu werden.
        let (reason, code) = if escalate_for_red {
            (
                "Daten-Risiko rot (KI-Zweitmeinung) – erfordert immer Bestätigung".to_string(),
                "FILTER_RED_RISK_REQUIRES_CONFIRM".to_string(),
            )
        } else {
            // Issue #102: fester Text, kein Provider-Fehlertext.
            (
                "KI-Zweitmeinung nicht verfügbar – erfordert Bestätigung".to_string(),
                "FILTER_SECOND_OPINION_UNAVAILABLE_REQUIRES_CONFIRM".to_string(),
            )
        };
        let Ok(pending) = PendingConfirmation::register(
            session,
            action_confirmations,
            action_id,
            matches!(origin, ActionOrigin::Mcp { .. }),
        ) else {
            return emit_action_error(
                session,
                emitter,
                session_id,
                ANOTHER_CONFIRMATION_PENDING_MESSAGE.to_string(),
                None,
                persist,
            )
            .await;
        };
        emit_action_decision_escalated(
            emitter,
            session_id,
            action_id,
            reason.clone(),
            code.clone(),
        );
        session.escalate_proposal(action_id, proposed_payload, &reason, &code);
        prepared = PreparedDecision::Confirm {
            reason,
            code,
            pending,
        };
    }

    match prepared {
        // spec-reviewer-Fund (Spec 0066): ein Stopp, der eintrifft, nachdem
        // der Vorschlag schon aus dem Stream geholt war (z. B. während der
        // Zweitmeinung oben), verhindert eine noch NICHT gestartete
        // Auto-Ausführung. Nur für den eigenen Chat — der Stopp gilt dem
        // Chat-Turn, nicht MCP-Clients. Ein bereits laufendes Kommando
        // bleibt davon unberührt (Entscheidung 2).
        PreparedDecision::AutoExec
            if matches!(origin, ActionOrigin::Internal)
                && session
                    .auto_continue_stop
                    .load(std::sync::atomic::Ordering::SeqCst) =>
        {
            skip_auto_exec_after_stop(session, session_id, action_id, &action, emitter, persist)
                .await
        }
        PreparedDecision::AutoExec => {
            if let AiAction::SuggestCommand { .. } = &action {
                write_ledger_entry(
                    session,
                    ledger_source_for_origin(&origin),
                    LedgerEntryContent::Decision {
                        outcome: LedgerDecisionOutcome::AutoApproved,
                        reason: None,
                        code: None,
                        matched_rule: matched_rule.clone(),
                        matched_rule_origin,
                    },
                )
                .await;
            }
            execute_action(
                session,
                session_id,
                action_id,
                ApprovedAction::from_auto_exec(action),
                emitter,
                profile_store,
                persist,
                ledger_source_for_origin(&origin),
                uses_password,
            )
            .await
        }
        PreparedDecision::Deny { reason, code } => {
            earlier_rejection.store(true, std::sync::atomic::Ordering::SeqCst);
            if let AiAction::SuggestCommand { .. } = &action {
                write_ledger_entry(
                    session,
                    ledger_source_for_origin(&origin),
                    LedgerEntryContent::Decision {
                        outcome: LedgerDecisionOutcome::Rejected,
                        reason: Some(reason.clone()),
                        code: Some(code),
                        matched_rule: matched_rule.clone(),
                        matched_rule_origin,
                    },
                )
                .await;
            }
            // Spec 0007 Abschnitt 5: informiert nur, keine Ausführung, kein
            // Warten auf `respond_to_action` — das Event oben ist bereits
            // die vollständige Reaktion an den Nutzer. Spec 0021, Abschnitt
            // 3, Fall 4: die KI bekommt zusätzlich einen Kontext-Eintrag mit
            // dem Blockier-Grund und automatisch eine Folgerunde, statt
            // stillschweigend übergangen zu werden.
            push_history_scoped(
                session,
                ChatMessage {
                    role: Role::ActionResult,
                    content: MessageContent::ActionRejected {
                        command: describe_rejected_action(&action),
                        reason: RejectionReason::Blocked(reason),
                    },
                },
                persist,
            )
            .await;
            true
        }
        PreparedDecision::Confirm {
            reason: confirm_reason,
            code: confirm_code,
            pending,
        } => {
            // Spec 0017, Abschnitt 5: Grundlage für den Hintergrund-Tab-
            // Indikator (`SessionSummaryDto.has_pending_action`) — gesetzt,
            // solange auf die Antwort gewartet wird.
            //
            // Spec 0088, A1.1: Das Zurücksetzen hängt nicht mehr an einer
            // zweiten Anweisung hinter dem `await`, sondern am `Drop` von
            // `pending` (s. `pending_confirmation.rs`). Es läuft damit auch
            // auf den Wegen, die dieser Code selbst nicht nimmt: Abbruch
            // der wartenden Task und Panic im Wartepfad. Dasselbe `Drop`
            // räumt den Registry-Eintrag ab (A1.2).
            let (timeout_result, cleanup) = pending
                .wait_for_decision(PENDING_ACTION_CONFIRM_TIMEOUT)
                .await;
            let (user_decision, deny_reason) = match timeout_result {
                Ok(Ok(ActionUserDecision::DenyReconnect)) => {
                    // Issue #1019: discarded by a reconnect — a deny with
                    // its own reason; the card is told below.
                    (ActionUserDecision::Deny, RejectionReason::Reconnect)
                }
                Ok(Ok(decision)) => (decision, RejectionReason::User),
                Ok(Err(_)) => {
                    // Sender wurde gedroppt (z. B. App beendet, bevor der
                    // Nutzer reagiert hat) — kein Absturz, einfach nichts
                    // ausführen.
                    earlier_rejection.store(true, std::sync::atomic::Ordering::SeqCst);
                    return false;
                }
                Err(_elapsed) => {
                    // Spec 0046, Fund 4: `PENDING_ACTION_CONFIRM_TIMEOUT`
                    // abgelaufen, ohne dass `respond_to_action` je kam (z. B.
                    // ein Frontend-Reload, der die wartende Aktion aus seinem
                    // eigenen State verlor, bevor der Tab-Schließen-Handler
                    // sie ablehnen konnte). Registry-Eintrag selbst abräumen
                    // (niemand wartet mehr auf `rx`) und als Ablehnung
                    // behandeln — fail-safe, nie als Genehmigung.
                    // `RejectionReason::Timeout` statt `User`: spec-reviewer-
                    // Fund (Review dieses Schritts) — kein Mensch hat hier
                    // tatsächlich entschieden, die KI (und ein Mensch, der
                    // die Historie später liest) soll das nicht fälschlich
                    // als bewusste Ablehnung lesen.
                    //
                    // Spec 0088: Das frühere `action_confirmations.cancel(
                    // &action_id)` steht hier nicht mehr — es erledigt das
                    // `Drop` von `cleanup` unten, und zwar für jeden
                    // Ausgang statt nur für diesen einen.
                    tracing::warn!(
                        ?action_id,
                        timeout_secs = PENDING_ACTION_CONFIRM_TIMEOUT.as_secs(),
                        "pending action confirmation timed out without a response, treating as denied"
                    );
                    (ActionUserDecision::Deny, RejectionReason::Timeout)
                }
            };
            // Erst hier fällt der Guard: Indikator und Registry-Eintrag
            // sind abgeräumt, BEVOR die Entscheidung ausgeführt wird —
            // dasselbe Verhalten wie zuvor das `= None` direkt hinter dem
            // `await`. Explizit statt am Blockende, damit es nicht von der
            // Länge des folgenden Aufrufs abhängt.
            drop(cleanup);
            emit_settled_for_rejection(emitter, session_id, action_id, &deny_reason);
            if matches!(user_decision, ActionUserDecision::Deny) {
                earlier_rejection.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            handle_user_decision(
                session,
                session_id,
                action_id,
                action,
                user_decision,
                deny_reason,
                emitter,
                profile_store,
                origin,
                matched_rule,
                matched_rule_origin,
                confirm_reason,
                confirm_code,
                uses_password,
                earlier_rejection,
            )
            .await
        }
    }
}

/// Öffentlicher Einstiegspunkt für Spec 0028 (MCP), von
/// `app_shell::mcp_backend` genutzt — dieselbe Orchestrierungs-Funktion wie der
/// interne Chat-Flow oben, nur mit `origin` fest auf `ActionOrigin::Mcp`
/// (erzwingt die Verschärfung aus Abschnitt 5) und `round` fest auf `1`
/// (kein Auto-Continuation-Konzept für MCP-Aufrufe, s. Spec 0028,
/// Abschnitt 3 — es gibt keinen Chatverlauf, der automatisch fortgesetzt
/// werden müsste). Bewusst dieser schmale Wrapper statt
/// `handle_action_proposed` selbst `pub(crate)` zu machen: die
/// internen Parameter `round`/`origin` sollen von außerhalb dieses Moduls
/// nicht frei wählbar sein.
pub async fn handle_mcp_action_proposed(
    session: &Session,
    session_id: SessionId,
    action: AiAction,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
    client_name: Option<String>,
) -> bool {
    // Spec 0096, A2: der dritte der drei KI-Wege — ein externer Agent über
    // MCP. Geschwärzt wird hier, vor `handle_action_proposed`, damit sowohl
    // der Bestätigungsdialog in der App als auch das daraus abgeleitete
    // Ergebnis an den MCP-Client (s. `app_shell::mcp_backend::CaptureEmitter`)
    // dieselbe geschwärzte Fassung sehen wie die gespeicherte Revision.
    let Some(action) = super::notes::redact_note_proposal(action, session.redactor.as_ref()) else {
        return emit_action_error(
            session,
            emitter,
            session_id,
            super::notes::NOTE_PROPOSAL_FULLY_REDACTED_MESSAGE.to_string(),
            None,
            // MCP-Herkunft schreibt nie in die persistierte Historie — s.
            // `handle_action_proposed`s `persist`-Ableitung.
            false,
        )
        .await;
    };
    handle_action_proposed(
        session,
        session_id,
        action,
        emitter,
        profile_store,
        action_confirmations,
        ActionOrigin::Mcp { client_name },
        // Ein MCP-Aufruf ist genau eine Aktion — keine Nachbar-Aktionen.
        &std::sync::atomic::AtomicBool::new(false),
    )
    .await
}

/// Aktuelle Tags des Servers dieser Session, für die Filter-Engine-Auswertung
/// (`EvalContext::tags`) — bevorzugt frisch aus dem `ProfileStore` gelesen
/// (Tags können sich seit Sitzungsbeginn geändert haben), fällt auf die bei
/// `connect()` eingefrorene Kopie zurück, falls der Server inzwischen nicht
/// mehr auflösbar ist.
async fn tags_for_session(session: &Session, profile_store: &dyn ProfileStore) -> Vec<String> {
    server_tags_lookup(session, profile_store).await.0
}

/// [`tags_for_session`] plus whether the profile store failed with anything
/// but "server not stored" (issue #843). `ServerNotFound` is normal (the
/// local pseudo-server has no row) and keeps the frozen tags silently; any
/// other error keeps them too but is reported, so the caller can escalate.
/// Logs the server id and error kind only.
async fn server_tags_lookup(
    session: &Session,
    profile_store: &dyn ProfileStore,
) -> (Vec<String>, bool) {
    match profile_store.get_server(&session.server_id).await {
        Ok(server) => (server.tags, false),
        Err(ssh_manager_core::profiles::ProfileError::ServerNotFound(_)) => {
            (session.tags.clone(), false)
        }
        Err(_) => {
            // Error kind only, never the message (ADR 0086).
            tracing::warn!(
                server_id = %session.server_id.0,
                error_kind = "store_error",
                "server tags could not be read — rules use the tags from connect time"
            );
            (session.tags.clone(), true)
        }
    }
}

/// Issue #696: the rules behind an automatic run, from the trace of the
/// evaluation that made the decision (see `confirmation_explain::auto_exec_rules`).
async fn auto_exec_rules_for(
    session: &Session,
    profile_store: &dyn ProfileStore,
    action: &AiAction,
    evaluation: &EvaluationTrace,
) -> Vec<crate::dto::AutoExecRuleDto> {
    // Only filter-evaluated actions carry rule matches; notes never auto-run.
    if !matches!(
        action,
        AiAction::SuggestCommand { .. } | AiAction::ReadRemoteFile { .. }
    ) {
        return Vec::new();
    }
    let tags = tags_for_session(session, profile_store).await;
    let ctx = EvalContext {
        server_id: session.server_id,
        tags,
    };
    crate::confirmation_explain::auto_exec_rules(session.filter_engine.as_ref(), &ctx, evaluation)
        .await
}

/// Spec 0020, Abschnitt 4.1/4.2: `ReadRemoteFile`/`WriteRemoteFile` werden
/// für die Filter-Engine-Auswertung auf Pseudokommandos abgebildet
/// (`sftp-read <pfad>`/`sftp-write <pfad>`) — dieselbe Präzedenz-Kette wie
/// für Shell-Kommandos, kein zweites paralleles Regelkonzept.
fn sftp_read_pseudo_command(path: &str) -> String {
    format!("sftp-read {path}")
}

fn sftp_write_pseudo_command(path: &str) -> String {
    format!("sftp-write {path}")
}

/// Spec 0026, Abschnitt 2, Punkt 5: synchron beim Erzeugen des
/// `chat-action-proposed`-Events berechnet, für exakt dieselben drei
/// Aktionstypen, die die Filter-Engine kennt (`evaluate_action`) —
/// `ReadRemoteFile`/`WriteRemoteFile` nutzen dieselbe Pseudokommando-
/// Abbildung wie dort (Spec 0020, Abschnitt 4.1), keine zweite
/// Mapping-Logik. `None` für `ProposeNoteUpdate`/`GenerateDocument`, die
/// Spec 0026 nicht abdeckt (kein ausführbares Kommando/kein Dateipfad).
fn pseudo_command_for_risk_classification(action: &AiAction) -> Option<String> {
    match action {
        AiAction::SuggestCommand { command } => Some(command.clone()),
        AiAction::ReadRemoteFile { path } => Some(sftp_read_pseudo_command(path)),
        AiAction::WriteRemoteFile { path, .. } => Some(sftp_write_pseudo_command(path)),
        AiAction::ProposeNoteUpdate { .. }
        | AiAction::GenerateDocument { .. }
        | AiAction::AskUser { .. } => None,
    }
}

/// Issue #688 / Spec 0094: name of a decision's kind, for log lines.
fn decision_kind(decision: &Decision) -> &'static str {
    match decision {
        Decision::AutoExec => "AutoExec",
        Decision::Confirm { .. } => "Confirm",
        Decision::Deny { .. } => "Deny",
    }
}

/// Issue #688: logs `filter decision escalated` when the post-engine chain
/// changed the engine decision. Only kinds, the escalation code, the action
/// kind, the session id and the byte length — no command, path or reason.
fn log_filter_escalation(
    session_id: SessionId,
    action: &AiAction,
    engine_decision: &Decision,
    final_decision: &Decision,
) {
    if matches!(engine_decision, Decision::AutoExec) && matches!(final_decision, Decision::AutoExec)
    {
        return;
    }
    // The chain only ever turns `AutoExec` into `Confirm`.
    if !matches!(engine_decision, Decision::AutoExec) {
        return;
    }
    let code = match final_decision {
        Decision::Confirm { code, .. } | Decision::Deny { code, .. } => code.as_str(),
        Decision::AutoExec => return,
    };
    let action_kind = match action {
        AiAction::SuggestCommand { .. } => "command",
        AiAction::ReadRemoteFile { .. } => "file_read",
        AiAction::WriteRemoteFile { .. } => "file_write",
        AiAction::ProposeNoteUpdate { .. } => "note_update",
        AiAction::GenerateDocument { .. } => "document",
        AiAction::AskUser { .. } => "ask_user",
    };
    tracing::info!(
        session_id = %session_id,
        engine_decision = decision_kind(engine_decision),
        final_decision = decision_kind(final_decision),
        code,
        action_kind,
        command_len = pseudo_command_for_risk_classification(action).map_or(0, |c| c.len()),
        "filter decision escalated",
    );
}

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
fn peek_injection_reason(session: &Session) -> Option<String> {
    session
        .injection_suspected_reason
        .lock()
        .ok()
        .and_then(|g| g.clone())
}

/// Issue #693: takes the suspicion reason out together with the flag.
fn take_injection_reason(session: &Session) -> Option<String> {
    session
        .injection_suspected_reason
        .lock()
        .ok()
        .and_then(|mut g| g.take())
}

/// Test helper: the decision the secret-path / `sftp-server` step produces for
/// `finding`, via the shared `escalate` (the former stand-alone function).
#[cfg(test)]
fn check_finding_confirm(
    finding: ssh_manager_core::risk::CommandCheckFinding,
    match_code: &str,
    injection_suspected: bool,
    injection_reason: Option<&str>,
) -> Decision {
    let mut facts = super::escalation::EscalationFacts {
        injection_suspected,
        injection_reason: injection_reason.map(str::to_string),
        ..Default::default()
    };
    if match_code == "FILTER_SFTP_SERVER_REQUIRES_CONFIRM" {
        facts.sftp_server = Some(finding);
    } else {
        facts.secret_path = Some(finding);
    }
    super::escalation::escalate(Decision::AutoExec, &facts).decision
}

/// Reads the session state the escalation chain depends on (read-only: the
/// one-shot flags are loaded, never consumed). `mcp_origin` and
/// `earlier_rejection` are transient and left `false`; the caller sets them.
fn escalation_facts(
    session: &Session,
    action: &AiAction,
    risk_assessment: Option<&RiskAssessment>,
    transform_scope: Option<&ssh_manager_core::outgoing::ServerScope>,
) -> super::escalation::EscalationFacts {
    use std::sync::atomic::Ordering::SeqCst;
    let pseudo = pseudo_command_for_risk_classification(action);
    let post_ingest_escalates = session.untrusted_content_ingested.load(SeqCst) && {
        let is_modifying_action = risk_assessment.is_some_and(|r| r.server_risk != RiskLevel::None);
        match session.post_ingest_policy {
            PostIngestPolicy::Strict => true,
            PostIngestPolicy::Balanced => is_modifying_action,
            PostIngestPolicy::Standard => false,
        }
    };
    super::escalation::EscalationFacts {
        secret_path: pseudo
            .as_deref()
            .and_then(ssh_manager_core::risk::secret_path_read_reason),
        sftp_server: pseudo
            .as_deref()
            .and_then(ssh_manager_core::risk::sftp_server_invocation_reason),
        red_risk_reason: if session.red_risk_always_confirm {
            red_risk_confirm_reason(action, risk_assessment)
        } else {
            None
        },
        post_ingest_escalates,
        injection_suspected: session.injection_suspected.load(SeqCst),
        injection_reason: peek_injection_reason(session),
        injection_check_unavailable: session.injection_check_unavailable.load(SeqCst),
        mcp_origin: false,
        uses_stored_sudo_password: uses_stored_sudo_password(session, action),
        transform_marks_text: transform_requires_confirmation(session, transform_scope, action),
        earlier_rejection: false,
        server_tags_unavailable: false,
    }
}

/// Issue #596: the escalation code that does not depend on the user's choice,
/// for the verdict of a rule suggestion: the code of the step that would turn
/// an `AutoExec` into a `Confirm` for `command` (chat origin, no earlier
/// rejection), or `None`. Runs the same [`escalate`](super::escalation::escalate)
/// as the decision path, so the codes are identical (an injection suspicion
/// shows the injection code for a secret-path hit, like the main chain). It
/// never consumes a one-shot flag: the returned `Escalation` is only read.
pub(crate) fn state_independent_escalation_code(
    session: &Session,
    command: &str,
    transform_scope: Option<&ssh_manager_core::outgoing::ServerScope>,
) -> Option<&'static str> {
    let action = AiAction::SuggestCommand {
        command: command.to_string(),
    };
    let risk_assessment = risk_assessment_for_action(&action);
    let facts = escalation_facts(session, &action, risk_assessment.as_ref(), transform_scope);
    super::escalation::escalate(Decision::AutoExec, &facts).code
}

/// Issue #815: asks the registered outgoing transform (pure query) whether it
/// will change the text of `action`. `false` without a transform or scope.
fn transform_requires_confirmation(
    session: &Session,
    scope: Option<&ssh_manager_core::outgoing::ServerScope>,
    action: &AiAction,
) -> bool {
    use ssh_manager_core::outgoing::OutgoingKind;
    let (Some(transform), Some(scope)) = (session.outgoing_transform.as_ref(), scope) else {
        return false;
    };
    match action {
        AiAction::SuggestCommand { command } => {
            transform.requires_confirmation(scope, OutgoingKind::Command, command)
        }
        AiAction::WriteRemoteFile { content, .. } => {
            transform.requires_confirmation(scope, OutgoingKind::FileContent, content)
        }
        _ => false,
    }
}

fn risk_assessment_for_action(action: &AiAction) -> Option<RiskAssessment> {
    let pseudo_command = pseudo_command_for_risk_classification(action)?;
    Some(RuleBasedRiskClassifier.classify(&pseudo_command))
}

/// Spec 0092, A2: der Grund, mit dem das neue Glied eskaliert — `None`, wenn
/// es nicht greifen soll.
///
/// **Fail-safe bei Überlänge** (spec-reviewer-Fund, Runde 1, adversarialer
/// Fall 7): Der Klassifizierer bricht ab, sobald das Pseudokommando länger
/// als [`DEFAULT_MAX_COMMAND_LENGTH`] Bytes ist, und liefert dann
/// `None`/`None` — „kein Risiko erkannt" heißt dort „nicht geprüft". Früher
/// zählte die Filter-Engine Zeichen statt Bytes, und ein Kommando mit
/// Mehrbyte-Zeichen konnte unter ihrer Schranke liegen (mit Allow-Regel also
/// `AutoExec` werden), während der Klassifizierer schon aufgegeben hatte.
/// Seit Issue #110 messen beide über [`exceeds_command_length_limit`]
/// dasselbe, die Filter-Engine verlangt in diesem Fall also selbst schon
/// `Confirm`. Der Zweig bleibt trotzdem als Tiefenverteidigung: Das
/// Pseudokommando ist nicht immer das Kommando, das die Filter-Engine sieht,
/// und „nicht geprüft" darf hier nie als „unauffällig" durchgehen. Derselbe
/// Helfer und dieselbe Schranke wie im Klassifizierer, damit genau das
/// Fenster abgedeckt ist, in dem er aussteigt.
fn red_risk_confirm_reason(
    action: &AiAction,
    assessment: Option<&RiskAssessment>,
) -> Option<String> {
    if let Some(pseudo_command) = pseudo_command_for_risk_classification(action) {
        if exceeds_command_length_limit(&pseudo_command, DEFAULT_MAX_COMMAND_LENGTH) {
            return Some(
                "Kommando zu lang für eine Risiko-Einschätzung – wird wie rot behandelt"
                    .to_string(),
            );
        }
    }
    red_risk_reason(assessment)
}

/// Spec 0092, A2.1: `Some`, wenn **eine** der beiden Achsen `Red` ist — mit
/// einem Grund, der die rote Achse und deren Begründung nennt (beide, falls
/// beide rot sind). `None` für „keine Einschätzung vorhanden" (`ProposeNote
/// Update`/`GenerateDocument`, s. `pseudo_command_for_risk_classification`)
/// und für alles unter `Red`.
///
/// Der Text landet in Ledger und KI-Kontext; im Dialog sieht der Nutzer den
/// festen übersetzten Text zum Code und die rote Achse am Badge (Spec 0092,
/// §5, „Anzeige des Grunds").
fn red_risk_reason(assessment: Option<&RiskAssessment>) -> Option<String> {
    let assessment = assessment?;
    let axis = |level: RiskLevel, label: &str, reason: Option<&String>| {
        (level == RiskLevel::Red).then(|| match reason {
            Some(reason) => format!("{label}: {reason}"),
            // Der Klassifizierer liefert zu einem `Red` immer eine
            // Begründung; fehlt sie doch, wird trotzdem eskaliert statt die
            // Eskalation an einem fehlenden Text scheitern zu lassen.
            None => format!("{label}: rot eingestuft"),
        })
    };
    let parts: Vec<String> = [
        axis(
            assessment.server_risk,
            "Server-Risiko rot",
            assessment.server_risk_reason.as_ref(),
        ),
        axis(
            assessment.data_risk,
            "Daten-Risiko rot",
            assessment.data_risk_reason.as_ref(),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// Spec 0026, Abschnitt 3: "Nur Eskalation, nie Abschwächung" — als reine
/// Funktion getrennt von der Event-/Async-Maschinerie um
/// `fetch_second_opinion`, damit genau dieser Fall (ein regelbasiertes
/// `Red` bleibt `Red`, egal was die KI zurückgibt) direkt und ohne
/// Event-Mitschnitt testbar ist.
fn escalate_data_risk(
    rule_based_level: RiskLevel,
    rule_based_reason: Option<String>,
    second_opinion: Option<(RiskLevel, String)>,
) -> (RiskLevel, Option<String>) {
    match second_opinion {
        Some((ai_level, ai_reason)) if ai_level > rule_based_level => (ai_level, Some(ai_reason)),
        _ => (rule_based_level, rule_based_reason),
    }
}

/// Menschen-/KI-lesbare Kurzbeschreibung einer Aktion für
/// `MessageContent::ActionRejected.command` (Spec 0021, Abschnitt 3) — für
/// `SuggestCommand` das Kommando selbst, für `ReadRemoteFile`/
/// `WriteRemoteFile` dieselbe Pseudokommando-Form wie in der
/// Filter-Engine-Auswertung (Spec 0020). `ProposeNoteUpdate` folgt "demselben
/// Muster ... aber ohne eigene Sonderbehandlung" (Spec 0021, Abschnitt 3,
/// letzter Absatz) — bekommt trotzdem eine sinnvolle Kurzbezeichnung statt
/// eines rohen Debug-Werts. `GenerateDocument` erreicht diese Funktion nie
/// (durchläuft weder `evaluate_action` noch einen Bestätigungsdialog, s.
/// dort) — der `unreachable!()`-Arm hält dieselbe Invariante fest.
/// Spec 0066: eine wegen Stopp nicht gestartete Auto-Ausführung wird wie
/// ein abgebrochenes Kommando gemeldet (Spec 0027, `cancelled: true`,
/// leere Ausgabe) — die Aktionskarte zeigt dadurch „abgebrochen" statt
/// dauerhaft „läuft", und die KI sieht im Verlauf, dass nichts lief.
async fn skip_auto_exec_after_stop(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    action: &AiAction,
    emitter: &dyn EventEmitter,
    persist: bool,
) -> bool {
    let command = match action {
        AiAction::SuggestCommand { command } => command.clone(),
        other => describe_rejected_action(other),
    };
    emit_chat_action_result(
        emitter,
        session_id,
        action_id,
        ActionResultPayload::Command {
            command: command.clone(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            cancelled: true,
            truncated: false,
        },
    );
    push_history_scoped(
        session,
        ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::CommandResult {
                command,
                output: CommandOutput {
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    exit_code: None,
                    truncated: false,
                },
                cancelled: true,
                waiting_for_input: false,
            },
        },
        persist,
    )
    .await;
    false
}

/// `AiAction::SuggestCommand` läuft durch die Filter-Engine;
/// `AiAction::ProposeNoteUpdate` verlangt **immer** eine Bestätigung,
/// unabhängig von der Filter-Engine (Spec 0003, Abschnitt 5.2 — explizit
/// wiederholt in Spec 0007, Abschnitt 6, letzter Punkt). `ReadRemoteFile`/
/// `WriteRemoteFile` laufen ebenfalls durch die Filter-Engine (Spec 0020,
/// Abschnitt 4.1/4.2, Punkt 1) — `WriteRemoteFile` bekommt dabei aber nie
/// `AutoExec` (Abschnitt 4.2, Punkt 2: "Auch bei einer Allow-Regel wird nie
/// ohne Anzeige geschrieben").
async fn evaluate_action(
    session: &Session,
    action: &AiAction,
    profile_store: &dyn ProfileStore,
) -> (EvaluationTrace, bool) {
    match action {
        AiAction::SuggestCommand { command } => {
            let (tags, tags_unavailable) = server_tags_lookup(session, profile_store).await;
            let ctx = EvalContext {
                server_id: session.server_id,
                tags,
            };
            let trace = session
                .filter_engine
                .evaluate_explained(command, &ctx)
                .await;
            (trace, tags_unavailable)
        }
        AiAction::ProposeNoteUpdate { .. } => (
            EvaluationTrace {
                decision: Decision::Confirm {
                    reason: "Notiz-Aktualisierungen erfordern immer eine manuelle Bestätigung"
                        .to_string(),
                    code: "FILTER_NOTE_UPDATE_REQUIRES_CONFIRM".to_string(),
                },
                matched_rule: None,
                matched_rule_origin: None,
                matched_hard_blacklist_entry: None,
                sub_command_traces: Vec::new(),
                kind: Default::default(),
                segment_index: None,
                checks: Vec::new(),
            },
            false,
        ),
        AiAction::GenerateDocument { .. } => unreachable!(
            "GenerateDocument wird bereits in run_one_round abgefangen \
             (Spec 0012: kein Filter-Engine-/Bestätigungspfad) und erreicht \
             evaluate_action nie"
        ),
        AiAction::AskUser { .. } => unreachable!(
            "AskUser is intercepted in run_one_round (issue #545: no filter/confirmation path) \
             and never reaches evaluate_action"
        ),
        AiAction::ReadRemoteFile { path } => {
            let (tags, tags_unavailable) = server_tags_lookup(session, profile_store).await;
            let ctx = EvalContext {
                server_id: session.server_id,
                tags,
            };
            let trace = session
                .filter_engine
                .evaluate_explained(&sftp_read_pseudo_command(path), &ctx)
                .await;
            (trace, tags_unavailable)
        }
        AiAction::WriteRemoteFile { path, .. } => {
            let (tags, tags_unavailable) = server_tags_lookup(session, profile_store).await;
            let ctx = EvalContext {
                server_id: session.server_id,
                tags,
            };
            let mut trace = session
                .filter_engine
                .evaluate_explained(&sftp_write_pseudo_command(path), &ctx)
                .await;
            if matches!(trace.decision, Decision::AutoExec) {
                trace.decision = Decision::Confirm {
                    reason: "Dateischreibvorgänge werden immer zur Bestätigung angezeigt \
                             (Spec 0020, Abschnitt 4.2)"
                        .to_string(),
                    code: "FILTER_FILE_WRITE_REQUIRES_CONFIRM".to_string(),
                };
            }
            (trace, tags_unavailable)
        }
    }
}

/// Gibt zurück, ob die Aktion tatsächlich ausgeführt wurde.
#[allow(clippy::too_many_arguments)]
async fn handle_user_decision(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    action: AiAction,
    user_decision: ActionUserDecision,
    // Spec 0046, Fund 4: nur für den `ActionUserDecision::Deny`-Zweig
    // relevant — `RejectionReason::User` für eine echte Nutzer-Ablehnung,
    // `RejectionReason::Timeout` für die vom Backend-Sicherheitsnetz
    // fabrizierte Ablehnung, wenn `PENDING_ACTION_CONFIRM_TIMEOUT` ablief,
    // ohne dass je eine Entscheidung eintraf.
    deny_reason: RejectionReason,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    origin: ActionOrigin,
    matched_rule: Option<RuleId>,
    matched_rule_origin: Option<RuleOrigin>,
    // spec-reviewer-Fund (Review dieses Schritts): die ursprüngliche
    // `Decision::Confirm { reason, code }`, die zum Bestätigungsdialog
    // führte — z. B. `FILTER_MCP_ORIGIN_REQUIRES_CONFIRM`/
    // `FILTER_SUDO_PASSWORD_REQUIRES_CONFIRM`/
    // `FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM`. Vorher gingen diese
    // Eskalationsgründe für den finalen Ledger-`Decision`-Eintrag
    // verloren (`reason`/`code` liefen dort fest auf `None`), obwohl sie
    // hier längst vorlagen — ein Leser des Ledgers konnte dann nicht
    // rekonstruieren, WARUM überhaupt eine Bestätigung nötig war, nur DASS
    // eine Regel gegriffen hätte.
    confirm_reason: String,
    confirm_code: String,
    // Spec 0068, Teil 3 (Review-Fund): der im Dialog tatsächlich
    // angekündigte Wert (`usesStoredSudoPassword`) — nur damit darf ein
    // Schreib-Fallback Sudo nutzen.
    sudo_fallback_announced: bool,
    // Spec 0068, Teil 4 (zweite Review-Runde): auch ein vom Nutzer
    // bearbeitetes, dann regelbasiert blockiertes Kommando zählt als
    // Ablehnung für die übrigen Aktionen derselben Antwort.
    earlier_rejection: &std::sync::atomic::AtomicBool,
) -> bool {
    // Spec 0040, Abschnitt 4: s. identischer Kommentar in
    // `handle_action_proposed` — MCP-Herkunft persistiert nie, auch nicht
    // über diesen Bestätigungsdialog-Pfad (ein MCP-Vorschlag mit `Confirm`
    // landet ebenfalls hier, s. Spec 0028, Abschnitt 9a).
    let persist = !matches!(origin, ActionOrigin::Mcp { .. });

    // Spec 0057, §1.1, zweiter Punkt: die im Bestätigungsdialog getroffene
    // (oder per Timeout fabrizierte, s. `deny_reason`-Doc-Kommentar oben)
    // Freigabe-Entscheidung. `LedgerSource::User` NUR für eine echte
    // Nutzer-Entscheidung (`RejectionReason::User`/ein tatsächliches
    // `Approve`/`EditThenApprove`) — der Timeout-Fall hat keinen Menschen
    // entscheiden lassen, bekommt deshalb dieselbe Herkunfts-Quelle wie
    // der ursprüngliche Vorschlag (s. `ledger_source_for_origin`).
    //
    // `(decision_reason, decision_code)`: der Timeout-Fall bekommt einen
    // eigenen, spezifischeren Code (`TIMEOUT`) statt des ursprünglichen
    // Eskalationsgrunds — spec-reviewer-Fund: ohne diesen Code war eine
    // Timeout-Ablehnung von einer echten Nutzer-Ablehnung im Ledger nur
    // indirekt über `source` unterscheidbar.
    let (decision_source, decision_reason, decision_code) =
        if matches!(deny_reason, RejectionReason::Timeout) {
            (
                ledger_source_for_origin(&origin),
                Some(
                    "Bestätigung nicht innerhalb des Zeitfensters erfolgt, als Ablehnung \
                     behandelt (Spec 0046, Fund 4)"
                        .to_string(),
                ),
                Some("TIMEOUT".to_string()),
            )
        } else {
            (LedgerSource::User, Some(confirm_reason), Some(confirm_code))
        };

    match user_decision {
        ActionUserDecision::Deny | ActionUserDecision::DenyReconnect => {
            if let AiAction::SuggestCommand { .. } = &action {
                write_ledger_entry(
                    session,
                    decision_source,
                    LedgerEntryContent::Decision {
                        outcome: LedgerDecisionOutcome::Rejected,
                        reason: decision_reason.clone(),
                        code: decision_code.clone(),
                        matched_rule: matched_rule.clone(),
                        matched_rule_origin,
                    },
                )
                .await;
            }
            // Spec 0021, Abschnitt 3, Fall 3: der Nutzer hat abgelehnt — das
            // Frontend weiß es bereits (es hat den Aufruf selbst gemacht),
            // aber die KI bisher nicht. `RejectionReason::User`/`Blocked`
            // lassen die KI unterscheiden "die Filter-Engine hat blockiert"
            // von "der Mensch wollte das nicht" und entsprechend reagieren
            // (Alternative vorschlagen, nachfragen, akzeptieren). Das war
            // der Kern des gemeldeten Bugs: ohne diesen Eintrag + die
            // automatische Folgerunde blieb der Chat nach "Ablehnen" stumm
            // (s. Moduldoc). `deny_reason` ist normalerweise `User`; für den
            // Timeout-Fall (Spec 0046, Fund 4) `Timeout` — dieselbe
            // fail-safe Richtung, aber mit ehrlicher Ursache in der
            // Historie.
            push_history_scoped(
                session,
                ChatMessage {
                    role: Role::ActionResult,
                    content: MessageContent::ActionRejected {
                        command: describe_rejected_action(&action),
                        reason: deny_reason,
                    },
                },
                persist,
            )
            .await;
            true
        }
        ActionUserDecision::Approve => {
            if let AiAction::SuggestCommand { .. } = &action {
                write_ledger_entry(
                    session,
                    decision_source,
                    LedgerEntryContent::Decision {
                        outcome: LedgerDecisionOutcome::Confirmed,
                        reason: decision_reason.clone(),
                        code: decision_code.clone(),
                        matched_rule: matched_rule.clone(),
                        matched_rule_origin,
                    },
                )
                .await;
            }
            execute_action(
                session,
                session_id,
                action_id,
                ApprovedAction::from_user_approval(action),
                emitter,
                profile_store,
                persist,
                decision_source,
                sudo_fallback_announced,
            )
            .await
        }
        ActionUserDecision::EditThenApprove { command: edited } => {
            let effective_action = match &action {
                // Ein editiertes Kommando darf die Prüfung nicht umgehen
                // (Aufgabenstellung Teil 1, Punkt 4): erneut durch die
                // Filter-Engine schicken. Nur ein *hartes* `Deny`
                // überstimmt die bereits erteilte Nutzer-Freigabe — ein
                // erneutes `Confirm` (z. B. weil der bearbeitete Text
                // wieder auf die Blacklist trifft, die selbst nur auf
                // `Confirm` abbildet) nicht: der Klick auf "Ausführen" im
                // Bearbeiten-Dialog *ist* bereits die verlangte
                // Bestätigung.
                AiAction::SuggestCommand { .. } => {
                    // Spec 0057, §1.1: der bearbeitete Text ist ein eigener
                    // Vorschlag (nicht identisch mit dem ursprünglich von
                    // der KI vorgeschlagenen Kommando) — vom Nutzer selbst
                    // verfasst, deshalb `LedgerSource::User`, nicht die
                    // Herkunft des ursprünglichen Vorschlags.
                    write_ledger_entry(
                        session,
                        LedgerSource::User,
                        LedgerEntryContent::CommandProposed {
                            command: edited.clone(),
                        },
                    )
                    .await;

                    // A store failure is logged by the lookup; only `Deny`
                    // matters in this re-evaluation, so the frozen tags
                    // stay in use (issue #843).
                    let (tags, _tags_unavailable) =
                        server_tags_lookup(session, profile_store).await;
                    let ctx = EvalContext {
                        server_id: session.server_id,
                        tags,
                    };
                    let re_evaluation = session
                        .filter_engine
                        .evaluate_explained(&edited, &ctx)
                        .await;
                    if let Decision::Deny { reason, code } = re_evaluation.decision {
                        earlier_rejection.store(true, std::sync::atomic::Ordering::SeqCst);
                        write_ledger_entry(
                            session,
                            ledger_source_for_origin(&origin),
                            LedgerEntryContent::Decision {
                                outcome: LedgerDecisionOutcome::Rejected,
                                reason: Some(reason.clone()),
                                code: Some(code.clone()),
                                matched_rule: re_evaluation.matched_rule,
                                matched_rule_origin: re_evaluation.matched_rule_origin,
                            },
                        )
                        .await;
                        let blocked = AiAction::SuggestCommand {
                            command: edited.clone(),
                        };
                        let risk_assessment = risk_assessment_for_action(&blocked);
                        emit_chat_action_proposed(
                            emitter,
                            session_id,
                            Uuid::new_v4(),
                            blocked,
                            Decision::Deny {
                                reason: reason.clone(),
                                code,
                            },
                            None,
                            false,
                            None,
                            None,
                            None,
                            risk_assessment,
                            origin,
                            Vec::new(),
                        );
                        // Spec 0021, Abschnitt 3, Fall 4: das bearbeitete
                        // Kommando ist am Ende genau ein durch die
                        // Filter-Engine blockierter Vorschlag, nur über den
                        // Bearbeiten-Dialog statt des ursprünglichen Wegs
                        // erreicht — dieselbe Kontext-Eintrag +
                        // automatische-Folgerunde-Behandlung gilt
                        // einheitlich.
                        push_history_scoped(
                            session,
                            ChatMessage {
                                role: Role::ActionResult,
                                content: MessageContent::ActionRejected {
                                    command: edited,
                                    reason: RejectionReason::Blocked(reason),
                                },
                            },
                            persist,
                        )
                        .await;
                        return true;
                    }
                    // Der Klick auf "Ausführen" im Bearbeiten-Dialog *ist*
                    // bereits die verlangte Bestätigung (s. Doc-Kommentar
                    // oben) — dieselbe `Confirmed`-Semantik wie
                    // `ActionUserDecision::Approve` oben, `LedgerSource::
                    // User` aus demselben Grund wie beim `CommandProposed`-
                    // Eintrag oben.
                    write_ledger_entry(
                        session,
                        LedgerSource::User,
                        LedgerEntryContent::Decision {
                            outcome: LedgerDecisionOutcome::Confirmed,
                            reason: None,
                            code: None,
                            matched_rule: re_evaluation.matched_rule,
                            matched_rule_origin: re_evaluation.matched_rule_origin,
                        },
                    )
                    .await;
                    AiAction::SuggestCommand { command: edited }
                }
                // Weder `ProposeNoteUpdate` noch `ReadRemoteFile`/
                // `WriteRemoteFile` bieten im Frontend ein Editierfeld an
                // (Spec 0020, Abschnitt 4.2 sieht nur Bestätigen/Ablehnen
                // vor, kein Editieren des Inhalts vor dem Schreiben) — träfe
                // `EditThenApprove` trotzdem ein, wird die ursprünglich
                // vorgeschlagene Aktion unverändert ausgeführt, analog zu
                // `ProposeNoteUpdate`.
                AiAction::ProposeNoteUpdate { .. }
                | AiAction::ReadRemoteFile { .. }
                | AiAction::WriteRemoteFile { .. } => action,
                AiAction::AskUser { .. } => unreachable!(
                    "AskUser needs no confirmation and never reaches execution, see run_one_round"
                ),
                AiAction::GenerateDocument { .. } => unreachable!(
                    "GenerateDocument braucht nie eine Bestätigung, s. evaluate_action"
                ),
            };
            execute_action(
                session,
                session_id,
                action_id,
                ApprovedAction::from_user_approval(effective_action),
                emitter,
                profile_store,
                persist,
                LedgerSource::User,
                sudo_fallback_announced,
            )
            .await
        }
    }
}

/// Gibt zurück, ob die Aktion tatsächlich ausgeführt wurde (d.h. ob ihr
/// Ergebnis in `context.history` gelandet ist) — bei einem Fehlschlag
/// (`SshError`/`ProfileError`) ist der Kontext unverändert, eine
/// automatische Folgerunde (s. Moduldoc) würde dann nur denselben
/// Vorschlag erneut auslösen, statt der KI etwas Neues mitzuteilen.
#[allow(clippy::too_many_arguments)]
async fn execute_action(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    action: ApprovedAction,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    persist: bool,
    // spec-reviewer-Fund (Review dieses Schritts): vorher leitete
    // `execute_suggested_command` die `LedgerSource` seines
    // `CommandExecuted`-Eintrags aus `persist` ab (`persist == false` <=>
    // MCP) — funktioniert nur zufällig, weil `persist` heute dieselbe
    // Bedeutung trägt, UND schreibt ein vom Menschen *editiertes*
    // Kommando (`EditThenApprove`) fälschlich `Ai` statt `User` zu. Explizit
    // durchgereicht statt aus einem semantisch anderen Flag abgeleitet —
    // nur der `SuggestCommand`-Zweig unten braucht ihn (Etappe-1-Scope,
    // s. ADR 0047 Punkt 3), die anderen Aktionstypen ignorieren ihn.
    ledger_source: LedgerSource,
    // Spec 0068, Teil 3: s. `handle_user_decision`.
    sudo_fallback_announced: bool,
) -> bool {
    let action = match action {
        ApprovedAction::Command(command) => {
            return execute_suggested_command(
                session,
                session_id,
                action_id,
                command,
                emitter,
                profile_store,
                persist,
                ledger_source,
            )
            .await;
        }
        ApprovedAction::FileWrite(approved) => {
            return execute_write_remote_file(
                session,
                session_id,
                action_id,
                approved,
                emitter,
                profile_store,
                persist,
                sudo_fallback_announced,
            )
            .await;
        }
        ApprovedAction::Other(action) => action,
    };
    match action {
        AiAction::ProposeNoteUpdate {
            target,
            new_content,
        } => {
            execute_note_update(
                session,
                session_id,
                action_id,
                target,
                new_content,
                emitter,
                profile_store,
                persist,
            )
            .await
        }
        AiAction::GenerateDocument { .. } => {
            unreachable!("GenerateDocument braucht nie eine Bestätigung, s. evaluate_action")
        }
        AiAction::AskUser { .. } => {
            unreachable!("AskUser is never executed, see run_one_round")
        }
        AiAction::ReadRemoteFile { path } => {
            execute_read_remote_file(session, session_id, action_id, path, emitter, persist).await
        }
        AiAction::SuggestCommand { .. } | AiAction::WriteRemoteFile { .. } => {
            unreachable!("approved commands and file writes are wrapped by ApprovedAction")
        }
    }
}

/// Spec 0018, Abschnitt 3: erkennt einen `sudo`/`doas`-Aufruf, der das
/// **gesamte** Kommando bildet (kein Treffer mitten in einer Kommandokette,
/// s. Spec-Dokument für die bewusste Einschränkung). Liefert bei Treffer
/// das Präfix (`"sudo"`/`"doas"`) zurück.
fn detect_elevation_prefix(command: &str) -> Option<&'static str> {
    let trimmed = command.trim_start();
    ["sudo", "doas"].into_iter().find(|&prefix| {
        trimmed
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    })
}

/// Spec 0018, Abschnitt 5: liefert das für `-S`+Stdin-Passworteingabe
/// vorbereitete Kommando, falls `command` elevation-fähig ist und noch kein
/// eigenes `-S`/`-A`-Flag enthält (dann haben KI/Nutzer die
/// Passworteingabe bereits selbst vorgesehen, nicht gegensteuern).
fn command_with_stdin_password_flag(command: &str) -> Option<String> {
    let prefix = detect_elevation_prefix(command)?;
    if command.contains("-S") || command.contains("-A") {
        return None;
    }
    let trimmed = command.trim_start();
    let rest = &trimmed[prefix.len()..];
    Some(format!("{prefix} -S{rest}"))
}

/// Spec 0018, Abschnitt 7 / Spec 0068, Teil 3: ob die Aktion das hinterlegte Sudo-Passwort nutzt bzw. nutzen KANN —
/// steuert die Ankündigung im Bestätigungsdialog (`usesStoredSudoPassword`)
/// und die Eskalation auf `Confirm`.
///
/// Spec 0068, Teil 3 (Release-Gate C): für `WriteRemoteFile` auch dann,
/// wenn Sudo erst als Fallback nach einem Rechte-Fehler nötig würde
/// (`execute_write_remote_file`) — sonst schöbe die App Sudo nach der
/// Bestätigung still nach. Der Fallback dort bekommt genau den im Dialog
/// gesendeten Wert durchgereicht, ist also an die Ankündigung gebunden.
fn uses_stored_sudo_password(session: &Session, action: &AiAction) -> bool {
    session.sudo_password.is_some()
        && match action {
            AiAction::SuggestCommand { command } => detect_elevation_prefix(command).is_some(),
            AiAction::WriteRemoteFile { .. } => true,
            _ => false,
        }
}

/// Meldet einen bei der Ausführung einer Aktion aufgetretenen Fehler sowohl
/// als `chat-error`-Event (sofort sichtbare Meldung im UI) als auch als
/// `ActionResult`-Eintrag im Kontext (Spec 0021, Abschnitt 3, analog zum
/// `Decision::Deny`-Fall) — ohne Letzteres bekäme die KI den Fehlschlag nie
/// zu sehen und der Turn bräche ohne Folgerunde ab, obwohl aus ihrer Sicht
/// offen bliebe, was aus der Aktion geworden ist (das war der aus einem
/// Nutzer-Bugreport bekannte "Chat hängt nach SFTP-Fehler"-Fall). Gibt immer
/// `true` zurück, zur direkten Verwendung als `return`-Ausdruck an den
/// bisherigen `false`-Rückgabestellen.
pub(crate) async fn emit_action_error(
    session: &Session,
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    message: String,
    code: Option<&'static str>,
    persist: bool,
) -> bool {
    emit_chat_error(emitter, session_id, message.clone(), code);
    push_history_scoped(
        session,
        ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::Text(message),
        },
        persist,
    )
    .await;
    true
}

#[allow(clippy::too_many_arguments)]
async fn execute_suggested_command(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    approved: ApprovedCommand,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    persist: bool,
    ledger_source: LedgerSource,
) -> bool {
    let command = approved.as_str().to_owned();
    // Spec 0018, Abschnitt 5: nur umschreiben/Stdin füttern, wenn tatsächlich
    // ein Passwort hinterlegt ist — sonst unverändertes Verhalten
    // (`execute()` wie bisher, scheitert wie gewohnt ohne TTY).
    let sudo_password = session.sudo_password.get();
    // Issue #464 (Spec 0038): the edition's transform runs on the final
    // approved command, right before the transport call. Everything that
    // is shown or stored below uses `command`; `wire_command` exists only
    // for the transport call.
    let wire_command = match super::outgoing::prepare_for_send(
        session,
        profile_store,
        ssh_manager_core::outgoing::OutgoingKind::Command,
        &command,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(message) => {
            return emit_action_error(session, emitter, session_id, message, None, persist).await;
        }
    };
    let effective_command = sudo_password
        .as_ref()
        .and_then(|_| command_with_stdin_password_flag(&command));
    let wire_effective_command = match &wire_command {
        Some(prepared) => {
            use secrecy::ExposeSecret;
            sudo_password
                .as_ref()
                .and_then(|_| command_with_stdin_password_flag(prepared.expose_secret()))
        }
        None => effective_command.clone(),
    };

    // Spec 0027, Abschnitt 3: vor dem eigentlichen Aufruf registriert, damit
    // `commands::cancel_running_command` diese `action_id` jederzeit
    // während der Ausführung treffen kann. Der Aufruf unten (egal welcher
    // Zweig) konsumiert `cancel_rx` vollständig — der abschließende
    // `resolve()`-Aufruf danach räumt den Registry-Eintrag in **jedem**
    // Fall auf (regulär beendet: Empfänger bereits gedroppt, `send()`
    // schlägt harmlos fehl; abgebrochen: Eintrag existiert dann schon
    // nicht mehr, weil genau dieser `resolve()`-Aufruf — aus
    // `cancel_running_command` — die Ausführung erst beendet hat) — ohne
    // dieses Aufräumen bliebe für jedes regulär beendete Kommando ein
    // nie entfernter Eintrag in der Registry zurück.
    let cancel_rx = session.running_command_cancellations.register(action_id);

    // Issue #325 (Spec 0106): the transport additionally hands every output
    // chunk to `live_sink` while the command runs; `forward_live_output`
    // redacts and throttles it into `chat-action-output` events. Display
    // only — the outcome below (history, ledger, AI) is unchanged. The
    // forwarder ends when the transport drops the sink, i.e. before the
    // final result is emitted.
    let (live_sink, live_chunks) = tokio::sync::mpsc::unbounded_channel();
    // Issue #327: set by the forwarder while the "probably waiting for
    // input" hint is shown. Backend-owned, so the cancel note does not
    // depend on anything the frontend claims.
    let waiting_for_input = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let execution = async {
        // Issue #328: AI commands of one session stay serial through the
        // command slot, but the transport lock is only held to open the exec
        // channel. While the command runs, file browser, terminal and
        // disconnect can use the transport.
        let Some(_slot) = session.transport.begin_command(action_id).await else {
            return Err(ssh_manager_core::ssh::SshError::ChannelError(
                "session is disconnecting".to_string(),
            ));
        };
        let stdin = match (&wire_effective_command, &sudo_password) {
            (Some(_), Some(password)) => {
                use secrecy::ExposeSecret;
                let mut stdin = password.expose_secret().as_bytes().to_vec();
                stdin.push(b'\n');
                Some(stdin)
            }
            _ => None,
        };
        let run_command = match (&wire_effective_command, &stdin) {
            (Some(rewritten), Some(_)) => rewritten.as_str(),
            _ => match &wire_command {
                Some(prepared) => {
                    use secrecy::ExposeSecret;
                    prepared.expose_secret()
                }
                None => command.as_str(),
            },
        };
        // Spec 0114 §4: keep the device awake while the command runs.
        let _activity = crate::background_keepalive::background_activity();
        let mut transport = session.transport.lock().await;
        match transport.start_exec(run_command, stdin.as_deref()).await? {
            Some(running) => {
                drop(transport);
                running.run(cancel_rx, live_sink).await
            }
            // Transport without split start/run: keeps the lock for the
            // whole command, as before.
            None => {
                transport
                    .execute_streaming(run_command, stdin.as_deref(), cancel_rx, live_sink)
                    .await
            }
        }
    };
    let (raw_outcome, ()) = tokio::join!(
        execution,
        super::live_output::forward_live_output(
            live_chunks,
            session.redactor.as_ref(),
            emitter,
            session_id,
            action_id,
            waiting_for_input.clone(),
        )
    );
    let _ = session
        .running_command_cancellations
        .resolve(&action_id, ());
    // Spec 0018, Abschnitt 5: das tatsächlich ausgeführte Kommando (mit
    // `-S`, ohne Passwort) landet in Ergebnis-Event/Log/Kontext — voll
    // transparent, da nie das Passwort selbst enthalten.
    let command = effective_command.unwrap_or(command);

    match raw_outcome {
        Ok(ExecOutcome { output, cancelled }) => {
            // Spec 0057, §1.1, dritter Punkt: "Kommando ausgeführt +
            // Ergebnis (stdout/stderr/exit)". Bewusst mit dem noch
            // UNREDIGIERTEN `output` aufgerufen — `write_ledger_entry`
            // redigiert selbst zentral (s. dortiger Doc-Kommentar), eine
            // hier vorab redigierte Fassung würde nur doppelt redigieren,
            // ohne einen Sicherheitsgewinn. `ledger_source` kommt jetzt
            // explizit vom Aufrufer (`handle_action_proposed`/
            // `handle_user_decision`), nicht mehr aus `persist` abgeleitet
            // — spec-reviewer-Fund (Review dieses Schritts): `persist`
            // beschreibt einen ganz anderen Sachverhalt
            // (Chat-Persistenz-Ausschluss für MCP) und hätte ein vom
            // Menschen editiertes Kommando (`EditThenApprove`) fälschlich
            // `Ai` zugeschrieben.
            write_ledger_entry(
                session,
                ledger_source,
                LedgerEntryContent::CommandExecuted {
                    command: command.clone(),
                    output: output.clone(),
                    cancelled,
                },
            )
            .await;
            let redacted = session.redactor.redact(&output);
            // Unabhängiger Review-Pass (Spec 0016): Spec 0016 Abschnitt 2/3
            // verlangt dieselbe Redaction-Regel für Logs wie für den
            // tatsächlichen API-Request — bisher lief nur `output` durch
            // den Redactor, das Kommando selbst (kann z. B. `mysql
            // --password=hunter2 ...` sein) landete roh im Log. Nur der
            // LOG-Aufruf bekommt die redigierte Fassung; das Event/der
            // Kontext-Eintrag unten bleibt bewusst unverändert (Spec 0018
            // Abschnitt 5: "voll transparent" für das tatsächlich
            // ausgeführte Kommando gegenüber Nutzer/KI).
            log_command_execution(
                session_id,
                &session.redactor.redact_text(&command),
                &redacted,
            );
            emit_chat_action_result(
                emitter,
                session_id,
                action_id,
                ActionResultPayload::Command {
                    command: command.clone(),
                    stdout: String::from_utf8_lossy(&redacted.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&redacted.stderr).into_owned(),
                    exit_code: redacted.exit_code,
                    cancelled,
                    truncated: redacted.truncated,
                },
            );
            let combined_output = format!(
                "{}\n{}",
                String::from_utf8_lossy(&redacted.stdout),
                String::from_utf8_lossy(&redacted.stderr)
            );
            push_history_scoped(
                session,
                ChatMessage {
                    role: Role::ActionResult,
                    content: MessageContent::CommandResult {
                        command,
                        output: redacted,
                        cancelled,
                        // The flag is only meaningful for a stop; a command
                        // that ended on its own never gets the note.
                        waiting_for_input: cancelled
                            && waiting_for_input.load(std::sync::atomic::Ordering::SeqCst),
                    },
                },
                persist,
            )
            .await;
            // Spec 0039, Abschnitt 5: `CommandResult` wird erst beim
            // tatsächlichen Versand an den KI-Provider über `ai::
            // fence_untrusted` in <stdout>/<stderr>-Tags gepackt
            // (`ai-providers::format_command_result`) — zählt aber schon
            // hier als "Inhalt aus einer nicht vertrauenswürdigen Quelle
            // in den KI-Kontext gelangt", s. `session::history_contains_
            // untrusted_content`, das denselben Fall genauso behandelt.
            session
                .untrusted_content_ingested
                .store(true, std::sync::atomic::Ordering::SeqCst);
            check_for_injected_instructions(
                session,
                session_id,
                emitter,
                action_id,
                &combined_output,
                "command_output",
            )
            .await;
            true
        }
        Err(err) => {
            // spec-reviewer-Fund (Review dieses Schritts): vorher blieb der
            // Fehlerfall ganz ohne `CommandExecuted`-Eintrag — im Ledger
            // sah es dann so aus, als sei das Kommando nie ausgeführt
            // worden, obwohl es den Server ggf. bereits erreicht hat (ein
            // Transport-/Kanalfehler sagt nichts darüber aus, ob es dort
            // angekommen ist). `exit_code: None` markiert bewusst "Ergebnis
            // unbekannt", nicht "erfolgreich beendet"; die Fehlermeldung
            // selbst landet in `stderr`, damit sie beim (zentralen)
            // Redigieren in `write_ledger_entry` denselben Behandlung
            // durchläuft wie eine echte Kommando-Ausgabe.
            write_ledger_entry(
                session,
                ledger_source,
                LedgerEntryContent::CommandExecuted {
                    command: command.clone(),
                    output: CommandOutput {
                        stdout: Vec::new(),
                        stderr: err.to_string().into_bytes(),
                        exit_code: None,
                        truncated: false,
                    },
                    cancelled: false,
                },
            )
            .await;
            // Unabhängiger Review-Pass (Spec 0016/0027): derselbe Fund wie im
            // Erfolgsfall oben (`redact_text` vor dem Log) - ohne das würde
            // z. B. `mysql --password=hunter2 ...` bei einem Kanalfehler im
            // Klartext im Logfile landen, obwohl der Erfolgspfad dasselbe
            // Kommando korrekt redigiert.
            log_command_execution_failed(session_id, &session.redactor.redact_text(&command), &err);
            let code = err.code();
            emit_action_error(
                session,
                emitter,
                session_id,
                format!("Kommando '{command}' konnte nicht ausgeführt werden: {err}"),
                Some(code),
                persist,
            )
            .await
        }
    }
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
pub(crate) async fn check_for_injected_instructions(
    session: &Session,
    session_id: SessionId,
    emitter: &dyn EventEmitter,
    item_id: Uuid,
    content: &str,
    source: &'static str,
) {
    let Some(provider) = session.injection_check_provider.as_deref() else {
        return;
    };
    // Issue #693: progress under the checked result (display only).
    emit_chat_injection_check(emitter, session_id, item_id, InjectionCheckState::Running);
    wait_for_ai_request_slot(session).await;
    if let Some(budget) = session.injection_check_budget.as_deref() {
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
            if let Ok(mut slot) = session.injection_suspected_reason.lock() {
                *slot = Some(reason.clone());
            }
            session
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

/// Test helper: a store with nothing in it that outlives the call expression.
#[cfg(test)]
fn empty_store() -> &'static crate::test_support::InMemoryProfileStore {
    Box::leak(Box::default())
}
// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_injection_reason;
