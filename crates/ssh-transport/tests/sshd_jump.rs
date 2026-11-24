//! Jump-host end-to-end tests against **real OpenSSH `sshd` containers**
//! (issue #180, ADR 0008 update, option e). They check exec, PTY and SFTP over
//! bastion -> target against a real server. Scenarios: `jump-bastion` and
//! `jump-target` (`tests/sshd/scenarios/`); the target is reachable from the
//! bastion as `jump-target:22`.
//!
//! All tests are `#[ignore]` and need Docker; plain `cargo test --workspace`
//! skips them. Run from the repo root with
//! `crates/ssh-transport/tests/sshd/run.sh` (see `tests/sshd/README.md`).
//! Each hop's host key is confirmed individually through the normal
//! `PendingHostKeyConfirmation` path (Spec 0005), no blanket trust.
#![expect(clippy::unwrap_used, reason = "integration test")]

mod sshd_support;

use ssh_manager_core::ssh::{Hop, PtySize, SshTransport};
use sshd_support::{connect_confirming, hop, scenario_port, step};

const TARGET_HOST: &str = "jump-target";

fn bastion_port() -> u16 {
    scenario_port("jump-bastion")
}
fn target_port() -> u16 {
    scenario_port("jump-target")
}
fn target_host() -> String {
    TARGET_HOST.to_string()
}

async fn hostname(t: &mut Box<dyn SshTransport>) -> String {
    let out = step("execute(hostname)", t.execute("hostname"))
        .await
        .expect("execute(hostname)");
    assert_eq!(out.exit_code, Some(0));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn jump_hops() -> Vec<Hop> {
    vec![hop("127.0.0.1", bastion_port()), hop(&target_host(), 22)]
}

/// Hostname der beiden Container über direkte Verbindungen: `(bastion, ziel)`.
async fn direct_hostnames() -> (String, String) {
    let (mut b, _) = connect_confirming(vec![hop("127.0.0.1", bastion_port())]).await;
    let (mut t, _) = connect_confirming(vec![hop("127.0.0.1", target_port())]).await;
    let names = (hostname(&mut b).await, hostname(&mut t).await);
    assert_ne!(names.0, names.1, "Container müssen unterscheidbar sein");
    names
}

#[ignore = "needs sshd containers, see module docs"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jump_exec_runs_on_target_not_bastion() {
    let (bastion_name, target_name) = direct_hostnames().await;
    let (mut t, confirmed) = connect_confirming(jump_hops()).await;

    // Host-Key je Hop einzeln bestätigt, nicht pauschal.
    assert_eq!(
        confirmed,
        vec![
            ("127.0.0.1".to_string(), bastion_port()),
            (target_host(), 22)
        ]
    );

    let out = step("execute(echo)", t.execute("echo via-jump"))
        .await
        .unwrap();
    assert_eq!(out.stdout, b"via-jump\n");
    assert_eq!(out.exit_code, Some(0));

    let name = hostname(&mut t).await;
    assert_eq!(name, target_name, "Befehl muss auf dem Ziel laufen");
    assert_ne!(name, bastion_name, "Befehl lief fälschlich auf der Bastion");
}

/// Gegenprobe: verbindet man nur zur Bastion, ist der Hostname der der
/// Bastion — die Prüfung oben unterscheidet also wirklich.
#[ignore = "needs sshd containers, see module docs"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn single_hop_to_bastion_is_distinguishable_from_target() {
    let (bastion_name, target_name) = direct_hostnames().await;
    let (mut t, _) = connect_confirming(vec![hop("127.0.0.1", bastion_port())]).await;
    let name = hostname(&mut t).await;
    assert_eq!(name, bastion_name);
    assert_ne!(name, target_name);
}

#[ignore = "needs sshd containers, see module docs"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jump_pty_shell_output_and_resize() {
    let (mut t, _) = connect_confirming(jump_hops()).await;
    let mut shell = step("open_shell", t.open_shell(PtySize { cols: 80, rows: 24 }))
        .await
        .expect("open_shell()");

    step("write", shell.write(b"echo pty-$((40+2))\n"))
        .await
        .unwrap();
    let mut seen = Vec::new();
    // Die Eingabezeile wird mit `$((40+2))` zurückgeschrieben; erst das
    // ausgewertete `pty-42` am Zeilenanfang belegt die Ausführung.
    while !String::from_utf8_lossy(&seen)
        .lines()
        .any(|l| l.trim() == "pty-42")
    {
        let chunk = step("read", shell.read()).await.expect("read()");
        assert!(!chunk.is_empty(), "EOF vor pty-42: {seen:?}");
        seen.extend_from_slice(&chunk);
    }

    step(
        "resize",
        shell.resize(PtySize {
            cols: 120,
            rows: 40,
        }),
    )
    .await
    .expect("resize()");
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

#[ignore = "needs sshd containers, see module docs"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jump_sftp_roundtrip_including_one_mib() {
    let (mut t, _) = connect_confirming(jump_hops()).await;
    let mut sftp = step("open_sftp", t.open_sftp()).await.expect("open_sftp()");

    for (name, len) in [
        ("small", 10usize),
        ("mib", 1024 * 1024),
        ("four-mib", 4 << 20),
    ] {
        let path = format!("/home/testuser/jump-{name}.bin");
        let data = pattern(len);
        step("write_file", sftp.write_file(&path, &data))
            .await
            .unwrap_or_else(|e| panic!("write_file {name}: {e:?}"));
        let listing = step("list_dir", sftp.list_dir("/home/testuser"))
            .await
            .expect("list_dir()");
        let entry = listing
            .iter()
            .find(|e| e.name == format!("jump-{name}.bin"))
            .unwrap_or_else(|| panic!("{name} fehlt im Listing"));
        assert_eq!(entry.size, len as u64);
        let back = step("read_file", sftp.read_file(&path))
            .await
            .unwrap_or_else(|e| panic!("read_file {name}: {e:?}"));
        assert!(back == data, "{name}: Inhalt nicht byte-genau");
    }
}
