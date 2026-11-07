//! Broken connections and session limits against real servers (issue #1179).
//! Scenarios `net-restart`, `net-pause`, `net-maxsessions` and `net-rekey`
//! (OpenSSH, `tests/sshd/scenarios/`); the "not an SSH server" cases use
//! throw-away listeners inside the test process. Run through
//! `tests/sshd/run.sh` (see `tests/sshd/README.md`).
#![expect(clippy::unwrap_used, reason = "integration test")]

mod sshd_support;

use std::io::Write;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ssh_manager_core::ssh::{
    ChunkControl, ChunkSource, ConnectLog, ConnectionLossWatch, ConnectionTarget, HostKeyStore,
    NoPrompter, PtySize, SshError, SshTransport, TransferEnd,
};
use ssh_transport::{ConnectLimits, ConnectOutcome};
use sshd_support::{
    connect_scenario, daemon_log, hop, pause, restart, scenario_port, step, unpause, Creds, Keys,
    NoKeyFiles,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const LIMIT: Duration = Duration::from_secs(3);

fn limits() -> ConnectLimits {
    ConnectLimits {
        handshake: LIMIT,
        authentication: Duration::from_secs(10),
    }
}

/// One connect attempt against `127.0.0.1:port` with the given limits; the
/// error (or unexpected success) is returned together with the elapsed time.
async fn try_connect(port: u16) -> (Result<(), SshError>, Duration) {
    let target = ConnectionTarget {
        hops: vec![hop("127.0.0.1", port)],
    };
    let keys: Arc<dyn HostKeyStore> = Arc::new(Keys::default());
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        ssh_transport::connect_with_limits(
            &target,
            &Creds,
            &NoKeyFiles,
            keys,
            &NoPrompter,
            &ConnectLog::new(),
            limits(),
        ),
    )
    .await
    .expect("connect must never hang");
    let elapsed = started.elapsed();
    match result {
        Ok(_) => (Ok(()), elapsed),
        Err(e) => (Err(e), elapsed),
    }
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_instead_of_ssh_fails_clearly_within_the_handshake_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 256];
                // Like a web server: answer once the peer has sent something.
                let _ = sock.read(&mut buf).await;
                let _ = sock
                    .write_all(
                        b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                let _ = sock.shutdown().await;
            });
        }
    });

    let (result, elapsed) = try_connect(port).await;
    let err = result.expect_err("an HTTP server is not an SSH server");
    assert_ne!(
        err,
        SshError::Timeout,
        "expected a clear error, not a timeout"
    );
    assert!(
        elapsed < LIMIT,
        "failed after {elapsed:?}, limit is {LIMIT:?}"
    );
    assert!(!format!("{err}").is_empty());
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn silent_listener_times_out_within_the_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    // Accept and hold the sockets open, never send a byte.
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((sock, _)) = listener.accept().await {
            held.push(sock);
        }
    });

    let (result, elapsed) = try_connect(port).await;
    assert_eq!(
        result.expect_err("no banner, no session"),
        SshError::Timeout
    );
    assert!(elapsed >= LIMIT, "timed out early: {elapsed:?}");
    assert!(
        elapsed < LIMIT + Duration::from_secs(5),
        "timeout fired late: {elapsed:?}"
    );
}

/// Connects with a short keepalive; the host key is confirmed on the way.
async fn connect_fast_keepalive(scenario: &str) -> Box<dyn SshTransport> {
    let port = scenario_port(scenario);
    let target = ConnectionTarget {
        hops: vec![hop("127.0.0.1", port)],
    };
    let keys = Arc::new(Keys::default());
    let store: Arc<dyn HostKeyStore> = keys.clone();
    for _ in 0..2 {
        match step(
            "connect_with_keepalive()",
            ssh_transport::connect_with_keepalive(
                &target,
                &Creds,
                &NoKeyFiles,
                store.clone(),
                &NoPrompter,
                &ConnectLog::new(),
                ConnectLimits::DEFAULT,
                Duration::from_millis(500),
                2,
            ),
        )
        .await
        .expect("connect")
        {
            ConnectOutcome::Connected(t) => return t,
            ConnectOutcome::PendingHostKeyConfirmation {
                host,
                port,
                raw_key,
                ..
            } => {
                keys.trust(&host, port, &raw_key).unwrap();
            }
        }
    }
    panic!("not connected after confirming the host key");
}

