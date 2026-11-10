use super::*;

#[tauri::command]
pub async fn sftp_list(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<Vec<RemoteEntryDto>> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move { list_impl(&session, &channel, &path).await },
    )
    .await
}

/// Spec 0102: Startpfad des Dateibrowsers dieser Sitzung (`"."` = Home)
/// und ggf. der einmalige Hinweis, dass das konfigurierte Startverzeichnis
/// fehlt. Läuft über den normalen SFTP-Kanal der Sitzung — das
/// Startverzeichnis gilt für den Login-Nutzer, nicht für den erhöhten Modus.
#[tauri::command]
pub async fn sftp_start_directory(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<app_logic::start_directory::StartDirectoryDto> {
    let session = app_logic::session::require_session(&state.sessions, session_id)?;
    Ok(app_logic::start_directory::file_browser_start(&session).await)
}

/// Issue #544: whether the login user of this session is root (`id -u` probe
/// at connect). `None` = unknown. The file browser hides its elevation
/// controls only for `Some(true)`; the backend's elevation checks are unchanged.
#[tauri::command]
pub async fn session_is_root(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<Option<bool>> {
    let session = app_logic::session::require_session(&state.sessions, session_id)?;
    Ok(session.is_root())
}

pub(super) async fn list_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<Vec<RemoteEntryDto>> {
    let mut dtos = {
        let mut guard = lock_browser_sftp(session, channel).await;
        let mut sftp = guard.sftp().await?;
        let mut entries = sftp.list_dir(path).await?;
        // Issue #653: SFTP v3 servers often send only uid/gid; resolve names
        // once per session from /etc/passwd and /etc/group (SFTP reads only).
        if entries.iter().any(|e| {
            (e.owner.is_none() && e.uid.is_some()) || (e.group.is_none() && e.gid.is_some())
        }) {
            let names = session
                .id_names_or_init(|| app_logic::owner_names::load_id_names(&mut sftp))
                .await;
            app_logic::owner_names::apply_id_names(names, &mut entries);
        }
        // Issue #1291: resolve links (one stat + one readlink each).
        app_logic::file_browser::resolve_symlinks(&mut sftp, &entries).await
    };
    sort_remote_entries(&mut dtos);
    Ok(dtos)
}

/// Spec 0054, Teil 3: existiert ein Zielpfad bereits? Grundlage für die
/// Kollisionsprüfung bei "Umbenennen"/"Verschieben" (beide laufen über
/// dieselbe `sftp_rename` unten — SFTP `RENAME` versteht keinen Unterschied
/// zwischen "im selben Ordner umbenennen" und "in einen anderen Ordner
/// verschieben") und bei "Hochladen" (Überschreib-Erkennung vor der
/// Diff-Vorschau). `stat()` ist hier bewusst der einzige Signalweg — kein
/// gesonderter `exists()`-Trait-Befehl, das SFTP-Protokoll kennt ohnehin
/// keine schnellere Existenzprüfung als `STAT`.
#[tauri::command]
pub async fn sftp_exists(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<bool> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move { exists_impl(&session, &channel, &path).await },
    )
    .await
}

pub(super) async fn exists_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<bool> {
    let mut guard = lock_browser_sftp(session, channel).await;
    let mut sftp = guard.sftp().await?;
    // `is_ok()`: „gibt es den Pfad?" ist genau die Frage, ein fehlender Pfad
    // also kein Fehler. Ein **Widerruf** verschwindet dadurch nicht still —
    // er steht im Abbruch-Vermerk des Befehls und macht das Ergebnis beim
    // Abschluss zu `ELEVATED_CHANNEL_INACTIVE` statt zu `Ok(false)` (Spec
    // 0085, A1.2/T6c).
    Ok(sftp.stat(path).await.is_ok())
}

/// Spec 0054, Teil 4: einzelnen Eintrag abfragen — Grundlage für die
/// Konflikt-Prüfung beim "Lokal öffnen"-Upload ("hat sich die Remote-Datei
/// seit dem Download geändert?", s. `local.ts`/`useLocalEditSession`s
/// Vergleich von `RemoteEntryDto.modified` vor Download gegen einen
/// frischen `sftp_stat`-Aufruf vor dem Hochladen). Bislang gab es dafür nur
/// `sftp_list` (ganzes Verzeichnis) und `sftp_exists` (nur `bool`) — ein
/// generischer Einzelabfrage-Befehl fehlte.
#[tauri::command]
pub async fn sftp_stat(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<RemoteEntryDto> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let mut guard = lock_browser_sftp(&session, &channel).await;
            let mut sftp = guard.sftp().await?;
            let entry = sftp.stat(&path).await?;
            Ok(RemoteEntryDto::from(&entry))
        },
    )
    .await
}
