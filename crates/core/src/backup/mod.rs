//! Encrypted backup file format and crypto (Spec 0109 §3, §4, §8; ADR 0134).
//!
//! Pure logic over byte buffers: no database, no Tauri, no file I/O (the
//! atomic write lives in [`crate::atomic_write`]). Sections are opaque
//! payloads with a type, a version and a "required" flag; the typed v1
//! payloads (F10) belong to the export/import issues.
//!
//! Layout (all integers little-endian):
//!
//! ```text
//! header core : magic[8] version:u16 kdf_id:u8 m:u32 t:u32 p:u32
//!               section_count:u16 created_at:i64 writer_len:u8 writer
//!               ext_count:u8 { id:u16 required:u8 len:u16 data }*
//! key slots   : count:u8 { kind:u8 salt_len:u8 salt nonce[24] wrapped[48] }*
//! sections    : { nonce[24] ct_len:u32 ct }*
//! ```
//!
//! The header core is the AAD of every section (together with the section
//! index and ciphertext length); the key slots are not, so re-wrapping (K7)
//! can change them without touching the content. A file always has at least
//! one section, because the sections are what authenticates the header core.

mod code;
mod format;
#[cfg(test)]
mod tests;

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroizing;

pub use code::RecoveryCode;
pub use format::{
    open, open_with_slot, rewrap, seal, HeaderExtension, OpenedBackup, SealInput, Section,
    SlotMaterial,
};
#[cfg(test)]
pub(crate) use format::{open_with, rewrap_with};

/// Magic at the start of every backup file (F1.1, F9).
pub const MAGIC: &[u8; 8] = b"SSHBKUP\x00";
/// Highest format version this program reads and writes (F8).
pub const FORMAT_VERSION: u16 = 1;

/// F12 limits.
pub const MAX_FILE_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_SECTION_CIPHERTEXT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SECTIONS: usize = 64;
pub const MAX_KEY_SLOTS: usize = 8;

/// F3: parameters written, and the bounds accepted when reading.
pub const WRITE_M_COST_KIB: u32 = 64 * 1024;
pub const WRITE_T_COST: u32 = 3;
pub const WRITE_P_COST: u32 = 1;
pub const MAX_M_COST_KIB: u32 = 1024 * 1024; // 1 GiB
pub const MAX_T_COST: u32 = 10;
pub const MAX_P_COST: u32 = 8;

pub(crate) const KEY_LEN: usize = 32;

/// Which limit of F12 was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    FileSize,
    HeaderSize,
    SectionSize,
    SectionCount,
    KeySlotCount,
}

/// Every failure is a fixed text; none carries secret or file content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackupError {
    /// The data does not start with the backup magic.
    #[error("this is not a Smart SSH backup file")]
    NotABackup,
    /// F8: checked before any key derivation.
    #[error(
        "backup format version {file}, this program knows versions up to {supported}; install \
         the latest version of Smart SSH"
    )]
    NewerVersion { file: u16, supported: u16 },
    /// F7: an unknown *required* section or header field.
    #[error(
        "the backup contains a required part this program does not know; install the latest \
         version of Smart SSH"
    )]
    UnsupportedRequired,
    /// F3: KDF parameters below the write values or above the maxima.
    #[error("the backup uses unacceptable key derivation parameters")]
    InvalidKdfParameters,
    #[error("the backup exceeds a size limit ({0:?})")]
    LimitExceeded(Limit),
    /// K3: the recovery code has a typo (checksum or alphabet mismatch).
    #[error("the code contains a typo")]
    CodeTypo,
    /// K5.
    #[error(
        "the passphrase must have at least {} characters",
        crate::crypto::MIN_PASSWORD_CHARS
    )]
    PassphraseTooShort,
    /// I1: wrong code/passphrase and damaged or truncated file are the same
    /// error, on purpose (no oracle).
    #[error("wrong code or passphrase, or the backup file is damaged")]
    CannotOpen,
    #[error("key derivation failed")]
    KeyDerivationFailed,
    #[error("no secure random source available")]
    RandomUnavailable,
    /// The caller passed something `seal`/`rewrap` cannot write.
    #[error("invalid backup input: {0}")]
    InvalidInput(&'static str),
}

