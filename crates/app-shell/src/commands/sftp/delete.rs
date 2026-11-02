use super::*;

/// Sammelt rekursiv **alle** Verzeichnispfade unter `root` (inklusive
/// `root` selbst) — geteilte Traversierung für `sftp_delete_preview` und
/// den eigentlichen rekursiven Löschvorgang in `sftp_delete` unten. Liefert
/// zusätzlich die Anzahl der gefundenen Dateien, damit ein Aufrufer nicht
/// zweimal denselben Baum ablaufen muss.
///
/// Die zurückgegebenen Verzeichnispfade stehen in **Entdeckungsreihenfolge**
/// (ein Verzeichnis erscheint immer erst NACHDEM sein Elternverzeichnis
/// verarbeitet wurde) — das reicht, um sie für ein bottom-up-Löschen später
/// einfach umzudrehen (`.rev()`), unabhängig davon, ob hier DFS oder BFS
/// traversiert wird (s. `sftp_delete`).
pub(super) async fn walk_dirs_and_count_files(
    sftp: &mut dyn SftpSession,
    root: &str,
) -> Result<(Vec<String>, u64), SshError> {
    let mut dirs = vec![root.to_string()];
    let mut queue = vec![root.to_string()];
    let mut file_count = 0u64;
    while let Some(dir) = queue.pop() {
        let entries = sftp.list_dir(&dir).await?;
        for entry in entries {
            if entry.is_dir {
                queue.push(entry.path.clone());
                dirs.push(entry.path);
            } else {
                file_count += 1;
            }
        }
    }
    Ok((dirs, file_count))
}

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
        elevated_user.as_deref(),
        "delete",
        path,
        matches!(result, Ok(Some(()))),
    );
    match result? {
        Some(()) => Ok(()),
        None => Err(stopped_error()),
    }
}

/// Eigentliche Rekursions-Logik hinter `sftp_delete` — von der
/// Tauri-Befehls-Signatur (`State<AppState>`, `SessionId`) losgelöst, damit
/// sie sich direkt gegen ein `SftpSession`-Testdouble prüfen lässt (s.
/// `sftp_mutation_tests` unten, gegen den echten lokalen Pseudo-Server via
/// `ssh_transport::LocalFileSession`).
///
/// Issue #622: `Ok(None)` = vom Nutzer gestoppt; geprüft wird vor jedem
/// Eintrag, schon Gelöschtes bleibt gelöscht (wie bei einem Widerruf, Spec
/// 0085).
pub(super) async fn delete_recursive(
    sftp: &mut dyn SftpSession,
    path: &str,
    op: &OpHandle,
) -> Result<Option<()>, SshError> {
    // `lstat` statt `stat` (Spec-Reviewer-Fund, Spec 0054, Review des
    // Gesamtpakets, ERHÖHTE Priorität): `stat` folgt Symlinks — ein
    // Eintrag, der selbst ein Symlink auf ein Verzeichnis ist, würde damit
    // als "ist ein Ordner" erkannt und der Baum DAHINTER (potenziell weit
    // außerhalb des eigentlich sichtbaren Verzeichnisses, z. B.
    // `/var/www/current -> /etc`) rekursiv gelöscht. `lstat` meldet für
    // einen Symlink dessen eigenen Typ, nie den des Ziels — der Symlink
    // selbst wird dann korrekt nur entfernt, nie in ihn hinein rekursiert.
    if op.is_user_stopped() {
        return Ok(None);
    }
    let root_entry = sftp.lstat(path).await?;
    if !root_entry.is_dir {
        sftp.remove(path).await?;
        return Ok(Some(()));
    }

    // Issue #658: from here on the operation counts entries (row + progress).
    op.begin_entries();
    let (dirs, file_count) = walk_dirs_and_count_files(sftp, path).await?;
    op.set_files(0, Some(dirs.len() as u64 + file_count));
    // Alle Dateien im Baum entfernen — dafür noch einmal denselben Baum
    // ablaufen statt die Pfade aus `walk_dirs_and_count_files` zu sammeln:
    // deren Rückgabe zählt Dateien nur, trägt ihre Pfade aber bewusst nicht
    // mit (für die reine Vorschau in `sftp_delete_preview` unnötiger
    // Speicher-/Allokations-Ballast bei großen Bäumen). Der zweite Durchlauf
    // liest dieselben, kleinen Verzeichnislisten erneut — für den ohnehin
    // seltenen "Ordner löschen"-Fall keine spürbare Mehrkosten.
    for dir in &dirs {
        if op.is_user_stopped() {
            return Ok(None);
        }
        let entries = sftp.list_dir(dir).await?;
        for entry in entries {
            if !entry.is_dir {
                if op.is_user_stopped() {
                    return Ok(None);
                }
                sftp.remove(&entry.path).await?;
                op.add_files_done();
            }
        }
    }
    for dir in dirs.into_iter().rev() {
        if op.is_user_stopped() {
            return Ok(None);
        }
        sftp.remove_dir(&dir).await?;
        op.add_files_done();
    }
    Ok(Some(()))
}
