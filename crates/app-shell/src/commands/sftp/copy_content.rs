use super::*;

/// Obergrenze für die Remote-Seite der Upload-Diff-Vorschau (`sftp_read_text`;
/// „Dateiinhalt kopieren" hat seit Issue #357 eine eigene, größere Grenze,
/// [`MAX_COPY_CONTENT_BYTES`]) UND für die lokale Seite (Teil 3, `read_local_text_preview` unten). Bewusst
/// **kein** gemeinsamer Code-Pfad und keine gemeinsame Konstante mit
/// `orchestration::ReadRemoteFile`s 256-KB-Cap (Spec 0020, Abschnitt
/// 4.1) — das liefe für einen manuellen Klick über KI-Infrastruktur
/// (Redaction, Filter-Mapping), genau die Vermischung, die Spec 0054s
/// Sicherheitsmodell ausschließt ("manuelle Aktionen laufen NIE durch
/// KI-/Filter-Code, auch nicht nur durch eine Hilfsfunktion davon"). Der
/// gleiche Zahlenwert ist reiner Zufall gleich guter Praxis, keine
/// geteilte Definition.
pub(super) const MAX_TEXT_PREVIEW_BYTES: u64 = 256 * 1024;

/// Spec 0086, A1.6: die Ablehnungs-Meldung der Vorschau (`sftp_read_text`) —
/// **wörtlich dieselbe** vor und nach dem Lesen. An einer Stelle gebildet
/// statt an zwei formatiert: der Nutzer soll nicht daran ablesen können, an
/// welcher der beiden Prüfungen es lag (und zwei Formatierungen wären
/// auseinandergelaufen).
pub(super) fn too_large_for_clipboard() -> CommandError {
    CommandError::with_code(
        format!(
            "Datei ist größer als {} KB — zu groß zum Kopieren in die Zwischenablage",
            MAX_TEXT_PREVIEW_BYTES / 1024
        ),
        SFTP_PREVIEW_TOO_LARGE,
    )
}

/// Spec 0054, Teil 2: "Dateiinhalt kopieren" — liest eine Remote-Datei als
/// Text für die Zwischenablage (der eigentliche `writeText`-Aufruf passiert
/// im Frontend, s. `navigator.clipboard` dort). Kein Filter-Engine-/KI-Gate
/// (Sicherheitsmodell, Spec 0054): eine direkte, unkritische Nutzeraktion,
/// wie jeder andere `sftp_*`-Befehl in diesem Abschnitt.
///
/// Größenprüfung vor dem eigentlichen Lesen (per `stat`), damit eine sehr
/// große Datei nicht erst vollständig übertragen wird, bevor sie doch
/// abgelehnt wird — ein fehlgeschlagenes `stat` blockiert den Lesevorgang
/// selbst nicht (analog zu `sftp_download`s Größen-Vorablauf oben), die
/// eigentliche `read_file`-Fehlermeldung ist dann aussagekräftig genug.
#[tauri::command]
pub async fn sftp_read_text(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<String> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move { read_text_impl(&session, &channel, &path).await },
    )
    .await
}

pub(super) async fn read_text_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<String> {
    let mut guard = lock_browser_sftp(session, channel).await;
    let mut sftp = guard.sftp().await?;

    // Ein fehlgeschlagenes `stat` blockiert das Lesen nicht (s.
    // Doc-Kommentar). Ein Widerruf **zwischen** `stat` und `read_file` lässt
    // die Datei ungelesen: die zweite Operation prüft ihn selbst (Spec 0085,
    // A1.1/T6b).
    if let Ok(entry) = sftp.stat(path).await {
        if entry.size > MAX_TEXT_PREVIEW_BYTES {
            return Err(too_large_for_clipboard());
        }
    }

    let bytes = sftp.read_file(path).await?;
    // Spec 0086, A1.1: Die `stat`-Prüfung oben ist eine Vorabprüfung, keine
    // Zusage. Die Kanal-Sperre wird je Operation genommen (Spec 0085, A1.3;
    // ADR 0078 §1), zwischen `stat` und `read_file` kann also ein anderer
    // Befehl laufen und die Datei wachsen — und ein gescheitertes `stat`
    // lässt sie bewusst ganz ungeprüft (s. Doc-Kommentar oben). Maßgeblich
    // ist deshalb diese Prüfung am tatsächlich gelesenen Inhalt: nur sie
    // kann die Grenze halten, die der Nutzer zugesagt bekommt.
    if bytes.len() as u64 > MAX_TEXT_PREVIEW_BYTES {
        return Err(too_large_for_clipboard());
    }
    String::from_utf8(bytes).map_err(|_| {
        CommandError::with_code(
            "Datei ist keine Textdatei (kein gültiges UTF-8)",
            SFTP_PREVIEW_NOT_TEXT,
        )
    })
}

/// Issue #357, Spec 0086 A1.1: Obergrenze für „Dateiinhalt kopieren"
/// (50 × 1024 × 1024 Bytes, fest, normaler und erhöhter Kanal). Eine Datei
/// von genau dieser Größe ist erlaubt. Unabhängig von
/// [`MAX_TEXT_PREVIEW_BYTES`] (Vorschau) und dem KI-Lesepfad.
pub(super) const MAX_COPY_CONTENT_BYTES: u64 = 50 * 1024 * 1024;

