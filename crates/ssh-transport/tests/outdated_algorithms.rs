//! Issue #1173 (Spec 0005, "Outdated algorithms"): connections to servers
//! that only offer outdated SSH algorithms succeed with a warning; a server
//! with no usable algorithm of one kind fails with a typed error before any
//! credential is sent.
//!
//! The servers are real in-process `russh` servers whose preference lists
//! are restricted; the client under test is the production `connect_with_log`.
#![expect(clippy::unwrap_used, reason = "integration test")]

use std::borrow::Cow;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, ChannelOpenHandle, Config, Handler, Msg, Session};
use russh::{cipher, kex, mac, Channel, Preferred};
use secrecy::SecretString;
use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore,
};
use ssh_manager_core::ssh::{
    AlgorithmKind, ConnectLog, ConnectStep, ConnectionTarget, Hop, HostKeyDecision, HostKeyStore,
    KeyFileContent, KeyFileError, KeyFileFacts, KeyFileReader, SshError, StepStatus,
    STRICT_KEX_MISSING,
};
use ssh_transport::ConnectOutcome;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

const USER: &str = "testuser";
const PASSWORD: &str = "testpass";
const CREDENTIAL: &str = "cred";

struct NoKeyFiles;
impl KeyFileReader for NoKeyFiles {
    fn read(&self, path: &str, _: bool) -> Result<KeyFileContent, KeyFileError> {
        panic!("unexpected key file read: {path}");
    }
    fn inspect(&self, path: &str) -> KeyFileFacts {
        panic!("unexpected key file inspect: {path}");
    }
}

struct Creds;
impl CredentialStore for Creds {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        if r.as_str() == CREDENTIAL {
            Ok(SecretString::from(PASSWORD.to_string()))
        } else {
            Err(CredentialError::NotFound(r.clone()))
        }
    }
    fn set(&self, _: &CredentialRef, _: SecretString) -> CredentialResult<()> {
        Ok(())
    }
    fn delete(&self, _: &CredentialRef) -> CredentialResult<()> {
        Ok(())
    }
}

#[derive(Default)]
struct Keys(Mutex<HashMap<(String, u16), Vec<u8>>>);
impl HostKeyStore for Keys {
    fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyDecision {
        match self.0.lock().unwrap().get(&(host.to_string(), port)) {
            Some(k) if k.as_slice() == key => HostKeyDecision::Trusted,
            _ => HostKeyDecision::Unknown {
                fingerprint: "x".into(),
            },
        }
    }
    fn trust(&self, host: &str, port: u16, key: &[u8]) -> Result<(), SshError> {
        self.0
            .lock()
            .unwrap()
            .insert((host.to_string(), port), key.to_vec());
        Ok(())
    }
}

/// Server that records every authentication request it receives.
struct Server {
    addr: SocketAddr,
    host_key: Vec<u8>,
    auth_requests: Arc<Mutex<usize>>,
    task: JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Srv {
    auth_requests: Arc<Mutex<usize>>,
}

impl Handler for Srv {
    type Error = russh::Error;

    async fn auth_none(&mut self, _: &str) -> Result<Auth, Self::Error> {
        *self.auth_requests.lock().unwrap() += 1;
        Ok(Auth::reject())
    }

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        *self.auth_requests.lock().unwrap() += 1;
        if user == USER && password == PASSWORD {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        match TcpStream::connect(format!("{host}:{port}")).await {
            Ok(mut tcp) => {
                reply.accept().await;
                let mut stream = channel.into_stream();
                tokio::spawn(async move {
                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut tcp).await;
                });
            }
            Err(_) => drop(reply),
        }
        Ok(())
    }
}

/// The preference lists of a modern server (everything russh offers by
/// default plus the legacy algorithms), to be narrowed per test.
fn everything() -> Preferred {
    let mut p = Preferred::DEFAULT;
    let mut kexes = p.kex.to_vec();
    kexes.extend([kex::DH_G14_SHA1, kex::DH_GEX_SHA1, kex::DH_G1_SHA1]);
    p.kex = Cow::Owned(kexes);
    let mut ciphers = p.cipher.to_vec();
    ciphers.extend([
        cipher::AES_128_CBC,
        cipher::AES_192_CBC,
        cipher::AES_256_CBC,
        cipher::TRIPLE_DES_CBC,
    ]);
    p.cipher = Cow::Owned(ciphers);
    let mut macs = p.mac.to_vec();
    macs.extend([mac::HMAC_SHA1, mac::HMAC_SHA1_ETM]);
    p.mac = Cow::Owned(macs);
    p
}

