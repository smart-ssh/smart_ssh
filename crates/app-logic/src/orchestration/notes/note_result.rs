//! Issue #1018: model-facing result of an approved AI note update and the
//! prompt refresh for the automatic follow-up round.

use ssh_manager_core::profiles::NoteTarget;

use ssh_manager_core::ai::{ChatMessage, MessageContent, Role};

use super::super::chat_turn::push_history_scoped;
use crate::session::Session;
use crate::system_prompt::PromptLanguage;
use ssh_manager_core::profiles::ProfileStore;

async fn note_target_display_name(target: NoteTarget, profile_store: &dyn ProfileStore) -> String {
    let (name, id) = match target {
        NoteTarget::Server(id) => (profile_store.get_server(&id).await.map(|s| s.name), id.0),
        NoteTarget::Group(id) => (profile_store.get_group(&id).await.map(|g| g.name), id.0),
    };
    // The name is a convenience for the model; a lookup failure must not
    // undo the saved note, but is not silent either.
    name.unwrap_or_else(|err| {
        tracing::warn!(error = %err, "note target name lookup failed");
        id.to_string()
    })
}

/// Issue #1018: tagged result of an approved AI note update for the model
/// (Spec 0021 §3). `new_content` is the stored, already redacted text.
pub(super) fn note_update_result_text(
    target: NoteTarget,
    name: &str,
    new_content: &str,
    language: PromptLanguage,
) -> String {
    let (kind, kind_text_en, kind_text_de) = match target {
        NoteTarget::Server(_) => ("current_server", "server", "Server"),
        NoteTarget::Group(_) => ("server_group", "server group", "Servergruppe"),
    };
    let name = name.replace('"', "'");
    let content = new_content.replace("</note_update_result>", "<\\/note_update_result>");
    let message = match language {
        PromptLanguage::De => format!(
            "Dein Vorschlag (propose_note_update) wurde vom Nutzer bestätigt und angewendet. \
             Die Notiz für {kind_text_de} \"{name}\" lautet jetzt:"
        ),
        _ => format!(
            "Your proposal (propose_note_update) was confirmed by the user and applied. \
             The note for {kind_text_en} \"{name}\" now reads:"
        ),
    };
    format!(
        "<note_update_result target=\"{kind}\" name=\"{name}\">\n{message}\n{content}\n</note_update_result>"
    )
}

/// Issue #1018: the automatic follow-up round after the decision reuses the
/// system prompt of the last user message, so the stored note text is
/// reloaded here and the prompt reassembled with it.
async fn refresh_session_note_context(session: &Session, profile_store: &dyn ProfileStore) {
    let server = match profile_store.get_server(&session.server_id).await {
        Ok(server) => server,
        Err(err) => {
            tracing::warn!(error = %err, "note refresh: get_server failed, prompt keeps the old note");
            return;
        }
    };
    let sections =
        match ssh_manager_core::profiles::effective_notes_sections(&server, profile_store).await {
            Ok(sections) => sections,
            Err(err) => {
                tracing::warn!(error = %err, "note refresh failed, prompt keeps the old note");
                return;
            }
        };
    let assembled = {
        let mut parts = session.system_context_parts.lock().await;
        parts.note_sections = sections;
        if parts.has_notes() {
            session
                .untrusted_content_ingested
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        parts.assemble()
    };
    session.context.lock().await.system_context = assembled;
}

/// Pushes the tagged result as an `ActionResult` history entry (kind and
/// display name instead of a bare UUID, stored redacted text) and then
/// refreshes the session prompt (Issue #1018).
pub(super) async fn record_note_update_for_model(
    session: &Session,
    target: NoteTarget,
    new_content: &str,
    profile_store: &dyn ProfileStore,
    persist: bool,
) {
    let language = session.system_context_parts.lock().await.language;
    let name = note_target_display_name(target, profile_store).await;
    let text = note_update_result_text(target, &name, new_content, language);
    push_history_scoped(
        session,
        ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::Text(text),
        },
        persist,
    )
    .await;
    refresh_session_note_context(session, profile_store).await;
}
