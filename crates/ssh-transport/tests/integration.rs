//! Integrationstests gegen einen echten, in-process laufenden `russh`-
//! Server (Spec 0005, Abschnitt 8, zweiter Punkt). Bewusst als eigenes
//! Cargo-Test-Target getrennt von den reinen Unit-Tests in `src/` — läuft
//! **nicht** im normalen `cargo test`-Lauf mit, sondern gezielt über:
//!
//! ```text
//! cargo test -p ssh-transport --test integration
//! ```

mod fixtures;

use std::collections::HashMap;
use std::sync::Mutex;

use secrecy::SecretString;
use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore,
};
use ssh_manager_core::ssh::{
    ConnectionTarget, Hop, HostKeyDecision, HostKeyStore, PtySize, SshError,
};
use ssh_transport::ConnectOutcome;

use fixtures::test_server::{sftp_local_path, RunningTestServer, TEST_PASSWORD, TEST_USERNAME};

const PASSWORD_CREDENTIAL: &str = "test-password-credential";

#[derive(Default)]
struct TestCredentialStore {
    /// Spec 0022, Abschnitt 3: zählt `get()`-Aufrufe, damit Tests
    /// nachweisen können, dass das SSH-Login-Credential nur einmalig beim
    /// Verbindungsaufbau gelesen wird, nicht erneut pro `execute()`-Aufruf.
    get_calls: Mutex<usize>,
}

impl TestCredentialStore {
    fn get_calls(&self) -> usize {
        *self.get_calls.lock().unwrap()
    }
}

impl CredentialStore for TestCredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        *self.get_calls.lock().unwrap() += 1;
        if r.as_str() == PASSWORD_CREDENTIAL {
            Ok(SecretString::from(TEST_PASSWORD.to_string()))
        } else {
            Err(CredentialError::NotFound(r.clone()))
        }
    }
    fn set(&self, _r: &CredentialRef, _value: SecretString) -> CredentialResult<()> {
        Ok(())
    }
    fn delete(&self, _r: &CredentialRef) -> CredentialResult<()> {
        Ok(())
    }
}

#[derive(Default)]
struct TestHostKeyStore {
    known: Mutex<HashMap<(String, u16), Vec<u8>>>,
    /// Spec 0069, Teil A3, Test 11/13: zählt `trust()`-Aufrufe, damit der
    /// Timeout-Gegenbeweis belegen kann, dass ein Verbindungs-Timeout NIE
    /// einen Host-Key akzeptiert (Spec 0069 §5, Sicherheits-Invariante).
    trust_calls: Mutex<usize>,
}

impl TestHostKeyStore {
    fn trust_calls(&self) -> usize {
        *self.trust_calls.lock().unwrap()
    }
}

impl HostKeyStore for TestHostKeyStore {
    fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyDecision {
        match self.known.lock().unwrap().get(&(host.to_string(), port)) {
            None => HostKeyDecision::Unknown {
                fingerprint: hex(key),
            },
            Some(stored) if stored.as_slice() == key => HostKeyDecision::Trusted,
            Some(stored) => HostKeyDecision::Mismatch {
                expected_fingerprint: hex(stored),
                actual_fingerprint: hex(key),
            },
        }
    }

    fn trust(&self, host: &str, port: u16, key: &[u8]) -> Result<(), SshError> {
        *self.trust_calls.lock().unwrap() += 1;
        self.known
            .lock()
            .unwrap()
            .insert((host.to_string(), port), key.to_vec());
        Ok(())
    }
}

fn hex(key: &[u8]) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

fn password_hop(host: &str, port: u16) -> Hop {
    Hop {
        host: host.to_string(),
        port,
        username: TEST_USERNAME.to_string(),
        auth: AuthMethod::Password {
            credential_ref: CredentialRef::new(PASSWORD_CREDENTIAL),
        },
    }
}

