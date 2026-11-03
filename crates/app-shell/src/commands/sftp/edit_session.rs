use super::*;
use app_logic::error::ResultCodeExt;

// --- Spec 0054, Teil 4: "Lokal öffnen -> bearbeiten -> Upload anbieten" ----
//
// Download in ein **kontrolliertes** Temp-Verzeichnis (Spec-Wortlaut: "nicht
// irgendwo — ein definiertes Temp-Verzeichnis der App"), das eigentliche
// "mit lokalem Programm öffnen" läuft über `@tauri-apps/plugin-opener`s
// bereits registrierte, produktionsreife `openPath`-Funktion direkt im
// Frontend (kein eigener Befehl nötig — s. `capabilities/default.json`s
// neu ergänztes `opener:allow-open-path`). Die lokale Änderungserkennung
// (Datei-Watcher) läuft als Polling auf `local_file_mtime` im Frontend
// statt über einen nativen Dateisystem-Watcher (z. B. `notify`-Crate): für
// eine einzelne, während einer aktiven Bearbeitung beobachtete Datei ist
// ein Poll-Intervall im Sekundenbereich unauffällig genug, um dafür keine
// neue, plattformübergreifend nicht triviale native Abhängigkeit
// einzuführen — s. ADR zu dieser Spec.

/// Basisordner für alle Editier-Temp-Dateien EINER Session — eigener
/// Unterordner pro `session_id`, damit `disconnect()` (unten) beim
/// Trennen der Verbindung gezielt genau diese und keine fremden
/// Editier-Sessions aufräumen kann ("Temp aufräumen bei Session-Ende
/// spätestens", Spec 0054 Teil 4, Punkt 6).
///
/// Issue #93: liegt im Editier-Kopien-Ordner **dieser Instanz** (je
/// Datenverzeichnis, s. [`crate::edit_copies`]), den der Start von
/// Überbleibseln eines abgestürzten Prozesses befreit.
pub(crate) fn edit_session_dir(session_id: SessionId) -> CommandResult<std::path::PathBuf> {
    edit_session_dir_in(crate::edit_copies::instance_root().as_deref(), session_id)
}

/// [`edit_session_dir`] below an explicitly given edit-copy root (`None` =
/// the OS reports no cache directory, which stays an error here).
///
/// Issue #134: the root is a **parameter** of every `*_impl` function that
/// derives an edit-session directory (`authorize_local_path`,
/// `open_for_editing_impl`, `close_edit_session_impl`, …), and each Tauri
/// command passes [`crate::edit_copies::instance_root`] — the same value
/// the old code read inside. Tests pass a `tempfile::tempdir()` of their
/// own instead, so they never touch the real user cache, and the guard
/// removes it even when a test panics. Chosen over a settable test
/// override: a parameter has no process-global (or thread-local) state one
/// test could change under another running in parallel, and nothing
/// test-specific is compiled into the production build — the commands
/// simply have no other value to pass.
pub(super) fn edit_session_dir_in(
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
) -> CommandResult<std::path::PathBuf> {
    let root = edit_root
        .ok_or("Kein Cache-Verzeichnis gefunden")
        .or_code("NO_CACHE_DIR")?;
    Ok(root.join(session_id.to_string()))
}

/// Spec 0086, A1.2: Obergrenze für „Lokal öffnen". Bewusst eine **eigene**
/// Konstante ohne gemeinsamen Code mit [`MAX_TEXT_PREVIEW_BYTES`] oder dem
/// KI-Lesepfad (A1.4) — die Begründung am Doc-Kommentar von
/// `MAX_TEXT_PREVIEW_BYTES` gilt hier genauso, und inhaltlich sind es zwei
/// verschiedene Fragen: was sinnvoll in die Zwischenablage passt, und was
/// sinnvoll in einen lokalen Editor geladen wird.
pub(super) const MAX_EDIT_OPEN_BYTES: u64 = 50 * 1024 * 1024;

