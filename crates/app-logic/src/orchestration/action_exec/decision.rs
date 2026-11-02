//! Split out of `action_exec.rs` (issue #859, pure move).

use super::*;
use uuid::Uuid;

use ssh_manager_core::ai::{ChatMessage, MessageContent, RejectionReason, Role};
use ssh_manager_core::audit::{LedgerDecisionOutcome, LedgerEntryContent, LedgerSource};
use ssh_manager_core::filter::{Decision, EvalContext, RuleId, RuleOrigin};
use ssh_manager_core::profiles::{AiAction, ProfileStore};

use crate::dto::{ActionOrigin, ActionUserDecision};
use crate::events::{emit_chat_action_proposed, EventEmitter};
use crate::session::Session;
use crate::state::{ActionId, SessionId};

use super::super::approval::ApprovedAction;
use super::super::chat_turn::{ledger_source_for_origin, push_history_scoped, write_ledger_entry};

/// Gibt zurück, ob die Aktion tatsächlich ausgeführt wurde.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_user_decision(
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
