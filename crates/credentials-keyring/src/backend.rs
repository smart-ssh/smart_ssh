//! Platform credential-store entry used by [`crate::KeyringCredentialStore`]
//! and the availability probe (Spec 0101, Spec 0114).
//!
//! Desktop: the `keyring` crate's own `Entry` (macOS Keychain, Windows
//! Credential Manager, Secret Service). `keyring` has no Android backend
//! (its store initialisation reports `Invalid("platform")`), so on Android
//! this module offers an `Entry` with the same surface on top of
//! `keyring-core` and the Android Keystore store
//! (`android-native-keyring-store`: values are encrypted with a
//! non-exportable Keystore key and kept in the app-private
//! `SharedPreferences`). The rest of the crate does not branch on the OS.

#[cfg(not(target_os = "android"))]
pub use keyring::Entry;

#[cfg(target_os = "android")]
pub use android::{set_forced_unavailable, Entry};

#[cfg(target_os = "android")]
mod android {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::LazyLock;

    use keyring::Error;
    use keyring_core::Result;

    /// Test hook (Spec 0101, A3): while set, the store counts as not
    /// initialisable. Only read when the store is first initialised, i.e.
    /// set it before the first use.
    static FORCED_UNAVAILABLE: AtomicBool = AtomicBool::new(false);

    /// See [`FORCED_UNAVAILABLE`].
    pub fn set_forced_unavailable(forced: bool) {
        FORCED_UNAVAILABLE.store(forced, Ordering::SeqCst);
    }

    static STORE_RESULT: LazyLock<Result<()>> = LazyLock::new(|| {
        if FORCED_UNAVAILABLE.load(Ordering::SeqCst) {
            return Err(Error::NoDefaultStore);
        }
        let store = android_native_keyring_store::Store::new()?;
        keyring_core::set_default_store(store);
        Ok(())
    });

    pub struct Entry {
        inner: keyring_core::Entry,
    }

    impl Entry {
        pub fn new(service: &str, username: &str) -> Result<Self> {
            if STORE_RESULT.is_err() {
                return Err(Error::NoDefaultStore);
            }
            Ok(Self {
                inner: keyring_core::Entry::new(service, username)?,
            })
        }

        pub fn store_status() -> &'static Result<()> {
            &STORE_RESULT
        }

        pub fn set_password(&self, password: &str) -> Result<()> {
            self.inner.set_password(password)
        }

        pub fn get_password(&self) -> Result<String> {
            self.inner.get_password()
        }

        pub fn delete_credential(&self) -> Result<()> {
            self.inner.delete_credential()
        }
    }
}