async fn wait_for_loss(mut watch: ConnectionLossWatch, within: Duration) {
    tokio::time::timeout(within, async {
        loop {
            if watch.borrow_and_update().is_some() {
                return;
            }
            watch.changed().await.expect("sender alive");
        }
    })
    .await
    .unwrap_or_else(|_| panic!("connection loss not reported within {within:?}"));
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sshd_restart_closes_the_session_and_a_new_connect_works() {
    let mut t = connect_scenario("net-restart").await;
    let watch = t.connection_loss_watch().expect("loss watch");
    assert!(watch.borrow().is_none());

    restart("net-restart");
    wait_for_loss(watch, Duration::from_secs(30)).await;

    let err = step("execute after restart", t.execute("echo ok"))
        .await
        .expect_err("the old session is gone");
    assert_ne!(
        err,
        SshError::Timeout,
        "expected session closed, got a hang"
    );

    // The daemon comes back; allow a few attempts while it starts listening.
    let mut last = None;
    for _ in 0..10 {
        let port = scenario_port("net-restart");
        let (result, _) = try_connect(port).await;
        // A fresh host key is generated at container start, so a connect
        // that merely pauses for confirmation also proves the daemon is up.
        match result {
            Ok(()) => {
                last = None;
                break;
            }
            Err(e) => last = Some(e),
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    assert_eq!(last, None, "new connect after restart failed");
    let mut fresh = connect_scenario("net-restart").await;
    let out = step("execute on new session", fresh.execute("echo ok"))
        .await
        .unwrap();
    assert_eq!(out.stdout, b"ok\n");
}

struct Unpause(&'static str);
impl Drop for Unpause {
    fn drop(&mut self) {
        unpause(self.0);
    }
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paused_container_is_detected_by_keepalive() {
    let t = connect_fast_keepalive("net-pause").await;
    let watch = t.connection_loss_watch().expect("loss watch");
    assert!(watch.borrow().is_none());

    let _guard = Unpause("net-pause");
    pause("net-pause");
    // 500 ms interval, 2 unanswered keepalives: well inside 15 s.
    wait_for_loss(watch, Duration::from_secs(15)).await;
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_sessions_one_rejects_the_second_channel_and_keeps_the_first() {
    let mut t = connect_scenario("net-maxsessions").await;
    let mut shell = step("open_shell", t.open_shell(PtySize { cols: 80, rows: 24 }))
        .await
        .expect("first session channel");

    let err = step("second channel", t.execute("echo second"))
        .await
        .expect_err("MaxSessions 1 must reject the second channel");
    assert_ne!(
        err,
        SshError::Timeout,
        "rejection must be an error, not a hang"
    );
    assert!(!format!("{err}").is_empty());

    step("shell write", shell.write(b"echo $((20+22))\n"))
        .await
        .expect("first session still writable");
    let mut seen = String::new();
    while !seen.contains("42") {
        let chunk = step("shell read", shell.read())
            .await
            .expect("first session still readable");
        assert!(!chunk.is_empty(), "shell closed unexpectedly: {seen:?}");
        seen.push_str(&String::from_utf8_lossy(&chunk));
    }
    let log = daemon_log("net-maxsessions");
    assert!(!log.is_empty());
}

const SIZE: usize = 50 * 1024 * 1024;
const CHUNK: usize = 32 * 1024;

fn sha256_of(path: &std::path::Path) -> String {
    let out = Command::new("sha256sum").arg(path).output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// Deterministic, incompressible-looking bytes (xorshift64).
fn fill(state: &mut u64, buf: &mut [u8]) {
    for b in buf.iter_mut() {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *b = (*state >> 24) as u8;
    }
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rekey_limit_1m_keeps_a_50_mib_sftp_roundtrip_intact() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.bin");
    let downloaded = dir.path().join("downloaded.bin");
    {
        let mut f = std::fs::File::create(&source).unwrap();
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut buf = vec![0u8; CHUNK];
        for _ in 0..SIZE / CHUNK {
            fill(&mut state, &mut buf);
            f.write_all(&buf).unwrap();
        }
    }
    let expected = sha256_of(&source);

    let mut t = connect_scenario("net-rekey").await;
    let mut sftp = step("open_sftp", t.open_sftp()).await.expect("sftp");

    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut sent = 0usize;
    let end = tokio::time::timeout(
        Duration::from_secs(240),
        sftp.write_file_chunked("/tmp/rekey.bin", CHUNK, &mut |buf: &mut [u8]| {
            if sent >= SIZE {
                return ChunkSource::Eof;
            }
            fill(&mut state, buf);
            sent += buf.len();
            ChunkSource::Data(buf.len())
        }),
    )
    .await
    .expect("upload must not hang")
    .expect("upload");
    assert_eq!(end, TransferEnd::Completed);

    let mut out = std::fs::File::create(&downloaded).unwrap();
    let mut write_err = None;
    let end =
        tokio::time::timeout(
            Duration::from_secs(240),
            sftp.read_file_chunked("/tmp/rekey.bin", CHUNK, &mut |data: &[u8]| match out
                .write_all(data)
            {
                Ok(()) => ChunkControl::Continue,
                Err(e) => {
                    write_err = Some(e);
                    ChunkControl::Stop
                }
            }),
        )
        .await
        .expect("download must not hang")
        .expect("download");
    assert!(write_err.is_none());
    assert_eq!(end, TransferEnd::Completed);
    out.flush().unwrap();
    drop(out);
    drop(sftp);

    assert_eq!(sha256_of(&downloaded), expected, "downloaded file differs");
    let remote = step("remote sha256sum", t.execute("sha256sum /tmp/rekey.bin"))
        .await
        .expect("session usable after the transfers");
    assert!(
        String::from_utf8_lossy(&remote.stdout).starts_with(&expected),
        "uploaded file differs: {}",
        String::from_utf8_lossy(&remote.stdout)
    );
    let log = daemon_log("net-rekey");
    assert!(!log.contains("error"), "daemon logged an error: {log}");
}
