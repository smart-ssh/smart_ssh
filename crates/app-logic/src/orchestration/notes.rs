//! Notiz-Aktualisierung, Auto-Titel und Notiz-Kürzung (Spec 0003, 0010,
//! 0012, 0034, 0057, 0058) — s. Moduldoc in `orchestration.rs` für den
//! vollständigen Kontext (Spec 0083: reine Verschiebung aus
//! `orchestration.rs`, keine Verhaltensänderung).

use uuid::Uuid;

use futures::StreamExt;

use ssh_manager_core::ai::{
    fence_untrusted, ActionSchema, AiEvent, AiProvider, ChatMessage, MessageContent,
    OutputRedactor, Role, SessionContext, UntrustedKind,
};
use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};
use ssh_manager_core::profiles::{
    AiAction, NoteEditor, NoteTarget, NoteTargetSelector, ProfileStore,
};
use ssh_manager_core::shared::ServerId;

use crate::confirmation::ConfirmationRegistry;
use crate::dto::{ActionOrigin, ActionUserDecision};
use crate::events::{
    emit_chat_action_result, emit_chat_document_generated, emit_note_shrink_failed,
    emit_note_shrink_succeeded, emit_note_shrink_suggested, emit_note_update_suggested,
    ActionResultPayload, EventEmitter,
};
use crate::session::Session;
use crate::state::{ActionId, SessionId};

use super::action_exec::{emit_action_error, handle_action_proposed};
use super::chat_turn::{
    push_history, push_history_scoped, reapply_redaction_for_send, wait_for_ai_request_slot,
    wait_for_rate_limit_budget, write_ledger_entry, PENDING_ACTION_CONFIRM_TIMEOUT,
    SIDE_CALL_MAX_TOKENS,
};

#[cfg(test)]
mod tests;

/// Spec 0016, Abschnitt 6: löst den von der KI gewählten
/// [`NoteTargetSelector`] in die tatsächliche `ServerId`/`GroupId` auf — nie
/// eine von der KI selbst gelieferte ID (das war die Ursache des `target_id
/// ist keine gültige UUID`-Bugfalls). Für `CurrentServerGroup` ohne
/// zugeordnete Gruppe gibt es keine sinnvolle Ziel-ID; das ist ein Fehler
/// (an den Nutzer über `chat-error` zurückgemeldet), kein stiller Fallback
/// auf den Server.
///
/// Nimmt bewusst `server_id: ServerId` statt `session: &Session` entgegen
/// (spec-reviewer-Nachtrag/Etappe 4, Spec 0057 §4.2): der einzige Wert, den
/// diese Funktion je aus einer `Session` gelesen hat, war `session.
/// server_id` — der Sitzungsende-Kürzungs-Vorschlag (`execute_note_shrink_
/// request`) hat aber strukturell KEINE lebende `Session` mehr (die
/// auslösende Session-`disconnect()`-Hintergrund-Aufgabe ist zu dem
/// Zeitpunkt, an dem der Nutzer "Ja, zusammenfassen" anklickt, typischerweise
/// längst beendet und gedroppt), kennt aber die `ServerId` direkt. Beide
/// bestehenden Aufrufer (`note_target_preview_for_action`/`execute_note_
/// update`) übergeben weiterhin `session.server_id` — reines Signatur-
/// Downcasting, keine Verhaltensänderung für sie.
async fn resolve_note_target(
    selector: NoteTargetSelector,
    server_id: ServerId,
    profile_store: &dyn ProfileStore,
) -> Result<NoteTarget, String> {
    match selector {
        NoteTargetSelector::CurrentServer => Ok(NoteTarget::Server(server_id)),
        NoteTargetSelector::CurrentServerGroup => {
            let server = profile_store
                .get_server(&server_id)
                .await
                .map_err(|err| format!("Server nicht gefunden: {err}"))?;
            let group_id = server.group_id.ok_or_else(|| {
                "Server ist keiner Gruppe zugeordnet — Notiz kann nicht für die Gruppe \
                 aktualisiert werden"
                    .to_string()
            })?;
            Ok(NoteTarget::Group(group_id))
        }
    }
}

