use super::*;

/// Lädt genau eine Remote-Datei nach `local_path` herunter — der
/// eigentliche Transfer-Kern hinter `sftp_download`, `sftp_download_default`
/// und `sftp_download_dir` (Ordner-Rekursion, s. `download_recursive`
/// unten): jeweils ein `sftp-transfer-started`/`-finished`-Ereignispaar (s.
/// `app_logic::events`-Moduldoc zur Fortschritts-Design-Entscheidung), dann
/// Lesen per SFTP + lokales Schreiben via `spawn_blocking` (Downloads können
/// beliebig groß sein, Spec 0020 Abschnitt 5 verlangt ausdrücklich, dass
/// Transfers die Session nicht blockieren).
/// Spec 0086, T8: `emitter` statt eines `AppHandle` — der Transfer-Kern baute
/// den Emitter bisher selbst, damit war er für keinen Test erreichbar und die
/// Meldung im Ereignis nirgends abgedeckt. `app_logic::events::EventEmitter`
/// ist dieselbe Abstraktion, die der Produktivpfad benutzt
/// (`TauriEventEmitter`), also keine Testsonderbahn.
pub(super) async fn download_one_file(
    emitter: &dyn EventEmitter,
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    remote_path: &str,
    local_path: std::path::PathBuf,
    total_bytes: Option<u64>,
) -> CommandResult<()> {
    // Issue #622: in Stücken, mit Fortschritt und Abbruch — s. `transfer`.
    let transfer_id = Uuid::new_v4();
    let guard = channel.register_op(transfer_id, OpKind::Download, remote_path);
    guard.handle().set_bytes_total(total_bytes);
    let run = FileRun {
        emitter,
        session,
        channel,
        session_id,
        op: guard.handle(),
        folder: false,
    };
    download_file(&run, transfer_id, remote_path, local_path, total_bytes, 0).await?;
    Ok(())
}

/// Spec 0020, Abschnitt 5: "nativer Speichern-Dialog" — derselbe
/// oneshot-Kanal-Umweg wie `export_document` (dortiger Doc-Kommentar erklärt
/// das Warum). Datei-only (s. Moduldoc "Design-Entscheidung" oben) und
/// per Dialog an einen präzisen lokalen Zielpfad — Ordner-Download und
/// Download ohne Dialog sind `sftp_download_dir`/`sftp_download_default`
/// (Spec 0054, Teil 2) weiter unten.
#[tauri::command]
pub async fn sftp_download(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    remote_path: String,
    elevated_user: Option<String>,
) -> CommandResult<Option<app_logic::dto::DownloadResultDto>> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            download_impl(&app, &session, &channel, session_id, &remote_path).await
        },
    )
    .await
}

