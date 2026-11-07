use super::*;

pub(super) use app_logic::sftp_ops::{delete_recursive, walk_dirs_and_count_files};

/// Issue #658: the row of a recursive delete/chmod. A ticker reads the
/// operation's entry counters (monotonic, only ever increasing) and emits
/// throttled progress; the row appears once the recursion found a folder, so
/// a single file or a non-recursive chmod never gets one.
pub(super) struct EntryRows<'a> {
    sink: Option<(&'a dyn EventEmitter, SessionId)>,
    kind: SftpTransferKind,
    op: OpHandle,
    path: String,
    started: bool,
}

impl<'a> EntryRows<'a> {
    pub(super) fn new(
        sink: Option<(&'a dyn EventEmitter, SessionId)>,
        kind: SftpTransferKind,
        op: &OpHandle,
        path: &str,
    ) -> Self {
        Self {
            sink,
            kind,
            op: op.clone(),
            path: path.to_string(),
            started: false,
        }
    }

    /// Emits `started` (once) and the current counts, if the row exists.
    fn sync(&mut self) {
        let (Some((emitter, session_id)), Some((done, total))) = (self.sink, self.op.entries())
        else {
            return;
        };
        if !self.started {
            self.started = true;
            emit_sftp_transfer_started(
                emitter,
                session_id,
                self.op.id(),
                self.kind,
                self.path.clone(),
                None,
            );
        }
        emit_sftp_entries_progress(
            emitter,
            session_id,
            self.op.id(),
            SftpEntriesProgress { done, total },
        );
    }

    pub(super) async fn run<T>(&mut self, work: impl std::future::Future<Output = T>) -> T {
        if self.sink.is_none() {
            return work.await;
        }
        tokio::pin!(work);
        let mut tick = tokio::time::interval(crate::file_ops::PROGRESS_MIN_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                r = &mut work => return r,
                _ = tick.tick() => self.sync(),
            }
        }
    }

    /// Final state: done / cancelled / stopped / failed, with the number of
    /// entries changed. `Ok(false)` = stopped before the end.
    pub(super) fn finish(&mut self, outcome: Result<bool, String>) {
        self.sync();
        let (Some((emitter, session_id)), true) = (self.sink, self.started) else {
            return;
        };
        let done = self.op.entries().map_or(0, |(d, _)| d);
        let (error, stopped) = match outcome {
            Ok(true) => (None, None),
            Ok(false) => (
                None,
                Some(match self.op.stop_reason() {
                    Some(StopReason::Cancelled) => "cancelled",
                    _ => "stopped",
                }),
            ),
            Err(e) => (Some(e), None),
        };
        emit_sftp_entries_finished(emitter, session_id, self.op.id(), error, stopped, done);
    }
}

/// Spec 0054, Teil 3: Vorschau vor dem eigentlichen Löschen, analog zum
/// zweistufigen `delete_server` — bei einem Ordner zeigt das Frontend damit
/// "X Dateien, Y Ordner werden gelöscht" statt einer inhaltslosen
/// Ja/Nein-Frage. Für eine Datei ist das Ergebnis trivial (1 Datei, 0
/// Ordner), das Frontend ruft diesen Befehl trotzdem einheitlich für
/// beide Fälle auf.
#[tauri::command]
pub async fn sftp_delete_preview(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<app_logic::dto::DeletePreviewDto> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move { delete_preview_impl(&session, &channel, &path).await },
    )
    .await
}

pub(super) async fn delete_preview_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<app_logic::dto::DeletePreviewDto> {
    use app_logic::dto::DeletePreviewDto;

    let mut guard = lock_browser_sftp(session, channel).await;
    let mut sftp = guard.sftp().await?;

    // `lstat` statt `stat` — dieselbe Symlink-Begründung wie in
    // `delete_recursive` (dieselbe Vorschau soll die Zahlen zeigen, die
    // der anschließende `sftp_delete`-Aufruf tatsächlich löscht).
    let root_entry = sftp.lstat(path).await?;
    if !root_entry.is_dir {
        return Ok(DeletePreviewDto {
            file_count: 1,
            dir_count: 0,
        });
    }
    let (dirs, file_count) = walk_dirs_and_count_files(&mut sftp, path).await?;
    Ok(DeletePreviewDto {
        file_count,
        dir_count: dirs.len() as u64,
    })
}

/// Löschen einer Datei ODER eines Ordners (Spec 0054, Teil 3 hebt die
/// bisherige Datei-Beschränkung auf, s. Moduldoc-Kommentar oben) — die
/// Bestätigungsrückfrage selbst läuft im Frontend (Spec 0020, Abschnitt 5),
/// dieser Befehl führt sie nur noch aus.
///
/// Ordner werden bottom-up gelöscht: erst alle Dateien im gesamten Baum
/// (Reihenfolge egal), dann alle Verzeichnisse in umgekehrter
/// Entdeckungsreihenfolge (tiefste zuerst) — SFTP `RMDIR` verlangt ein
/// leeres Verzeichnis, ein Verzeichnis kann also erst entfernt werden,
/// nachdem alles darunter bereits weg ist.
#[tauri::command]
pub async fn sftp_delete(
    app: AppHandle,
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
        |session, channel| async move {
            let emitter = TauriEventEmitter(app);
            delete_impl_reporting(Some((&emitter, session_id)), &session, &channel, &path).await
        },
    )
    .await
}

#[cfg(test)]
pub(super) async fn delete_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<()> {
    delete_impl_reporting(None, session, channel, path).await
}

/// Issue #658: a recursive delete of a folder shows a row (events
/// `sftp-transfer-*` with kind `delete`); `progress: None` runs silently.
pub(super) async fn delete_impl_reporting(
    progress: Option<(&dyn EventEmitter, SessionId)>,
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<()> {
    let mut guard = lock_browser_sftp(session, channel).await;
    // Spec 0085, A1.4: **vor** den Operationen abfragen — nach einem Widerruf
    // mitten in der Rekursion fehlte die Audit-Zeile sonst ganz, obwohl schon
    // Einträge mit erhöhten Rechten gelöscht wurden.
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    // Issue #622: als laufende Operation eingetragen (Bestätigungsdialog).
    let op = channel.register_op(Uuid::new_v4(), OpKind::Delete, path);
    let mut rows = EntryRows::new(progress, SftpTransferKind::Delete, op.handle(), path);
    let result = rows
        .run(delete_recursive(&mut sftp, path, op.handle()))
        .await;
    rows.finish(
        result
            .as_ref()
            .map(|r| r.is_some())
            .map_err(|e| e.to_string()),
    );
    audit_elevated_change(
        elevated_user.as_ref(),
        "delete",
        path,
        matches!(result, Ok(Some(()))),
    );
    match result? {
        Some(()) => Ok(()),
        None => Err(stopped_error()),
    }
}