/// Spec 0086, A1.2: der Wortlaut der Ablehnung, wörtlich aus der Spec.
/// Fester Text statt aus [`MAX_EDIT_OPEN_BYTES`] formatiert, weil der Wert
/// in Tests als Parameter kleiner gesetzt wird (s.
/// [`open_for_editing_impl`]) — ein formatierter Text würde dort eine
/// Grenze nennen, die es im Produktivbetrieb nicht gibt.
pub(super) const TOO_LARGE_FOR_EDITING: &str =
    "Datei ist größer als 50 MB — zu groß zum lokalen Öffnen. Bitte stattdessen herunterladen.";

/// Spec 0054, Teil 4, Punkt 1: Download in das kontrollierte
/// Editier-Temp-Verzeichnis dieser Session. Ein erneutes Öffnen derselben
/// Remote-Datei überschreibt die lokale Kopie einfach mit dem aktuellen
/// Remote-Inhalt (keine zweite, veraltete Kopie unter neuem Namen) — wer
/// eine bereits laufende Bearbeitung fortsetzen will, nutzt die
/// weiterhin geöffnete Anwendung, nicht einen erneuten "Lokal öffnen"-Klick.
///
/// Spec 0086, A1.2: Dateien über [`MAX_EDIT_OPEN_BYTES`] werden abgelehnt —
/// vor dem Lesen, wenn `stat` die Größe schon zeigt, sonst nach dem Lesen.
#[tauri::command]
pub async fn sftp_open_for_editing(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    remote_path: String,
    elevated_user: Option<String>,
) -> CommandResult<EditSessionDto> {
    let edit_root = crate::edit_copies::instance_root();
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            open_for_editing_impl(
                &session,
                &channel,
                edit_root.as_deref(),
                session_id,
                &remote_path,
                MAX_EDIT_OPEN_BYTES,
            )
            .await
        },
    )
    .await
}

/// `max_bytes` ist ein Parameter statt direkt [`MAX_EDIT_OPEN_BYTES`]
/// (Spec 0086, T5): die Prüfung **nach** dem Lesen ließe sich sonst nur mit
/// einem echten 50-MB-Puffer je Testfall abdecken. Der Produktivpfad
/// (`sftp_open_for_editing` oben) gibt immer die echte Konstante mit und ist
/// der einzige **produktive** Aufrufer; die übrigen Aufrufstellen liegen in
/// `revocation_tests.rs` (ebenfalls mit der echten Konstante) und
/// `size_limit_tests.rs` (mit kleinen Werten) — s. ADR 0080.
///
/// `edit_root`: s. [`edit_session_dir_in`] (Issue #134).
pub(super) async fn open_for_editing_impl(
    session: &Session,
    channel: &BrowserChannel,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    remote_path: &str,
    max_bytes: u64,
) -> CommandResult<EditSessionDto> {
    let file_name = file_name_of(remote_path);
    // Zip-Slip-Schutz (s. `safe_local_segment`-Doc-Kommentar) — auch hier
    // landet ein transitiv server-kontrollierter Name als lokales
    // Pfadsegment.
    safe_local_segment(&file_name)?;

    let (bytes, remote_modified) = {
        let mut guard = lock_browser_sftp(session, channel).await;
        let mut sftp = guard.sftp().await?;
        // Zwei Operationen: ein Widerruf dazwischen lässt die Datei
        // ungelesen (Spec 0085, A1.1/T6b).
        let entry = sftp.stat(remote_path).await?;
        // Spec 0086, A1.2: vor dem Lesen, wenn `stat` die Größe schon zeigt —
        // dann wird eine zu große Datei nicht erst komplett übertragen.
        if entry.size > max_bytes {
            return Err(CommandError::with_code(
                TOO_LARGE_FOR_EDITING,
                "SFTP_TOO_LARGE_FOR_EDITING",
            ));
        }
        let bytes = sftp.read_file(remote_path).await?;
        (bytes, entry.modified.map(|dt| dt.to_rfc3339()))
    };

    // Spec 0086, A1.2/A1.3: und nach dem Lesen, denn die `stat`-Größe oben
    // ist keine Zusage — die Kanal-Sperre wird je Operation genommen (Spec
    // 0085, A1.3; ADR 0078 §1), die Datei kann zwischen `stat` und
    // `read_file` gewachsen sein. Die Prüfung steht **vor** jedem
    // Schreibzugriff ins Temp-Verzeichnis: bei einer Ablehnung entsteht dort
    // weder Datei noch Verzeichnis, und eine aus einem früheren „Lokal
    // öffnen" schon liegende Kopie bleibt unverändert (A1.3).
    if bytes.len() as u64 > max_bytes {
        return Err(CommandError::with_code(
            TOO_LARGE_FOR_EDITING,
            "SFTP_TOO_LARGE_FOR_EDITING",
        ));
    }

    let dir = edit_session_dir_in(edit_root, session_id)?;
    let local_path = dir.join(&file_name);
    let local_path_for_write = local_path.clone();
    // Spec-Reviewer-Härtungshinweis (Spec 0054) / Spec 0067, A5: `0700`/
    // `0600` auf Unix, s. `write_edit_copy`.
    tokio::task::spawn_blocking(move || {
        crate::edit_copies::write_edit_copy(&dir, &local_path_for_write, &bytes)
    })
    .await
    .map_err(|e| {
        app_logic::error::CommandError::with_code(
            format!("Hintergrund-Task für lokalen Download fehlgeschlagen: {e}"),
            "BACKGROUND_TASK_FAILED",
        )
    })??;

    Ok(EditSessionDto {
        local_path: local_path.to_string_lossy().into_owned(),
        remote_modified,
    })
}

