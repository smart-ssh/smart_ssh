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
    Decision, EvalContext, EvaluationTrace, RuleId, RuleOrigin, DEFAULT_MAX_COMMAND_LENGTH,
};
use ssh_manager_core::profiles::{AiAction, NoteTargetSelector, PostIngestPolicy, ProfileStore};
use ssh_manager_core::risk::{RiskAssessment, RiskClassifier, RiskLevel, RuleBasedRiskClassifier};
use ssh_manager_core::ssh::{CommandOutput, ExecOutcome, SshError};

use crate::confirmation::ConfirmationRegistry;
use crate::dto::{ActionOrigin, ActionUserDecision};
use crate::events::{
    emit_action_decision_escalated, emit_chat_action_proposed, emit_chat_action_result,
    emit_chat_error, emit_risk_assessment_updated, ActionResultPayload, EventEmitter,
};
use crate::session::Session;
use crate::state::{ActionId, SessionId};

use super::chat_turn::{
    ledger_source_for_origin, push_history_scoped, wait_for_ai_request_slot,
    wait_for_rate_limit_budget, write_ledger_entry, PENDING_ACTION_CONFIRM_TIMEOUT,
};
use super::notes::{execute_note_update, note_target_preview_for_action};
use super::pending_confirmation::PendingConfirmation;
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
mod tests_files_and_ledger;
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

    // Spec 0057, §1.1, erster Punkt: "Kommando vorgeschlagen". Bewusst auf
    // `SuggestCommand` beschränkt (diese Etappe deckt nur diesen
    // Aktionstyp ab, s. Spec 0057 §8 "Etappe 1") — `ReadRemoteFile`/
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
    // hier, EINMALIG, bevor irgendein Konsument unten (Filter-Auswertung,
    // Risikoeinschätzung, Vorschau, `execute_action`) den Pfad sieht — alle
    // greifen auf dasselbe `action` zu, das ab hier bereits normalisiert
    // ist.
    if let AiAction::ReadRemoteFile { path } | AiAction::WriteRemoteFile { path, .. } = &mut action
    {
        *path = normalize_remote_path(path);
    }

    let evaluation = evaluate_action(session, &action, profile_store).await;
    let matched_rule = evaluation.matched_rule.clone();
    let matched_rule_origin = evaluation.matched_rule_origin;
    let mut decision = evaluation.decision;

    // Vorgezogen (war vorher erst nach der Eskalationskette berechnet, s.
    // Git-Historie) — die neue Spec-0039-Eskalation unten braucht die
    // Server-Risiko-Achse bereits hier für die `Balanced`-Stufe.
    // `risk_assessment_for_action` ist eine reine, zustandslose Funktion
    // von `action` allein, der Zeitpunkt der Berechnung ändert also nichts
    // an ihrem Ergebnis.
    let risk_assessment = risk_assessment_for_action(&action);

    // Spec 0068, Teil 2: ein Lesebefehl/`sftp-read` auf einen Secret-Pfad
    // (`~/.ssh/id_*`, `.env`, `/etc/shadow`, …) verlangt IMMER eine
    // Bestätigung — auch wenn eine Allow-Regel greift. Reine Eskalation
    // (nur `AutoExec` → `Confirm`, `Deny` bleibt `Deny`), für Chat UND MCP,
    // weil beide durch diese Funktion laufen.
    //
    // Bewusst VOR der Injection-Prüfung (zweite Review-Runde): deren `swap`
    // verbraucht das Verdachts-Flag nur bei `AutoExec`; läge diese Prüfung
    // dahinter, verbrauchte eine Secret-Lese-Aktion das Flag, und die
    // eigentliche Folgeaktion liefe wieder automatisch. Damit der Dialog
    // trotzdem den alarmierenderen Grund zeigt, wird das Flag hier nur
    // GELESEN (nicht verbraucht) und bei gesetztem Flag der Injection-Grund
    // angezeigt.
    if matches!(decision, Decision::AutoExec) {
        if let Some(reason) = pseudo_command_for_risk_classification(&action)
            .as_deref()
            .and_then(ssh_manager_core::risk::secret_path_read_reason)
        {
            decision = if session
                .injection_suspected
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                Decision::Confirm {
                    reason: format!(
                        "Möglicher Versuch, Anweisungen über Serverinhalt einzuschleusen, \
                         erkannt; außerdem: {reason} – erfordert Bestätigung"
                    ),
                    code: "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM".to_string(),
                }
            } else {
                Decision::Confirm {
                    reason: format!("{reason} – erfordert immer Bestätigung"),
                    code: "FILTER_SECRET_PATH_READ_REQUIRES_CONFIRM".to_string(),
                }
            };
        }
    }

    // ADR 0058 §8 (Entscheidung, 2026-09-21): ein Aufruf von
    // `sftp-server` verlangt IMMER eine Bestätigung — auch gegen eine
    // Allow-Regel (mit der NOPASSWD-Regel des erhöhten Dateibrowsers ist das
    // passwortloser Root-Dateizugriff). Dasselbe Muster wie die
    // Secret-Prüfung oben: nur `AutoExec` → `Confirm`, vor der
    // Injection-Prüfung, deren Flag hier nur gelesen wird.
    if matches!(decision, Decision::AutoExec) {
        if let Some(reason) = pseudo_command_for_risk_classification(&action)
            .as_deref()
            .and_then(ssh_manager_core::risk::sftp_server_invocation_reason)
        {
            decision = if session
                .injection_suspected
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                Decision::Confirm {
                    reason: format!(
                        "Möglicher Versuch, Anweisungen über Serverinhalt einzuschleusen, \
                         erkannt; außerdem: {reason} – erfordert Bestätigung"
                    ),
                    code: "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM".to_string(),
                }
            } else {
                Decision::Confirm {
                    reason: format!("{reason} – erfordert immer Bestätigung"),
                    code: "FILTER_SFTP_SERVER_REQUIRES_CONFIRM".to_string(),
                }
            };
        }
    }

    // Spec 0092, A2: Ist die app-weite Einstellung „Bei rotem Risiko immer
    // nachfragen" an, verlangt ein auf EINER der beiden Achsen rot
    // eingestufter Vorschlag immer eine Bestätigung — auch gegen eine
    // Allow-Regel. Reine Eskalation (`AutoExec` → `Confirm`), für Chat UND
    // MCP, weil beide durch diese Funktion laufen.
    //
    // Stelle in der Kette (Spec 0092, §5): NACH Secret-Pfad und
    // `sftp-server`, damit deren genauerer Code erhalten bleibt (A2.4 — die
    // beiden Glieder oben haben `decision` dann schon auf `Confirm` gesetzt,
    // die Bedingung hier greift nicht mehr), und VOR der Injection-Prüfung,
    // damit das Verdachts-Flag hier nur GELESEN und nicht verbraucht wird
    // (A2.3, dasselbe Muster und derselbe Grund wie bei den beiden oben).
    // Ein bereits vorliegendes `Confirm` (z. B. `FILTER_HARD_BLACKLIST`,
    // Spec 0092 §5) bleibt dadurch ebenfalls unberührt, ein `Deny` erst
    // recht.
    if session.red_risk_always_confirm && matches!(decision, Decision::AutoExec) {
        if let Some(reason) = red_risk_confirm_reason(&action, risk_assessment.as_ref()) {
            decision = if session
                .injection_suspected
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                Decision::Confirm {
                    reason: format!(
                        "Möglicher Versuch, Anweisungen über Serverinhalt einzuschleusen, \
                         erkannt; außerdem: {reason} – erfordert Bestätigung"
                    ),
                    code: "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM".to_string(),
                }
            } else {
                Decision::Confirm {
                    reason: format!("{reason} – erfordert immer Bestätigung"),
                    code: "FILTER_RED_RISK_REQUIRES_CONFIRM".to_string(),
                }
            };
        }
    }

    // Unabhängiger Review-Pass (Spec 0039): ersetzt die bisherige SEC-03-
    // Bremse aus Spec 0013. Die ALTE Logik: `round` war ein rein lokaler
    // Schleifenzähler in `run_chat_turn`s `for round in 1..=MAX_AUTO_
    // FOLLOWUP_ROUNDS`-Schleife, kein Feld auf `Session` — jeder Aufruf von
    // `send_chat_message` (= jede neue Nutzer-Nachricht) rief `run_chat_
    // turn` frisch auf, wodurch `round` wieder bei 1 begann. Die alte
    // Eskalation (`round >= 2 && (SuggestCommand | ReadRemoteFile) &&
    // AutoExec -> Confirm`) griff deshalb nur INNERHALB einer einzigen
    // automatischen Fortsetzungskette, nicht über die ganze Sitzung hinweg
    // — ein in Runde 1 eingeschleustes, aber erst bei der NÄCHSTEN
    // Nutzer-Nachricht ausgelöstes Payload traf wieder auf `round == 1` und
    // lief unter normaler, nicht eskalierter Policy (Spec 0039, Abschnitt
    // 1, Punkt 4).
    //
    // `session.untrusted_content_ingested` ist stattdessen session-weit
    // monoton: einmal gesetzt (durch `fence_untrusted`-Inhalt, der in den
    // Kontext gelangt ist — Kommando-Ausgabe, SFTP-Dateiinhalt, Notizen im
    // System-Prompt), bleibt es für den Rest der Sitzung gesetzt,
    // unabhängig von Runden-/Nachrichtengrenzen. Die tatsächliche Schärfe
    // danach ist pro Server über `Session::post_ingest_policy`
    // konfigurierbar (Spec 0039, Abschnitt 5.1), statt global fest, damit
    // Allow-Regeln nicht für jeden Server wertlos werden.
    if session
        .untrusted_content_ingested
        .load(std::sync::atomic::Ordering::SeqCst)
        && matches!(decision, Decision::AutoExec)
    {
        let is_modifying_action = risk_assessment
            .as_ref()
            .is_some_and(|r| r.server_risk != RiskLevel::None);
        let escalate = match session.post_ingest_policy {
            PostIngestPolicy::Strict => true,
            PostIngestPolicy::Balanced => is_modifying_action,
            PostIngestPolicy::Standard => false,
        };
        // `sftp-write`/`ProposeNoteUpdate`/MCP-Herkunft bleiben unabhängig
        // von dieser Stufe eskalationspflichtig (Spec 0039, Abschnitt 5.1,
        // letzter Absatz) — nicht als Sonderfall hier nötig: `evaluate_
        // action` liefert für `WriteRemoteFile`/`ProposeNoteUpdate` nie
        // `AutoExec`, und die MCP-Eskalation unten läuft unabhängig davon,
        // ob dieser Zweig hier schon eskaliert hat.
        if escalate {
            decision = Decision::Confirm {
                reason: "Serverinhalt wurde in dieser Sitzung bereits eingelesen – erfordert \
                         Bestätigung"
                    .to_string(),
                code: "FILTER_POST_INGEST_REQUIRES_CONFIRM".to_string(),
            };
        }
    }

    // Spec 0039, Abschnitt 5.2: die optionale KI-Prüfung auf eingeschleuste
    // Anweisungen (s. `execute_suggested_command`/`execute_read_remote_
    // file`, wo sie tatsächlich läuft) bezieht sich auf "die auf diesem
    // Inhalt basierende Folgeaktion" — deshalb wird das Flag hier beim
    // Eskalieren VERBRAUCHT (auf `false` zurückgesetzt), anders als
    // `untrusted_content_ingested` oben, das für die ganze Sitzung gilt.
    // Nur Eskalation nach oben, wie bei der Risiko-Zweitmeinung aus Spec
    // 0026 (ein "nein"/keine Prüfung macht nichts zusätzlich AutoExec-
    // fähig, das es sonst nicht wäre).
    //
    // Unabhängiger Review-Pass (Spec 0039): `swap` muss auf den
    // `AutoExec`-Zweig BESCHRÄNKT bleiben, statt unbedingt bei jedem
    // Aktionsvorschlag zu laufen — sonst verbraucht z. B. eine an der
    // Hard-Blacklist scheiternde `Deny`-Aktion (die der Payload absichtlich
    // vorschieben kann, etwa `rm -rf /`) das Verdachts-Flag, bevor die
    // eigentlich gemeinte Folgeaktion überhaupt vorgeschlagen wird — diese
    // liefe dann trotz erkanntem Verdacht ungebremst als `AutoExec`.
    if matches!(decision, Decision::AutoExec)
        && session
            .injection_suspected
            .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
        decision = Decision::Confirm {
            reason: "Möglicher Versuch, Anweisungen über Serverinhalt einzuschleusen, erkannt – \
                     erfordert Bestätigung"
                .to_string(),
            code: "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM".to_string(),
        };
    }

    // Spec 0028, Abschnitt 5: ein über MCP (externes Tool) ausgelöster
    // Vorschlag landet **immer** bei einer Bestätigung, unabhängig von
    // einer sonst greifenden Allow-Regel — ein externes Tool ist eine neue
    // Vertrauensgrenze, die strenger behandelt wird als die interne KI
    // (dieselbe Denkweise wie bei SFTP-Schreibzugriffen, Spec 0020,
    // Abschnitt 4.2). Diese Einschränkung ist für die Free-Version fest
    // codiert, keine Einstellung — bewusst als eigener, benannter Schritt
    // statt als verstecktes Sonderverhalten irgendwo in `evaluate_action`.
    if matches!(origin, ActionOrigin::Mcp { .. }) && matches!(decision, Decision::AutoExec) {
        decision = Decision::Confirm {
            reason: "Über MCP (externes Tool) angefragt – erfordert immer Bestätigung".to_string(),
            code: "FILTER_MCP_ORIGIN_REQUIRES_CONFIRM".to_string(),
        };
    }

    // Unabhängiger Review-Pass (Spec 0018): das gespeicherte Sudo-Passwort
    // ist ein Root-Zugangsdatum — dessen Verwendung verdient dieselbe
    // "neue Vertrauensgrenze verlangt immer Bestätigung"-Behandlung wie
    // MCP-Herkunft oder ein SFTP-Schreibzugriff (Spec 0020, Abschnitt 4.2),
    // unabhängig davon, ob eine (oft für den unprivilegierten Fall
    // angelegte) Allow-Regel per Dual-Text-Matching (ADR 0002) zufällig
    // auch die `sudo`-Variante mit abdeckt. Ohne diese Eskalation hätte
    // z. B. eine harmlos gemeinte Regel "systemctl restart *" ein
    // gespeichertes Root-Passwort ohne jede Rückfrage verbraucht.
    let uses_password = uses_stored_sudo_password(session, &action);
    if uses_password && matches!(decision, Decision::AutoExec) {
        decision = Decision::Confirm {
            reason: "Verwendet das hinterlegte Sudo-Passwort – erfordert immer Bestätigung"
                .to_string(),
            code: "FILTER_SUDO_PASSWORD_REQUIRES_CONFIRM".to_string(),
        };
    }

    // Spec 0068, Teil 4 (Review-Fund): nach einer Ablehnung in derselben
    // Antwort nie mehr ohne Rückfrage — nur Eskalation.
    if matches!(decision, Decision::AutoExec)
        && earlier_rejection.load(std::sync::atomic::Ordering::SeqCst)
    {
        decision = Decision::Confirm {
            reason: "Eine vorherige Aktion dieser Antwort wurde abgelehnt – erfordert Bestätigung"
                .to_string(),
            code: "FILTER_EARLIER_ACTION_REJECTED_REQUIRES_CONFIRM".to_string(),
        };
    }

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
        Decision::Confirm { reason, code } => PreparedDecision::Confirm {
            reason,
            code,
            pending: PendingConfirmation::register(session, action_confirmations, action_id),
        },
    };

    let (previous_note_content, target_name) =
        note_target_preview_for_action(&action, session, profile_store).await;
    let (previous_file_content, previous_file_size) =
        previous_file_content_for_action(&action, session).await;

    emit_chat_action_proposed(
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
    );

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
            let second_opinion =
                crate::second_opinion::fetch_second_opinion(provider, &pseudo_command).await;
            let (data_risk, reason) = escalate_data_risk(
                assessment.data_risk,
                assessment.data_risk_reason,
                second_opinion,
            );
            emit_risk_assessment_updated(emitter, session_id, action_id, data_risk, reason);
            // Spec 0092, A3: nur die STUFE festhalten, nicht den Text der
            // Zweitmeinung — s. Kommentar unten am Grundtext.
            second_opinion_data_risk = Some(data_risk);
        }
    }

    // Spec 0092, A3: Die Zweitmeinung hat das Daten-Risiko auf Rot gehoben,
    // nachdem `chat-action-proposed` mit `AutoExec` längst draußen ist. Die
    // Aktion wird ab hier wie jedes andere `Confirm` behandelt — dieselbe
    // Wartezeit, dieselbe Abbruchlogik, derselbe Tab-Indikator, dieselben
    // Ledger-Einträge (A3.1), weil es buchstäblich derselbe
    // `PreparedDecision::Confirm`-Zweig unten ist.
    //
    // Dass `prepared` hier noch `AutoExec` ist, heißt zugleich: Das
    // regelbasierte Rot hat NICHT gegriffen (sonst hätte das Glied oben
    // schon eskaliert) — die Anhebung kommt also tatsächlich von der
    // Zweitmeinung. Nur Eskalation, wie überall: aus `Deny`/`Confirm` wird
    // hier nichts.
    //
    // **Reihenfolge**: `register` VOR dem Ereignis (A3.2) — das Frontend
    // erfährt erst von der wartenden Bestätigung, wenn der Empfänger schon
    // steht, ein sehr schneller Klick geht also nicht ins Leere. Und das
    // Ereignis NACH `risk-assessment-updated` (§5), damit das Badge rot ist,
    // bevor der Dialog erscheint.
    //
    // **A3.3, Stopp hat Vorrang**: hier geprüft, nicht erst unten im
    // `match` — ein während der Zweitmeinung eingetroffener Stopp soll die
    // Aktion überspringen lassen, nicht einen Dialog zeigen. Ohne diese
    // Prüfung eskalierte die Aktion und der `AutoExec`-Stopp-Zweig unten
    // käme nie dran.
    //
    // Restlücke, ehrlich benannt (spec-reviewer-Fund, Runde 1): Ein Stopp,
    // der NACH dieser Zeile eintrifft, sieht einen Dialog für eine gerade
    // gestoppte Aktion — A3.3 verlangt „bekommt keinen Dialog". Das ist eine
    // EIGENE, neue Lücke, nicht die bekannte „Kommando läuft trotz Stopp":
    // Sie ist die sichere Richtung (eine Rückfrage zu viel, nie eine
    // Ausführung zu viel) und lässt sich nicht schließen, solange das
    // Ereignis überhaupt gesendet werden muss, bevor der Nutzer klicken
    // kann. Ausgeführt wird in diesem Rennen nichts ohne Klick.
    let mut prepared = prepared;
    let raised_to_red = second_opinion_data_risk == Some(RiskLevel::Red);
    if session.red_risk_always_confirm
        && matches!(prepared, PreparedDecision::AutoExec)
        && raised_to_red
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
        // 0092, §5) und ist damit sichtbar, ohne persistiert zu werden.
        let reason = "Daten-Risiko rot (KI-Zweitmeinung) – erfordert immer Bestätigung".to_string();
        let code = "FILTER_RED_RISK_REQUIRES_CONFIRM".to_string();
        let pending = PendingConfirmation::register(session, action_confirmations, action_id);
        emit_action_decision_escalated(
            emitter,
            session_id,
            action_id,
            reason.clone(),
            code.clone(),
        );
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
                action,
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
    profile_store
        .get_server(&session.server_id)
        .await
        .map(|s| s.tags)
        .unwrap_or_else(|_| session.tags.clone())
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

