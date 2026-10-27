//! Tests of the rename-overwrite handling of `sftp.rs`.

use ssh_manager_core::ssh::mock::MockSftpSession;

use super::*;

#[tokio::test]
async fn test_overwrite_uses_posix_rename_when_offered() {
    let mut sftp = MockSftpSession::new()
        .with_file("/a", "new")
        .with_file("/b", "old")
        .with_posix_rename();
    rename_replacing(&mut sftp, "/a", "/b").await.unwrap();
    assert_eq!(sftp.file_content("/b").unwrap(), b"new");
    assert!(sftp.file_content("/a").is_none());
    let calls = sftp.calls();
    assert!(calls.contains(&"posix_rename /a -> /b".to_string()));
    assert!(!calls.iter().any(|c| c.starts_with("remove ")));
}

#[tokio::test]
async fn test_overwrite_without_posix_rename_removes_then_renames() {
    let mut sftp = MockSftpSession::new()
        .with_file("/a", "new")
        .with_file("/b", "old");
    rename_replacing(&mut sftp, "/a", "/b").await.unwrap();
    assert_eq!(sftp.file_content("/b").unwrap(), b"new");
    let calls = sftp.calls();
    let rm = calls.iter().position(|c| c == "remove /b").unwrap();
    let mv = calls.iter().position(|c| c == "rename /a -> /b").unwrap();
    assert!(rm < mv);
}

#[tokio::test]
async fn test_plain_rename_onto_existing_file_does_not_overwrite() {
    // Ohne `overwrite` ruft der Befehl nur `rename`; der Mock überschreibt
    // zwar (er kennt keinen Konflikt), entscheidend ist hier: kein
    // `remove`, kein `posix_rename` auf diesem Pfad.
    let mut sftp = MockSftpSession::new()
        .with_file("/a", "new")
        .with_posix_rename();
    sftp.rename("/a", "/b").await.unwrap();
    let calls = sftp.calls();
    assert_eq!(calls, vec!["rename /a -> /b".to_string()]);
}

#[tokio::test]
async fn test_overwrite_with_failing_rename_names_the_situation() {
    let mut sftp = MockSftpSession::new()
        .with_file("/a", "new")
        .with_file("/b", "old")
        .with_rename_failure("/a");
    let err = rename_replacing(&mut sftp, "/a", "/b").await.unwrap_err();
    let SshError::ChannelError(text) = err else {
        panic!("unexpected error");
    };
    assert!(text.contains("bereits entfernt"), "{text}");
    assert!(text.contains("Quelle ist unverändert"), "{text}");
    assert!(sftp.file_content("/b").is_none());
    assert_eq!(sftp.file_content("/a").unwrap(), b"new");
}

#[tokio::test]
async fn test_overwrite_onto_folder_is_refused_untouched() {
    let mut sftp = MockSftpSession::new()
        .with_file("/a", "new")
        .with_dir("/d")
        .with_posix_rename();
    let err = rename_replacing(&mut sftp, "/a", "/d").await.unwrap_err();
    assert!(matches!(err, SshError::ChannelError(t) if t.contains("Ordner")));
    assert!(!sftp
        .calls()
        .iter()
        .any(|c| c.starts_with("remove") || c.contains("rename")));
}