pub(super) async fn download_impl(
    app: &AppHandle,
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    remote_path: &str,
) -> CommandResult<Option<app_logic::dto::DownloadResultDto>> {
    use tauri_plugin_dialog::DialogExt;

    let file_name = file_name_of(remote_path);

    // Größe vorab für die Fortschrittsanzeige — ein fehlgeschlagenes
    // `stat()` (z. B. eingeschränkte Leserechte aufs Elternverzeichnis)
    // blockiert den eigentlichen Download nicht, die Anzeige zeigt dann
    // schlicht keine Gesamtgröße. Ein **Widerruf** an dieser Stelle wird
    // dadurch nicht verschluckt: er ist im Abbruch-Vermerk des Befehls
    // festgehalten und schlägt beim Abschluss durch (Spec 0085, A1.2).
    let total_bytes = {
        let mut guard = lock_browser_sftp(session, channel).await;
        let mut sftp = guard.sftp().await?;
        sftp.stat(remote_path).await.ok().map(|entry| entry.size)
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(&file_name)
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(local_path) = rx.await.ok().flatten() else {
        return Ok(None); // Abbrechen ist kein Fehler, s. `export_document`.
    };
    // Issue #679: a `content://` URI (Android) is never a path. The file is
    // downloaded into an app-private staging copy and then written through
    // the URI; progress and cancellation work as for any download.
    let handle = crate::local_file::LocalFileHandle::from_dialog(local_path)?;
    let (target, staging) = match handle.as_path() {
        Some(p) => (p.to_path_buf(), None),
        None => {
            let dir = crate::edit_copies::picked_root()
                .ok_or_else(|| CommandError::from("No cache directory available"))?
                .join(Uuid::new_v4().to_string());
            (dir.join(&file_name), Some(dir))
        }
    };
    if let Some(dir) = &staging {
        std::fs::create_dir_all(dir)?;
    }

    let downloaded = download_one_file(
        &TauriEventEmitter(app.clone()),
        session,
        channel,
        session_id,
        remote_path,
        target.clone(),
        total_bytes,
    )
    .await;
    let copied = match (&staging, &downloaded) {
        (Some(_), Ok(())) => {
            let opener = crate::local_file::platform_opener(app);
            let from = target.clone();
            let h = handle.clone();
            tokio::task::spawn_blocking(move || h.copy_from(opener.as_ref(), &from))
                .await
                .map_err(|e| CommandError::from(e.to_string()))
                .and_then(|r| r.map(|_| ()).map_err(CommandError::from))
        }
        _ => Ok(()),
    };
    if let Some(dir) = &staging {
        if let Err(err) = std::fs::remove_dir_all(dir) {
            tracing::warn!(error = %err, "could not remove the staged download");
        }
    }
    downloaded?;
    copied?;
    Ok(Some(app_logic::dto::DownloadResultDto {
        local_path: handle.display(),
        is_dir: false,
        file_count: 1,
    }))
}

/// Folder picker. Desktop only: `tauri-plugin-dialog` offers no folder
/// picker on Android (Spec 0114); there the UI does not show the actions and
/// a direct call is refused. `None` = the user cancelled.
#[cfg(not(target_os = "android"))]
pub(super) async fn pick_folder_dialog(
    app: &AppHandle,
    title: &str,
) -> CommandResult<Option<std::path::PathBuf>> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .pick_folder(move |path| {
            let _ = tx.send(path);
        });
    match rx.await.ok().flatten() {
        Some(picked) => Ok(Some(picked.into_path()?)),
        None => Ok(None),
    }
}

#[cfg(target_os = "android")]
pub(super) async fn pick_folder_dialog(
    _app: &AppHandle,
    _title: &str,
) -> CommandResult<Option<std::path::PathBuf>> {
    Err(CommandError::from(
        "Folder selection is not available on Android",
    ))
}

/// Ermittelt das Standard-Downloadverzeichnis des Betriebssystems (Spec
/// 0054, Teil 2: "Standard-Downloadverzeichnis ODER präziser Pfad per
/// Dialog") — `directories::UserDirs` ist bereits Projektabhängigkeit (s.
/// `logging.rs`).
pub(super) fn default_downloads_dir() -> CommandResult<std::path::PathBuf> {
    // Android (Spec 0114): kein `HOME`; app-privater Ordner als Ersatz.
    #[cfg(target_os = "android")]
    {
        Ok(persistence_sqlite::android_files_dir().join("downloads"))
    }
    #[cfg(not(target_os = "android"))]
    {
        directories::UserDirs::new()
            .and_then(|dirs| dirs.download_dir().map(|p| p.to_path_buf()))
            .ok_or("Kein Standard-Downloadverzeichnis gefunden")
            .map_err(CommandError::from)
    }
}

/// Lists one directory through the browser channel, taking the channel lock
/// per call (see `download_recursive`).
struct BrowserDirLister<'a> {
    session: &'a Session,
    channel: &'a BrowserChannel,
}