/// Spec 0019, Abschnitt 3 / Spec 0023, Abschnitt 3: aktueller Inhalt des
/// aufgelösten Ziels (für die Diff-Vorschau, Spec 0003 Abschnitt 5.2) sowie
/// dessen Name (Server- oder Gruppenname, für die im Frontend immer
/// sichtbare Ziel-Kennzeichnung — Spec 0023: "Der Nutzer muss immer
/// eindeutig erkennen können, worauf sich eine Bestätigung bezieht",
/// unabhängig davon, welcher Server/Tab gerade im Frontend als "aktuell"
/// gilt). `(None, None)` für alle anderen Aktionstypen sowie wenn die
/// Zielauflösung fehlschlägt (z. B. Server inzwischen gelöscht) — dann
/// zeigt das Frontend den neuen Inhalt ohne Diff-Hervorhebung bzw. ohne
/// Zielnamen, kein Fehler (bewusst `Option<String>` statt eines nicht
/// nullbaren Strings — dieselbe Best-Effort-Behandlung wie beim bisherigen
/// `previous_note_content` an derselben Stelle, nicht "targetName: string"
/// aus der Spec-Skizze wörtlich übernommen).
pub(crate) async fn note_target_preview_for_action(
    action: &AiAction,
    session: &Session,
    profile_store: &dyn ProfileStore,
) -> (Option<String>, Option<String>) {
    let AiAction::ProposeNoteUpdate { target, .. } = action else {
        return (None, None);
    };
    let Ok(resolved) = resolve_note_target(*target, session.server_id, profile_store).await else {
        return (None, None);
    };
    match resolved {
        NoteTarget::Server(id) => match profile_store.get_server(&id).await {
            Ok(server) => (Some(server.notes), Some(server.name)),
            Err(_) => (None, None),
        },
        NoteTarget::Group(id) => match profile_store.get_group(&id).await {
            Ok(group) => (Some(group.notes), Some(group.name)),
            Err(_) => (None, None),
        },
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_note_update(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    target_selector: NoteTargetSelector,
    new_content: String,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    persist: bool,
) -> bool {
    let target = match resolve_note_target(target_selector, session.server_id, profile_store).await
    {
        Ok(target) => target,
        Err(reason) => {
            tracing::warn!(session_id = %session_id, reason, "note target resolution failed");
            return emit_action_error(
                session,
                emitter,
                session_id,
                format!("Notiz konnte nicht aktualisiert werden: {reason}"),
                None,
                persist,
            )
            .await;
        }
    };

    match persist_note_revision(
        profile_store,
        target,
        new_content,
        NoteEditor::Ai {
            provider: session.ai_provider_label.clone(),
            model: session.ai_model.clone(),
        },
    )
    .await
    {
        Ok(summary) => {
            emit_chat_action_result(
                emitter,
                session_id,
                action_id,
                ActionResultPayload::NoteUpdate {
                    summary: summary.clone(),
                },
            );
            push_history_scoped(
                session,
                ChatMessage {
                    role: Role::ActionResult,
                    content: MessageContent::Text(summary),
                },
                persist,
            )
            .await;
            true
        }
        Err(err) => {
            emit_action_error(
                session,
                emitter,
                session_id,
                format!("Notiz konnte nicht aktualisiert werden: {err}"),
                None,
                persist,
            )
            .await
        }
    }
}

fn note_update_summary(target: NoteTarget) -> String {
    match target {
        NoteTarget::Server(id) => format!("Notiz für Server {} aktualisiert.", id.0),
        NoteTarget::Group(id) => format!("Notiz für Gruppe {} aktualisiert.", id.0),
    }
}

/// spec-reviewer-Vorgriff (Etappe 4, Spec 0057 §4.2): der reine DB-
/// Schreibpfad aus `execute_note_update` herausgelöst — `execute_note_
/// shrink_request` (Sitzungsende-Kürzungs-Vorschlag) braucht exakt dieselbe
/// Persistenz (`record_revision` + `ProfileStore::record_note_revision`),
/// hat aber KEINE lebende `Session`, an die ein `chat-action-result`-Event
/// oder ein `push_history_scoped`-Aufruf gebunden werden könnte (s. Doc-
/// Kommentar dort) — beide Aufrufer teilen sich deshalb nur diesen
/// gemeinsamen Kern, jeder behält seine eigenen, session-abhängigen bzw.
/// -unabhängigen Nebenwirkungen um den Aufruf herum.
async fn persist_note_revision(
    profile_store: &dyn ProfileStore,
    target: NoteTarget,
    new_content: String,
    editor: NoteEditor,
) -> Result<String, String> {
    let revision = ssh_manager_core::profiles::record_revision(target, new_content, editor);
    profile_store
        .record_note_revision(&revision)
        .await
        .map(|()| note_update_summary(target))
        .map_err(|err| err.to_string())
}

/// Spec 0012, Abschnitt 2/3: `GenerateDocument` erzeugt reinen lokalen
/// Inhalt — kein Filter-Engine-Aufruf, kein Bestätigungsdialog, nichts wird
/// automatisch geschrieben. Zählt deshalb auch **nicht** als "ausgeführte
/// Aktion" für die automatische Folgerunde (ADR 0014, `run_chat_turn`s
/// Moduldoc): anders als ein Kommando-Ergebnis gibt es hier kein Ergebnis,
/// über das die KI in einer weiteren Runde noch nachdenken müsste — das
/// Dokument selbst *ist* bereits die vollständige Antwort auf die
/// Nutzeranfrage.
pub(crate) async fn handle_document_generated(
    session: &Session,
    session_id: SessionId,
    title: String,
    content_markdown: String,
    emitter: &dyn EventEmitter,
) {
    let action_id: ActionId = Uuid::new_v4();
    emit_chat_document_generated(
        emitter,
        session_id,
        action_id,
        title,
        content_markdown.clone(),
    );
    // Spec 0057, §1.1, vierter Punkt + Spec 0012, Abschnitt 5: dieselbe
    // Gleichsetzung mit normalem Chat-Text wie unten bei `push_history`—
    // gilt genauso für den Ledger-Eintrag, s. `flush_text_buffer`s
    // identischer Kommentar.
    write_ledger_entry(
        session,
        LedgerSource::Ai,
        LedgerEntryContent::AiMessage {
            text: content_markdown.clone(),
        },
    )
    .await;
    // Spec 0012, Abschnitt 5: "wird als Teil der Assistant-Nachricht in
    // context.history übernommen (wie ein normaler Chat-Text)" — kein
    // Sonderfall gegenüber `flush_text_buffer` oben, derselbe
    // `Role::Assistant`/`MessageContent::Text`.
    push_history(
        session,
        ChatMessage {
            role: Role::Assistant,
            content: MessageContent::Text(content_markdown),
        },
    )
    .await;
}

/// Spec 0010, Abschnitt 2, Punkt 2 — nahezu wörtlich aus der Spec-Skizze
/// übernommen (dort bereits als "sinngemäß"-Formulierung vorgegeben), daher
/// keine eigene Design-Entscheidung/ADR nötig für den genauen Wortlaut.
/// Wird nur dem für diesen einen Aufruf **geklonten** `SessionContext`
/// hinzugefügt, nie der echten `session.context` — Spec: "kein sichtbarer
/// Chat-Eintrag".
/// Spec 0034, Abschnitt 7, letzter Satz vor den Punkten: reine Textanfrage,
/// kein Tool-Schema — die KI kann in diesem Aufruf keine Aktion vorschlagen.
const TITLE_GENERATION_INSTRUCTION: &str = "Die Sitzung wird jetzt beendet. Fasse den Zweck \
     dieser Unterhaltung in 2-4 Worten zusammen, als kurzer Titel zum Wiedererkennen. \
     Antworte NUR mit dem Titel selbst — keine Anführungszeichen, keine Erklärung, kein \
     Satzzeichen am Ende.";

/// Spec 0034, Abschnitt 7, letzter Punkt vor "`rename_chat_session`":
/// defensive Obergrenze für den von der KI gelieferten Titel-Text — ein
/// Provider, der die Instruktion ignoriert und einen ganzen Absatz
/// zurückgibt, darf keinen unbrauchbar langen "Titel" erzeugen.
const MAX_GENERATED_TITLE_LENGTH: usize = 60;

/// Spec 0034, Abschnitt 7: automatische Kurztitel-Generierung beim
/// Verbindungsende. Wie `suggest_note_update_on_disconnect` (s. dortiger
/// Doc-Kommentar zu `session`s Gültigkeit nach dem Entfernen aus
/// `AppState.sessions`) — dieselbe "beim Trennen"-Grundvoraussetzung, aber
/// unabhängige Auslösebedingung: "mindestens eine Nutzer-Nachricht ... und
/// noch keinen Titel". Kein-op, wenn diese Sitzung gar nicht persistiert
/// ist (`chat_session_store`/`chat_session_id` beide `Some` nötig, s.
/// `Session`-Doc-Kommentar) — ohne `chat_sessions`-Zeile gibt es nichts,
/// dem ein Titel zugeordnet werden könnte.
#[tracing::instrument(skip_all)]
pub async fn generate_session_title_on_disconnect(
    session: &Session,
    session_id: SessionId,
    emitter: &dyn EventEmitter,
) {
    let Some(store) = &session.chat_session_store else {
        return;
    };
    let Some(chat_session_id) = *session.chat_session_id.lock().await else {
        return;
    };

    let has_user_message = session
        .context
        .lock()
        .await
        .history
        .iter()
        .any(|m| matches!(m.role, Role::User));
    if !has_user_message {
        return;
    }

    let mut request_context = session.context.lock().await.clone();
    // s. identischer Kommentar in `run_one_round` — geklont, um den
    // `MutexGuard` nicht über den potenziell langen `compact_for_send`-
    // Aufruf hinweg zu halten.
    let system_context_parts = session.system_context_parts.lock().await.clone();
    request_context = crate::compaction::compact_for_send(
        session,
        session_id,
        emitter,
        request_context,
        &system_context_parts,
        session.model_context_window_tokens,
    )
    .await;
    // Spec 0040, Abschnitt 5: s. Kommentar an der anderen `send()`-Stelle in
    // `run_one_round`.
    request_context.history =
        reapply_redaction_for_send(request_context.history, session.redactor.as_ref());
    request_context.history.push(ChatMessage {
        role: Role::User,
        content: MessageContent::Text(TITLE_GENERATION_INSTRUCTION.to_string()),
    });
    // Kein Tool-Schema anbieten (Spec 0034, Abschnitt 7: "hier aber ohne
    // Tool-Schema, reine Textanfrage") — analog zu `available_actions:
    // vec![ActionSchema::propose_note_update()]` beim Notiz-Vorschlag
    // unten, hier aber gar keine Aktion, nur Text.
    request_context.available_actions = Vec::new();
    // Spec 0065, Teil 1: Nebenaufruf — s. `SIDE_CALL_MAX_TOKENS`-Kommentar.
    // `request_context` ist ein Klon von `session.context`, das selbst
    // schon `max_tokens_hint: None` trägt (Haupt-Chat-Default) — hier
    // ausdrücklich überschrieben, sonst würde diese Kurztitel-Anfrage den
    // vollen Haupt-Chat-Default erben.
    request_context.max_tokens_hint = Some(SIDE_CALL_MAX_TOKENS);

    wait_for_ai_request_slot(session).await;
    wait_for_rate_limit_budget(
        &session.ai_provider_budget,
        crate::compaction::estimate_request_tokens(&request_context),
        emitter,
        session_id,
    )
    .await;
    let mut stream = session.ai_provider.send(request_context);
    let mut text_buffer = String::new();
    while let Some(event) = stream.next().await {
        match event {
            AiEvent::TextDelta(delta) => text_buffer.push_str(&delta),
            // Kein `ActionProposed` erwartet (keine Schemas angeboten),
            // aber defensiv wie beim Notiz-Vorschlag: einfach ignorieren
            // statt eine Aktion auszuführen, die niemand angefordert hat.
            AiEvent::ActionProposed(_) => {}
            // Spec 0065, Teil 2: kein „Weiter"-Hinweis für diesen
            // Nebenaufruf — ein abgeschnittener Titel wird einfach genau
            // wie ein sonst leerer/fehlerhafter Titel behandelt (s.
            // `sanitize_generated_title` unten).
            AiEvent::Done | AiEvent::Error(_) | AiEvent::TextTruncated => break,
        }
    }

    let Some(title) = sanitize_generated_title(&text_buffer) else {
        return;
    };
    if let Err(err) = store.set_title_if_absent(chat_session_id, &title).await {
        tracing::warn!(error = %err, "chat session auto-titling failed");
    }
}

/// Trimmt Whitespace und ein ggf. von der KI trotz Instruktion hinzugefügtes
/// umschließendes Anführungszeichen-Paar, kürzt defensiv auf
/// [`MAX_GENERATED_TITLE_LENGTH`] Zeichen, und liefert `None` für einen
/// (nach dem Trimmen) leeren Text — kein leerer/bedeutungsloser Titel wird
/// gespeichert.
fn sanitize_generated_title(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_GENERATED_TITLE_LENGTH).collect())
}

