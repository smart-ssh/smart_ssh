use std::sync::Arc;

use ssh_manager_core::profiles::CredentialStore;
use ssh_manager_core::ssh::{
    ConnectLog, ConnectStepRecord, ConnectionTarget, HostKeyStore, KeyFileReader, NoPrompter,
    SshError,
};
use ssh_transport::ConnectOutcome;

/// `ssh_transport::connect` without an interactive prompter.
pub async fn connect_plain(
    target: &ConnectionTarget,
    credentials: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
    host_keys: Arc<dyn HostKeyStore>,
) -> Result<ConnectOutcome, SshError> {
    ssh_transport::connect(target, credentials, key_files, host_keys, &NoPrompter).await
}

/// Ein Versuch mit Protokoll; liefert das Ergebnis ohne `Debug`-Zwang
/// (`ConnectOutcome` trägt ein `Box<dyn SshTransport>`) und die Schritte.
pub async fn connect_logged(
    target: &ConnectionTarget,
    credentials: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
    host_keys: std::sync::Arc<dyn HostKeyStore>,
) -> (Result<ConnectOutcome, SshError>, Vec<ConnectStepRecord>) {
    let log = ConnectLog::new();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        ssh_transport::connect_with_log(
            target,
            credentials,
            key_files,
            host_keys,
            &NoPrompter,
            &log,
        ),
    )
    .await
    .expect("must not hang");
    (result, log.snapshot())
}