/// Fortschritt wird in Schritten dieser Größe gemeldet, nicht je Lesestück.
pub(super) const COPY_PROGRESS_STEP_BYTES: u64 = 1024 * 1024;

pub(super) fn content_too_large_for_clipboard() -> CommandError {
    CommandError::with_code(
        "The file is larger than 50 MB and cannot be copied to the clipboard.",
        SFTP_CONTENT_TOO_LARGE,
    )
}

pub(super) fn copy_cancelled() -> CommandError {
    CommandError::with_code("Copying was cancelled.", SFTP_COPY_CANCELLED)
}

/// Fortschritt von „Dateiinhalt kopieren": `done` Bytes gelesen, `total`
/// die per `stat` gemeldete Größe (`None`, wenn `stat` scheiterte).
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyContentProgress {
    pub done: u64,
    pub total: Option<u64>,
}

/// Laufende „Dateiinhalt kopieren"-Aufträge nach Anfrage-ID, damit
/// `sftp_cancel_copy_content` den Lesevorgang beenden kann.
#[derive(Default)]
pub struct CopyContentJobs(
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Notify>>>,
);

impl CopyContentJobs {
    pub(super) fn register(&self, request_id: &str) -> std::sync::Arc<tokio::sync::Notify> {
        let notify = std::sync::Arc::new(tokio::sync::Notify::new());
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(request_id.to_string(), notify.clone());
        notify
    }

    pub(super) fn remove(&self, request_id: &str) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(request_id);
    }

    pub(super) fn cancel(&self, request_id: &str) {
        if let Some(notify) = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(request_id)
        {
            notify.notify_one();
        }
    }
}

/// Issue #357: „Dateiinhalt kopieren" — liest eine Remote-Textdatei bis
/// 50 MB für die Zwischenablage (der `writeText`-Aufruf passiert im
/// Frontend). Anders als [`sftp_read_text`] (Vorschau des Upload-Diffs,
/// 256 KB) liest dieser Befehl **begrenzt**: nach Grenze + 1 Bytes ist
/// Schluss, auch wenn `stat` scheitert; der Speicher hält nie mehr. Der
/// Fortschritt geht über `on_progress`, `sftp_cancel_copy_content` bricht
/// ab. Kein Filter-Engine-/KI-Gate (Spec 0054, Sicherheitsmodell).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn sftp_copy_content(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    jobs: State<'_, CopyContentJobs>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
    request_id: String,
    on_progress: Channel<CopyContentProgress>,
) -> CommandResult<String> {
    let cancel = jobs.register(&request_id);
    let result = run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| {
            let cancel = cancel.clone();
            async move {
                let mut send = |p: CopyContentProgress| {
                    let _ = on_progress.send(p);
                };
                copy_content_impl(
                    &session,
                    &channel,
                    &path,
                    MAX_COPY_CONTENT_BYTES,
                    &cancel,
                    &mut send,
                )
                .await
            }
        },
    )
    .await;
    jobs.remove(&request_id);
    result
}

/// Bricht einen laufenden `sftp_copy_content` ab (unbekannte ID: nichts).
#[tauri::command]
pub async fn sftp_cancel_copy_content(
    jobs: State<'_, CopyContentJobs>,
    request_id: String,
) -> CommandResult<()> {
    jobs.cancel(&request_id);
    Ok(())
}

pub(super) async fn copy_content_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    limit: u64,
    cancel: &tokio::sync::Notify,
    report: &mut (dyn FnMut(CopyContentProgress) + Send),
) -> CommandResult<String> {
    // Der Abbruch gilt auch, solange der Kanal noch von einem anderen Befehl
    // gehalten wird; das Verwerfen des Futures gibt die Sperre frei.
    tokio::select! {
        result = copy_content_read(session, channel, path, limit, report) => result,
        _ = cancel.notified() => Err(copy_cancelled()),
    }
}

pub(super) async fn copy_content_read_bytes(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    limit: u64,
    report: &mut (dyn FnMut(CopyContentProgress) + Send),
) -> CommandResult<Vec<u8>> {
    let mut guard = lock_browser_sftp(session, channel).await;
    let mut sftp = guard.sftp().await?;

    // Ein fehlgeschlagenes `stat` blockiert das Lesen nicht; die Grenze hält
    // dann das begrenzte Lesen unten.
    let mut total = None;
    if let Ok(entry) = sftp.stat(path).await {
        if entry.size > limit {
            return Err(content_too_large_for_clipboard());
        }
        total = Some(entry.size);
    }

    let mut last_reported = 0u64;
    let mut on_chunk = |done: u64| {
        if last_reported == 0 || done >= last_reported + COPY_PROGRESS_STEP_BYTES {
            last_reported = done;
            report(CopyContentProgress { done, total });
        }
    };
    // Grenze + 1: ein Byte mehr als erlaubt beweist die Überschreitung, ohne
    // den Rest der Datei zu lesen.
    let bytes = sftp
        .read_file_capped(path, limit.saturating_add(1), &mut on_chunk)
        .await?;
    if bytes.len() as u64 > limit {
        return Err(content_too_large_for_clipboard());
    }
    Ok(bytes)
}