/// Verbindet gegen `server` und vertraut dessen Host-Key vorab (über den
/// bekannten `host_public_key` aus der Fixture) — für Tests, die nicht die
/// Host-Key-Bestätigung selbst testen, sondern eine bereits vertraute
/// Verbindung voraussetzen.
async fn connect_trusted(
    server: &RunningTestServer,
) -> Box<dyn ssh_manager_core::ssh::SshTransport> {
    let host_keys = TestHostKeyStore::default();
    host_keys
        .trust("127.0.0.1", server.addr.port(), &server.host_public_key)
        .unwrap();
    let host_keys: std::sync::Arc<dyn HostKeyStore> = std::sync::Arc::new(host_keys);

    let target = ConnectionTarget {
        hops: vec![password_hop("127.0.0.1", server.addr.port())],
    };
    let credentials = TestCredentialStore::default();

    match ssh_transport::connect(&target, &credentials, host_keys)
        .await
        .expect("connect() sollte gelingen")
    {
        ConnectOutcome::Connected(transport) => transport,
        ConnectOutcome::PendingHostKeyConfirmation { .. } => {
            panic!("Host-Key war vorab als vertraut hinterlegt, PendingHostKeyConfirmation war nicht erwartet")
        }
    }
}

/// Spec 0005 Abschnitt 8 / Aufgabenstellung Teil 2 Punkt 5: einfacher
/// Exec-Roundtrip (Kommando hin, Output zurück, korrekter Exit-Code).
#[tokio::test]
async fn test_exec_roundtrip() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;

    let output = transport
        .execute("hello world")
        .await
        .expect("execute() sollte gelingen");

    assert_eq!(output.stdout, b"echo:hello world\n");
    assert_eq!(output.exit_code, Some(0));

    transport
        .disconnect()
        .await
        .expect("disconnect() sollte gelingen");
}

/// Spec 0022, Abschnitt 3, zweiter Punkt: das SSH-Login-Credential wird nur
/// einmal während des Verbindungsaufbaus (Passwort-Authentifizierung) aus
/// dem `CredentialStore` gelesen — mehrere nachfolgende `execute()`-Aufrufe
/// auf derselben Verbindung dürfen keine weiteren Abrufe auslösen, da
/// `SshTransport::execute()` auf dem bereits authentifizierten Kanal
/// arbeitet und den `CredentialStore` gar nicht mehr referenziert (s.
/// `crates/ssh-transport/src/transport.rs`).
#[tokio::test]
async fn test_ssh_login_credential_read_once_regardless_of_execute_calls() {
    let server = RunningTestServer::start().await;
    let host_keys = TestHostKeyStore::default();
    host_keys
        .trust("127.0.0.1", server.addr.port(), &server.host_public_key)
        .unwrap();
    let host_keys: std::sync::Arc<dyn HostKeyStore> = std::sync::Arc::new(host_keys);
    let target = ConnectionTarget {
        hops: vec![password_hop("127.0.0.1", server.addr.port())],
    };
    let credentials = TestCredentialStore::default();

    let mut transport = match ssh_transport::connect(&target, &credentials, host_keys)
        .await
        .expect("connect() sollte gelingen")
    {
        ConnectOutcome::Connected(transport) => transport,
        ConnectOutcome::PendingHostKeyConfirmation { .. } => {
            panic!("Host-Key war vorab als vertraut hinterlegt")
        }
    };
    assert_eq!(
        credentials.get_calls(),
        1,
        "Passwort-Authentifizierung liest das Credential genau einmal während des Handshakes"
    );

    for _ in 0..5 {
        transport
            .execute("hello world")
            .await
            .expect("execute() sollte gelingen");
    }

    assert_eq!(
        credentials.get_calls(),
        1,
        "execute()-Aufrufe dürfen das SSH-Login-Credential nicht erneut aus dem CredentialStore lesen"
    );

    transport
        .disconnect()
        .await
        .expect("disconnect() sollte gelingen");
}

/// PTY-Shell-Aufbau inkl. Resize.
#[tokio::test]
async fn test_pty_shell_with_resize() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;

    let mut shell = transport
        .open_shell(PtySize { cols: 80, rows: 24 })
        .await
        .expect("open_shell() sollte gelingen");

    shell
        .write(b"ping\n")
        .await
        .expect("write() sollte gelingen");
    let echoed = shell.read().await.expect("read() sollte Daten liefern");
    assert_eq!(echoed, b"ping\n");

    shell
        .resize(PtySize {
            cols: 120,
            rows: 40,
        })
        .await
        .expect("resize() sollte gelingen");

    // Nach dem Resize funktioniert der Shell-Kanal weiterhin normal.
    shell
        .write(b"pong\n")
        .await
        .expect("write() nach resize() sollte gelingen");
    let echoed_again = shell
        .read()
        .await
        .expect("read() nach resize() sollte Daten liefern");
    assert_eq!(echoed_again, b"pong\n");
}

