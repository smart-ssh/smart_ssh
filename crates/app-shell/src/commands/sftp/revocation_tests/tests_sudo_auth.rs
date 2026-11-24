use super::*;

/// Issue #1191: the audit line of a change records that sudo authenticated
/// with the stored password (never the password itself).
#[tokio::test]
async fn test_change_audit_line_records_password_sudo_auth() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    s.registry.insert_for_tests(
        s.session_id,
        ElevatedSftp {
            target_user: "root".to_string(),
            sudo_auth: crate::elevated_sftp::SudoAuth::Password,
            server_logging: false,
            sftp: Box::new(elevated.clone()),
        },
    );
    let (logs, _capture) = capture_logs();

    create_file_impl(&s.session, &s.elevated_channel(), "/t/new.txt")
        .await
        .expect("create succeeds");

    let lines = logs.audit_lines("create_file", "/t/new.txt");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(
        lines[0].get("sudo_auth").map(String::as_str),
        Some("password")
    );
}
