//! Unit-Tests: reine Logik ohne echtes Netzwerk (`cargo test -p ssh-transport
//! --lib`). Echter Verbindungsaufbau/Protokolltests sind Sache der
//! Integrationstests (`tests/integration.rs`, `cargo test -p ssh-transport
//! --test integration`).

use std::collections::HashMap;
use std::sync::Mutex;

use bytes::Bytes;
use russh::ChannelMsg;
use ssh_manager_core::ssh::{HostKeyDecision, HostKeyStore, SshError};

use crate::error::{map_russh_error, TransportError};
use crate::exec::accumulate_exec_output;
use crate::host_key::evaluate_host_key;

// --- accumulate_exec_output ------------------------------------------

#[test]
fn test_accumulate_exec_output_collects_stdout_stderr_and_exit_code() {
    let messages = vec![
        ChannelMsg::Data {
            data: Bytes::from_static(b"hello "),
        },
        ChannelMsg::Data {
            data: Bytes::from_static(b"world\n"),
        },
        ChannelMsg::ExtendedData {
            data: Bytes::from_static(b"warn: x\n"),
            ext: 1,
        },
        ChannelMsg::ExitStatus { exit_status: 0 },
        ChannelMsg::Eof,
        ChannelMsg::Close,
    ];

    let output = accumulate_exec_output(messages);

    assert_eq!(output.stdout, b"hello world\n");
    assert_eq!(output.stderr, b"warn: x\n");
    assert_eq!(output.exit_code, Some(0));
}

#[test]
fn test_accumulate_exec_output_ignores_extended_data_with_unknown_code() {
    // Nur ext == 1 ist per RFC4254 stderr definiert; andere Codes sind nicht
    // spezifiziert und dürfen nicht fälschlich als stderr landen.
    let messages = vec![ChannelMsg::ExtendedData {
        data: Bytes::from_static(b"?"),
        ext: 99,
    }];

    let output = accumulate_exec_output(messages);

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn test_accumulate_exec_output_without_exit_status_yields_none() {
    let messages = vec![ChannelMsg::Data {
        data: Bytes::from_static(b"x"),
    }];

    let output = accumulate_exec_output(messages);

    assert_eq!(output.exit_code, None);
}

// --- Host-Key-Auswertung ------------------------------------------------

#[derive(Default)]
struct MockHostKeyStore {
    known: Mutex<HashMap<(String, u16), Vec<u8>>>,
}

impl MockHostKeyStore {
    fn new() -> Self {
        Self::default()
    }

    fn with_trusted(self, host: &str, port: u16, key: &[u8]) -> Self {
        self.known
            .lock()
            .unwrap()
            .insert((host.to_string(), port), key.to_vec());
        self
    }
}

impl HostKeyStore for MockHostKeyStore {
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

#[test]
fn test_evaluate_host_key_trusted_proceeds() {
    let store = MockHostKeyStore::new().with_trusted("host.invalid", 22, b"the-key");

    let result = evaluate_host_key(&store, "host.invalid", 22, b"the-key".to_vec());

    assert!(matches!(result, Ok(true)));
}

#[test]
fn test_evaluate_host_key_unknown_yields_host_key_error_not_bare_false() {
    let store = MockHostKeyStore::new();

    let result = evaluate_host_key(&store, "host.invalid", 22, b"the-key".to_vec());

    match result {
        Err(TransportError::HostKey {
            raw_key,
            decision: HostKeyDecision::Unknown { .. },
        }) => {
            assert_eq!(raw_key, b"the-key");
        }
        other => panic!("expected TransportError::HostKey(Unknown), got {other:?}"),
    }
}

#[test]
fn test_evaluate_host_key_mismatch_yields_host_key_error() {
    let store = MockHostKeyStore::new().with_trusted("host.invalid", 22, b"old-key");

    let result = evaluate_host_key(&store, "host.invalid", 22, b"new-key".to_vec());

    match result {
        Err(TransportError::HostKey {
            decision: HostKeyDecision::Mismatch { .. },
            ..
        }) => {}
        other => panic!("expected TransportError::HostKey(Mismatch), got {other:?}"),
    }
}

// --- Fehler-Mapping -------------------------------------------------------

#[test]
fn test_map_russh_error_not_authenticated_maps_to_authentication_failed() {
    assert_eq!(
        map_russh_error(russh::Error::NotAuthenticated),
        SshError::AuthenticationFailed
    );
}

#[test]
fn test_map_russh_error_unmapped_variant_yields_channel_error_not_panic() {
    let result = map_russh_error(russh::Error::KexInit);
    assert!(matches!(result, SshError::ChannelError(_)));
}