/// Zwei-Hop-Jump-Verbindung gegen zwei in-process Test-Server: Server A
/// agiert als Bastion (leitet den `direct-tcpip`-Kanal an Server B weiter),
/// die eigentliche Session (Exec) läuft gegen Server B.
///
/// `#[ignore]`: schlägt reproduzierbar mit "Bad packet size" fehl. Per
/// Byte-Level-Tracing direkt auf dem rohen `TcpStream` verifiziert (nicht
/// nur vermutet): der über den Tunnel erreichte Ziel-Server sendet seine
/// eigene SSH-Identifikationszeile ein zweites Mal, direkt vor seiner
/// KEXINIT-Antwort; der Client liest diese zweite Kopie fälschlich als
/// 4-Byte-Paketlängen-Präfix. Nicht diese Implementierung ist die Ursache
/// (Bastion-TCP-Proxy und `connect()`-Ablauf entsprechen exakt Spec 0005
/// Abschnitt 5) — es ist ein Verhalten von `russh` 0.63.1 selbst. Siehe
/// `docs/adr/0008-russh-nested-tunnel-limitation.md` für die vollständige
/// Fehlersuche (u. a. ausgeschlossen: TCP-Nagle-Koaleszenz, doppelte
/// `channel_open_direct_tcpip`-/`run_stream`-Aufrufe) und zwei unabhängige,
/// offene `russh`-Upstream-Reports mit demselben grundsätzlichen Muster.
/// Der Test bleibt bestehen (nicht gelöscht) als Dokumentation des
/// erwarteten Verhaltens und als Regressionscheck für einen künftigen Fix.
#[ignore = "bekannte russh-0.63.1-Einschränkung bei verschachteltem SSH-über-SSH-Handshake, s. Doc-Kommentar/ADR 0008"]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_two_hop_jump_connection() {
    let bastion = RunningTestServer::start().await;
    let target_server = RunningTestServer::start().await;

    let host_keys = TestHostKeyStore::default();
    host_keys
        .trust("127.0.0.1", bastion.addr.port(), &bastion.host_public_key)
        .unwrap();
    host_keys
        .trust(
            "127.0.0.1",
            target_server.addr.port(),
            &target_server.host_public_key,
        )
        .unwrap();
    let host_keys: std::sync::Arc<dyn HostKeyStore> = std::sync::Arc::new(host_keys);

    let target = ConnectionTarget {
        hops: vec![
            password_hop("127.0.0.1", bastion.addr.port()),
            password_hop("127.0.0.1", target_server.addr.port()),
        ],
    };
    let credentials = TestCredentialStore::default();

    let outcome = ssh_transport::connect(&target, &credentials, host_keys)
        .await
        .expect("Zwei-Hop-connect() sollte gelingen");

    let mut transport = match outcome {
        ConnectOutcome::Connected(transport) => transport,
        ConnectOutcome::PendingHostKeyConfirmation { .. } => {
            panic!("beide Host-Keys waren vorab vertraut")
        }
    };

    let output = transport
        .execute("via-jump")
        .await
        .expect("execute() über den Tunnel sollte gelingen");
    assert_eq!(output.stdout, b"echo:via-jump\n");
    assert_eq!(output.exit_code, Some(0));
}

