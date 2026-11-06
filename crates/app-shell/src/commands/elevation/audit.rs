//! Audit line of a file-browser change made with elevated rights.

/// Who a change was made as and how sudo authenticated (never the password).
pub(crate) struct ElevatedAudit {
    pub(super) user: String,
    pub(super) sudo_auth: crate::elevated_sftp::SudoAuth,
    /// Issue #1192: `sftp-server` logs to the server's auth log.
    pub(super) server_logging: bool,
}

pub(crate) fn audit_elevated_change(
    elevated_user: Option<&ElevatedAudit>,
    action: &'static str,
    path: &str,
    ok: bool,
) {
    if let Some(audit) = elevated_user {
        let target_user = audit.user.as_str();
        tracing::info!(
            sudo_auth = audit.sudo_auth.as_str(),
            server_logging = audit.server_logging,
            action,
            path,
            ok,
            elevated = true,
            target_user,
            source = "manual",
            "file browser change with elevated rights"
        );
    }
}
