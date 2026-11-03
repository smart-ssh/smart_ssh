//! Checks that refuse a server input before anything is read or written.

use ssh_manager_core::shared::ServerId;

use crate::error::{CommandError, CommandResult};

/// Spec 0032, Abschnitt 6: der lokale Pseudo-Server ist explizit als
/// Jump-Host ausgeschlossen — vor dieser Prüfung fiel das erst implizit,
/// tief in `resolve_connection_target`, mit einer generischen "nicht
/// auflösbar"-Meldung auf (unabhängiger Review-Pass, s. docs/adr/0026).
///
/// Spec 0082, A6/A7: lag bis dahin als private Funktion im
/// `app-shell`-Command-Modul. Sie zieht hierher, weil [`update_server`]
/// sie **innerhalb** der Tauri-freien Fassung aufrufen muss — die
/// Ablehnung gehört vor jedes Lesen und Schreiben im Schlüsselbund und
/// darf deshalb nicht in einer Schicht sitzen, die kein Unit-Test
/// erreicht. `commands::create_server` ruft unverändert dieselbe Prüfung,
/// nur über diesen Pfad.
pub fn reject_local_jump_host(jump_host: Option<ServerId>) -> CommandResult<()> {
    if jump_host.is_some_and(crate::dto::is_local) {
        return Err(CommandError::with_code(
            "Der lokale Pseudo-Server kann nicht als Jump-Host verwendet werden",
            "SERVER_JUMP_HOST_LOCAL",
        ));
    }
    Ok(())
}