const DISCONNECT_COMPLETION_INSTRUCTION: &str = "Die Sitzung wird jetzt beendet. Gibt es aus \
     dieser Sitzung Informationen, die für künftige Sitzungen an diesem Server als Notiz \
     festgehalten werden sollten (z. B. neue Pfade, installierte Versionen, getroffene \
     Entscheidungen)? Schlage eine Notiz-Aktualisierung nur bei echtem Mehrwert vor — keine \
     Wiederholung bereits bestehender Notizinhalte.";

/// Spec 0010: nach `disconnect()` aufgerufen (`crate::commands::disconnect`,
/// als eigener `tokio::spawn`-Task — läuft nicht blockierend für den
/// eigentlichen Trennvorgang, der zu diesem Zeitpunkt bereits abgeschlossen
/// ist). `session` ist zu diesem Zeitpunkt bereits aus `AppState.sessions`
/// entfernt, aber über den `Arc`, den `disconnect()` vor dem Entfernen
/// geklont hat, weiterhin gültig — `SshTransport`/Terminal werden hier
/// nicht mehr angefasst, nur `session.context`/`session.ai_provider`.
///
/// Rückgabewert (Etappe 4, Spec 0057 §4.2, Zusammenspiel-Design — s.
/// `should_suggest_note_shrink`-Doc-Kommentar): `true` genau dann, wenn
/// tatsächlich ein `note-update-suggested`-Vorschlag emittiert wurde
/// (unabhängig davon, ob der Nutzer ihn später annimmt/ablehnt/den Dialog
/// ignoriert) — `commands::disconnect` nutzt das, um den neuen
/// Kürzungs-Vorschlag NUR zu zeigen, wenn dieser hier gerade KEINEN
/// Update-Vorschlag gemacht hat (nie zwei konkurrierende Notiz-Dialoge am
/// selben Verbindungsende).
#[tracing::instrument(skip_all, fields(session_id = %session_id))]
pub async fn suggest_note_update_on_disconnect(
    session: &Session,
    session_id: SessionId,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
) -> bool {
    // Spec 0010, Abschnitt 3: "mindestens ein erfolgreich ausgeführtes
    // Kommando in der Session, sonst wird der KI-Aufruf gar nicht erst
    // gemacht". Als "erfolgreich ausgeführt" zählt hier jedes Kommando, für
    // das `SshTransport::execute()` tatsächlich ein Ergebnis geliefert hat
    // (unabhängig vom Exit-Code des Kommandos selbst) — genau die
    // Kommandos, die als `MessageContent::CommandResult` in der Historie
    // stehen (s. `execute_suggested_command`). Auch ein *fehlgeschlagenes*
    // Kommando (Exit-Code ≠ 0) ist potenziell notizwürdig ("Pfad X
    // existiert nicht, Y verwenden"); nur eine Sitzung ganz ohne
    // Ausführungsversuch hat garantiert nichts beizutragen — das deckt sich
    // mit der in der Spec genannten Begründung ("ein Vorschlag, der ohnehin
    // nichts liefern würde").
    let has_executed_command = session
        .context
        .lock()
        .await
        .history
        .iter()
        .any(|m| matches!(m.content, MessageContent::CommandResult { .. }));
    if !has_executed_command {
        return false;
    }

    let mut request_context = session.context.lock().await.clone();
    // s. identischer Kommentar in `run_one_round` — geklont statt den
    // `MutexGuard` über den potenziell langen `compact_for_send`-Aufruf
    // hinweg zu halten.
    let system_context_parts = session.system_context_parts.lock().await.clone();
    let uncompacted_system_context = request_context.system_context.clone();
    // Spec 0057, §3 / Spec 0040, Abschnitt 5: "vor jedem
    // `AiProvider::send()`-Aufruf" — s. Kommentar an der anderen
    // `send()`-Stelle in `run_one_round`.
    request_context = crate::compaction::compact_for_send(
        session,
        session_id,
        emitter,
        request_context,
        &system_context_parts,
        session.model_context_window_tokens,
    )
    .await;
    // spec-reviewer-Fund (Review dieses Schritts): Kompaktierung kann die
    // im System-Prompt gesendete Notiz-Fassung kürzen (Spec 0057, §3.2
    // Schritt 3/§4.1) — genau dieser eine Aufruf bittet die KI aber um eine
    // VOLLSTÄNDIGE Ersatznotiz (`AiAction::ProposeNoteUpdate::new_content`
    // ersetzt die gespeicherte Notiz komplett, s. `execute_note_update`).
    // Sähe die KI nur die gekürzte Fassung, könnte ihr Vorschlag den
    // weggekürzten Teil verlieren — ein versehentlicher Notiz-Schrumpf, den
    // Spec 0057 §4.2 bewusst nur über einen eigenen, nutzergeführten Dialog
    // vorsieht, nicht als Nebeneffekt der stillen Sende-Kompaktierung. Statt
    // dieses Randfalls einfach hinzunehmen: den Vorschlag für diesen einen
    // (seltenen — nur bei bereits sehr voller Sitzung) Aufruf überspringen.
    if request_context.system_context != uncompacted_system_context {
        tracing::info!(
            "skipping note-update suggestion on disconnect: context compaction shortened the \
             note for this request, a proposal based on it could drop content"
        );
        return false;
    }
    request_context.history =
        reapply_redaction_for_send(request_context.history, session.redactor.as_ref());
    request_context.history.push(ChatMessage {
        role: Role::User,
        content: MessageContent::Text(DISCONNECT_COMPLETION_INSTRUCTION.to_string()),
    });
    // Spec 0010, Abschnitt 2, Punkt 3: keine `SuggestCommand`-Schemas
    // anbieten — die KI kann in diesem Aufruf gar nicht erst ein Kommando
    // vorschlagen.
    request_context.available_actions = vec![ActionSchema::propose_note_update()];
    // Spec 0065, Teil 1: Nebenaufruf — s. `SIDE_CALL_MAX_TOKENS`-Kommentar
    // (analog zur Auto-Titel-Stelle oben).
    request_context.max_tokens_hint = Some(SIDE_CALL_MAX_TOKENS);

    wait_for_ai_request_slot(session).await;
    wait_for_rate_limit_budget(
        &session.ai_provider_budget,
        crate::compaction::estimate_request_tokens(&request_context),
        emitter,
        session_id,
    )
    .await;
    let mut stream = session.ai_provider.send(request_context);
    let mut proposed: Option<AiAction> = None;
    while let Some(event) = stream.next().await {
        match event {
            AiEvent::ActionProposed(action) => {
                // Defensiv: `available_actions` lässt der KI gar keine
                // andere Wahl, aber ein Mock/fehlerhafter Provider könnte
                // trotzdem etwas anderes liefern — dann zählt das wie "kein
                // Vorschlag" (Spec Abschnitt 2, Punkt 4), statt eine
                // `AiAction`, die wir gar nicht ausführen könnten,
                // weiterzureichen.
                if matches!(action, AiAction::ProposeNoteUpdate { .. }) {
                    proposed = Some(action);
                }
                break;
            }
            // Spec Abschnitt 2, Punkt 4: kein `ActionProposed` oder ein
            // Fehler -> kommentarlos beenden, kein `chat-error`. Der
            // Nutzer hat den Screen evtl. längst verlassen — eine
            // Fehlermeldung für ein rein optionales Extra wäre hier
            // aufdringlicher als hilfreich.
            // Spec 0065, Teil 2: dieser Nebenaufruf zeigt keinen „Weiter"-
            // Hinweis an (kein sichtbarer Chat-Turn) — ein abgeschnittener
            // Vorschlag ist hier gleichbedeutend mit "kein Vorschlag".
            AiEvent::Done | AiEvent::Error(_) | AiEvent::TextTruncated => break,
            AiEvent::TextDelta(_) => {}
        }
    }

    let Some(AiAction::ProposeNoteUpdate {
        target,
        new_content,
    }) = proposed
    else {
        return false;
    };

    let action_id: ActionId = Uuid::new_v4();
    let proposed_action = AiAction::ProposeNoteUpdate {
        target,
        new_content: new_content.clone(),
    };
    // Spec 0019, Abschnitt 3 / Spec 0023, Abschnitt 3: dieselbe Diff-/
    // Ziel-Grundlage wie beim regulären In-Chat-Vorschlag
    // (`handle_action_proposed`) — hier besonders wichtig, da diese
    // Benachrichtigung bewusst app-weit statt tab-gebunden ist (Spec 0010,
    // Abschnitt 2, Punkt 6) und der Nutzer beim Empfang ggf. einen ganz
    // anderen Server/Tab offen hat.
    let (previous_note_content, target_name) =
        note_target_preview_for_action(&proposed_action, session, profile_store).await;
    emit_note_update_suggested(
        emitter,
        session_id,
        action_id,
        proposed_action,
        previous_note_content,
        target_name,
    );

    let rx = action_confirmations.register(action_id);
    let user_decision = match tokio::time::timeout(PENDING_ACTION_CONFIRM_TIMEOUT, rx).await {
        Ok(Ok(decision)) => decision,
        Ok(Err(_)) => {
            // Sender gedroppt (z. B. App wurde beendet, bevor der Nutzer
            // reagiert hat) — kein Absturz, einfach nichts weiter tun. Der
            // Vorschlag wurde trotzdem emittiert (s. Rückgabewert-Doc oben).
            return true;
        }
        Err(_elapsed) => {
            // Spec 0046, Fund 4 — s. identischer Kommentar in
            // `handle_action_proposed`.
            action_confirmations.cancel(&action_id);
            tracing::warn!(
                ?action_id,
                timeout_secs = PENDING_ACTION_CONFIRM_TIMEOUT.as_secs(),
                "pending note-update confirmation timed out without a response, treating as denied"
            );
            ActionUserDecision::Deny
        }
    };

    // Spec 0010, Abschnitt 2, Punkt 5: "identischer Ablauf wie bei einem
    // regulären Notiz-Vorschlag" — ruft dieselbe Funktion wie der reguläre
    // In-Chat-Pfad auf, keine Sonderbehandlung. `EditThenApprove` macht für
    // `ProposeNoteUpdate` schon im regulären Pfad keinen Sinn (kein
    // Editierfeld im Frontend dafür, s. `handle_user_decision`); trifft es
    // trotzdem ein, wird der Vorschlag unverändert übernommen — exakt wie
    // dort.
    match user_decision {
        ActionUserDecision::Deny => {}
        ActionUserDecision::Approve | ActionUserDecision::EditThenApprove { .. } => {
            execute_note_update(
                session,
                session_id,
                action_id,
                target,
                new_content,
                emitter,
                profile_store,
                true,
            )
            .await;
        }
    }
    true
}

