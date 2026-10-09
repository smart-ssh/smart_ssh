//! Issue #89: the file browser reads only local paths the user granted to
//! this session — picked in the backend's open dialog, dropped onto the
//! window (native event), or the session's own edit copy. Every test runs
//! the same steps the commands run (`authorize_local_path` + `upload_impl`,
//! `read_local_text_preview_impl`, `local_file_mtime_impl`,
//! `claim_dropped_paths_impl`) against a session with a mock SFTP channel.

use std::sync::Arc;

use async_trait::async_trait;

use ssh_manager_core::ssh::mock::MockSftpSession;
use ssh_manager_core::ssh::{CommandOutput, InteractiveShell, PtySize, SshError, SshTransport};

use app_logic::events::TestEmitter;
use app_logic::local_path_grants::{LocalPathGrants, LOCAL_PATH_NOT_GRANTED};
use app_logic::session::SessionManager;
use app_logic::state::SessionId;

use super::*;
use crate::commands::elevation::BrowserChannel;
use crate::elevated_sftp::ElevatedSftpRegistry;

struct NoTransport;
#[async_trait]
impl SshTransport for NoTransport {
    async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
        unreachable!("these tests run no command")
    }
    async fn open_shell(&mut self, _size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        unreachable!()
    }
    async fn disconnect(&mut self) -> Result<(), SshError> {
        Ok(())
    }
}

struct Setup {
    sessions: SessionManager,
    registry: ElevatedSftpRegistry,
    grants: LocalPathGrants,
    session_id: SessionId,
    session: Arc<Session>,
    remote: MockSftpSession,
    emitter: TestEmitter,
    /// Issue #134: this test's edit-copy root, never the real user cache.
    /// The guard removes it even when the test panics.
    edit_root: tempfile::TempDir,
}

async fn setup() -> Setup {
    let session = Arc::new(app_logic::test_support::session_with_transport(Box::new(
        NoTransport,
    )));
    let remote = MockSftpSession::new();
    session.set_sftp_for_tests(Box::new(remote.clone())).await;
    let sessions = SessionManager::new();
    let session_id = SessionId::new_v4();
    sessions.insert(session_id, session.clone());
    Setup {
        sessions,
        registry: ElevatedSftpRegistry::default(),
        grants: LocalPathGrants::new(),
        session_id,
        session,
        remote,
        emitter: TestEmitter::default(),
        edit_root: tempfile::tempdir().unwrap(),
    }
}

impl Setup {
    fn edit_root(&self) -> Option<&std::path::Path> {
        Some(self.edit_root.path())
    }

    /// The edit-session directory of `session_id` below this test's root.
    fn edit_dir(&self, session_id: SessionId) -> std::path::PathBuf {
        edit_session_dir_in(self.edit_root(), session_id).unwrap()
    }

    async fn upload(&self, local_path: &std::path::Path, remote_path: &str) -> CommandResult<()> {
        let channel = BrowserChannel::for_tests(&self.registry, self.session_id, None);
        // The same two steps as `sftp_upload`: grant check first, then the
        // transfer on the browser channel.
        let local_path = authorize_local_path(
            &self.sessions,
            &self.grants,
            self.edit_root(),
            self.session_id,
            local_path.to_str().unwrap(),
        )?;
        upload_impl(
            &self.emitter,
            &self.session,
            &channel,
            self.session_id,
            &local_path,
            remote_path,
        )
        .await
    }

    fn transfer_events(&self) -> usize {
        self.emitter
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name.starts_with("sftp-transfer-"))
            .count()
    }

    /// Simulates a native drop onto the window followed by this session's
    /// file browser claiming it.
    fn drop_and_claim(&self, paths: Vec<std::path::PathBuf>) -> Vec<String> {
        self.grants.record_drop(paths);
        claim_dropped_paths_impl(&self.sessions, &self.grants, self.session_id).unwrap()
    }
}