/// Spec 0054, Teil 4, Punkt 3/4: Polling-Grundlage für die lokale
/// Änderungserkennung (s. Moduldoc-Kommentar oben zur Polling- statt
/// Watcher-Entscheidung). `Ok(None)` sowohl bei einer nicht (mehr)
/// existierenden Datei als auch bei einem sonstigen Lesefehler — für den
/// Aufrufer (reines "hat sich etwas geändert?"-Polling) ist "kein
/// verlässlicher Zeitstempel verfügbar" in beiden Fällen dieselbe
/// Situation, ein technischer Fehlerdialog dafür wäre für einen
/// Hintergrund-Poll unangemessen aufdringlich.
///
/// Issue #89: same rule as [`sftp_upload`]. A path outside every grant of
/// this session also yields `None` — for a background poll a refusal is the
/// same "no reliable timestamp", and answering `None` for both a missing
/// and a refused path keeps the command from telling the webview which
/// arbitrary paths exist. The refusal itself is logged by the grant check.
#[tauri::command]
pub async fn local_file_mtime(
    state: State<'_, AppState>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
    local_path: String,
) -> CommandResult<Option<String>> {
    // Always `Ok` — `CommandResult` only because Tauri requires it for an
    // async command with borrowed `State` arguments.
    Ok(local_file_mtime_impl(
        &state.sessions,
        grants.inner(),
        crate::edit_copies::instance_root().as_deref(),
        session_id,
        &local_path,
    )
    .await)
}

pub(super) async fn local_file_mtime_impl(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
) -> Option<String> {
    let local_path =
        authorize_local_path(sessions, grants, edit_root, session_id, local_path).ok()?;
    tokio::task::spawn_blocking(move || {
        std::fs::metadata(local_path.as_path())
            .and_then(|m| m.modified())
            .ok()
    })
    .await
    .ok()
    .flatten()
    .map(chrono::DateTime::<chrono::Utc>::from)
    .map(|dt| dt.to_rfc3339())
}

/// Issue #679: upper bound for the in-app editor of an edit copy (Android has
/// no external editor to hand the copy to). Larger files are refused.
pub(super) const MAX_IN_APP_EDIT_BYTES: u64 = 2 * 1024 * 1024;

/// Resolves `local_path` to a file inside this session's own edit-copy
/// directory (nothing else — not any other granted path), canonicalised.
pub(super) fn in_app_edit_copy(
    sessions: &SessionManager,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
) -> CommandResult<std::path::PathBuf> {
    if sessions.get(session_id).is_none() {
        return Err(app_logic::local_path_grants::local_path_not_granted());
    }
    let dir = edit_session_dir_in(edit_root, session_id)?
        .canonicalize()
        .map_err(|_| app_logic::local_path_grants::local_path_not_granted())?;
    let canonical = std::path::Path::new(local_path)
        .canonicalize()
        .map_err(|_| app_logic::local_path_grants::local_path_not_granted())?;
    if canonical.starts_with(&dir) && canonical.is_file() {
        Ok(canonical)
    } else {
        Err(app_logic::local_path_grants::local_path_not_granted())
    }
}