/// Spec 0057, §4.2 (Etappe 4), Zusammenspiel-Design mit `suggest_note_
/// update_on_disconnect` (Aufgabenstellung, Abschnitt 4 — "sie dürfen sich
/// nicht widersprechen oder den Nutzer mit zwei konkurrierenden
/// Notiz-Dialogen überfallen"): **klare Priorität statt eines kombinierten
/// KI-Aufrufs.** Der Update-Vorschlag geht vor — er fasst frisches
/// Sitzungswissen ein, das sonst verloren ginge, während der
/// Kürzungs-Vorschlag rein evergreen ist (eine große Notiz bleibt groß,
/// bis sie gekürzt wird). Hat `suggest_note_update_on_disconnect` bereits
/// einen Vorschlag gemacht (Rückgabewert `true`), wird der
/// Kürzungs-Vorschlag für DIESES Verbindungsende komplett übersprungen —
/// nicht nur verzögert oder in denselben Dialog gequetscht: **nie zwei
/// Notiz-Dialoge gleichzeitig oder auch nur kurz hintereinander** für
/// dasselbe Verbindungsende. Bleibt die Notiz danach weiterhin groß (der
/// Update-Vorschlag ändert sie ja nur bei Zustimmung, und selbst dann
/// potenziell nicht klein genug), taucht der Kürzungs-Vorschlag beim
/// NÄCHSTEN Verbindungsende ganz regulär wieder auf — nichts geht
/// dauerhaft verloren, es ist reine zeitliche Entflechtung.
///
/// Eine kombinierte KI-Anfrage ("aktualisiere UND kürze in einem Aufruf")
/// wurde bewusst verworfen: §4.2 verlangt explizit, dass der
/// Zusammenfassungs-Aufruf NUR nach einem eigenen, expliziten "Ja,
/// zusammenfassen" läuft, nie automatisch — eine Verschmelzung mit dem
/// (automatischen) Update-Vorschlag hätte genau das verletzt.
pub fn should_suggest_note_shrink(note_update_was_suggested: bool) -> bool {
    !note_update_was_suggested
}

