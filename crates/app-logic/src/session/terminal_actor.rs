//! The terminal actor task (module comment, point 3, in `super`).

use super::*;

/// Startet den Terminal-Aktor (Modul-Kommentar, Punkt 3) als eigenen Task.
/// Läuft, bis `shell.read()` EOF liefert (leerer Chunk, s.
/// `ssh_transport::RusshShell::read`), ein Lesefehler auftritt, oder der
/// Sender-Teil des Kommando-Kanals gedroppt wird (Session verworfen).
pub fn spawn_terminal_actor(
    session_id: SessionId,
    session: Arc<Session>,
    shell: Box<dyn ssh_manager_core::ssh::InteractiveShell>,
    commands: mpsc::Receiver<TerminalCommand>,
    emitter: Arc<dyn EventEmitter>,
    tasks: &crate::tasks::TaskScope,
) {
    spawn_terminal_actor_with_idle(
        session_id,
        session,
        shell,
        commands,
        emitter,
        tasks,
        crate::background_keepalive::IdleActivity::new(),
    );
}

/// Like [`spawn_terminal_actor`] with an injected wake-lock tracker (seam
/// for tests, issue #784).
pub fn spawn_terminal_actor_with_idle(
    session_id: SessionId,
    session: Arc<Session>,
    mut shell: Box<dyn ssh_manager_core::ssh::InteractiveShell>,
    mut commands: mpsc::Receiver<TerminalCommand>,
    emitter: Arc<dyn EventEmitter>,
    tasks: &crate::tasks::TaskScope,
    mut idle: crate::background_keepalive::IdleActivity,
) {
    // Issue #881: runs in the session scope; closing the session cancels it
    // (no further output event, no status event — the close path emits its
    // own).
    tasks.spawn("terminal-actor", async move {
        // `Some(reason)` löst am Ende ein `connection-status-changed`-Event
        // aus, `None` unterdrückt es bewusst — das passiert nur, wenn der
        // Sender absichtlich gedroppt wurde (`app_shell::commands::disconnect`
        // hat `session.terminal` auf `None` gesetzt), und dieser Befehl hat
        // das Event dafür bereits selbst gesendet. Ohne diese Unterscheidung
        // gäbe es bei jedem expliziten `disconnect()` zwei Events statt
        // eines (einmal vom Befehl, einmal vom hier endenden Aktor).
        // Issue #922: output chunks are collected and emitted as one event
        // after `TERMINAL_COALESCE_INTERVAL` or `TERMINAL_COALESCE_MAX_BYTES`;
        // order is kept and nothing is dropped.
        let mut pending: Vec<u8> = Vec::new();
        let mut flush_at: Option<tokio::time::Instant> = None;
        // Issue #784: wake lock while the terminal has recent input/output.
        // Failures are logged inside the controller and never end the loop.
        let disconnect_reason: Option<Option<String>> = loop {
            tokio::select! {
                read_result = shell.read() => {
                    match read_result {
                        Ok(data) if !data.is_empty() => {
                            idle.touch();
                            if pending.is_empty() {
                                flush_at = Some(tokio::time::Instant::now() + TERMINAL_COALESCE_INTERVAL);
                            }
                            pending.extend_from_slice(&data);
                            if pending.len() >= TERMINAL_COALESCE_MAX_BYTES {
                                terminal_output::flush_terminal_output(emitter.as_ref(), session_id, &mut pending, &mut flush_at);
                            }
                        }
                        Ok(_) => break Some(None), // leerer Chunk == EOF/Close (s. InteractiveShell-Doc)
                        Err(err) => break Some(Some(err.to_string())),
                    }
                }
                _ = async { tokio::time::sleep_until(flush_at.unwrap_or_else(tokio::time::Instant::now)).await },
                    if flush_at.is_some() =>
                {
                    terminal_output::flush_terminal_output(emitter.as_ref(), session_id, &mut pending, &mut flush_at);
                }
                _ = async { tokio::time::sleep_until(idle.deadline().unwrap_or_else(tokio::time::Instant::now)).await },
                    if idle.deadline().is_some() =>
                {
                    idle.release_if_idle(tokio::time::Instant::now());
                }
                cmd = commands.recv() => {
                    match cmd {
                        Some(TerminalCommand::Write(data)) => {
                            idle.touch();
                            if let Err(err) = shell.write(&data).await {
                                break Some(Some(err.to_string()));
                            }
                        }
                        Some(TerminalCommand::Resize(size)) => {
                            // Ein Resize-Fehler ist nicht fatal für die
                            // laufende Sitzung (z. B. kurzzeitiger
                            // Channel-Hänger) — anders als ein Lese-/
                            // Schreibfehler kein Grund, den Aktor zu
                            // beenden.
                            if let Err(err) = shell.resize(size).await {
                                eprintln!("terminal_resize fehlgeschlagen: {err}");
                            }
                        }
                        None => break None, // absichtlich beendet, s. o.
                    }
                }
            }
        };

        // Output read before the end is still shown.
        terminal_output::flush_terminal_output(emitter.as_ref(), session_id, &mut pending, &mut flush_at);

        if let Some(reason) = disconnect_reason {
            // Spec 0017, Abschnitt 2: unerwarteter Verbindungsabbruch (nicht
            // über den expliziten `disconnect()`-Befehl) muss sich auch in
            // `list_sessions()` widerspiegeln, solange der Tab im Frontend
            // noch offen ist — sonst zeigt eine wiederhergestellte Tab-Leiste
            // (Abschnitt 2, "Backend ist maßgebliche Quelle") eine tote
            // Session fälschlich als weiterhin verbunden an.
            report_connection_lost(session_id, &session, emitter.as_ref(), reason);
        }
    });
}