/// Verhalten bei `Unknown`-Host-Key: Verbindung pausiert korrekt (kein
/// automatisches Akzeptieren), wird nach `trust()` fortgesetzt (per
/// erneutem `connect()`-Aufruf — s. ADR-Vorschlag in der
/// Abschluss-Nachricht dazu, warum "fortgesetzt" hier einen frischen
/// Reconnect statt einer buchstäblich pausierten Verbindung bedeutet).
#[tokio::test]
async fn test_unknown_host_key_pauses_then_trust_continues() {
    let server = RunningTestServer::start().await;
    let host_keys: std::sync::Arc<dyn HostKeyStore> =
        std::sync::Arc::new(TestHostKeyStore::default());

    let target = ConnectionTarget {
        hops: vec![password_hop("127.0.0.1", server.addr.port())],
    };
    let credentials = TestCredentialStore::default();

    let first_attempt = ssh_transport::connect(&target, &credentials, host_keys.clone())
        .await
        .expect("connect() selbst darf bei Unknown-Key nicht fehlschlagen");

    let (host, port, raw_key) = match first_attempt {
        ConnectOutcome::PendingHostKeyConfirmation {
            host,
            port,
            raw_key,
            decision,
        } => {
            assert!(
                matches!(decision, HostKeyDecision::Unknown { .. }),
                "erwartet Unknown, bekam {decision:?}"
            );
            (host, port, raw_key)
        }
        ConnectOutcome::Connected(_) => {
            panic!("unbekannter Host-Key hätte pausieren müssen, nicht direkt verbinden")
        }
    };

    // Ohne trust() bleibt jeder weitere Versuch PendingHostKeyConfirmation.
    let second_attempt = ssh_transport::connect(&target, &credentials, host_keys.clone())
        .await
        .expect("connect() darf bei erneutem Unknown-Key nicht fehlschlagen");
    assert!(matches!(
        second_attempt,
        ConnectOutcome::PendingHostKeyConfirmation { .. }
    ));

    host_keys
        .trust(&host, port, &raw_key)
        .expect("trust() sollte gelingen");

    let after_trust = ssh_transport::connect(&target, &credentials, host_keys)
        .await
        .expect("connect() nach trust() sollte gelingen");
    match after_trust {
        ConnectOutcome::Connected(_) => {}
        ConnectOutcome::PendingHostKeyConfirmation { .. } => {
            panic!("nach trust() hätte die Verbindung gelingen müssen")
        }
    }
}

// --- Spec 0069, Teil A3 (ERHÖHT): Verbindungsfehler unterscheiden ---------

/// Test 7: Verbindung zu einem geschlossenen lokalen Port → `ConnectionRefused`.
/// *Gegenbeweis:* vor diesem Schritt lieferte `map_russh_error` hier
/// unterschiedslos `ConnectionFailed` (jeder `io::Error` fiel in denselben
/// Zweig) — dieser Test schlug vor dem Fix fehl (der `matches!` traf nicht
/// zu), s. Bericht.
#[tokio::test]
async fn test_connect_to_closed_local_port_yields_connection_refused() {
    // Port binden, dann sofort wieder freigeben — verlässlich "zu" (kein
    // Dienst dahinter), ohne einen Port fest zu verdrahten.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let target = ConnectionTarget {
        hops: vec![password_hop("127.0.0.1", port)],
    };
    let credentials = TestCredentialStore::default();
    let host_keys: std::sync::Arc<dyn HostKeyStore> =
        std::sync::Arc::new(TestHostKeyStore::default());

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        ssh_transport::connect(&target, &credentials, host_keys),
    )
    .await
    .expect("darf nicht hängen");
    let err = match result {
        Err(err) => err,
        Ok(_) => panic!("ein geschlossener Port darf keine Verbindung liefern"),
    };

    assert!(
        matches!(err, SshError::ConnectionRefused(_)),
        "erwartet ConnectionRefused, bekam {err:?}"
    );
    assert_eq!(err.code(), "SSH_CONNECTION_REFUSED");
}

/// Test 7: Verbindung zu einem nicht auflösbaren Hostnamen → `HostNotFound`
/// (nachträgliche DNS-Diagnose, s. `connect::diagnose_connection_failed_dns`).
/// `.invalid` ist laut RFC 2606 reserviert und darf nie auflösbar sein.
/// *Gegenbeweis:* vor der DNS-Diagnose lieferte dieser Fall `ConnectionFailed`
/// (derselbe Auffangfall wie jeder andere `io::Error`), dieser Test schlug
/// vorher fehl.
#[tokio::test]
async fn test_connect_to_unresolvable_host_yields_host_not_found() {
    let target = ConnectionTarget {
        hops: vec![password_hop("this-host-does-not-exist.invalid", 22)],
    };
    let credentials = TestCredentialStore::default();
    let host_keys: std::sync::Arc<dyn HostKeyStore> =
        std::sync::Arc::new(TestHostKeyStore::default());

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        ssh_transport::connect(&target, &credentials, host_keys),
    )
    .await
    .expect("darf nicht hängen");
    let err = match result {
        Err(err) => err,
        Ok(_) => panic!("ein nicht auflösbarer Hostname darf keine Verbindung liefern"),
    };

    assert!(
        matches!(err, SshError::HostNotFound(_)),
        "erwartet HostNotFound, bekam {err:?}"
    );
    assert_eq!(err.code(), "SSH_HOST_NOT_FOUND");
}