/// Unabhängiger Review-Pass (Spec 0020): löst `.`/`..`-Segmente rein
/// lexikalisch auf und kollabiert wiederholte `/`, OHNE das entfernte
/// Dateisystem zu berühren. `globset`-Muster (z. B. `Allow: sftp-read
/// /home/deploy/*`) lassen `*` `/` kreuzen — ohne Normalisierung VOR der
/// Filter-Auswertung hätte ein KI-gelieferter Pfad wie
/// `/home/deploy/../../etc/shadow` dieselbe Allow-Regel getroffen (und
/// `/etc/nginx//secret.conf`/`/etc/nginx/./secret.conf` hätten eine
/// Deny-Regel für `/etc/nginx/secret.conf` umgangen). Wird EINMAL in
/// `handle_action_proposed` angewendet, bevor irgendein Konsument (Filter-
/// Auswertung, Risikoeinschätzung, Vorschau, tatsächliche SFTP-Ausführung)
/// den Pfad sieht, damit alle dieselbe (normalisierte) Zeichenkette
/// verwenden.
fn normalize_remote_path(path: &str) -> String {
    let is_absolute = path.starts_with('/');
    let mut stack: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if matches!(stack.last(), Some(&last) if last != "..") {
                    stack.pop();
                } else if !is_absolute {
                    stack.push("..");
                }
                // Absoluter Pfad: `..` über der Wurzel hinaus wird verworfen
                // (kann nicht höher als `/`).
            }
            other => stack.push(other),
        }
    }
    let joined = stack.join("/");
    if is_absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
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
        AiAction::ProposeNoteUpdate { .. } | AiAction::GenerateDocument { .. } => None,
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
/// als [`DEFAULT_MAX_COMMAND_LENGTH`] **Bytes** ist, und liefert dann
/// `None`/`None` — „kein Risiko erkannt" heißt dort „nicht geprüft". Die
/// Filter-Engine dagegen zählt **Zeichen**; ein Kommando mit Mehrbyte-Zeichen
/// kann deshalb unter ihrer Schranke liegen (also mit Allow-Regel `AutoExec`
/// werden), während der Klassifizierer schon aufgegeben hat. Ohne diesen
/// Zweig griffe das Glied genau dann nicht — und A2 hinge daran, dass
/// `secret_path_read_reason` für Überlänge zufällig selbst eskaliert (mit
/// einem sachlich falschen Code). Dieselbe Schranke und dasselbe `.len()`
/// wie im Klassifizierer, damit genau das Fenster abgedeckt ist, in dem er
/// aussteigt.
fn red_risk_confirm_reason(
    action: &AiAction,
    assessment: Option<&RiskAssessment>,
) -> Option<String> {
    if let Some(pseudo_command) = pseudo_command_for_risk_classification(action) {
        if pseudo_command.len() > DEFAULT_MAX_COMMAND_LENGTH {
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
            },
        },
        persist,
    )
    .await;
    false
}

