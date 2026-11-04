//! Spec 0005: interaktives Terminal — Teil der Spec-0083-Aufteilung von
//! `commands.rs`.

use app_logic::error::ResultCodeExt;
use ssh_manager_core::sync::lock_tolerating_poison;
use std::sync::Arc;

use tauri::{AppHandle, State};
use tokio::sync::mpsc;

use ssh_manager_core::ssh::PtySize;

use crate::event_emitter::TauriEventEmitter;
use app_logic::error::{CommandError, CommandResult};
use app_logic::events::EventEmitter;
use app_logic::session::{spawn_terminal_actor, Session, TerminalCommand};
use app_logic::start_directory::{open_shell_in_start_directory, TerminalStartDto};
use app_logic::state::{AppState, SessionId};

/// Spec 0102: öffnet die Shell und wechselt ggf. sichtbar ins
/// Startverzeichnis des Servers (s. `app_logic::start_directory::
/// open_shell_in_start_directory`). Das Ergebnis trägt den einmaligen
/// Hinweis, falls das Verzeichnis fehlt.
#[tauri::command]
pub async fn open_terminal(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<TerminalStartDto> {
    // Spec 0104, §4: Eine MCP-Sitzung hat kein interaktives Terminal.
    state.mcp.sessions.ensure_user_session(session_id)?;
    let session = app_logic::session::require_session(&state.sessions, session_id)?;

    // Standardgröße, bis das Frontend die tatsächliche Terminal-Größe
    // per `terminal_resize` meldet (Spec 0007 Abschnitt 4 sieht für
    // `open_terminal` selbst keinen Größen-Parameter vor).
    let (shell, start) =
        open_shell_in_start_directory(&session, PtySize { cols: 80, rows: 24 }).await?;

    let (tx, rx) = mpsc::channel(app_logic::session::TERMINAL_COMMAND_CAPACITY);
    *lock_tolerating_poison(&session.conn.terminal) = Some(tx);
    spawn_terminal_actor(
        session_id,
        Arc::clone(&session),
        shell,
        rx,
        Arc::new(TauriEventEmitter(app.clone())) as Arc<dyn EventEmitter>,
        &crate::background_tasks::session_scope(&app, session_id),
    );
    Ok(start)
}

fn terminal_sender(session: &Session) -> CommandResult<mpsc::Sender<TerminalCommand>> {
    lock_tolerating_poison(&session.conn.terminal)
        .clone()
        .ok_or_else(|| {
            CommandError::with_code(
                "Terminal wurde noch nicht geöffnet (open_terminal aufrufen)",
                "TERMINAL_NOT_OPEN",
            )
        })
}

#[tauri::command]
pub async fn terminal_input(
    state: State<'_, AppState>,
    session_id: SessionId,
    data: Vec<u8>,
) -> CommandResult<()> {
    let session = app_logic::session::require_session(&state.sessions, session_id)?;
    terminal_sender(&session)?
        .send(TerminalCommand::Write(data))
        .await
        .map_err(|_| "Terminal-Kanal bereits geschlossen")
        .or_code("TERMINAL_CHANNEL_CLOSED")?;
    Ok(())
}

#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, AppState>,
    session_id: SessionId,
    cols: u16,
    rows: u16,
) -> CommandResult<()> {
    let session = app_logic::session::require_session(&state.sessions, session_id)?;
    terminal_sender(&session)?
        .send(TerminalCommand::Resize(PtySize { cols, rows }))
        .await
        .map_err(|_| "Terminal-Kanal bereits geschlossen")
        .or_code("TERMINAL_CHANNEL_CLOSED")?;
    Ok(())
}
