//! Jump-Host-Ende-zu-Ende-Tests gegen **echte OpenSSH-`sshd`-Container**
//! (Issue #180, ADR 0008 Update, Option e). Die übrigen Integrationstests
//! laufen gegen die In-Process-Fixture; diese hier prüfen Exec, PTY und
//! SFTP über Bastion → Ziel gegen einen echten Server.
//!
//! Alle Tests sind `#[ignore]` und brauchen Docker; das normale
//! `cargo test --workspace` überspringt sie. Lokal (vom Repo-Root):
//!
//! ```text
//! crates/ssh-transport/tests/openssh/run-jump-tests.sh
//! ```
//!
//! Das Skript baut `tests/openssh/Dockerfile`, startet zwei Container
//! (Bastion auf 127.0.0.1:2301, Ziel auf 127.0.0.1:2302, von der Bastion aus
//! als `jump-target:22` erreichbar) und ruft `cargo test -p ssh-transport
//! --test openssh_jump -- --ignored` auf. Ports und Zielname lassen sich per
//! `SSH_JUMP_BASTION_PORT`, `SSH_JUMP_TARGET_PORT`, `SSH_JUMP_TARGET_HOST`
//! überschreiben. Jeder Host-Key wird je Hop über den normalen
//! `PendingHostKeyConfirmation`-Weg bestätigt (Spec 0005), kein Pauschal-Trust.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use secrecy::SecretString;
use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore,
};
use ssh_manager_core::ssh::{
    ConnectionTarget, Hop, HostKeyDecision, HostKeyStore, KeyFileContent, KeyFileError,
    KeyFileFacts, KeyFileReader, PtySize, SshError, SshTransport,
};
use ssh_transport::ConnectOutcome;

const USER: &str = "testuser";
const PASS: &str = "testpass";
const STEP: Duration = Duration::from_secs(30);

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn bastion_port() -> u16 {
    env_or("SSH_JUMP_BASTION_PORT", "2301").parse().unwrap()
}
fn target_port() -> u16 {
    env_or("SSH_JUMP_TARGET_PORT", "2302").parse().unwrap()
}
fn target_host() -> String {
    env_or("SSH_JUMP_TARGET_HOST", "jump-target")
}

/// Jeder Schritt hat ein Timeout: lieber scheitern als hängen.
async fn step<T>(what: &str, fut: impl Future<Output = T>) -> T {
    tokio::time::timeout(STEP, fut)
        .await
        .unwrap_or_else(|_| panic!("Timeout nach {STEP:?}: {what}"))
}

struct NoKeyFiles;
impl KeyFileReader for NoKeyFiles {
    fn read(&self, path: &str, _e: bool) -> Result<KeyFileContent, KeyFileError> {
        panic!("unerwarteter Schlüsseldatei-Zugriff auf {path}");
    }
    fn inspect(&self, path: &str) -> KeyFileFacts {
        panic!("unerwarteter Schlüsseldatei-Befund zu {path}");
    }
}

struct Creds;
impl CredentialStore for Creds {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        if r.as_str() == "jump-test-password" {
            Ok(SecretString::from(PASS.to_string()))
        } else {
            Err(CredentialError::NotFound(r.clone()))
        }
    }
    fn set(&self, _r: &CredentialRef, _v: SecretString) -> CredentialResult<()> {
        Ok(())
    }
    fn delete(&self, _r: &CredentialRef) -> CredentialResult<()> {
        Ok(())
    }
}

/// Leerer Speicher: jeder Host-Key ist zunächst unbekannt und muss
/// ausdrücklich per `trust()` bestätigt werden.
#[derive(Default)]
struct Keys {
    known: Mutex<HashMap<(String, u16), Vec<u8>>>,
}
impl HostKeyStore for Keys {
    fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyDecision {
        match self.known.lock().unwrap().get(&(host.to_string(), port)) {
            None => HostKeyDecision::Unknown {
                fingerprint: format!("{key:02x?}"),
            },
            Some(s) if s.as_slice() == key => HostKeyDecision::Trusted,
            Some(s) => HostKeyDecision::Mismatch {
                expected_fingerprint: format!("{s:02x?}"),
                actual_fingerprint: format!("{key:02x?}"),
            },
        }
    }
    fn trust(&self, host: &str, port: u16, key: &[u8]) -> Result<(), SshError> {
        self.known
            .lock()
            .unwrap()
            .insert((host.to_string(), port), key.to_vec());
        Ok(())
    }
}

fn hop(host: &str, port: u16) -> Hop {
    Hop {
        host: host.to_string(),
        port,
        username: USER.to_string(),
        auth: AuthMethod::Password {
            credential_ref: CredentialRef::new("jump-test-password"),
        },
    }
}

/// Verbindet über `hops`. Jede `PendingHostKeyConfirmation` betrifft genau
/// einen Hop; sie wird bestätigt (`trust()` für genau diesen Schlüssel) und
/// neu verbunden. Gibt die Transportverbindung und die Liste der
/// bestätigten `(host, port)` zurück.
async fn connect_confirming(hops: Vec<Hop>) -> (Box<dyn SshTransport>, Vec<(String, u16)>) {
    let expected = hops.len();
    let keys: Arc<dyn HostKeyStore> = Arc::new(Keys::default());
    let target = ConnectionTarget { hops };
    let mut confirmed: Vec<(String, u16)> = Vec::new();
    for _ in 0..=expected {
        match step(
            "connect()",
            ssh_transport::connect(&target, &Creds, &NoKeyFiles, keys.clone()),
        )
        .await
        .expect("connect() sollte gelingen oder pausieren")
        {
            ConnectOutcome::Connected(t) => return (t, confirmed),
            ConnectOutcome::PendingHostKeyConfirmation {
                host,
                port,
                raw_key,
                decision,
            } => {
                assert!(
                    matches!(decision, HostKeyDecision::Unknown { .. }),
                    "erwartet Unknown für {host}:{port}, bekam {decision:?}"
                );
                assert!(
                    !confirmed.contains(&(host.clone(), port)),
                    "derselbe Hop {host}:{port} wurde zweimal abgefragt"
                );
                keys.trust(&host, port, &raw_key).unwrap();
                confirmed.push((host, port));
            }
        }
    }
    panic!("Verbindung nach {expected} Bestätigungen nicht zustande gekommen");
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

#[ignore = "braucht OpenSSH-Container, s. Modul-Doku"]
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
#[ignore = "braucht OpenSSH-Container, s. Modul-Doku"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn single_hop_to_bastion_is_distinguishable_from_target() {
    let (bastion_name, target_name) = direct_hostnames().await;
    let (mut t, _) = connect_confirming(vec![hop("127.0.0.1", bastion_port())]).await;
    let name = hostname(&mut t).await;
    assert_eq!(name, bastion_name);
    assert_ne!(name, target_name);
}

#[ignore = "braucht OpenSSH-Container, s. Modul-Doku"]
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

#[ignore = "braucht OpenSSH-Container, s. Modul-Doku"]
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