#[async_trait::async_trait]
impl app_logic::sftp_ops::DirLister for BrowserDirLister<'_> {
    async fn list_dir(
        &mut self,
        path: &str,
    ) -> CommandResult<Vec<ssh_manager_core::ssh::RemoteEntry>> {
        let mut guard = lock_browser_sftp(self.session, self.channel).await;
        let mut sftp = guard.sftp().await?;
        Ok(sftp.list_dir(path).await?)
    }
}

/// Rekursiver Ordner-Download (Spec 0054, Teil 2: "Ordner rekursiv"):
/// listet iterativ (kein async-rekursiver Aufruf nötig — vermeidet das
/// Boxing, das ein `async fn`, das sich selbst aufruft, in Rust braucht)
/// über eine Arbeits-Warteschlange, legt lokale Unterordner an und lädt
/// jede gefundene Datei einzeln über `download_one_file` — dadurch bekommt
/// jede Datei ihr eigenes `sftp-transfer-started`/`-finished`-Paar, die
/// Transfer-Liste im Frontend zeigt also automatisch den Fortschritt über
/// den ganzen Baum, ohne einen zweiten Fortschritts-Mechanismus.
pub(super) async fn download_recursive(
    emitter: &dyn EventEmitter,
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    remote_root: &str,
    local_root: &std::path::Path,
) -> CommandResult<u64> {
    // Issue #622: ein Vorgang für den ganzen Ordner (Bestätigungsdialog,
    // Abbrechen), erst den Baum durchlaufen (Dateizahl und Gesamtgröße
    // bekannt), dann Datei für Datei in Stücken laden.
    let op_guard = channel.register_op(Uuid::new_v4(), OpKind::Download, remote_root);
    let op = op_guard.handle();
    tokio::fs::create_dir_all(local_root).await?;
    // The walk lives in `app_logic::sftp_ops`; the channel lock is taken per
    // listing (by `BrowserDirLister`) so a revocation is noticed between them.
    let mut lister = BrowserDirLister { session, channel };
    let found = app_logic::sftp_ops::walk_tree(&mut lister, remote_root, op)
        .await?
        .ok_or_else(stopped_error)?;
    let mut files: Vec<(String, std::path::PathBuf, u64)> = Vec::new();
    // Remote directory path -> its local counterpart. A directory's entry
    // always precedes the entries inside it, so the parent is known.
    let mut local_dirs: std::collections::HashMap<String, std::path::PathBuf> =
        std::collections::HashMap::new();
    local_dirs.insert(remote_root.to_string(), local_root.to_path_buf());
    for (remote_dir, entry) in found {
        safe_local_segment(&entry.name)?;
        let local_dir = local_dirs
            .get(&remote_dir)
            .ok_or_else(|| CommandError::from("internal error: unknown parent directory"))?;
        let local_entry_path = local_dir.join(&entry.name);
        if entry.is_dir {
            tokio::fs::create_dir_all(&local_entry_path).await?;
            local_dirs.insert(entry.path, local_entry_path);
        } else {
            files.push((entry.path, local_entry_path, entry.size));
        }
    }
    op.set_files(0, Some(files.len() as u64));
    op.set_bytes_total(Some(files.iter().map(|f| f.2).sum()));
    let run = FileRun {
        emitter,
        session,
        channel,
        session_id,
        op,
        folder: true,
    };
    let mut file_count = 0u64;
    let mut base_bytes = 0u64;
    for (remote_path, local_path, size) in files {
        if op.is_user_stopped() {
            return Err(stopped_error());
        }
        base_bytes += download_file(
            &run,
            Uuid::new_v4(),
            &remote_path,
            local_path,
            Some(size),
            base_bytes,
        )
        .await?;
        op.add_files_done();
        file_count += 1;
    }
    Ok(file_count)
}

