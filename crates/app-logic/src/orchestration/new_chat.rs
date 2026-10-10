//! Issue #271, Spec 0034 §11: „Neuer Chat" innerhalb einer bestehenden
//! Verbindung. Die SSH-Verbindung, das Terminal, der Dateibrowser, das
//! Sudo-Passwort und `untrusted_content_ingested` bleiben unangetastet; nur
//! der Chat (Zeile in `chat_sessions` und `SessionContext`) wird getauscht.

use uuid::Uuid;

use crate::error::CommandError;
use crate::events::EventEmitter;
use crate::session::Session;
use crate::state::SessionId;
use ssh_manager_core::ai::{MessageContent, Role};

use super::generate_session_title_on_disconnect;

/// Stabiler Fehlercode: In diesem Tab läuft ein KI-Turn oder wartet eine
/// Bestätigung.
pub const NEW_CHAT_BUSY: &str = "NEW_CHAT_BUSY";

/// Der `uname -a`-Banner steht seit Spec 0064 als erste, nie persistierte
/// Verlaufs-Nachricht (gefencter `<remote_system>`-Block, Rolle
/// `ActionResult`) im Kontext. Er gehört zur Verbindung, nicht zum Chat, und
/// bleibt deshalb beim Leeren erhalten — ein frischer Chat startet damit wie
/// eine frische Verbindung.
fn is_os_banner(message: &ssh_manager_core::ai::ChatMessage) -> bool {
    matches!(message.role, Role::ActionResult)
        && matches!(&message.content, MessageContent::Text(t) if t.starts_with("<remote_system>"))
}

fn busy_error() -> CommandError {
    CommandError::with_code(
        "Neuer Chat nicht möglich: Es läuft gerade eine KI-Antwort oder eine \
         Bestätigung ist offen.",
        NEW_CHAT_BUSY,
    )
}

fn is_confirmation_open(session: &Session) -> bool {
    crate::poison::lock_tolerating_poison(&session.pending_action).is_some()
        || session
            .mcp_confirmation_claim
            .load(std::sync::atomic::Ordering::SeqCst)
}

/// Gibt `chat_turn.running` wieder frei, auch bei vorzeitigem Ende.
struct TurnClaim<'a>(&'a Session);

impl Drop for TurnClaim<'_> {
    fn drop(&mut self) {
        let mut turn = self
            .0
            .chat_turn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        turn.running = false;
    }
}

/// Beginnt einen neuen Chat für `session`.
///
/// Ablauf: (1) Leerlauf prüfen; (2) bei persistiertem Chat den Titel des
/// alten Chats aus dem noch vollständigen Kontext erzeugen (derselbe Pfad wie
/// beim Trennen); (3) den Turn-Slot belegen, damit kein Turn dazwischen
/// startet; (4) neue Zeile anlegen, alte beenden; (5) Kontext leeren und auf
/// die neue Zeile umschalten. Schlägt das Anlegen der neuen Zeile fehl, ist
/// nichts verändert. Der `context`-Lock wird nie über Datenbankzugriffe
/// gehalten.
///
/// Gibt die neue `chat_sessions.id` zurück (`None` bei einem Tab ohne
/// persistierten Chat).
#[tracing::instrument(skip_all)]
pub async fn start_new_chat(
    session: &Session,
    session_id: SessionId,
    emitter: &dyn EventEmitter,
    provider_id: Option<Uuid>,
) -> Result<Option<Uuid>, CommandError> {
    if crate::poison::lock_tolerating_poison(&session.chat_turn).running
        || is_confirmation_open(session)
    {
        return Err(busy_error());
    }

    let persisted = match (
        &session.chat_session_store,
        *session.chat_session_id.lock().await,
    ) {
        (Some(store), Some(id)) => Some((store.clone(), id)),
        _ => None,
    };

    if persisted.is_some() {
        // Wie beim Trennen (Spec 0034 §7), solange der Verlauf noch da ist.
        generate_session_title_on_disconnect(session, session_id, emitter).await;
    }

    // Belegt den Turn-Slot atomar mit der Prüfung: eine währenddessen
    // eintreffende Nutzer-Nachricht wird nur eingereiht, nicht ausgeführt.
    {
        let mut turn = crate::poison::lock_tolerating_poison(&session.chat_turn);
        if turn.running {
            return Err(busy_error());
        }
        turn.running = true;
    }
    let _claim = TurnClaim(session);
    if is_confirmation_open(session) {
        return Err(busy_error());
    }

    let new_id = if let Some((store, old_id)) = &persisted {
        let new_id = store
            .create_session(&session.server_id, provider_id)
            .await
            .map_err(|err| {
                tracing::warn!(error = %err, "new chat session creation failed");
                CommandError::from(format!("Neuer Chat konnte nicht angelegt werden: {err}"))
            })?;
        if let Err(err) = store.mark_ended(*old_id).await {
            tracing::warn!(error = %err, "chat session mark_ended failed");
        }
        Some(new_id)
    } else {
        None
    };

    {
        let mut chat_session_id = session.chat_session_id.lock().await;
        let mut context = session.context.lock().await;
        let mut summary = session.summary.lock().await;
        let mut flags = crate::poison::lock_tolerating_poison(&session.mcp_origin_flags);
        // Kontext-Lock nur für diese reinen Speicheroperationen.
        context.history.retain(is_os_banner);
        flags.clear();
        flags.resize(context.history.len(), false);
        *summary = None;
        if new_id.is_some() {
            *chat_session_id = new_id;
        }
    }
    // Per-Chat-Zustand der laufenden Verbindung; `untrusted_content_ingested`
    // bleibt bewusst unberührt (Issue #271, Entscheidung 2), ebenso
    // `injection_suspected`/`injection_check_unavailable`.
    session
        .auto_continue_stop
        .store(false, std::sync::atomic::Ordering::SeqCst);

    Ok(new_id)
}

