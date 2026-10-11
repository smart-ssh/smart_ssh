//! Issue #356, Spec 0107 §8: automatic reconnect after a connection loss and
//! the AI continuation afterwards. The schedule and the loop live in
//! `app_logic::auto_reconnect`; this module supplies the concrete attempt
//! (the same path as a manual reconnect, but never interactive), reads the
//! setting and starts the follow-up AI round.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_store::StoreExt;

use app_logic::auto_reconnect::{
    budget_from_stored, emit_auto_reconnect_changed, is_retryable_code, run_auto_reconnect,
    AttemptResult, AutoReconnectEnd, AutoReconnectPhase, SETTING_KEY,
};
use app_logic::session::Session;
use app_logic::state::{AppState, SessionId};

use crate::event_emitter::TauriEventEmitter;
use crate::settings_store::SETTINGS_STORE_FILE;

/// Registers the loss hook of a freshly connected session.
pub(crate) fn install_hook<R: Runtime>(app: &AppHandle<R>, session: &Session) {
    let app = app.clone();
    session.auto_reconnect.set_hook(Arc::new(move |session_id| {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { run(app, session_id).await });
    }));
}

/// Read at every drop, so a changed setting applies from the next drop on.
/// `None` = off. A missing, unreadable or unknown value means the default.
fn read_budget<R: Runtime>(app: &AppHandle<R>) -> Option<Duration> {
    match app.store(SETTINGS_STORE_FILE) {
        Ok(store) => budget_from_stored(store.get(SETTING_KEY).as_ref()),
        Err(_) => budget_from_stored(None),
    }
}

async fn run<R: Runtime>(app: AppHandle<R>, session_id: SessionId) {
    let state = app.state::<AppState>();
    let Some(session) = state.sessions.get(session_id) else {
        return;
    };
    // MCP sessions are not user tabs; they are reconnected on demand.
    if state.mcp.sessions.ensure_user_session(session_id).is_err() {
        return;
    }
    let Some(budget) = read_budget(&app) else {
        return;
    };
    let emitter = TauriEventEmitter(app.clone());
    let stop = session.auto_reconnect.begin_run();
    emit_auto_reconnect_changed(&emitter, session_id, AutoReconnectPhase::Running, 1);

    let end = run_auto_reconnect(
        budget,
        &stop,
        |n| emit_auto_reconnect_changed(&emitter, session_id, AutoReconnectPhase::Running, n),
        || {
            let app = app.clone();
            async move { attempt(&app, session_id).await }
        },
    )
    .await;
    session.auto_reconnect.finish_run(&stop);
    emit_auto_reconnect_changed(&emitter, session_id, end.into(), 0);
    tracing::info!(session_id = %session_id, end = ?end, "automatic reconnect ended");

    if end == AutoReconnectEnd::Reconnected {
        continue_ai(&app, &state, &session, session_id).await;
    }
}

/// One automatic attempt: never opens the host key dialog, never accepts a
/// key. Only errors that waiting can fix are retried.
async fn attempt<R: Runtime>(app: &AppHandle<R>, session_id: SessionId) -> AttemptResult {
    let state = app.state::<AppState>();
    let elevated = app.state::<crate::elevated_sftp::ElevatedSftpRegistry>();
    let grants = app.state::<app_logic::local_path_grants::LocalPathGrants>();
    match crate::commands::reconnect_session_inner(
        app, &state, &elevated, &grants, session_id, false,
    )
    .await
    {
        Ok(()) => AttemptResult::Connected,
        Err(err) if is_retryable_code(err.code) => AttemptResult::Retry,
        Err(_) => AttemptResult::Blocked,
    }
}

/// The note for the AI after a reconnect, if an AI turn was cut off by the
/// drop and the user did not press Stop in the meantime. Takes the
/// "interrupted" flag, so a drop leads to at most one continuation.
pub(crate) async fn continuation_note(session: &Session) -> Option<String> {
    if !session.auto_reconnect.take_ai_turn_interrupted() {
        return None;
    }
    if session
        .auto_continue_stop
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return None;
    }
    let language = session.system_context_parts.lock().await.language;
    Some(app_logic::system_prompt::connection_restored_notice(language).to_string())
}

/// Starts a normal AI round with the note. The interrupted command is not
/// re-run; whatever the AI proposes next goes through filter and
/// confirmation like any other proposal.
async fn continue_ai<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    session: &Arc<Session>,
    session_id: SessionId,
) {
    let Some(note) = continuation_note(session).await else {
        return;
    };
    let emitter = TauriEventEmitter(app.clone());
    if let Err(err) = crate::commands::send_chat_message_impl(
        app,
        &emitter,
        session,
        session_id,
        note,
        None,
        state.profile_store.as_ref(),
        &state.policy_store,
        &state.pending_action_confirmations,
    )
    .await
    {
        tracing::warn!(
            session_id = %session_id,
            code = err.code.unwrap_or("UNKNOWN"),
            "AI continuation after reconnect did not start",
        );
    }
}
