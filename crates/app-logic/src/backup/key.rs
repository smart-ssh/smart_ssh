//! The device-local backup key (Spec 0109, K1, K2, K4–K6).
//!
//! It is stored as one reserved entry of the encrypted database
//! ([`persistence_sqlite::BACKUP_KEY_REF`]). It is not derived from, and does
//! not derive, the database or root key (K1): code and passphrase are
//! independent random / user-chosen secrets run through Argon2id. Per slot the
//! entry holds kind, salt, KDF parameters and the derived wrapping key; the
//! code itself is not stored.

use base64::Engine;
use persistence_sqlite::SqliteBackupStore;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use ssh_manager_core::backup::{KdfParams, RecoveryCode, SlotKind, SlotMaterial};
use zeroize::Zeroizing;

use super::BackupOpError;

const STORE_VERSION: u8 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredKey {
    version: u8,
    slots: Vec<StoredSlot>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSlot {
    kind: String,
    salt: String,
    m_cost_kib: u32,
    t_cost: u32,
    p_cost: u32,
    wrapping_key: String,
}

/// What the settings UI may know (no key material).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BackupKeyStatus {
    pub exists: bool,
    pub has_code: bool,
    pub has_passphrase: bool,
}

/// The loaded backup key: at most one code slot and one passphrase slot (K5).
pub struct BackupKey {
    pub(crate) code: Option<SlotMaterial>,
    pub(crate) passphrase: Option<SlotMaterial>,
}

fn kind_name(kind: SlotKind) -> &'static str {
    match kind {
        SlotKind::Code => "code",
        SlotKind::Passphrase => "passphrase",
    }
}

impl BackupKey {
    /// The key of this device, if any.
    pub async fn load(store: &SqliteBackupStore) -> Result<Option<BackupKey>, BackupOpError> {
        match store.load_backup_key().await? {
            None => Ok(None),
            Some(blob) => Self::parse(&blob).map(Some),
        }
    }

    pub async fn status(store: &SqliteBackupStore) -> Result<BackupKeyStatus, BackupOpError> {
        Ok(match Self::load(store).await? {
            None => BackupKeyStatus {
                exists: false,
                has_code: false,
                has_passphrase: false,
            },
            Some(k) => BackupKeyStatus {
                exists: true,
                has_code: k.code.is_some(),
                has_passphrase: k.passphrase.is_some(),
            },
        })
    }

    /// The slots an export wraps its data key under (X5). Fails with
    /// [`BackupOpError::KeyRequired`] when the device has no backup key.
    pub async fn active_slots(
        store: &SqliteBackupStore,
    ) -> Result<Vec<SlotMaterial>, BackupOpError> {
        let key = Self::load(store).await?.ok_or(BackupOpError::KeyRequired)?;
        Ok(key.into_slots())
    }

    fn into_slots(self) -> Vec<SlotMaterial> {
        self.code.into_iter().chain(self.passphrase).collect()
    }

    /// K2: first export. Creates the key with a recovery code and returns the
    /// code, which is shown exactly once.
    pub async fn create_with_code(
        store: &SqliteBackupStore,
    ) -> Result<RecoveryCode, BackupOpError> {
        let _guard = store.lock_key().await;
        if store.load_backup_key().await?.is_some() {
            return Err(BackupOpError::KeyAlreadyExists);
        }
        let (code, material) = derive_blocking(SlotMaterial::generate_code).await?;
        BackupKey {
            code: Some(material),
            passphrase: None,
        }
        .save(store)
        .await?;
        tracing::info!("backup key created (recovery code slot)");
        Ok(code)
    }

    /// K2 (alternative): first export with a passphrase instead of a code.
    /// K5: set or replace the passphrase on an existing key.
    pub async fn set_passphrase(
        store: &SqliteBackupStore,
        passphrase: SecretString,
    ) -> Result<(), BackupOpError> {
        let _guard = store.lock_key().await;
        // Reject too-short passphrases before any key derivation.
        ssh_manager_core::backup::validate_passphrase(&passphrase)?;
        let material = derive_blocking(move || SlotMaterial::from_passphrase(passphrase)).await?;
        let mut key = Self::load(store).await?.unwrap_or(BackupKey {
            code: None,
            passphrase: None,
        });
        key.passphrase = Some(material);
        key.save(store).await?;
        tracing::info!("backup key passphrase slot set");
        Ok(())
    }