/// Schwellwert für "die gespeicherte Notiz ist groß genug für den
/// Kürzungs-Vorschlag" (Spec 0057, §4.2: "Ist die Notiz groß (Schwellwert)
/// … Nur bei großer Notiz — bei normalen Notizen kein Dialog"). Schwelle in
/// Unicode-Skalarwerten (Zeichen, nicht Byte), bewusst deutlich über der
/// Zusammenfassungs-Obergrenze [`NOTE_SHRINK_MAX_BYTES`] (4_000 Byte), damit
/// normal genutzte Notizen (typischerweise wenige hundert Zeichen) nicht
/// auslösen.
///
/// `pub(crate)` statt privat (spec 0058, Teil 1/Etappe 5): derselbe
/// Schwellwert entscheidet jetzt auch über den proaktiven Hinweis im
/// Notiz-Editor (`commands::large_note_dialog_threshold_chars`, von dort ans
/// Frontend gereicht) — eine Quelle der Wahrheit statt einer zweiten,
/// hartkodierten Zahl im Frontend.
pub const LARGE_NOTE_DIALOG_THRESHOLD_CHARS: usize = 10_000;

/// Spec 0057, §4.2 (Etappe 4): beim Verbindungsende geprüft, im selben
/// Hintergrund-Task wie `suggest_note_update_on_disconnect`
/// (`commands::disconnect`) und NUR aufgerufen, wenn jene Funktion keinen
/// Vorschlag gemacht hat (s. `should_suggest_note_shrink`-Doc-Kommentar).
///
/// Anders als `suggest_note_update_on_disconnect`: **kein KI-Aufruf hier**
/// — nur eine billige Größenprüfung der GESPEICHERTEN Server-Notiz (Spec
/// 0057 §4.2, wörtlich "Notiz für diesen Server", immer Server-Scope, nie
/// Gruppe — anders als `ProposeNoteUpdate`, das auch `CurrentServerGroup`
/// kennt). Der eigentliche KI-Aufruf passiert erst nach explizitem "Ja,
/// zusammenfassen" (`commands::request_note_shrink` →
/// `execute_note_shrink_request`) — kein automatischer
/// Zusammenfassungsversuch ohne Nutzer-Anstoß, wie §4.2 es verlangt.
///
/// **Bewusst rein synchron und ohne `ProfileStore`/`Session`/`AppHandle`**
/// (spec-0058-Fund, Teil 2 — Etappe-4-Review hatte offen gelassen, ob der
/// lokale Pseudo-Server je eine Notiz-Größenprüfung durchläuft): die
/// GESPEICHERTE Notiz eines Servers aufzulösen unterscheidet sich für den
/// lokalen Pseudo-Server (kein `servers`-Zeile, `local_server::
/// synthetic_server` + `tauri::AppHandle` nötig, s. dortige Moduldoc) von
/// jedem echten Server (`profile_store.get_server`) — diese Datei bleibt
/// laut eigenem Moduldoc-Kommentar bewusst Tauri-unabhängig (kein
/// `tauri::AppHandle` direkt). Die Auflösung passiert deshalb VOR diesem
/// Aufruf in `commands::disconnect` (derselbe `is_local`-Verzweigungs-
/// Idiom wie `commands::build_session_system_context`) — diese Funktion
/// bekommt Name/Notiz bereits aufgelöst und prüft nur noch die Schwelle.
/// Ergebnis: der Dialog funktioniert jetzt für JEDEN Server gleich,
/// einschließlich des lokalen Pseudo-Servers (der sehr wohl eine Notiz
/// haben kann, s. `local_server::synthetic_server`).
pub fn suggest_note_shrink_on_disconnect(
    emitter: &dyn EventEmitter,
    server_id: ServerId,
    server_name: String,
    note_text: &str,
) {
    if note_text.chars().count() < LARGE_NOTE_DIALOG_THRESHOLD_CHARS {
        return;
    }
    emit_note_shrink_suggested(emitter, server_id, server_name);
}