async fn start(preferred: Preferred, algorithm: Algorithm) -> Server {
    let key = PrivateKey::random(&mut rand::rng(), algorithm).unwrap();
    let host_key = key.public_key().to_bytes().unwrap();
    let config = Arc::new(Config {
        keys: vec![key],
        preferred,
        ..Default::default()
    });
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let auth_requests = Arc::new(Mutex::new(0));
    let counter = auth_requests.clone();
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let _ = stream.set_nodelay(true);
            let config = config.clone();
            let handler = Srv {
                auth_requests: counter.clone(),
            };
            tokio::spawn(async move {
                let _ = russh::server::run_stream(config, stream, handler).await;
            });
        }
    });
    Server {
        addr,
        host_key,
        auth_requests,
        task,
    }
}

async fn start_ed25519(preferred: Preferred) -> Server {
    start(preferred, Algorithm::Ed25519).await
}

fn hop(server: &Server) -> Hop {
    Hop {
        host: "127.0.0.1".into(),
        port: server.addr.port(),
        username: USER.into(),
        auth: AuthMethod::Password {
            credential_ref: CredentialRef::new(CREDENTIAL),
        },
    }
}

fn hop_label(server: &Server) -> String {
    format!("{USER}@127.0.0.1:{}", server.addr.port())
}

struct Attempt {
    result: Result<ConnectOutcome, SshError>,
    log: ConnectLog,
}

async fn connect(servers: &[&Server]) -> Attempt {
    let keys = Keys::default();
    for s in servers {
        keys.trust("127.0.0.1", s.addr.port(), &s.host_key).unwrap();
    }
    let target = ConnectionTarget {
        hops: servers.iter().map(|s| hop(s)).collect(),
    };
    let log = ConnectLog::new();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        ssh_transport::connect_with_log(&target, &Creds, &NoKeyFiles, Arc::new(keys), &log),
    )
    .await
    .expect("connect finished in time");
    Attempt { result, log }
}

fn connected(attempt: &Attempt) {
    assert!(
        matches!(attempt.result, Ok(ConnectOutcome::Connected(_))),
        "expected a connection, got {:?}",
        attempt.result.as_ref().err()
    );
}

/// Preferred list of a server that offers exactly one algorithm of one kind
/// and everything modern otherwise.
fn only_kex(name: kex::Name) -> Preferred {
    Preferred {
        kex: Cow::Owned(vec![name, kex::EXTENSION_OPENSSH_STRICT_KEX_AS_SERVER]),
        ..Preferred::DEFAULT
    }
}

async fn assert_single_warning(preferred: Preferred, algorithm: Algorithm, expected: &str) {
    let server = start(preferred, algorithm).await;
    let attempt = connect(&[&server]).await;
    connected(&attempt);
    let warnings = attempt.log.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].hop, hop_label(&server));
    assert_eq!(warnings[0].algorithms, vec![expected.to_string()]);
}

#[tokio::test]
async fn kex_group14_sha1_only_connects_with_warning() {
    assert_single_warning(
        only_kex(kex::DH_G14_SHA1),
        Algorithm::Ed25519,
        "diffie-hellman-group14-sha1",
    )
    .await;
}

#[tokio::test]
async fn kex_group_exchange_sha1_only_connects_with_warning() {
    assert_single_warning(
        only_kex(kex::DH_GEX_SHA1),
        Algorithm::Ed25519,
        "diffie-hellman-group-exchange-sha1",
    )
    .await;
}

#[tokio::test]
async fn kex_group1_sha1_only_connects_with_warning() {
    assert_single_warning(
        only_kex(kex::DH_G1_SHA1),
        Algorithm::Ed25519,
        "diffie-hellman-group1-sha1",
    )
    .await;
}

#[tokio::test]
async fn ssh_rsa_host_key_only_connects_with_warning() {
    let preferred = Preferred {
        key: Cow::Owned(vec![Algorithm::Rsa { hash: None }]),
        ..Preferred::DEFAULT
    };
    assert_single_warning(preferred, Algorithm::Rsa { hash: None }, "ssh-rsa").await;
}

#[tokio::test]
async fn aes128_cbc_only_connects_with_warning() {
    let preferred = Preferred {
        cipher: Cow::Owned(vec![cipher::AES_128_CBC]),
        ..Preferred::DEFAULT
    };
    assert_single_warning(preferred, Algorithm::Ed25519, "aes128-cbc").await;
}

