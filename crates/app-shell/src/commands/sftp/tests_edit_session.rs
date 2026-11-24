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

fn assert_path_outside(result: CommandResult<()>) {
    let err = result.expect_err("the path must be rejected");
    assert_eq!(err.code, Some("EDIT_SESSION_PATH_OUTSIDE"));
}

/// The scope check is not lexical: `<dir>/../victim.txt` starts with the
/// session dir as a string but names a file outside of it, which must
/// survive.
#[tokio::test]
async fn test_close_edit_session_rejects_dotdot_escape() {
    let root = temp_edit_root();
    let session_id = SessionId::new_v4();
    let dir = edit_session_dir_in(Some(root.path()), session_id).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let victim = root.path().join("victim.txt");
    std::fs::write(&victim, b"keep me").unwrap();
    let escape = dir.join("..").join("victim.txt");

    let result =
        close_edit_session_impl(Some(root.path()), session_id, escape.to_str().unwrap()).await;

    assert_path_outside(result);
    assert!(victim.exists(), "a file outside the session dir survives");
}

/// A `..` component is rejected even when the resolved path would end up
/// inside the session dir again — and even when nothing exists there.
#[tokio::test]
async fn test_close_edit_session_rejects_dotdot_even_back_into_its_own_dir() {
    let root = temp_edit_root();
    let session_id = SessionId::new_v4();
    let dir = edit_session_dir_in(Some(root.path()), session_id).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("edited.txt");
    std::fs::write(&file, b"x").unwrap();
    let detour = dir
        .join("..")
        .join(dir.file_name().unwrap())
        .join("edited.txt");

    let result =
        close_edit_session_impl(Some(root.path()), session_id, detour.to_str().unwrap()).await;

    assert_path_outside(result);
    assert!(file.exists());
}

/// A symlink inside the session dir that points outside: closing removes
/// the link itself, never its target.
#[cfg(unix)]
#[tokio::test]
async fn test_close_edit_session_removes_a_symlink_but_not_its_target() {
    let root = temp_edit_root();
    let session_id = SessionId::new_v4();
    let dir = edit_session_dir_in(Some(root.path()), session_id).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("target.txt");
    std::fs::write(&target, b"keep me").unwrap();
    let link = dir.join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    close_edit_session_impl(Some(root.path()), session_id, link.to_str().unwrap())
        .await
        .expect("closing a link inside the session dir succeeds");

    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "the link is gone"
    );
    assert!(target.exists(), "the link target survives");
}

/// A path through a symlinked directory inside the session dir that leads
/// outside of it is rejected; the file behind it survives.
#[cfg(unix)]
#[tokio::test]
async fn test_close_edit_session_rejects_a_path_through_a_symlinked_parent() {
    let root = temp_edit_root();
    let session_id = SessionId::new_v4();
    let dir = edit_session_dir_in(Some(root.path()), session_id).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join("victim.txt");
    std::fs::write(&victim, b"keep me").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.join("linkdir")).unwrap();
    let escape = dir.join("linkdir").join("victim.txt");

    let result =
        close_edit_session_impl(Some(root.path()), session_id, escape.to_str().unwrap()).await;

    assert_path_outside(result);
    assert!(
        victim.exists(),
        "the file behind the symlinked folder survives"
    );
}

/// The session dir itself is not a file of the session.
#[tokio::test]
async fn test_close_edit_session_rejects_the_session_dir_itself() {
    let root = temp_edit_root();
    let session_id = SessionId::new_v4();
    let dir = edit_session_dir_in(Some(root.path()), session_id).unwrap();
    std::fs::create_dir_all(&dir).unwrap();

    let result =
        close_edit_session_impl(Some(root.path()), session_id, dir.to_str().unwrap()).await;

    assert_path_outside(result);
    assert!(dir.is_dir());
}

/// A missing file inside an existing session dir is no error either.
#[tokio::test]
async fn test_close_edit_session_on_a_missing_file_in_an_existing_dir_is_not_an_error() {
    let root = temp_edit_root();
    let session_id = SessionId::new_v4();
    let dir = edit_session_dir_in(Some(root.path()), session_id).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("already-gone.txt");

    close_edit_session_impl(Some(root.path()), session_id, path.to_str().unwrap())
        .await
        .expect("a file that is already gone is not an error");
}