    /// K6: replaces the code slot with a new code and returns it. Needs an
    /// existing key (the first code comes from [`Self::create_with_code`]).
    pub async fn new_code(store: &SqliteBackupStore) -> Result<RecoveryCode, BackupOpError> {
        let _guard = store.lock_key().await;
        let mut key = Self::load(store).await?.ok_or(BackupOpError::KeyRequired)?;
        let (code, material) = derive_blocking(SlotMaterial::generate_code).await?;
        key.code = Some(material);
        key.save(store).await?;
        tracing::info!("backup key recovery code replaced");
        Ok(code)
    }

    async fn save(&self, store: &SqliteBackupStore) -> Result<(), BackupOpError> {
        store.store_backup_key(&self.serialize()?).await?;
        Ok(())
    }

    fn serialize(&self) -> Result<SecretString, BackupOpError> {
        let b64 = base64::engine::general_purpose::STANDARD;
        let slots = self
            .code
            .iter()
            .chain(self.passphrase.iter())
            .map(|m| StoredSlot {
                kind: kind_name(m.kind()).into(),
                salt: b64.encode(m.salt()),
                m_cost_kib: m.kdf_params().m_cost_kib,
                t_cost: m.kdf_params().t_cost,
                p_cost: m.kdf_params().p_cost,
                wrapping_key: b64.encode(m.wrapping_key_bytes()),
            })
            .collect();
        let json = serde_json::to_string(&StoredKey {
            version: STORE_VERSION,
            slots,
        })
        .map_err(|_| BackupOpError::KeyUnreadable)?;
        Ok(SecretString::from(json))
    }

    fn parse(blob: &SecretString) -> Result<BackupKey, BackupOpError> {
        let b64 = base64::engine::general_purpose::STANDARD;
        let stored: StoredKey =
            serde_json::from_str(blob.expose_secret()).map_err(|_| BackupOpError::KeyUnreadable)?;
        if stored.version != STORE_VERSION {
            return Err(BackupOpError::KeyUnreadable);
        }
        let mut key = BackupKey {
            code: None,
            passphrase: None,
        };
        for s in &stored.slots {
            let kind = match s.kind.as_str() {
                "code" => SlotKind::Code,
                "passphrase" => SlotKind::Passphrase,
                _ => return Err(BackupOpError::KeyUnreadable),
            };
            let salt = b64
                .decode(&s.salt)
                .map_err(|_| BackupOpError::KeyUnreadable)?;
            let raw = Zeroizing::new(
                b64.decode(&s.wrapping_key)
                    .map_err(|_| BackupOpError::KeyUnreadable)?,
            );
            let wrapping: [u8; 32] = raw[..]
                .try_into()
                .map_err(|_| BackupOpError::KeyUnreadable)?;
            let params = KdfParams {
                m_cost_kib: s.m_cost_kib,
                t_cost: s.t_cost,
                p_cost: s.p_cost,
            };
            let material = SlotMaterial::from_stored(kind, &salt, params, wrapping)
                .map_err(|_| BackupOpError::KeyUnreadable)?;
            let slot = match kind {
                SlotKind::Code => &mut key.code,
                SlotKind::Passphrase => &mut key.passphrase,
            };
            // K5: at most one slot per kind.
            if slot.replace(material).is_some() {
                return Err(BackupOpError::KeyUnreadable);
            }
        }
        if key.code.is_none() && key.passphrase.is_none() {
            return Err(BackupOpError::KeyUnreadable);
        }
        Ok(key)
    }
}

/// Argon2id takes a noticeable time and 64 MiB: off the async workers.
async fn derive_blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ssh_manager_core::backup::BackupError> + Send + 'static,
) -> Result<T, BackupOpError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| BackupOpError::KeyUnreadable)?
        .map_err(BackupOpError::from)
}
