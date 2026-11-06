//! Wiring test for the shared prompt pause (issue #1187).

use super::*;
use ssh_manager_core::profiles::{CredentialRef, CredentialResult};
use ssh_manager_core::ssh::{KeyFileContent, KeyFileError, KeyFileFacts, NoPrompter};

struct Unused;
impl CredentialStore for Unused {
    fn get(&self, _: &CredentialRef) -> CredentialResult<secrecy::SecretString> {
        unreachable!("not used")
    }
    fn set(&self, _: &CredentialRef, _: secrecy::SecretString) -> CredentialResult<()> {
        unreachable!("not used")
    }
    fn delete(&self, _: &CredentialRef) -> CredentialResult<()> {
        unreachable!("not used")
    }
}
impl KeyFileReader for Unused {
    fn read(&self, _: &str, _: bool) -> Result<KeyFileContent, KeyFileError> {
        unreachable!("not used")
    }
    fn inspect(&self, _: &str) -> KeyFileFacts {
        unreachable!("not used")
    }
}
impl HostKeyStore for Unused {
    fn check(&self, _: &str, _: u16, _: &[u8]) -> HostKeyDecision {
        unreachable!("not used")
    }
    fn trust(&self, _: &str, _: u16, _: &[u8]) -> Result<(), SshError> {
        unreachable!("not used")
    }
}

/// Issue #1187: the pause the transport holds while a login prompt is
/// open is the attempt log's pause, the one the app's outer connect
/// limit watches. Red if `RusshHops::new` uses a private pause.
#[tokio::test(start_paused = true)]
async fn test_transport_pause_is_the_attempt_logs_pause() {
    let log = ConnectLog::new();
    let (tx, _rx) = tokio::sync::watch::channel(None);
    let unused = Unused;
    let hops = RusshHops::new(
        Arc::new(client_config()),
        Arc::new(tx),
        Arc::new(Unused),
        &unused,
        &unused,
        &NoPrompter,
        &log,
    );
    // A prompt wait through the transport's pause stops the outer limit.
    let pause = hops.auth_pause();
    let work = async {
        let _held = pause.pause();
        tokio::time::sleep(Duration::from_secs(1000)).await;
        Ok::<_, SshError>(())
    };
    assert_eq!(
        timeout_unless_paused(work, Duration::from_secs(100), &log.prompt_pause()).await,
        Ok(())
    );
}
