//! Issue #1314: the one place that builds the process's OS-keychain store.
//!
//! Every keychain access (root key, credentials, provider keys, edition
//! secrets) goes through a store from here, so it uses the service of the
//! active data directory: `Smart SSH` without an override, `Smart SSH (<id>)`
//! with one (Spec 0101). `credentials_keyring::KeyringCredentialStore` has no
//! constructor without a service, and a guard test in that crate fails when
//! any other production file builds a store itself.

use credentials_keyring::KeyringCredentialStore;

/// Keychain service name of this process (shown in Settings → Diagnostics).
pub fn process_keychain_service() -> String {
    credentials_keyring::service_name_for(persistence_sqlite::data_dir_override().as_deref())
}

/// Keychain store of this process, bound to [`process_keychain_service`].
pub fn process_keyring_store() -> KeyringCredentialStore {
    KeyringCredentialStore::with_service(process_keychain_service())
}