fn describe_rejected_action(action: &AiAction) -> String {
    match action {
        AiAction::SuggestCommand { command } => command.clone(),
        AiAction::ReadRemoteFile { path } => sftp_read_pseudo_command(path),
        AiAction::WriteRemoteFile { path, .. } => sftp_write_pseudo_command(path),
        AiAction::ProposeNoteUpdate { target, .. } => {
            let target_label = match target {
                NoteTargetSelector::CurrentServer => "aktueller Server",
                NoteTargetSelector::CurrentServerGroup => "aktuelle Servergruppe",
            };
            format!("update-note ({target_label})")
        }
        AiAction::GenerateDocument { .. } => unreachable!(
            "GenerateDocument durchläuft nie evaluate_action/einen Bestätigungsdialog, s. dort"
        ),
    }
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
) -> EvaluationTrace {
    match action {
        AiAction::SuggestCommand { command } => {
            let tags = tags_for_session(session, profile_store).await;
            let ctx = EvalContext {
                server_id: session.server_id,
                tags,
            };
            session
                .filter_engine
                .evaluate_explained(command, &ctx)
                .await
        }
        AiAction::ProposeNoteUpdate { .. } => EvaluationTrace {
            decision: Decision::Confirm {
                reason: "Notiz-Aktualisierungen erfordern immer eine manuelle Bestätigung"
                    .to_string(),
                code: "FILTER_NOTE_UPDATE_REQUIRES_CONFIRM".to_string(),
            },
            matched_rule: None,
            matched_rule_origin: None,
            matched_hard_blacklist_entry: None,
            sub_command_traces: Vec::new(),
        },
        AiAction::GenerateDocument { .. } => unreachable!(
            "GenerateDocument wird bereits in run_one_round abgefangen \
             (Spec 0012: kein Filter-Engine-/Bestätigungspfad) und erreicht \
             evaluate_action nie"
        ),
        AiAction::ReadRemoteFile { path } => {
            let tags = tags_for_session(session, profile_store).await;
            let ctx = EvalContext {
                server_id: session.server_id,
                tags,
            };
            session
                .filter_engine
                .evaluate_explained(&sftp_read_pseudo_command(path), &ctx)
                .await
        }
        AiAction::WriteRemoteFile { path, .. } => {
            let tags = tags_for_session(session, profile_store).await;
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
            trace
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
        ActionUserDecision::Deny => {
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
                action,
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

                    let tags = profile_store
                        .get_server(&session.server_id)
                        .await
                        .map(|s| s.tags)
                        .unwrap_or_else(|_| session.tags.clone());
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
                AiAction::GenerateDocument { .. } => unreachable!(
                    "GenerateDocument braucht nie eine Bestätigung, s. evaluate_action"
                ),
            };
            execute_action(
                session,
                session_id,
                action_id,
                effective_action,
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
    action: AiAction,
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
    match action {
        AiAction::SuggestCommand { command } => {
            execute_suggested_command(
                session,
                session_id,
                action_id,
                command,
                emitter,
                persist,
                ledger_source,
            )
            .await
        }
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
        AiAction::ReadRemoteFile { path } => {
            execute_read_remote_file(session, session_id, action_id, path, emitter, persist).await
        }
        AiAction::WriteRemoteFile { path, content } => {
            execute_write_remote_file(
                session,
                session_id,
                action_id,
                path,
                content,
                emitter,
                persist,
                sudo_fallback_announced,
            )
            .await
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

async fn execute_suggested_command(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    command: String,
    emitter: &dyn EventEmitter,
    persist: bool,
    ledger_source: LedgerSource,
) -> bool {
    // Spec 0018, Abschnitt 5: nur umschreiben/Stdin füttern, wenn tatsächlich
    // ein Passwort hinterlegt ist — sonst unverändertes Verhalten
    // (`execute()` wie bisher, scheitert wie gewohnt ohne TTY).
    let effective_command = session
        .sudo_password
        .as_ref()
        .and_then(|_| command_with_stdin_password_flag(&command));

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

    let raw_outcome = {
        let mut transport = session.transport.lock().await;
        match (&effective_command, &session.sudo_password) {
            (Some(rewritten), Some(password)) => {
                use secrecy::ExposeSecret;
                let mut stdin = password.expose_secret().as_bytes().to_vec();
                stdin.push(b'\n');
                transport
                    .execute_with_stdin_cancellable(rewritten, &stdin, cancel_rx)
                    .await
            }
            _ => transport.execute_cancellable(&command, cancel_rx).await,
        }
    };
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
            check_for_injected_instructions(session, session_id, emitter, &combined_output).await;
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

/// Spec 0016, Abschnitt 4, Punkt 5: ab dieser Zeichenlänge wird geloggter
/// Kommando-Output gekürzt (mit Hinweis), statt den vollen — ggf. sehr
/// langen — Output in die Log-Datei zu schreiben. Der konfigurierbare
/// Knopf im Sinne der Spec ist diese Konstante selbst (analog zu
/// `core::filter::engine::DEFAULT_MAX_COMMAND_LENGTH`) — kein zur Laufzeit
/// änderbarer Wert, da dafür aktuell keine Einstellungs-UI existiert und
/// die Spec keine verlangt.
const MAX_LOGGED_OUTPUT_LEN: usize = 4096;

fn truncate_for_log(text: &str) -> String {
    if text.chars().count() <= MAX_LOGGED_OUTPUT_LEN {
        return text.to_string();
    }
    let truncated: String = text.chars().take(MAX_LOGGED_OUTPUT_LEN).collect();
    format!("{truncated}\n… (gekürzt, voller Output nicht geloggt)")
}

/// Spec 0016, Abschnitt 4, Punkt 5: Kommando, Exit-Code, Output-Länge.
/// Nimmt bewusst den bereits **redigierten** Output entgegen, nie den
/// rohen — Logs sind kein Schlupfloch für Secrets, die die Redaction sonst
/// unterdrückt (Spec 0016, Abschnitt 4, Punkt 1 — "dieselbe Redaction-Regel
/// gilt für Logs wie für den tatsächlichen API-Request").
///
/// Spec 0094, A1.4/A2: Redigiert reicht nicht — der Redactor kennt nicht
/// jede Schreibweise eines Passwort-Arguments (gemessen, §1.3), und
/// `stdout`/`stderr` können beliebigen Server-Inhalt tragen. Ab `info`
/// bleiben deshalb nur Zahlen: `session_id`, Exit-Code, Ausgabelängen,
/// Kommandolänge. Kommando und Ausgaben stehen auf `debug`, dort
/// unverändert redigiert und auf `MAX_LOGGED_OUTPUT_LEN` gekürzt.
fn log_command_execution(session_id: SessionId, command: &str, redacted_output: &CommandOutput) {
    let stdout = String::from_utf8_lossy(&redacted_output.stdout);
    let stderr = String::from_utf8_lossy(&redacted_output.stderr);
    tracing::info!(
        session_id = %session_id,
        command_len = command.chars().count(),
        exit_code = ?redacted_output.exit_code,
        stdout_len = stdout.len(),
        stderr_len = stderr.len(),
        "ssh command executed",
    );
    // A2 verlangt, dass der Inhalt dieser Zeile „vorher durch den Redactor
    // läuft". Beides ist an der einzigen Aufrufstelle bereits redigiert
    // (`session.redactor`); hier läuft trotzdem noch einmal der
    // prozessweite Redactor darüber. Nicht, weil die Aufrufstelle
    // unzuverlässig wäre, sondern damit die Zusage dieser Funktion nicht an
    // einer Bedingung hängt, die ein künftiger zweiter Aufrufer übersehen
    // kann — Redaction ist idempotent, der Preis also nur Rechenzeit, und
    // die fällt nur an, wenn `debug` überhaupt aufgezeichnet wird.
    //
    // **Reihenfolge**: redigieren, dann kürzen — nie umgekehrt.
    // spec-reviewer-Fund (Runde 1): hier stand `redact_text(&truncate_for_
    // log(...))`. Genau die Reihenfolge, die die Spec unter
    // „Angriffsrichtungen" als Fehler benennt: die Kürzung bei Zeichen 4096
    // kann ein Secret-Muster entzweischneiden, sodass danach kein Muster
    // mehr greift und der Anfang des Geheimnisses im Klartext stehen bleibt.
    // Am heutigen Aufrufer folgenlos (der redigiert den vollen Text schon
    // vorher) — aber der zweite Durchlauf hier existiert gerade für den
    // Fall, dass ein künftiger Aufrufer das nicht tut, und für den war die
    // alte Reihenfolge falsch.
    tracing::debug!(
        session_id = %session_id,
        exit_code = ?redacted_output.exit_code,
        command = %ssh_manager_core::ai::default_log_redactor().redact_text(command),
        stdout = %truncate_for_log(
            &ssh_manager_core::ai::default_log_redactor().redact_text(&stdout),
        ),
        stderr = %truncate_for_log(
            &ssh_manager_core::ai::default_log_redactor().redact_text(&stderr),
        ),
        "ssh command executed (command and output)",
    );
}

/// Spec 0094, A1.4/A1.7: `command` war hier schon redigiert, `error = %err`
/// nicht — und `SshError`s `Display` gibt bei `ConnectionFailed`/
/// `ChannelError`/`CredentialResolutionFailed` freien Text der darunter
/// liegenden Bibliothek wieder, der das Kommando enthalten kann. Ab `warn`
/// bleiben Kommandolänge und `code()`.
fn log_command_execution_failed(session_id: SessionId, command: &str, err: &SshError) {
    tracing::warn!(
        session_id = %session_id,
        command_len = command.chars().count(),
        code = err.code(),
        "ssh command execution failed",
    );
    // `default_log_redactor()` im Feldausdruck, nicht davor: so wird der
    // `OnceLock` (und damit das Übersetzen aller eingebauten Muster) im
    // Standardbetrieb ohne `debug` gar nicht erst angefasst
    // (spec-reviewer-Fund, Runde 1).
    tracing::debug!(
        session_id = %session_id,
        code = err.code(),
        command = %ssh_manager_core::ai::default_log_redactor().redact_text(command),
        error = %ssh_manager_core::ai::default_log_redactor().redact_text(&err.to_string()),
        "ssh command execution failed (command and error)",
    );
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
    content: &str,
) {
    let Some(provider) = session.injection_check_provider.as_deref() else {
        return;
    };
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
    if let Some((true, _reason)) =
        crate::second_opinion::fetch_injection_check(provider, content).await
    {
        session
            .injection_suspected
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
