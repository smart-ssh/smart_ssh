//! Smoke tests: one per sshd image (`openssh-current`, `openssh-legacy`,
//! `dropbear`) connects with a password and runs `echo ok`. The scenarios
//! (`smoke-*`) use each image's default config, i.e. only algorithms the
//! client supports today. Run through `tests/sshd/run.sh`.
#![expect(clippy::unwrap_used, reason = "integration test")]

mod sshd_support;

use sshd_support::{connect_scenario, step};

async fn echo_ok(scenario: &str) {
    let mut t = connect_scenario(scenario).await;
    let out = step("execute(echo ok)", t.execute("echo ok"))
        .await
        .unwrap();
    assert_eq!(out.stdout, b"ok\n");
    assert_eq!(out.exit_code, Some(0));
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smoke_openssh_current() {
    echo_ok("smoke-openssh-current").await;
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smoke_openssh_legacy() {
    echo_ok("smoke-openssh-legacy").await;
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smoke_dropbear() {
    echo_ok("smoke-dropbear").await;
}
