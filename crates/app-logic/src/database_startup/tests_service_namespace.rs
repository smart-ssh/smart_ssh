//! Issue #1314: "start over" in keychain mode writes the new root key only
//! under the keychain service of the instance's data directory.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use credentials_keyring::service_name_for;
use secrecy::{ExposeSecret, SecretString};
use ssh_manager_core::crypto::CHAT_CONTENT_ENCRYPTION_KEY_REF;
use ssh_manager_core::profiles::{
    CredentialError, CredentialRef, CredentialResult, CredentialStore,
};

use super::*;

type Entries = Arc<Mutex<HashMap<(String, String), String>>>;

/// Fake keyring backend: one shared map keyed by (service, account), and every
/// access recorded with its service name.
struct FakeKeyring {
    service: String,
    entries: Entries,
    log: Arc<Mutex<Vec<(String, &'static str)>>>,
}

impl FakeKeyring {
    fn key(&self, r: &CredentialRef) -> (String, String) {
        (self.service.clone(), r.as_str().to_string())
    }
    fn note(&self, op: &'static str) {
        self.log.lock().unwrap().push((self.service.clone(), op));
    }
}

impl CredentialStore for FakeKeyring {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        self.note("get");
        match self.entries.lock().unwrap().get(&self.key(r)) {
            Some(v) => Ok(SecretString::from(v.clone())),
            None => Err(CredentialError::NotFound(r.clone())),
        }
    }
    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()> {
        self.note("set");
        self.entries
            .lock()
            .unwrap()
            .insert(self.key(r), value.expose_secret().to_string());
        Ok(())
    }
    fn delete(&self, r: &CredentialRef) -> CredentialResult<()> {
        self.note("delete");
        self.entries.lock().unwrap().remove(&self.key(r));
        Ok(())
    }
}

#[test]
fn test_start_over_with_override_writes_k_only_under_the_override_service() {
    let entries: Entries = Arc::default();
    let log = Arc::new(Mutex::new(Vec::new()));
    let mk = |service: String| FakeKeyring {
        service,
        entries: entries.clone(),
        log: log.clone(),
    };
    let account = CredentialRef::new(CHAT_CONTENT_ENCRYPTION_KEY_REF);
    let default_store = mk(service_name_for(None));
    default_store
        .set(
            &account,
            SecretString::from("normal-instance-key".to_string()),
        )
        .unwrap();
    log.lock().unwrap().clear();

    let override_service = service_name_for(Some(Path::new("/a/b")));
    assert_ne!(override_service, service_name_for(None));
    let override_store = mk(override_service.clone());
    store_new_key_in_keychain(&override_store).expect("new key");

    let map = entries.lock().unwrap();
    assert_eq!(
        map[&(service_name_for(None), account.as_str().to_string())],
        "normal-instance-key"
    );
    assert!(map.contains_key(&(override_service.clone(), account.as_str().to_string())));
    assert_eq!(map.len(), 2);
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .all(|(s, _)| *s == override_service));
}
