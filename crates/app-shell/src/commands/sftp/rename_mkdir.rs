use super::*;

/// Spec 0054, Teil 3: "Umbenennen" UND "Verschieben" laufen über denselben
/// Befehl — SFTP `RENAME` unterscheidet nicht zwischen beidem, `to` kann im
/// selben Verzeichnis (Umbenennen) oder einem anderen (Verschieben)
/// liegen. Die Kollisionsprüfung (Zielname existiert schon) läuft im
/// Frontend **vor** diesem Aufruf über `sftp_exists` — dieser Befehl führt
/// nur noch aus, analog zu `sftp_delete`s Bestätigung.
/// Issue #625: Umbenennen/Verschieben auf einen vorhandenen **Dateinamen**,
/// nachdem der Nutzer „Überschreiben" bestätigt hat. SFTP `RENAME` ersetzt nie.
///
/// - Ziel ist ein Ordner: Fehler, nichts wird angefasst.
/// - Server mit `posix-rename@openssh.com`: ein atomarer Austausch.
/// - Sonst: Ziel entfernen, dann `rename`. Scheitert das `rename`, nennt der
///   Fehler, dass das Ziel schon entfernt ist und die Quelle unverändert bleibt.
///
/// Ein Fehler beim `stat` des Ziels gilt als „Ziel fehlt": dann ersetzt
/// nichts etwas, und das gewöhnliche `rename` entscheidet (es überschreibt
/// nie, und ein Widerruf der Erhöhung schlägt dort genauso an).
pub(super) async fn rename_replacing(
    sftp: &mut dyn SftpSession,
    from: &str,
    to: &str,
) -> Result<(), SshError> {
    if from == to {
        return Ok(());
    }
    match sftp.stat(to).await {
        Ok(entry) if entry.is_dir => {
            return Err(SshError::ChannelError(format!(
                "Das Ziel '{to}' ist ein Ordner und kann nicht ersetzt werden."
            )));
        }
        Ok(_) => {}
        Err(_) => return sftp.rename(from, to).await,
    }
    if sftp.posix_rename(from, to).await? {
        return Ok(());
    }
    sftp.remove(to).await?;
    sftp.rename(from, to).await.map_err(|e| {
        SshError::ChannelError(format!(
            "Das Ziel '{to}' wurde bereits entfernt, aber das Umbenennen von '{from}' ist \
             fehlgeschlagen; die Quelle ist unverändert: {e}"
        ))
    })
}

#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    from: String,
    to: String,
    overwrite: Option<bool>,
    elevated_user: Option<String>,
) -> CommandResult<()> {
    let overwrite = overwrite.unwrap_or(false);
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let mut guard = lock_browser_sftp(&session, &channel).await;
            let elevated_user = guard.elevated_user();
            let mut sftp = guard.sftp().await?;
            let result = if overwrite {
                rename_replacing(&mut sftp, &from, &to).await
            } else {
                sftp.rename(&from, &to).await
            };
            audit_elevated_change(
                elevated_user.as_deref(),
                if overwrite {
                    "rename-overwrite"
                } else {
                    "rename"
                },
                &format!("{from} -> {to}"),
                result.is_ok(),
            );
            result?;
            Ok(())
        },
    )
    .await
}

#[tauri::command]
pub async fn sftp_mkdir(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<()> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move { mkdir_impl(&session, &channel, &path).await },
    )
    .await
}

pub(super) async fn mkdir_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<()> {
    let mut guard = lock_browser_sftp(session, channel).await;
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    // Issue #570: ein vorhandener Name (Datei oder Ordner) wird mit
    // demselben Fehler gemeldet wie bei „Neue Datei". Scheitert die
    // Vorabprüfung selbst, entscheidet `create_dir` allein.
    let result = if sftp.lstat(path).await.is_ok() {
        Err(SshError::SftpAlreadyExists(path.to_string()))
    } else {
        sftp.create_dir(path).await
    };
    audit_elevated_change(elevated_user.as_deref(), "mkdir", path, result.is_ok());
    // Mit Code (z. B. `SSH_SFTP_ALREADY_EXISTS`), damit die Oberfläche die
    // Fehlerart ohne Textvergleich erkennt.
    result.map_err(|err| ssh_command_error(&err))
}

pub(super) async fn create_file_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<()> {
    let mut guard = lock_browser_sftp(session, channel).await;
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    let result = sftp.create_file_exclusive(path).await;
    audit_elevated_change(
        elevated_user.as_deref(),
        "create_file",
        path,
        result.is_ok(),
    );
    // Mit Code (z. B. `SSH_SFTP_ALREADY_EXISTS`), damit die Oberfläche die
    // Fehlerart ohne Textvergleich erkennt.
    result.map_err(|err| ssh_command_error(&err))
}

/// Issue #570: „Neue Datei" im Dateibrowser — legt eine leere Datei im
/// aktuellen Verzeichnis an, schmal und benannt wie `sftp_mkdir` (Invariante
/// 14: kein Filter-/KI-Pfad). Ein vorhandener Name wird nie überschrieben
/// (CREATE|EXCL, `SSH_SFTP_ALREADY_EXISTS`); fehlende Rechte werden einmal
/// versucht und gemeldet. Im erhöhten Modus läuft der Aufruf über den
/// erhöhten Kanal und wird wie `sftp_mkdir` protokolliert.
#[tauri::command]
pub async fn sftp_create_file(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<()> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move { create_file_impl(&session, &channel, &path).await },
    )
    .await
}