pub(super) fn read_edit_copy_text_impl(
    sessions: &SessionManager,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
    max: u64,
) -> CommandResult<String> {
    use std::io::Read;
    let path = in_app_edit_copy(sessions, edit_root, session_id, local_path)?;
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(max + 1)
        .read_to_string(&mut text)
        .map_err(|_| {
            CommandError::with_code("The file is not valid UTF-8 text", "EDIT_COPY_NOT_TEXT")
        })?;
    if text.len() as u64 > max {
        return Err(CommandError::with_code(
            "The file is too large to edit here",
            "SFTP_TOO_LARGE_FOR_EDITING",
        ));
    }
    Ok(text)
}

pub(super) fn write_edit_copy_text_impl(
    sessions: &SessionManager,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
    content: &str,
    max: u64,
) -> CommandResult<()> {
    if content.len() as u64 > max {
        return Err(CommandError::with_code(
            "The text is too large to save here",
            "EDIT_COPY_TEXT_TOO_LARGE",
        ));
    }
    let path = in_app_edit_copy(sessions, edit_root, session_id, local_path)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Issue #679: text of an edit copy for the in-app editor (Android). Only a
/// file inside this session's edit-copy directory is accepted.
#[tauri::command]
pub async fn read_edit_copy_text(
    state: State<'_, AppState>,
    session_id: SessionId,
    local_path: String,
) -> CommandResult<String> {
    read_edit_copy_text_impl(
        &state.sessions,
        crate::edit_copies::instance_root().as_deref(),
        session_id,
        &local_path,
        MAX_IN_APP_EDIT_BYTES,
    )
}

/// Issue #679: saves the in-app editor's text into the edit copy. The
/// existing change detection then offers the upload as for any edit.
#[tauri::command]
pub async fn write_edit_copy_text(
    state: State<'_, AppState>,
    session_id: SessionId,
    local_path: String,
    content: String,
) -> CommandResult<()> {
    write_edit_copy_text_impl(
        &state.sessions,
        crate::edit_copies::instance_root().as_deref(),
        session_id,
        &local_path,
        &content,
        MAX_IN_APP_EDIT_BYTES,
    )
}

/// Spec 0054, Teil 4, Punkt 6: "Watcher stoppt, wenn der Nutzer den Flow
/// beendet; Temp-Datei aufräumen." Best-effort — eine bereits vom Nutzer
/// oder dem externen Programm gelöschte Datei ist kein Fehlerfall.
#[tauri::command]
pub async fn close_edit_session(session_id: SessionId, local_path: String) -> CommandResult<()> {
    close_edit_session_impl(
        crate::edit_copies::instance_root().as_deref(),
        session_id,
        &local_path,
    )
    .await
}

/// `edit_root`: s. [`edit_session_dir_in`] (Issue #134).
pub(super) async fn close_edit_session_impl(
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
) -> CommandResult<()> {
    // Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets): ohne
    // `session_id`-Parameter hätte dieser Befehl JEDEN vom Frontend
    // übergebenen lokalen Pfad gelöscht — bei einem sauberen Frontend
    // passiert das nie, aber als Verteidigung in der Tiefe (dieselbe
    // Webview rendert auch KI-generierten Chat-Inhalt) kostet die
    // Einschränkung auf den eigenen Editier-Temp-Ordner dieser Session
    // nichts an Funktionalität — `close_edit_session` wird ohnehin nie mit
    // einem anderen Pfad aufgerufen.
    let dir = edit_session_dir_in(edit_root, session_id)?;
    if !std::path::Path::new(local_path).starts_with(&dir) {
        return Err(CommandError::with_code(
            "Pfad liegt außerhalb des Editier-Temp-Ordners dieser Session",
            "EDIT_SESSION_PATH_OUTSIDE",
        ));
    }
    match tokio::fs::remove_file(local_path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CommandError::from(e)),
    }
}