// Testcode-Ausnahme zum `deny` — s. `orchestration.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::events::TestEmitter;
    use crate::orchestration::test_support::*;
    use ssh_manager_core::ai::{AiEvent, ChatMessage};
    use std::sync::atomic::Ordering;

    fn user(text: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn banner() -> ChatMessage {
        ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::Text(
                "<remote_system>\n<source>uname -a</source>\nLinux\n</remote_system>".to_string(),
            ),
        }
    }

    async fn persisted_session() -> (
        Session,
        persistence_sqlite::SqliteChatSessionStore,
        Uuid,
        tempfile::TempDir,
    ) {
        let (session, store, id, tmp) = session_with_real_chat_persistence(
            vec![AiEvent::TextDelta("Alter Chat".to_string()), AiEvent::Done],
            MockSshTransport::default().with_response("echo hi", output("hi\n")),
        )
        .await;
        {
            let mut ctx = session.context.lock().await;
            ctx.history.push(banner());
            ctx.history.push(user("erste Frage"));
        }
        *session.mcp_origin_flags.lock().unwrap() = vec![false, false];
        (session, store, id, tmp)
    }

    #[tokio::test]
    async fn test_new_chat_ends_old_row_titles_it_and_switches_to_a_new_row() {
        let (session, store, old_id, _tmp) = persisted_session().await;

        let new_id = start_new_chat(&session, Uuid::new_v4(), &TestEmitter::default(), None)
            .await
            .unwrap()
            .expect("persisted tab gets a new row");

        assert_ne!(new_id, old_id);
        assert_eq!(*session.chat_session_id.lock().await, Some(new_id));
        let rows = store
            .list_sessions_for_server(&session.server_id)
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        let old = rows.iter().find(|r| r.id == old_id).unwrap();
        assert!(old.ended_at.is_some());
        assert_eq!(old.title.as_deref(), Some("Alter Chat"));
        let new = rows.iter().find(|r| r.id == new_id).unwrap();
        assert!(new.ended_at.is_none());
        // History is empty apart from the connection's OS banner.
        let ctx = session.context.lock().await;
        assert_eq!(ctx.history.len(), 1);
        assert!(is_os_banner(&ctx.history[0]));
        assert_eq!(session.mcp_origin_flags.lock().unwrap().len(), 1);
        assert!(session.summary.lock().await.is_none());
        assert!(!session.chat_turn.lock().unwrap().running);
    }

    #[tokio::test]
    async fn test_new_chat_keeps_untrusted_flag_and_connection() {
        let (session, _store, _old_id, _tmp) = persisted_session().await;
        session
            .untrusted_content_ingested
            .store(true, Ordering::SeqCst);
        session.injection_suspected.store(true, Ordering::SeqCst);
        session
            .injection_check_unavailable
            .store(true, Ordering::SeqCst);

        start_new_chat(&session, Uuid::new_v4(), &TestEmitter::default(), None)
            .await
            .unwrap();

        assert!(session.untrusted_content_ingested.load(Ordering::SeqCst));
        assert!(session.injection_suspected.load(Ordering::SeqCst));
        assert!(session.injection_check_unavailable.load(Ordering::SeqCst));
        // Same transport, still usable.
        let out = session
            .transport
            .lock()
            .await
            .execute("echo hi")
            .await
            .unwrap();
        assert_eq!(out.stdout, b"hi\n".to_vec());
    }

    #[tokio::test]
    async fn test_new_chat_is_refused_while_turn_runs_or_confirmation_pending() {
        for pending in [false, true] {
            let (session, store, old_id, _tmp) = persisted_session().await;
            if pending {
                *session.pending_action.lock().unwrap() = Some(Uuid::new_v4());
            } else {
                session.chat_turn.lock().unwrap().running = true;
            }

            let err = start_new_chat(&session, Uuid::new_v4(), &TestEmitter::default(), None)
                .await
                .unwrap_err();

            assert_eq!(err.code, Some(NEW_CHAT_BUSY));
            assert_eq!(*session.chat_session_id.lock().await, Some(old_id));
            assert_eq!(session.context.lock().await.history.len(), 2);
            let rows = store
                .list_sessions_for_server(&session.server_id)
                .await
                .unwrap();
            assert_eq!(rows.len(), 1);
            assert!(rows[0].ended_at.is_none());
            assert!(rows[0].title.is_none());
        }
    }

    #[tokio::test]
    async fn test_new_chat_in_local_tab_only_clears_context() {
        let session = test_session(vec![AiEvent::Done], MockSshTransport::default());
        session.context.lock().await.history.push(user("hallo"));
        *session.mcp_origin_flags.lock().unwrap() = vec![false];

        let result = start_new_chat(&session, Uuid::new_v4(), &TestEmitter::default(), None)
            .await
            .unwrap();

        assert_eq!(result, None);
        assert!(session.context.lock().await.history.is_empty());
        assert_eq!(*session.chat_session_id.lock().await, None);
        assert!(session.chat_session_store.is_none());
    }
}