#[tokio::test]
async fn triple_des_cbc_only_connects_with_warning() {
    let preferred = Preferred {
        cipher: Cow::Owned(vec![cipher::TRIPLE_DES_CBC]),
        ..Preferred::DEFAULT
    };
    assert_single_warning(preferred, Algorithm::Ed25519, "3des-cbc").await;
}

#[tokio::test]
async fn hmac_sha1_only_connects_with_warning() {
    // A CTR cipher needs a MAC; the server offers only SHA-1 ones.
    let preferred = Preferred {
        cipher: Cow::Owned(vec![cipher::AES_256_CTR]),
        mac: Cow::Owned(vec![mac::HMAC_SHA1]),
        ..Preferred::DEFAULT
    };
    assert_single_warning(preferred, Algorithm::Ed25519, "hmac-sha1").await;
}

#[tokio::test]
async fn hmac_sha1_etm_only_connects_with_warning() {
    let preferred = Preferred {
        cipher: Cow::Owned(vec![cipher::AES_256_CTR]),
        mac: Cow::Owned(vec![mac::HMAC_SHA1_ETM]),
        ..Preferred::DEFAULT
    };
    assert_single_warning(preferred, Algorithm::Ed25519, "hmac-sha1-etm@openssh.com").await;
}

#[tokio::test]
async fn server_offering_modern_and_legacy_negotiates_modern_without_warning() {
    let server = start_ed25519(everything()).await;
    let attempt = connect(&[&server]).await;
    connected(&attempt);
    assert!(attempt.log.warnings().is_empty());
    assert!(!attempt
        .log
        .snapshot()
        .iter()
        .any(|r| matches!(r.step, ConnectStep::OutdatedAlgorithms { .. })));
}

#[tokio::test]
async fn server_without_strict_kex_gets_the_terrapin_warning() {
    let preferred = Preferred {
        kex: Cow::Owned(vec![kex::CURVE25519]),
        ..Preferred::DEFAULT
    };
    let server = start_ed25519(preferred).await;
    let attempt = connect(&[&server]).await;
    connected(&attempt);
    let warnings = attempt.log.warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].algorithms, vec![STRICT_KEX_MISSING.to_string()]);
}

#[tokio::test]
async fn only_the_outdated_bastion_of_a_chain_is_reported() {
    let bastion = start_ed25519(Preferred {
        cipher: Cow::Owned(vec![cipher::AES_128_CBC]),
        ..Preferred::DEFAULT
    })
    .await;
    let target = start_ed25519(Preferred::DEFAULT).await;
    let attempt = connect(&[&bastion, &target]).await;
    connected(&attempt);
    let warnings = attempt.log.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].hop, hop_label(&bastion));
    assert_eq!(warnings[0].algorithms, vec!["aes128-cbc".to_string()]);
}

#[tokio::test]
async fn the_warning_is_a_non_failed_step_of_the_connect_log() {
    let server = start_ed25519(only_kex(kex::DH_G14_SHA1)).await;
    let attempt = connect(&[&server]).await;
    connected(&attempt);
    let records = attempt.log.snapshot();
    let warning = records
        .iter()
        .find(|r| matches!(r.step, ConnectStep::OutdatedAlgorithms { .. }))
        .expect("warning step in the log");
    assert!(matches!(warning.status, StepStatus::Warning { .. }));
    assert!(
        !records
            .iter()
            .any(|r| matches!(r.status, StepStatus::Failed { .. })),
        "{records:?}"
    );
    assert!(matches!(
        records.last().unwrap().step,
        ConnectStep::SessionReady
    ));
}

#[tokio::test]
async fn no_common_cipher_fails_typed_before_any_authentication() {
    // `aes128-gcm` is implemented by russh but not offered by this client:
    // there is no common cipher.
    let server = start_ed25519(Preferred {
        cipher: Cow::Owned(vec![cipher::AES_128_GCM]),
        ..Preferred::DEFAULT
    })
    .await;
    let attempt = connect(&[&server]).await;
    match attempt.result {
        Err(SshError::NoCommonAlgorithm {
            hop,
            kind,
            server_offers,
        }) => {
            assert_eq!(hop.to_string(), hop_label(&server));
            assert_eq!(kind, AlgorithmKind::Cipher);
            assert_eq!(server_offers, vec!["aes128-gcm@openssh.com".to_string()]);
        }
        other => panic!("expected NoCommonAlgorithm, got {:?}", other.err()),
    }
    assert_eq!(
        *server.auth_requests.lock().unwrap(),
        0,
        "no authentication request may reach the server"
    );
}
