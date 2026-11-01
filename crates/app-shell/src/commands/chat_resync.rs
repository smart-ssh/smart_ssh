//! Issue #1020, Spec 0114 §4: `get_chat_resync`.

use app_logic::chat_resync::ChatResyncDto;
use app_logic::dto::ChatHistoryEntryDto;
use app_logic::state::{AppState, SessionId};
use tauri::State;

use app_logic::error::CommandResult;

/// The history plus the confirmation that is still waiting, to rebuild a
/// chat after the webview missed events (app returns from the background).
#[tauri::command]
pub async fn get_chat_resync(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<ChatResyncDto> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    let history = session.context.lock().await.history.clone();
    // Only a confirmation the registry still knows may become a card with
    // active buttons — a stale proposal must not be confirmable.
    let pending_proposal = session
        .pending_proposal()
        .filter(|(action_id, _)| state.pending_action_confirmations.contains(action_id))
        .map(|(_, payload)| payload);
    Ok(ChatResyncDto {
        history: history.into_iter().map(ChatHistoryEntryDto::from).collect(),
        pending_proposal,
    })
}