/// Lädt `remote_path` (Datei oder Ordner) unter `local_base_dir` herunter —
/// gemeinsame Logik von `sftp_download_default` und `sftp_download_dir`.
/// Eine Datei landet direkt als `local_base_dir/<dateiname>`, ein Ordner
/// als `local_base_dir/<ordnername>/...` (rekursiv) — nie werden die
/// Inhalte eines Ordners direkt lose in `local_base_dir` verstreut, das
/// bliebe sonst nicht als "der heruntergeladene Ordner" wiedererkennbar.
pub(super) async fn download_entry_to(
    emitter: &dyn EventEmitter,
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    remote_path: &str,
    local_base_dir: &std::path::Path,
) -> CommandResult<app_logic::dto::DownloadResultDto> {
    let root_name = file_name_of(remote_path);
    // `remote_path` kommt vom Frontend, letztlich aber aus einem früheren
    // `sftp_list`-Ergebnis (`RemoteEntryDto.path`) — also transitiv
    // server-kontrolliert. Dieselbe Zip-Slip-Prüfung wie in
    // `download_recursive` für jeden rekursiv entdeckten Eintrag.
    safe_local_segment(&root_name)?;
    let root_entry = {
        let mut guard = lock_browser_sftp(session, channel).await;
        let mut sftp = guard.sftp().await?;
        sftp.stat(remote_path).await?
    };
    let local_path = local_base_dir.join(&root_name);
    let file_count = if root_entry.is_dir {
        download_recursive(
            emitter,
            session,
            channel,
            session_id,
            remote_path,
            &local_path,
        )
        .await?
    } else {
        download_one_file(
            emitter,
            session,
            channel,
            session_id,
            remote_path,
            local_path.clone(),
            Some(root_entry.size),
        )
        .await?;
        1
    };
    Ok(app_logic::dto::DownloadResultDto {
        local_path: local_path.to_string_lossy().into_owned(),
        is_dir: root_entry.is_dir,
        file_count,
    })
}

/// Spec 0054, Teil 2: Herunterladen ohne Dialog, direkt ins
/// Standard-Downloadverzeichnis — Datei oder Ordner (rekursiv).
#[tauri::command]
pub async fn sftp_download_default(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    remote_path: String,
    elevated_user: Option<String>,
) -> CommandResult<app_logic::dto::DownloadResultDto> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let downloads_dir = default_downloads_dir()?;
            download_entry_to(
                &TauriEventEmitter(app.clone()),
                &session,
                &channel,
                session_id,
                &remote_path,
                &downloads_dir,
            )
            .await
        },
    )
    .await
}

/// Spec 0054, Teil 2: Ordner-Download an einen per Dialog gewählten
/// Zielort — Gegenstück zu `sftp_download`s Datei-Speichern-Dialog, nur
/// dass ein Ordner keinen Dateinamen zum Speichern hat, sondern ein
/// Zielverzeichnis braucht (`pick_folder` statt `save_file`). Issue #153:
/// the dialog title comes translated from the frontend (pattern:
/// `pick_upload_files`); it is display-only.
#[tauri::command]
pub async fn sftp_download_dir(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    remote_path: String,
    title: String,
    elevated_user: Option<String>,
) -> CommandResult<Option<app_logic::dto::DownloadResultDto>> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let Some(local_dir) = pick_folder_dialog(&app, &title).await? else {
                return Ok(None); // Abbrechen ist kein Fehler.
            };

            download_entry_to(
                &TauriEventEmitter(app.clone()),
                &session,
                &channel,
                session_id,
                &remote_path,
                &local_dir,
            )
            .await
            .map(Some)
        },
    )
    .await
}

/// Issue #622: cancel button of a transfer row. Stops the transfer (for a
/// folder transfer: the whole folder) at the next chunk; the row shows
/// "cancelled". `false`: no such running transfer (already finished).
#[tauri::command]
pub async fn sftp_cancel_transfer(
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    transfer_id: Uuid,
) -> CommandResult<bool> {
    Ok(elevated.ops().cancel(session_id, transfer_id))
}