pub(super) async fn copy_content_read(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    limit: u64,
    report: &mut (dyn FnMut(CopyContentProgress) + Send),
) -> CommandResult<String> {
    let bytes = copy_content_read_bytes(session, channel, path, limit, report).await?;
    String::from_utf8(bytes).map_err(|_| {
        CommandError::with_code(
            "Datei ist keine Textdatei (kein gültiges UTF-8)",
            SFTP_PREVIEW_NOT_TEXT,
        )
    })
}

/// Issue #358: "Copy file" — reads a remote file (any type) up to 50 MB with
/// the same bounded read, progress and cancel as [`sftp_copy_content`]
/// (cancel via `sftp_cancel_copy_content`), stages it in the app-private
/// clipboard folder under its original name and puts it on the system
/// clipboard as a file reference. Nothing is opened or executed. A cancelled
/// or failed copy leaves no staged file and does not touch the clipboard; the
/// previous staged file is removed only after the new one is on the clipboard.
/// Returns the file name. No filter-engine/AI gate (Spec 0054 security model).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn sftp_copy_file(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    jobs: State<'_, CopyContentJobs>,
    clipboard: State<'_, crate::file_clipboard::FileClipboard>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
    request_id: String,
    on_progress: Channel<CopyContentProgress>,
) -> CommandResult<()> {
    let file_name = file_name_of(&path);
    safe_local_segment(&file_name)?;
    let cancel = jobs.register(&request_id);
    let result = run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| {
            let cancel = cancel.clone();
            async move {
                let mut send = |p: CopyContentProgress| {
                    let _ = on_progress.send(p);
                };
                tokio::select! {
                    result = copy_content_read_bytes(
                        &session, &channel, &path, MAX_COPY_CONTENT_BYTES, &mut send,
                    ) => result,
                    _ = cancel.notified() => Err(copy_cancelled()),
                }
            }
        },
    )
    .await;
    jobs.remove(&request_id);
    let bytes = result?;

    let root = crate::edit_copies::clipboard_root()
        .ok_or_else(|| CommandError::from("No cache directory available for the clipboard"))?;
    let staged = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || {
            crate::file_clipboard::stage_file(&root, &file_name, &bytes)
        })
        .await
        .map_err(|e| format!("Background task for Copy file failed: {e}"))??
    };
    if let Err(err) = clipboard.set_file(&staged) {
        crate::file_clipboard::discard(&staged);
        return Err(CommandError::with_code(
            format!("The file could not be placed on the clipboard: {err}"),
            app_logic::error::CLIPBOARD_FILE_FAILED,
        ));
    }
    crate::file_clipboard::retire_others(&root, &staged);
    Ok(())
}

/// Spec 0054, Teil 3: die "neue" (lokale) Seite der Upload-Überschreib-Diff-
/// Vorschau — Gegenstück zu `sftp_read_text` für die "alte" (Remote-)Seite,
/// nur **graceful** statt fehlschlagend: eine zu große oder nicht-Text-Datei
/// liefert `text: None` (das Frontend zeigt dann einen Größenvergleich-
/// Hinweis statt eines Diffs, analog zu `BinaryFileChangeHint` bei
/// KI-Dateischreibvorgängen, Spec 0020 Abschnitt 4.2), statt den ganzen
/// Upload-Bestätigungsdialog mit einem Fehler abzubrechen — anders als bei
/// "Dateiinhalt kopieren" ist eine Binärdatei hier ein erwarteter,
/// alltäglicher Fall (Uploads sind keine Textdateien), kein Ausnahmefall.
///
/// Issue #89: same rule as [`sftp_upload`] — `local_path` must lie inside a
/// grant of this session, otherwise nothing is read.
#[tauri::command]
pub async fn read_local_text_preview(
    state: State<'_, AppState>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
    local_path: String,
) -> CommandResult<app_logic::dto::LocalFilePreviewDto> {
    read_local_text_preview_impl(
        &state.sessions,
        grants.inner(),
        crate::edit_copies::instance_root().as_deref(),
        session_id,
        &local_path,
    )
    .await
}

pub(super) async fn read_local_text_preview_impl(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
) -> CommandResult<app_logic::dto::LocalFilePreviewDto> {
    use app_logic::dto::LocalFilePreviewDto;

    let local_path = authorize_local_path(sessions, grants, edit_root, session_id, local_path)?;
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(local_path.as_path()))
        .await
        .map_err(|e| format!("Hintergrund-Task für Datei-Vorschau fehlgeschlagen: {e}"))??;
    let size = bytes.len() as u64;
    if size > MAX_TEXT_PREVIEW_BYTES {
        return Ok(LocalFilePreviewDto { text: None, size });
    }
    Ok(LocalFilePreviewDto {
        text: String::from_utf8(bytes).ok(),
        size,
    })
}