/// Test 15: ein geänderter Host-Key bleibt weiterhin `Mismatch` — nie ein
/// Verbindungsfehler-Code, auch nicht nach Einführung der neuen
/// `SshError`-Varianten in diesem Schritt (Sicherheits-Invariante, Spec
/// 0069 §5).
#[tokio::test]
async fn test_changed_host_key_still_yields_mismatch_not_a_connection_error() {
    let server = RunningTestServer::start().await;
    let host_keys = TestHostKeyStore::default();
    // Absichtlich ein ANDERER Key als der tatsächliche Server-Key.
    let wrong_key = {
        let mut k = server.host_public_key.clone();
        k.push(0xFF);
        k
    };
    host_keys
        .trust("127.0.0.1", server.addr.port(), &wrong_key)
        .unwrap();
    let host_keys: std::sync::Arc<dyn HostKeyStore> = std::sync::Arc::new(host_keys);

    let target = ConnectionTarget {
        hops: vec![password_hop("127.0.0.1", server.addr.port())],
    };
    let credentials = TestCredentialStore::default();

    let outcome = ssh_transport::connect(&target, &credentials, host_keys)
        .await
        .expect("connect() selbst darf bei Mismatch nicht fehlschlagen");

    match outcome {
        ConnectOutcome::PendingHostKeyConfirmation { decision, .. } => {
            assert!(
                matches!(decision, HostKeyDecision::Mismatch { .. }),
                "erwartet Mismatch, bekam {decision:?}"
            );
        }
        ConnectOutcome::Connected(_) => {
            panic!("ein geänderter Host-Key darf niemals stillschweigend akzeptiert werden")
        }
    }
}

/// Test 13: ein hängender Handshake (Gegenstelle nimmt an, schickt aber nie
/// ein SSH-Banner) endet nach dem (verkürzten) Timeout mit `SshError::Timeout`
/// statt für immer zu hängen — und akzeptiert dabei keinen Host-Key (0
/// `trust()`-Aufrufe). Test selbst mit äußerem `tokio::time::timeout`
/// abgesichert, damit ein Regressions-Bug hier sauber fehlschlägt statt den
/// gesamten Testlauf hängen zu lassen.
#[tokio::test]
async fn test_hanging_handshake_times_out_and_never_trusts_a_host_key() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // Nimmt die Verbindung an, hält sie aber offen, ohne je ein
    // SSH-Identifikations-Banner zu schicken — genau der Fall, den
    // `SSH_CONNECT_TIMEOUT` abfangen soll.
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            std::thread::sleep(std::time::Duration::from_secs(30));
            drop(stream);
        }
    });

    let target = ConnectionTarget {
        hops: vec![password_hop("127.0.0.1", port)],
    };
    let credentials = TestCredentialStore::default();
    let host_keys = std::sync::Arc::new(TestHostKeyStore::default());
    let host_keys_dyn: std::sync::Arc<dyn HostKeyStore> = host_keys.clone();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        ssh_transport::connect_with_timeout(
            ssh_transport::connect(&target, &credentials, host_keys_dyn),
            std::time::Duration::from_millis(300),
        ),
    )
    .await
    .expect("connect_with_timeout selbst darf nicht hängen");

    match result {
        Err(SshError::Timeout) => {}
        Err(other) => panic!("erwartet SshError::Timeout, bekam {other:?}"),
        Ok(_) => panic!("ein hängender Handshake darf niemals als verbunden gelten"),
    }
    assert_eq!(
        host_keys.trust_calls(),
        0,
        "ein Timeout darf niemals einen Host-Key vertrauen"
    );
}