/// Eigener Zeitrahmen für den Notiz-Kürzungs-KI-Aufruf (Spec 0057, §4.2/§6:
/// "Rate-Limit-Handling + Body-Timeout — nicht ungeschützt") — bewusst eine
/// eigene Konstante statt `compaction`s privater `SUMMARY_CALL_TIMEOUT`
/// (anderes Modul, außerdem ein semantisch eigenständiger Aufruf:
/// Notiz-Kürzung statt rollierende Chat-Zusammenfassung). Derselbe Wert,
/// aus demselben Grund: der Provider-Aufruf selbst trägt bereits Schutz
/// (SSE-Inaktivitäts-Timeout ~90s, Rate-Limit-Retry-Budget ~20s), dieser
/// äußere Rahmen ist die zusätzliche, unabhängige Rückversicherung gegen
/// einen unvorhergesehen hängenden Zustand.
const NOTE_SHRINK_CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Obergrenze für die vom Provider zurückgelieferte gekürzte Notiz —
/// dieselbe Fehlerklasse/Begründung wie `compaction::SUMMARY_MAX_BYTES`:
/// ohne Cap könnte eine geschwätzige/fehlgeleitete Antwort größer als die
/// Original-Notiz ausfallen und das Kürzungsziel strukturell verfehlen.
const NOTE_SHRINK_MAX_BYTES: usize = 4_000;

const NOTE_SHRINK_INSTRUCTION: &str = "Fasse die folgende, gespeicherte Notiz kürzer, aber \
     inhaltlich vollständig zusammen — keine Informationen verlieren, die für künftige \
     Sitzungen an diesem Server relevant sein könnten (z. B. Pfade, installierte Versionen, \
     getroffene Entscheidungen). Antworte NUR mit der gekürzten Notiz selbst, ohne Einleitung, \
     Anführungszeichen oder Meta-Kommentar.";

/// Der eigentliche KI-Aufruf hinter "Ja, zusammenfassen" (Spec 0057, §4.2).
/// **Bewusst session-unabhängig** — anders als jeder andere
/// `AiProvider::send()`-Aufruf in dieser Datei nimmt diese Funktion `&dyn
/// AiProvider`/`&dyn OutputRedactor` direkt statt `session: &Session`:
/// zwischen dem Anzeigen des ersten Dialogs ("Notiz ist groß …") und dem
/// tatsächlichen Klick auf "Ja, zusammenfassen" kann beliebig viel Zeit
/// vergehen — die auslösende `Session` aus `commands::disconnect`s
/// Hintergrund-Task ist zu diesem späteren Zeitpunkt typischerweise längst
/// beendet und gedroppt (Spec 0057 §4.2 ist explizit ein
/// NACH-Verbindungsende-Ablauf). `commands::request_note_shrink` baut
/// deshalb einen FRISCHEN `AiProvider` aus der aktuell aktiven
/// Provider-Konfiguration (derselbe Aufbau-Pfad wie `commands::connect`/
/// `test_ai_provider_credentials`) und einen `OutputRedactor`, der — wie
/// bei `connect()` — ein bekanntes, hinterlegtes Sudo-Passwort dieses
/// Servers als zusätzliches Muster trägt (spec-reviewer-Fund, Review dieses
/// Schritts: eine über "In Notiz übernehmen"/einen angenommenen
/// KI-Notiz-Vorschlag in der Notiz gelandete Kommandoausgabe kann ein
/// zuvor nur wegen des NOPASSWD-/Timestamp-Sonderfalls unredigiertes
/// Passwort enthalten, s. Kommentar an `commands::connect`s
/// Redactor-Aufbau).
///
/// `note_source_label` wird für [`fence_untrusted`] gebraucht
/// (spec-reviewer-Fund, Review dieses Schritts): eine gespeicherte
/// Server-/Gruppen-Notiz ist eine der vier untrusted Quellen aus Spec 0039
/// §3 — genau wie `compaction::compact_for_send` sie beim SENDEN fenced
/// (`fence_untrusted(UntrustedKind::ServerNote, ...)`), muss auch dieser
/// KI-Aufruf die Notiz gefenced einbetten. Ohne das könnte eine über einen
/// zuvor angenommenen Notiz-Vorschlag eingeschleuste Instruktion (die
/// ursprünglich aus einer Kommandoausgabe eines Remote-Hosts stammte) hier
/// als gleichrangiger Prompt-Text statt als Daten gelesen werden.
async fn summarize_note_for_shrink(
    ai_provider: &dyn AiProvider,
    budget: &ai_providers::ProviderBudgetGuard,
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    redactor: &dyn OutputRedactor,
    note_source_label: &str,
    note_text: &str,
) -> Option<String> {
    if note_text.trim().is_empty() {
        return None;
    }
    // Spec 0040, Abschnitt 5, dieselbe Begründung wie bei
    // `reapply_redaction_for_send`/`generate_rolling_summary`: additiv vor
    // jedem `send()` redigiert, unabhängig davon, ob die gespeicherte
    // Notiz selbst schon redigiert wirkt.
    let redacted_note = redactor.redact_text(note_text);
    let fenced_note = fence_untrusted(UntrustedKind::ServerNote, note_source_label, &redacted_note);
    let request_context = SessionContext {
        system_context: "Du kürzt eine gespeicherte Notiz zu einem SSH-Server.".to_string(),
        history: vec![ChatMessage {
            role: Role::User,
            content: MessageContent::Text(format!("{NOTE_SHRINK_INSTRUCTION}\n\n{fenced_note}")),
        }],
        available_actions: Vec::new(),
        // Spec 0065, Teil 1: Nebenaufruf — s. `SIDE_CALL_MAX_TOKENS`-Kommentar.
        max_tokens_hint: Some(SIDE_CALL_MAX_TOKENS),
    };

    // Spec 0061, Abschnitt 3: dieser Aufruf läuft session-unabhängig (s.
    // Doc-Kommentar an `execute_note_shrink_request` weiter unten) — kein
    // `Session.ai_request_paced_at`/`wait_for_ai_request_slot` hier
    // (existierte für diesen Pfad auch vor Spec 0061 schon nicht), aber
    // das Rate-Limit-Gate gilt trotzdem: derselbe Provider/dieselbe
    // Provider-Identität kann sich das Budget mit einer noch laufenden
    // Session teilen (Spec 0061 Abschnitt 2).
    wait_for_rate_limit_budget(
        budget,
        crate::compaction::estimate_request_tokens(&request_context),
        emitter,
        session_id,
    )
    .await;

    let call = async {
        let mut stream = ai_provider.send(request_context);
        let mut text = String::new();
        while let Some(event) = stream.next().await {
            match event {
                AiEvent::TextDelta(delta) => text.push_str(&delta),
                // Kein Tool-Schema angeboten, aber defensiv wie an den
                // anderen reinen-Text-Aufrufstellen: einfach ignorieren.
                AiEvent::ActionProposed(_) => {}
                // Spec 0065, Teil 2: kein „Weiter"-Hinweis für diesen
                // Nebenaufruf — die gekürzte Notiz gilt trotzdem als
                // Ergebnis (besser eine unvollständig gekürzte Notiz
                // zurückgeben als gar keine).
                AiEvent::Done | AiEvent::TextTruncated => return Some(text),
                AiEvent::Error(err) => {
                    tracing::warn!(error = %err, "note shrink summarization failed");
                    return None;
                }
            }
        }
        // Stream endete ohne `Done`/`Error` — genauso wie ein Fehler
        // behandeln, nicht stillschweigend als Erfolg werten (dieselbe
        // Begründung wie in `compaction::generate_rolling_summary`).
        None
    };

    let text = match tokio::time::timeout(NOTE_SHRINK_CALL_TIMEOUT, call).await {
        Ok(result) => result,
        Err(_elapsed) => {
            tracing::warn!(
                timeout_secs = NOTE_SHRINK_CALL_TIMEOUT.as_secs(),
                "note shrink summarization timed out"
            );
            None
        }
    }?;

    // Spec 0057, §2.1-Muster (Etappe 3) wiederverwendet: die
    // zurückkommende Kürzung "wie normaler KI-Inhalt behandelt" — durch
    // denselben Redactor wie alles andere.
    let redacted = redactor.redact_text(&text);
    if redacted.trim().is_empty() {
        return None;
    }
    if redacted.len() <= NOTE_SHRINK_MAX_BYTES {
        return Some(redacted);
    }
    // spec-reviewer-Fund (Review dieses Schritts): ohne Hinweis sähe der
    // Nutzer im Diff eine mitten im Satz abbrechende Notiz, ohne dass
    // erkennbar wäre, dass die App selbst gekappt hat (statt die KI
    // absichtlich mitten im Wort geendet hätte). Der Hinweis selbst zählt
    // zum Cap mit — `NOTE_SHRINK_MAX_BYTES` bleibt die harte Obergrenze für
    // das GESAMTE Ergebnis, nicht nur für den reinen Notiztext davor.
    const TRUNCATION_NOTICE: &str =
        "\n\n[Hinweis: Zusammenfassung war länger als erlaubt und wurde hier gekappt.]";
    let budget = NOTE_SHRINK_MAX_BYTES.saturating_sub(TRUNCATION_NOTICE.len());
    let capped = crate::compaction::truncate_to_char_boundary(&redacted, budget);
    Some(format!("{capped}{TRUNCATION_NOTICE}"))
}

