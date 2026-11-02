//! Split out of `action_exec.rs` (issue #859, pure move).

use super::*;
use uuid::Uuid;

use ssh_manager_core::ai::{ChatMessage, MessageContent, RejectionReason, Role};
use ssh_manager_core::audit::{LedgerDecisionOutcome, LedgerEntryContent};
use ssh_manager_core::filter::{Decision, EvalContext, EvaluationTrace};
use ssh_manager_core::profiles::{AiAction, ProfileStore};
use ssh_manager_core::risk::RiskLevel;
use ssh_manager_core::ssh::CommandOutput;

use crate::confirmation::ConfirmationRegistry;
use crate::confirmation_settled::emit_settled_for_rejection;
use crate::dto::{ActionOrigin, ActionUserDecision};
use crate::events::{
    emit_action_decision_escalated, emit_chat_action_proposed, emit_chat_action_result,
    emit_risk_assessment_updated, ActionResultPayload, EventEmitter,
};
use crate::session::Session;
use crate::state::{ActionId, SessionId};

use super::super::approval::ApprovedAction;
use super::super::chat_turn::{
    ledger_source_for_origin, push_history_scoped, wait_for_ai_request_slot,
    wait_for_rate_limit_budget, write_ledger_entry, PENDING_ACTION_CONFIRM_TIMEOUT,
};
use super::super::notes::note_target_preview_for_action;
use super::super::pending_confirmation::{
    PendingConfirmation, ANOTHER_CONFIRMATION_PENDING_MESSAGE,
};
use super::super::remote_files::previous_file_content_for_action;

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
        Some(super::super::outgoing::outgoing_scope(session, profile_store).await)
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
    let escalation = super::super::escalation::escalate(decision, &facts);
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
    let Some(action) = super::super::notes::redact_note_proposal(action, session.redactor.as_ref())
    else {
        return emit_action_error(
            session,
            emitter,
            session_id,
            super::super::notes::NOTE_PROPOSAL_FULLY_REDACTED_MESSAGE.to_string(),
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
pub(super) async fn tags_for_session(
    session: &Session,
    profile_store: &dyn ProfileStore,
) -> Vec<String> {
    server_tags_lookup(session, profile_store).await.0
}

/// [`tags_for_session`] plus whether the profile store failed with anything
/// but "server not stored" (issue #843). `ServerNotFound` is normal (the
/// local pseudo-server has no row) and keeps the frozen tags silently; any
/// other error keeps them too but is reported, so the caller can escalate.
/// Logs the server id and error kind only.
pub(super) async fn server_tags_lookup(
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
pub(super) async fn auto_exec_rules_for(
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
pub(super) fn sftp_read_pseudo_command(path: &str) -> String {
    format!("sftp-read {path}")
}

pub(super) fn sftp_write_pseudo_command(path: &str) -> String {
    format!("sftp-write {path}")
}

/// Spec 0026, Abschnitt 2, Punkt 5: synchron beim Erzeugen des
/// `chat-action-proposed`-Events berechnet, für exakt dieselben drei
/// Aktionstypen, die die Filter-Engine kennt (`evaluate_action`) —
/// `ReadRemoteFile`/`WriteRemoteFile` nutzen dieselbe Pseudokommando-
/// Abbildung wie dort (Spec 0020, Abschnitt 4.1), keine zweite
/// Mapping-Logik. `None` für `ProposeNoteUpdate`/`GenerateDocument`, die
/// Spec 0026 nicht abdeckt (kein ausführbares Kommando/kein Dateipfad).
pub(super) fn pseudo_command_for_risk_classification(action: &AiAction) -> Option<String> {
    match action {
        AiAction::SuggestCommand { command } => Some(command.clone()),
        AiAction::ReadRemoteFile { path } => Some(sftp_read_pseudo_command(path)),
        AiAction::WriteRemoteFile { path, .. } => Some(sftp_write_pseudo_command(path)),
        AiAction::ProposeNoteUpdate { .. }
        | AiAction::GenerateDocument { .. }
        | AiAction::AskUser { .. } => None,
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
pub(super) async fn skip_auto_exec_after_stop(
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
pub(super) async fn evaluate_action(
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