/// Test 17: ein Verbindungsversuch mit Passwort gegen einen geschlossenen
/// Port darf das Passwort weder in der Fehlermeldung noch im Log
/// preisgeben — auch nicht in einer der neuen, präziseren `SshError`-
/// Varianten (deren Payload aus `io::Error`-Text besteht, nie aus
/// Zugangsdaten).
#[tokio::test]
async fn test_connection_error_against_closed_port_never_leaks_the_password() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    const SECRET_MARKER: &str = "s3cr3t-teil-a3-marker";
    struct PasswordProbeStore;
    impl CredentialStore for PasswordProbeStore {
        fn get(&self, _r: &CredentialRef) -> CredentialResult<SecretString> {
            Ok(SecretString::from(SECRET_MARKER.to_string()))
        }
        fn set(&self, _r: &CredentialRef, _value: SecretString) -> CredentialResult<()> {
            Ok(())
        }
        fn delete(&self, _r: &CredentialRef) -> CredentialResult<()> {
            Ok(())
        }
    }

    let target = ConnectionTarget {
        hops: vec![password_hop("127.0.0.1", port)],
    };
    let host_keys: std::sync::Arc<dyn HostKeyStore> =
        std::sync::Arc::new(TestHostKeyStore::default());

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        ssh_transport::connect(&target, &PasswordProbeStore, host_keys),
    )
    .await
    .expect("darf nicht hängen");
    let err = match result {
        Err(err) => err,
        Ok(_) => panic!("ein geschlossener Port darf keine Verbindung liefern"),
    };

    let rendered = format!("{err} {err:?}");
    assert!(
        !rendered.contains(SECRET_MARKER),
        "das Passwort darf nicht in der Fehlermeldung auftauchen: {rendered}"
    );
}

// --- Spec 0020, Abschnitt 6: SFTP-Integrationstests ------------------------

/// Upload (write_file) und Download (read_file) im selben Test: schreibt
/// über SFTP, verifiziert den Inhalt direkt auf der lokalen Festplatte
/// (echter Server-Effekt, nicht nur der Rückgabewert des Aufrufs), liest
/// danach über SFTP zurück und vergleicht.
#[tokio::test]
async fn test_sftp_upload_download_roundtrip() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;
    let mut sftp = transport
        .open_sftp()
        .await
        .expect("open_sftp() sollte gelingen");

    sftp.write_file("/roundtrip.txt", b"hallo sftp")
        .await
        .expect("write_file() sollte gelingen");

    let on_disk = std::fs::read(sftp_local_path(&server, "/roundtrip.txt"))
        .expect("Datei sollte auf der lokalen Festplatte des Testservers liegen");
    assert_eq!(on_disk, b"hallo sftp");

    let read_back = sftp
        .read_file("/roundtrip.txt")
        .await
        .expect("read_file() sollte gelingen");
    assert_eq!(read_back, b"hallo sftp");
}

/// Verzeichnisauflistung: mehrere Dateien lokal vorab anlegen, per SFTP
/// auflisten, Namen und Größen prüfen.
#[tokio::test]
async fn test_sftp_list_dir() {
    let server = RunningTestServer::start().await;
    std::fs::write(sftp_local_path(&server, "/a.txt"), b"eins").unwrap();
    std::fs::write(sftp_local_path(&server, "/b.txt"), b"zwei-zwei").unwrap();
    std::fs::create_dir(sftp_local_path(&server, "/subdir")).unwrap();

    let mut transport = connect_trusted(&server).await;
    let mut sftp = transport
        .open_sftp()
        .await
        .expect("open_sftp() sollte gelingen");

    let mut entries = sftp
        .list_dir("/")
        .await
        .expect("list_dir() sollte gelingen");
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].name, "a.txt");
    assert_eq!(entries[0].size, 4);
    assert!(!entries[0].is_dir);
    assert_eq!(entries[1].name, "b.txt");
    assert_eq!(entries[1].size, 9);
    assert_eq!(entries[2].name, "subdir");
    assert!(entries[2].is_dir);
}

/// Rename: Datei über SFTP umbenennen, alten/neuen Pfad lokal verifizieren.
#[tokio::test]
async fn test_sftp_rename() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;
    let mut sftp = transport
        .open_sftp()
        .await
        .expect("open_sftp() sollte gelingen");

    sftp.write_file("/old-name.txt", b"inhalt")
        .await
        .expect("write_file() sollte gelingen");
    sftp.rename("/old-name.txt", "/new-name.txt")
        .await
        .expect("rename() sollte gelingen");

    assert!(!sftp_local_path(&server, "/old-name.txt").exists());
    assert_eq!(
        std::fs::read(sftp_local_path(&server, "/new-name.txt")).unwrap(),
        b"inhalt"
    );
}