fn write(path: &std::path::Path, content: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// AC 1: a path neither picked, dropped nor an edit copy fails, and nothing
/// is read: no transfer starts and nothing reaches the server.
#[tokio::test]
async fn test_upload_of_a_path_outside_every_grant_fails_and_reads_nothing() {
    let s = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("id_ed25519");
    write(&secret, b"-----BEGIN OPENSSH PRIVATE KEY-----");

    let err = s.upload(&secret, "/tmp/stolen").await.unwrap_err();

    assert_eq!(err.message, LOCAL_PATH_NOT_GRANTED);
    assert_eq!(s.remote.file_content("/tmp/stolen"), None);
    assert!(
        s.remote.calls().is_empty(),
        "the channel must not be touched: {:?}",
        s.remote.calls()
    );
    assert_eq!(s.transfer_events(), 0, "no transfer may start");
}

#[tokio::test]
async fn test_upload_of_a_dropped_file_works() {
    let s = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.txt");
    write(&file, b"dropped");

    let claimed = s.drop_and_claim(vec![file.clone()]);
    assert_eq!(claimed, vec![file.to_string_lossy().into_owned()]);
    s.upload(&file, "/srv/a.txt").await.unwrap();

    assert_eq!(
        s.remote.file_content("/srv/a.txt"),
        Some(b"dropped".to_vec())
    );
}

/// The open-dialog path grants the same way (`grant_for_live_session`).
#[tokio::test]
async fn test_upload_of_a_picked_file_works() {
    let s = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("picked.txt");
    write(&file, b"picked");

    let granted = grant_for_live_session(&s.sessions, &s.grants, s.session_id, vec![file.clone()]);
    assert_eq!(granted, vec![file.to_string_lossy().into_owned()]);
    s.upload(&file, "/srv/picked.txt").await.unwrap();

    assert_eq!(
        s.remote.file_content("/srv/picked.txt"),
        Some(b"picked".to_vec())
    );
}

#[tokio::test]
async fn test_a_pick_for_a_closed_session_grants_nothing() {
    let s = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("picked.txt");
    write(&file, b"picked");
    s.sessions.remove(s.session_id);

    let granted = grant_for_live_session(&s.sessions, &s.grants, s.session_id, vec![file.clone()]);

    assert!(granted.is_empty());
    assert!(s.grants.check(s.session_id, &file, None).is_err());
}

/// AC 3 (folder part, ADR 0113): a dropped folder becomes a granted root;
/// files below it may be read. Uploading the folder itself behaves as
/// before — reading a directory as a file fails.
#[tokio::test]
async fn test_a_dropped_folder_grants_the_files_below_it() {
    let s = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("project");
    let nested = folder.join("sub/config.yml");
    write(&nested, b"nested");

    s.drop_and_claim(vec![folder.clone()]);

    s.upload(&nested, "/srv/config.yml").await.unwrap();
    assert_eq!(
        s.remote.file_content("/srv/config.yml"),
        Some(b"nested".to_vec())
    );
    let folder_upload = s.upload(&folder, "/srv/project").await.unwrap_err();
    assert_ne!(
        folder_upload.message, LOCAL_PATH_NOT_GRANTED,
        "the dropped folder passes the grant check and fails as before"
    );
}

/// AC 2: `..` out of a granted root is rejected.
#[tokio::test]
async fn test_dot_dot_out_of_a_dropped_folder_is_rejected() {
    let s = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("project");
    write(&folder.join("a.txt"), b"a");
    write(&dir.path().join("secret.txt"), b"secret");
    s.drop_and_claim(vec![folder.clone()]);

    let escaping = folder.join("..").join("secret.txt");
    let err = s.upload(&escaping, "/srv/secret.txt").await.unwrap_err();

    assert_eq!(err.message, LOCAL_PATH_NOT_GRANTED);
    assert_eq!(s.remote.file_content("/srv/secret.txt"), None);
}

/// AC 2: a symlink inside a granted root that resolves outside of it is
/// rejected.
#[cfg(unix)]
#[tokio::test]
async fn test_a_symlink_out_of_a_dropped_folder_is_rejected() {
    let s = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let secret = dir.path().join("outside/id_ed25519");
    write(&secret, b"secret");
    std::os::unix::fs::symlink(&secret, folder.join("innocent.txt")).unwrap();
    s.drop_and_claim(vec![folder.clone()]);

    let err = s
        .upload(&folder.join("innocent.txt"), "/srv/innocent.txt")
        .await
        .unwrap_err();

    assert_eq!(err.message, LOCAL_PATH_NOT_GRANTED);
    assert_eq!(s.remote.file_content("/srv/innocent.txt"), None);
}

/// AC 4: the session's own edit copy can be re-uploaded; the edit copy of
/// another session is rejected.
#[tokio::test]
async fn test_edit_copy_of_the_own_session_uploads_and_of_another_session_is_rejected() {
    let s = setup().await;
    let own_dir = s.edit_dir(s.session_id);
    let own_copy = own_dir.join("nginx.conf");
    write(&own_copy, b"edited");
    let other_id = SessionId::new_v4();
    let other_dir = s.edit_dir(other_id);
    let other_copy = other_dir.join("nginx.conf");
    write(&other_copy, b"other");

    let own = s.upload(&own_copy, "/etc/nginx.conf").await;
    let other = s.upload(&other_copy, "/etc/other.conf").await;

    own.unwrap();
    assert_eq!(
        s.remote.file_content("/etc/nginx.conf"),
        Some(b"edited".to_vec())
    );
    assert_eq!(other.unwrap_err().message, LOCAL_PATH_NOT_GRANTED);
    assert_eq!(s.remote.file_content("/etc/other.conf"), None);
}

/// AC 5: the overwrite preview enforces the same rule.
#[tokio::test]
async fn test_read_local_text_preview_enforces_the_grants() {
    let s = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let dropped = dir.path().join("a.txt");
    let secret = dir.path().join("id_ed25519");
    write(&dropped, b"hello");
    write(&secret, b"secret");
    s.drop_and_claim(vec![dropped.clone()]);

    let ok = read_local_text_preview_impl(
        &s.sessions,
        &s.grants,
        s.edit_root(),
        s.session_id,
        dropped.to_str().unwrap(),
    )
    .await
    .unwrap();
    let err = read_local_text_preview_impl(
        &s.sessions,
        &s.grants,
        s.edit_root(),
        s.session_id,
        secret.to_str().unwrap(),
    )
    .await
    .unwrap_err();

    assert_eq!(ok.text.as_deref(), Some("hello"));
    assert_eq!(err.message, LOCAL_PATH_NOT_GRANTED);
}

/// AC 5: the edit-session poll enforces the same rule; a refused path
/// yields the same `None` as a missing one.
#[tokio::test]
async fn test_local_file_mtime_enforces_the_grants() {
    let s = setup().await;
    let own_dir = s.edit_dir(s.session_id);
    let own_copy = own_dir.join("a.conf");
    write(&own_copy, b"x");
    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("id_ed25519");
    write(&secret, b"secret");

    let granted = local_file_mtime_impl(
        &s.sessions,
        &s.grants,
        s.edit_root(),
        s.session_id,
        own_copy.to_str().unwrap(),
    )
    .await;
    let refused = local_file_mtime_impl(
        &s.sessions,
        &s.grants,
        s.edit_root(),
        s.session_id,
        secret.to_str().unwrap(),
    )
    .await;
    let missing = local_file_mtime_impl(
        &s.sessions,
        &s.grants,
        s.edit_root(),
        s.session_id,
        own_dir.join("gone.conf").to_str().unwrap(),
    )
    .await;

    assert!(granted.is_some());
    assert!(refused.is_none());
    assert!(missing.is_none());
}

/// AC 6: after disconnect, previously granted paths are rejected — the
/// disconnect sequence removes the session and its grants.
#[tokio::test]
async fn test_after_disconnect_granted_paths_are_rejected() {
    let s = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.txt");
    write(&file, b"x");
    s.drop_and_claim(vec![file.clone()]);
    assert!(authorize_local_path(
        &s.sessions,
        &s.grants,
        s.edit_root(),
        s.session_id,
        file.to_str().unwrap()
    )
    .is_ok());

    s.registry.remove_session(&s.sessions, s.session_id);
    s.grants.remove_session(s.session_id);

    let err = s.upload(&file, "/srv/a.txt").await.unwrap_err();
    assert_eq!(err.message, LOCAL_PATH_NOT_GRANTED);
    assert!(read_local_text_preview_impl(
        &s.sessions,
        &s.grants,
        s.edit_root(),
        s.session_id,
        file.to_str().unwrap()
    )
    .await
    .is_err());
    assert!(local_file_mtime_impl(
        &s.sessions,
        &s.grants,
        s.edit_root(),
        s.session_id,
        file.to_str().unwrap()
    )
    .await
    .is_none());
}

/// A grant that outlives its session (e.g. granted in the moment of a
/// disconnect) still does not count: every reader requires the session.
#[tokio::test]
async fn test_a_grant_without_its_session_does_not_count() {
    let s = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.txt");
    write(&file, b"x");
    s.drop_and_claim(vec![file.clone()]);

    s.sessions.remove(s.session_id);

    let err = authorize_local_path(
        &s.sessions,
        &s.grants,
        s.edit_root(),
        s.session_id,
        file.to_str().unwrap(),
    )
    .unwrap_err();
    assert_eq!(err.message, LOCAL_PATH_NOT_GRANTED);
}

#[tokio::test]
async fn test_claiming_a_drop_for_an_unknown_session_fails_and_keeps_nothing() {
    let s = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.txt");
    write(&file, b"x");
    s.grants.record_drop(vec![file.clone()]);

    let unknown = SessionId::new_v4();
    assert!(claim_dropped_paths_impl(&s.sessions, &s.grants, unknown).is_err());
    assert!(s.grants.check(unknown, &file, None).is_err());
}