/// Spec 0058, Teil 2: abstrahiert Lesen/Schreiben der Notiz des
/// Kürzungs-Ziels — dieselbe Abstraktionsebene wie `AiProvider`/
/// `OutputRedactor` in dieser Datei, aus demselben Grund: `execute_note_
/// shrink_request` bleibt dadurch weiterhin Tauri-unabhängig (kein
/// `tauri::AppHandle` direkt, s. Moduldoc-Kommentar ganz oben), obwohl der
/// lokale Pseudo-Server (kein `servers`-Zeile, s. `local_server`-Moduldoc)
/// eine grundsätzlich andere Persistenz braucht als ein echter Server
/// (`ProfileStore`). `commands::request_note_shrink` wählt die passende
/// Implementierung anhand von `local_server::is_local`.
#[async_trait::async_trait]
pub trait NoteShrinkTarget: Send + Sync {
    /// Der Servername (für die Diff-Anzeige) und der aktuelle Notizinhalt
    /// — `None`, wenn das Ziel nicht (mehr) aufgelöst werden kann.
    async fn read(&self) -> Option<(String, String)>;
    async fn write(&self, new_content: String) -> Result<(), String>;
}

/// Spec 0058, Teil 2: die `NoteShrinkTarget`-Implementierung für einen
/// ECHTEN Server — spiegelt `persist_note_revision`s `ProfileStore`-Pfad,
/// eigenständig gehalten (statt `persist_note_revision` wiederzuverwenden),
/// weil Letzteres einen bereits fertigen `NoteEditor` entgegennimmt, den
/// dieser Trait bewusst nicht kennt (s. `NoteShrinkTarget::write`-Doc).
pub struct ProfileStoreNoteShrinkTarget<'a> {
    pub profile_store: &'a dyn ProfileStore,
    pub server_id: ServerId,
    pub provider_label: String,
    pub model: String,
}

#[async_trait::async_trait]
impl NoteShrinkTarget for ProfileStoreNoteShrinkTarget<'_> {
    async fn read(&self) -> Option<(String, String)> {
        self.profile_store
            .get_server(&self.server_id)
            .await
            .ok()
            .map(|server| (server.name, server.notes))
    }

    async fn write(&self, new_content: String) -> Result<(), String> {
        persist_note_revision(
            self.profile_store,
            NoteTarget::Server(self.server_id),
            new_content,
            NoteEditor::Ai {
                provider: self.provider_label.clone(),
                model: self.model.clone(),
            },
        )
        .await
        .map(|_summary| ())
    }
}