/// Delete (remove): Datei über SFTP löschen, lokal verifizieren, dass sie
/// weg ist.
#[tokio::test]
async fn test_sftp_delete() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;
    let mut sftp = transport
        .open_sftp()
        .await
        .expect("open_sftp() sollte gelingen");

    sftp.write_file("/to-delete.txt", b"weg damit")
        .await
        .expect("write_file() sollte gelingen");
    assert!(sftp_local_path(&server, "/to-delete.txt").exists());

    sftp.remove("/to-delete.txt")
        .await
        .expect("remove() sollte gelingen");

    assert!(!sftp_local_path(&server, "/to-delete.txt").exists());
}

/// Spec 0054, Teil 3: `remove_dir` löscht ein leeres Verzeichnis — die
/// eigentliche rekursive Lösch-Logik (erst Dateien, dann Verzeichnisse
/// bottom-up) lebt in `app-shell::commands::sftp_delete`, hier wird nur die
/// neue Trait-Methode selbst gegen einen echten SFTP-Roundtrip verifiziert.
#[tokio::test]
async fn test_sftp_remove_dir() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;
    let mut sftp = transport
        .open_sftp()
        .await
        .expect("open_sftp() sollte gelingen");

    sftp.create_dir("/empty-dir").await.unwrap();
    assert!(sftp_local_path(&server, "/empty-dir").is_dir());

    sftp.remove_dir("/empty-dir")
        .await
        .expect("remove_dir() sollte gelingen");

    assert!(!sftp_local_path(&server, "/empty-dir").exists());
}

/// Spec 0054, Teil 3 (chmod): `set_permissions` ändert die Rechte-Bits
/// einer existierenden Datei, ein anschließendes `stat()` meldet die neuen
/// Bits zurück.
#[tokio::test]
async fn test_sftp_set_permissions() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;
    let mut sftp = transport
        .open_sftp()
        .await
        .expect("open_sftp() sollte gelingen");

    sftp.write_file("/chmod-me.txt", b"x").await.unwrap();
    sftp.set_permissions("/chmod-me.txt", 0o640)
        .await
        .expect("set_permissions() sollte gelingen");

    let entry = sftp.stat("/chmod-me.txt").await.unwrap();
    assert_eq!(entry.permissions, 0o640);
}

/// Stat: Größe und Verzeichnis-Flag einer existierenden Datei korrekt
/// gemeldet; ein nicht existierender Pfad liefert einen Fehler statt eines
/// Platzhalter-Ergebnisses.
#[tokio::test]
async fn test_sftp_stat() {
    let server = RunningTestServer::start().await;
    std::fs::write(sftp_local_path(&server, "/stat-me.txt"), b"thirteen char").unwrap();

    let mut transport = connect_trusted(&server).await;
    let mut sftp = transport
        .open_sftp()
        .await
        .expect("open_sftp() sollte gelingen");

    let entry = sftp
        .stat("/stat-me.txt")
        .await
        .expect("stat() sollte gelingen");
    assert_eq!(entry.size, 13);
    assert!(!entry.is_dir);

    let missing = sftp.stat("/does-not-exist.txt").await;
    assert!(missing.is_err());
}

/// Spec 0027: `execute_cancellable` gegen ein nie von selbst endendes
/// Kommando (`"never-ending"`, s. `fixtures::test_server::exec_request`) —
/// muss zurückkehren, sobald `cancel` auflöst, statt für immer auf das
/// reguläre Kanal-Ende zu warten, und dabei die bereits eingetroffene
/// erste Ausgabezeile mitliefern.
#[tokio::test]
async fn test_execute_cancellable_returns_partial_output_on_cancel() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let _ = cancel_tx.send(());
    });

    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        transport.execute_cancellable("never-ending", cancel_rx),
    )
    .await
    .expect("execute_cancellable() darf nicht hängen bleiben, wenn cancel auflöst")
    .expect("execute_cancellable() sollte trotz Abbruch Ok liefern");

    assert!(outcome.cancelled);
    assert_eq!(outcome.output.stdout, b"first line\n");
    assert_eq!(outcome.output.exit_code, None);
}