/// KDF parameters as stored in the header core (F1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl KdfParams {
    /// The parameters new files are written with (F3).
    pub const WRITE: KdfParams = KdfParams {
        m_cost_kib: WRITE_M_COST_KIB,
        t_cost: WRITE_T_COST,
        p_cost: WRITE_P_COST,
    };

    /// F3: below the write values or above the maxima is rejected.
    pub fn is_acceptable(&self) -> bool {
        (WRITE_M_COST_KIB..=MAX_M_COST_KIB).contains(&self.m_cost_kib)
            && (WRITE_T_COST..=MAX_T_COST).contains(&self.t_cost)
            && (WRITE_P_COST..=MAX_P_COST).contains(&self.p_cost)
    }
}

/// What a key slot is wrapped under (F2). K5: at most one of each is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Code,
    Passphrase,
}

impl SlotKind {
    pub(crate) fn to_byte(self) -> u8 {
        match self {
            SlotKind::Code => 1,
            SlotKind::Passphrase => 2,
        }
    }
    pub(crate) fn from_byte(b: u8) -> Option<SlotKind> {
        match b {
            1 => Some(SlotKind::Code),
            2 => Some(SlotKind::Passphrase),
            _ => None,
        }
    }
}

/// The secret a caller opens a file with.
pub enum BackupSecret {
    Code(RecoveryCode),
    Passphrase(SecretString),
}

impl BackupSecret {
    pub(crate) fn kind(&self) -> SlotKind {
        match self {
            BackupSecret::Code(_) => SlotKind::Code,
            BackupSecret::Passphrase(_) => SlotKind::Passphrase,
        }
    }
    /// Bytes fed to the KDF.
    pub(crate) fn kdf_input(&self) -> Zeroizing<Vec<u8>> {
        match self {
            BackupSecret::Code(c) => Zeroizing::new(c.canonical().as_bytes().to_vec()),
            BackupSecret::Passphrase(p) => Zeroizing::new(p.expose_secret().as_bytes().to_vec()),
        }
    }
}

impl fmt::Debug for BackupSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BackupSecret({:?}, <redacted>)", self.kind())
    }
}

/// K5: a passphrase has at least 12 characters.
pub fn validate_passphrase(passphrase: &SecretString) -> Result<(), BackupError> {
    crate::crypto::check_password_length(passphrase).map_err(|_| BackupError::PassphraseTooShort)
}

/// Key derivation, behind a trait so tests can count calls and run without
/// the cost of Argon2.
pub(crate) trait KeyDeriver {
    fn derive(
        &self,
        secret: &[u8],
        salt: &[u8],
        params: &KdfParams,
    ) -> Result<Zeroizing<[u8; KEY_LEN]>, BackupError>;
}

pub(crate) struct Argon2Deriver;

impl KeyDeriver for Argon2Deriver {
    fn derive(
        &self,
        secret: &[u8],
        salt: &[u8],
        p: &KdfParams,
    ) -> Result<Zeroizing<[u8; KEY_LEN]>, BackupError> {
        // Re-check here as the last line of defence: nothing reaches Argon2
        // with parameters outside F3, whatever the caller did.
        if !p.is_acceptable() {
            return Err(BackupError::InvalidKdfParameters);
        }
        let params = Params::new(p.m_cost_kib, p.t_cost, p.p_cost, Some(KEY_LEN))
            .map_err(|_| BackupError::InvalidKdfParameters)?;
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut out = Zeroizing::new([0u8; KEY_LEN]);
        argon
            .hash_password_into(secret, salt, out.as_mut())
            .map_err(|_| BackupError::KeyDerivationFailed)?;
        Ok(out)
    }
}

pub(crate) fn random_bytes<const N: usize>() -> Result<Zeroizing<[u8; N]>, BackupError> {
    use chacha20poly1305::aead::rand_core::RngCore;
    use chacha20poly1305::aead::OsRng;
    let mut out = Zeroizing::new([0u8; N]);
    OsRng
        .try_fill_bytes(out.as_mut())
        .map_err(|_| BackupError::RandomUnavailable)?;
    Ok(out)
}
