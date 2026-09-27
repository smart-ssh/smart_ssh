//! Spec 0005: interaktives Terminal — Teil der Spec-0083-Aufteilung von
//! `commands.rs`.

use std::sync::Arc;

use tauri::{AppHandle, State};
use tokio::sync::mpsc;

use ssh_manager_core::ssh::PtySize;

use crate::error::CommandResult;
use crate::events::EventEmitter;
use crate::session::{spawn_terminal_actor, Session, TerminalCommand};
use crate::state::{AppState, SessionId};

#[tauri::command]
pub async fn open_terminal(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<()> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;

    let shell = {
        let mut transport = session.transport.lock().await;
        // Standardgröße, bis das Frontend die tatsächliche Terminal-Größe
        // per `terminal_resize` meldet (Spec 0007 Abschnitt 4 sieht für
        // `open_terminal` selbst keinen Größen-Parameter vor).
        transport.open_shell(PtySize { cols: 80, rows: 24 }).await?
    };

    let (tx, rx) = mpsc::unbounded_channel();
    *session.terminal.lock().unwrap() = Some(tx);
    spawn_terminal_actor(
        session_id,
        Arc::clone(&session),
        shell,
        rx,
        Arc::new(app) as Arc<dyn EventEmitter>,
    );
    Ok(())
}

fn terminal_sender(session: &Session) -> CommandResult<mpsc::UnboundedSender<TerminalCommand>> {
    session
        .terminal
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Terminal wurde noch nicht geöffnet (open_terminal aufrufen)".into())
}

#[tauri::command]
pub async fn terminal_input(
    state: State<'_, AppState>,
    session_id: SessionId,
    data: Vec<u8>,
) -> CommandResult<()> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    terminal_sender(&session)?
        .send(TerminalCommand::Write(data))
        .map_err(|_| "Terminal-Kanal bereits geschlossen")?;
    Ok(())
}

#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, AppState>,
    session_id: SessionId,
    cols: u16,
    rows: u16,
) -> CommandResult<()> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    terminal_sender(&session)?
        .send(TerminalCommand::Resize(PtySize { cols, rows }))
        .map_err(|_| "Terminal-Kanal bereits geschlossen")?;
    Ok(())
}