/// Ohne Abbruch verhält sich `execute_cancellable` wie `execute` — normaler
/// Exit-Code, `cancelled: false`.
#[tokio::test]
async fn test_execute_cancellable_behaves_like_execute_without_cancel() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;
    let (_cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();

    let outcome = transport
        .execute_cancellable("hello world", cancel_rx)
        .await
        .expect("execute_cancellable() sollte gelingen");

    assert!(!outcome.cancelled);
    assert_eq!(outcome.output.stdout, b"echo:hello world\n");
    assert_eq!(outcome.output.exit_code, Some(0));
}

/// Spec 0043, Fund A: der in-process-Testserver liefert über das
/// `"flood"`-Kommando (s. `fixtures::test_server`) deutlich mehr Bytes, als
/// der (über `SshTransport::set_max_output_bytes` künstlich klein gesetzte)
/// Output-Cap zulässt — belegt, dass der zurückgelieferte Puffer nie über
/// das Limit hinaus wächst und das Ergebnis als `truncated` markiert ist,
/// statt (wie vor dem Fix) erst nach vollständigem Puffern der gesamten
/// Server-Ausgabe zu greifen.
#[tokio::test]
async fn test_t43_execute_caps_output_during_streaming() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;
    const SMALL_LIMIT: usize = 4096;
    transport.set_max_output_bytes(SMALL_LIMIT);

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        transport.execute("flood"),
    )
    .await
    .expect("execute() darf bei einem flutenden Server nicht hängen bleiben")
    .expect("execute() sollte trotz Abschneiden Ok liefern");

    assert!(
        output.stdout.len() <= SMALL_LIMIT + ssh_transport::TRUNCATION_NOTICE.len(),
        "stdout darf nie über das konfigurierte Limit hinauswachsen, war aber {} Bytes",
        output.stdout.len()
    );
    assert!(
        output.truncated,
        "CommandOutput.truncated muss gesetzt sein, wenn der Cap gegriffen hat"
    );
}

/// Spec 0067, Teil A: der erhöhte Modus betreibt SFTP über einen
/// Exec-Kanal (`sudo -n <sftp-server>`) statt über das Subsystem — die
/// Fixture bedient dafür ein eigenes Unterverzeichnis, also sieht der Test
/// nur dann `nur-erhoeht.txt`, wenn wirklich der Exec-Kanal benutzt wurde.
#[tokio::test]
async fn test_sftp_via_exec_runs_sftp_over_the_exec_channel() {
    let server = RunningTestServer::start().await;
    std::fs::create_dir(sftp_local_path(&server, "/elevated")).unwrap();
    std::fs::write(
        sftp_local_path(&server, "/elevated/nur-erhoeht.txt"),
        b"root",
    )
    .unwrap();
    std::fs::write(sftp_local_path(&server, "/normal.txt"), b"user").unwrap();

    let mut transport = connect_trusted(&server).await;
    let mut elevated = transport
        .open_sftp_via_exec("sudo -n /usr/lib/openssh/sftp-server")
        .await
        .expect("SFTP über den Exec-Kanal sollte starten");
    let names: Vec<String> = elevated
        .list_dir("/")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, vec!["nur-erhoeht.txt".to_string()]);

    // Der normale Kanal bleibt daneben unverändert nutzbar.
    let mut normal = transport.open_sftp().await.unwrap();
    let normal_names: Vec<String> = normal
        .list_dir("/")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert!(normal_names.contains(&"normal.txt".to_string()));
    assert!(!normal_names.contains(&"nur-erhoeht.txt".to_string()));
}

/// Spec 0067, A1/A3: verweigert sudo (Passwort verlangt), scheitert der
/// Start sofort mit einem Fehler — er hängt nie.
#[tokio::test]
async fn test_sftp_via_exec_fails_fast_when_sudo_refuses() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        transport.open_sftp_via_exec("sudo -n /denied/sftp-server"),
    )
    .await
    .expect("ein verweigertes sudo muss nach der kurzen Handshake-Frist scheitern");

    assert!(result.is_err());
}
