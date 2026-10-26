//! Tests of the edit-session helpers of `sftp.rs`.
//! Spec 0054, Teil 4: die von `AppState`/einer echten SFTP-Session
//! losgelösten Bausteine des "Lokal öffnen"-Flows — `sftp_open_for_editing`
//! selbst bräuchte eine volle `Session` (kein bestehendes Test-Setup dafür,
//! s. Fehlen jeglicher `sftp_*`-Command-Tests auf dieser Ebene schon vor
//! Spec 0054), aber `edit_session_dir`/`close_edit_session` sind pure bzw.
//! rein-lokale Dateisystem-Funktionen. `local_file_mtime` steht seit
//! Issue #89 (braucht Sitzung und Freigaben) in `local_path_tests`.

use super::*;

/// Issue #134: every test owns its edit-copy root — a temporary
/// directory the guard removes even when the test panics, never the
/// real user cache.
fn temp_edit_root() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn test_edit_session_dir_is_distinct_per_session() {
    let root = temp_edit_root();
    let a = edit_session_dir_in(Some(root.path()), SessionId::new_v4()).unwrap();
    let b = edit_session_dir_in(Some(root.path()), SessionId::new_v4()).unwrap();
    assert_ne!(a, b);
    assert!(a.ends_with(a.file_name().unwrap()));
    assert!(a.starts_with(root.path()) && b.starts_with(root.path()));
}

#[test]
fn test_edit_session_dir_without_a_cache_dir_is_an_error() {
    assert!(edit_session_dir_in(None, SessionId::new_v4()).is_err());
}

#[tokio::test]
async fn test_close_edit_session_removes_the_file() {
    let root = temp_edit_root();
    let session_id = SessionId::new_v4();
    let dir = edit_session_dir_in(Some(root.path()), session_id).unwrap();
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let path = dir.join("edited.txt");
    tokio::fs::write(&path, b"x").await.unwrap();

    close_edit_session_impl(Some(root.path()), session_id, path.to_str().unwrap())
        .await
        .expect("close_edit_session() sollte gelingen");

    assert!(!path.exists());
}

/// Spec 0054, Teil 4, Punkt 6: "Watcher stoppt ... Temp-Datei
/// aufräumen" — ein bereits vom Nutzer oder dem externen Programm
/// selbst gelöschtes Temp-File ist kein Fehlerfall.
#[tokio::test]
async fn test_close_edit_session_on_an_already_missing_file_is_not_an_error() {
    let root = temp_edit_root();
    let session_id = SessionId::new_v4();
    let path = edit_session_dir_in(Some(root.path()), session_id)
        .unwrap()
        .join("never-existed.txt");

    close_edit_session_impl(Some(root.path()), session_id, path.to_str().unwrap())
        .await
        .expect("ein bereits fehlendes Temp-File darf close_edit_session nicht scheitern lassen");
}

/// Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets): ohne
/// Session-Eingrenzung hätte `close_edit_session` JEDEN übergebenen
/// lokalen Pfad gelöscht — Verteidigung in der Tiefe (s. Doc-Kommentar
/// an `close_edit_session`).
#[tokio::test]
async fn test_close_edit_session_rejects_a_path_outside_its_own_session_dir() {
    let root = temp_edit_root();
    let session_id = SessionId::new_v4();
    let outside = tempfile::tempdir().unwrap();
    let path = outside.path().join("not-mine.txt");
    std::fs::write(&path, b"x").unwrap();

    let result =
        close_edit_session_impl(Some(root.path()), session_id, path.to_str().unwrap()).await;

    assert!(result.is_err());
    assert!(
        path.exists(),
        "eine fremde Datei darf nicht gelöscht werden"
    );
}

/// Issue #134: the scope check also holds inside the same edit-copy
/// root — the copy of another session is not "its own session dir".
#[tokio::test]
async fn test_close_edit_session_rejects_the_copy_of_another_session() {
    let root = temp_edit_root();
    let other_dir = edit_session_dir_in(Some(root.path()), SessionId::new_v4()).unwrap();
    std::fs::create_dir_all(&other_dir).unwrap();
    let path = other_dir.join("nginx.conf");
    std::fs::write(&path, b"x").unwrap();

    let result = close_edit_session_impl(
        Some(root.path()),
        SessionId::new_v4(),
        path.to_str().unwrap(),
    )
    .await;

    assert!(result.is_err());
    assert!(path.exists(), "die Kopie einer anderen Session bleibt");
}