/// Orchestriert den vollständigen "Ja, zusammenfassen"-Ablauf (Spec 0057,
/// §4.2) NACH dem KI-Aufruf: emittiert bei Erfolg **denselben** `note-
/// update-suggested`-Vorschlag/Diff-Bestätigungsablauf wie ein regulärer
/// KI-Notiz-Vorschlag (Spec 0003/0023) — keine zweite, parallele UI für
/// dieselbe Sache (Aufgabenstellung: "denselben Mechanismus nutzen"). Bei
/// einem KI-Ausfall wird stattdessen `note-shrink-failed` emittiert (Spec
/// 0057 §4.2/§6: "KI-Aufruf schlägt fehl → Fehlermeldung, gespeicherte
/// Notiz unverändert, kein Hang") — die gespeicherte Notiz bleibt in
/// BEIDEN Fällen unangetastet, bis (und nur bis) der Nutzer im
/// Diff-Dialog tatsächlich zustimmt.
///
/// `session_id` im emittierten Event ist ein frischer, bedeutungsloser
/// Platzhalter (`Uuid::new_v4()`, vom Aufrufer erzeugt) — es gibt keine
/// lebende Session, auf die sich dieser Ablauf bezieht (s. Doc-Kommentar
/// an `summarize_note_for_shrink`); `commands::respond_to_action` ignoriert
/// `session_id` ohnehin bereits explizit (s. dortiger Kommentar), das Feld
/// existiert nur, weil das wiederverwendete Event-Schema es verlangt.
// 8 Parameter, alle unabhängige Kollaborateure ohne natürliche Gruppierung
// (kein `Session` verfügbar, s. Doc-Kommentar oben) — ein Bündel-Struct nur
// für diesen einen Aufrufer wäre reine Indirektion ohne Mehrwert.
#[allow(clippy::too_many_arguments)]
pub async fn execute_note_shrink_request(
    session_id: SessionId,
    server_id: ServerId,
    ai_provider: &dyn AiProvider,
    ai_provider_budget: &ai_providers::ProviderBudgetGuard,
    redactor: &dyn OutputRedactor,
    emitter: &dyn EventEmitter,
    target: &dyn NoteShrinkTarget,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
) {
    let Some((server_name, note_text)) = target.read().await else {
        emit_note_shrink_failed(
            emitter,
            server_id,
            "Server nicht gefunden — Notiz konnte nicht zusammengefasst werden.".to_string(),
        );
        return;
    };

    let Some(new_content) = summarize_note_for_shrink(
        ai_provider,
        ai_provider_budget,
        emitter,
        session_id,
        redactor,
        &server_name,
        &note_text,
    )
    .await
    else {
        emit_note_shrink_failed(
            emitter,
            server_id,
            "Die Notiz konnte nicht zusammengefasst werden (KI-Aufruf fehlgeschlagen oder \
             abgelaufen). Die gespeicherte Notiz wurde nicht verändert."
                .to_string(),
        );
        return;
    };

    let action_id: ActionId = Uuid::new_v4();
    let previous_notes = note_text;
    emit_note_update_suggested(
        emitter,
        session_id,
        action_id,
        AiAction::ProposeNoteUpdate {
            target: NoteTargetSelector::CurrentServer,
            new_content: new_content.clone(),
        },
        Some(previous_notes.clone()),
        Some(server_name),
    );

    let rx = action_confirmations.register(action_id);
    let user_decision = match tokio::time::timeout(PENDING_ACTION_CONFIRM_TIMEOUT, rx).await {
        Ok(Ok(decision)) => decision,
        Ok(Err(_)) => return,
        Err(_elapsed) => {
            action_confirmations.cancel(&action_id);
            tracing::warn!(
                ?action_id,
                timeout_secs = PENDING_ACTION_CONFIRM_TIMEOUT.as_secs(),
                "pending note-shrink confirmation timed out without a response, treating as \
                 denied"
            );
            ActionUserDecision::Deny
        }
    };

    if !matches!(
        user_decision,
        ActionUserDecision::Approve | ActionUserDecision::EditThenApprove { .. }
    ) {
        return;
    }

    // spec-reviewer-Fund (Review dieses Schritts): das Bestätigungsfenster
    // ist bis zu `PENDING_ACTION_CONFIRM_TIMEOUT` (3600s) lang — genug
    // Zeit, dass der Nutzer die Notiz in der Zwischenzeit selbst bearbeitet
    // (z. B. über "Mache ich selbst" auf einem ZWEITEN Aufruf desselben
    // Dialogs, oder einfach über die normale Notiz-Bearbeitung). Der Diff,
    // dem er gerade zugestimmt hat, bezog sich auf den zum Zeitpunkt des
    // KI-Aufrufs gelesenen Stand (`previous_notes`) — ist die tatsächlich
    // gespeicherte Notiz inzwischen eine ANDERE, würde ein blindes
    // Überschreiben genau die zwischenzeitliche Änderung verlieren, obwohl
    // der Nutzer NUR dem ALTEN Diff zugestimmt hat. Frisch nachgelesen statt
    // blind überschrieben — bei Abweichung wird abgebrochen (die
    // Revisions-Historie macht ein blindes Überschreiben zwar rückholbar,
    // aber Spec 0057 §6 verlangt "nie ohne Nutzer-Bestätigung verändert",
    // und bestätigt wurde hier ein Diff gegen einen inzwischen veralteten
    // Ausgangstext).
    let Some((_, current_notes)) = target.read().await else {
        emit_note_shrink_failed(
            emitter,
            server_id,
            "Notiz konnte nicht gespeichert werden — Server nicht mehr auffindbar.".to_string(),
        );
        return;
    };
    if current_notes != previous_notes {
        emit_note_shrink_failed(
            emitter,
            server_id,
            "Die Notiz wurde zwischenzeitlich anderweitig geändert — die Zusammenfassung wurde \
             NICHT gespeichert, um diese Änderung nicht zu überschreiben."
                .to_string(),
        );
        return;
    }

    match target.write(new_content).await {
        Ok(()) => {
            // spec-reviewer-Fund (Spec 0058, Review des Politur-Pakets): s.
            // `emit_note_shrink_succeeded`-Doc-Kommentar — ein zeitgleich
            // offener Notiz-Editor muss den frisch gekürzten Stand
            // übernehmen, sonst überschreibt sein nächster "Speichern"-Klick
            // die gerade akzeptierte Zusammenfassung wieder.
            emit_note_shrink_succeeded(emitter, server_id);
        }
        Err(err) => {
            // spec-reviewer-Fund (Review dieses Schritts): vorher nur
            // geloggt — der Nutzer hatte gerade "Annehmen" geklickt, die
            // Karte verschwand, und ohne dieses Event hätte er angenommen,
            // die Notiz sei jetzt gekürzt, obwohl nichts geschrieben wurde.
            // Kein `session`, an das ein `chat-action-result`/-`error`
            // gebunden werden könnte (s. Doc-Kommentar an der Funktion) —
            // deshalb dasselbe app-weite `note-shrink-failed` wie bei einem
            // KI-Fehlschlag.
            tracing::warn!(error = %err, "note shrink persistence failed");
            emit_note_shrink_failed(
                emitter,
                server_id,
                format!("Notiz konnte nicht gespeichert werden: {err}"),
            );
        }
    }
}

/// Spec 0040, Abschnitt 6: "In Notiz übernehmen" — eine UI-Aktion auf einer
/// Chat-/Ergebnis-Zeile, die den bestehenden `ProposeNoteUpdate`-Ablauf
/// (Spec 0003, Abschnitt 5.2) mit deren Inhalt vorbefüllt, statt auf einen
/// KI-Vorschlag zu warten. **Kein neuer Persistenz-/Bestätigungsmechanismus**
/// — ruft `handle_action_proposed` exakt wie ein regulärer, von der KI
/// selbst vorgeschlagener `ProposeNoteUpdate` auf (`ActionOrigin::Internal`,
/// also inkl. normaler Persistenz und desselben `chat-action-proposed`/
/// `NoteDiffPreview`-UI-Pfads, den `ChatPanel.tsx` bereits rendert).
///
/// Ziel ist immer der aktuelle Server (`NoteTargetSelector::CurrentServer`)
/// — dieselbe Server-Session, deren Chat die Zeile enthält; eine
/// Gruppen-Auswahl bietet die UI hier bewusst nicht an (Scope-Reduktion,
/// s. ADR zu Spec 0040). `new_content` ist der bestehende Notizinhalt plus
/// den übernommenen Zeileninhalt angehängt (Spec: "vollständiger neuer
/// Text, nicht nur ein Diff", s. `AiAction::ProposeNoteUpdate`-Doc) — die
/// Diff-Vorschau im bestehenden Dialog zeigt dem Nutzer genau diese
/// Ergänzung, bevor er bestätigt.
pub async fn propose_note_from_chat_content(
    session: &Session,
    session_id: SessionId,
    content: String,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    action_confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
) -> bool {
    let target = NoteTargetSelector::CurrentServer;
    let (previous_note_content, _) = note_target_preview_for_action(
        &AiAction::ProposeNoteUpdate {
            target,
            new_content: String::new(),
        },
        session,
        profile_store,
    )
    .await;
    let new_content = match previous_note_content {
        Some(existing) if !existing.trim().is_empty() => format!("{existing}\n\n{content}"),
        _ => content,
    };

    handle_action_proposed(
        session,
        session_id,
        AiAction::ProposeNoteUpdate {
            target,
            new_content,
        },
        emitter,
        profile_store,
        action_confirmations,
        ActionOrigin::Internal,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .await
}
