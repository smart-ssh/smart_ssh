//! Writing, reading and re-wrapping of the container (Spec 0109 F1–F8, U1–U4).

use std::fmt;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use secrecy::SecretString;
use zeroize::Zeroizing;

use super::{
    random_bytes, Argon2Deriver, BackupError, BackupSecret, KdfParams, KeyDeriver, Limit,
    RecoveryCode, SlotKind, FORMAT_VERSION, KEY_LEN, MAGIC, MAX_FILE_BYTES, MAX_HEADER_BYTES,
    MAX_KEY_SLOTS, MAX_SECTIONS, MAX_SECTION_CIPHERTEXT_BYTES,
};

const KDF_ARGON2ID: u8 = 1;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const WRAPPED_KEY_LEN: usize = KEY_LEN + TAG_LEN;
const SALT_WRITE_LEN: usize = 32;
const SALT_MIN_LEN: usize = 16;
const SALT_MAX_LEN: usize = 64;
const MAX_WRITER_LEN: usize = 64;
const MAX_EXTENSIONS: usize = 16;
const MAX_EXTENSION_BYTES: usize = 8192;
/// Section plaintext prefix: type u16, version u16, required u8.
const SECTION_PREFIX_LEN: usize = 5;
const MIN_SECTION_CT: usize = SECTION_PREFIX_LEN + TAG_LEN;

/// One section: opaque payload with type, version and the "required" flag (F4).
pub struct Section {
    pub kind: u16,
    pub version: u16,
    pub required: bool,
    pub payload: Zeroizing<Vec<u8>>,
}

impl Section {
    pub fn new(kind: u16, version: u16, required: bool, payload: Vec<u8>) -> Section {
        Section {
            kind,
            version,
            required,
            payload: Zeroizing::new(payload),
        }
    }
}

impl PartialEq for Section {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.version == other.version
            && self.required == other.required
            && *self.payload == *other.payload
    }
}

impl fmt::Debug for Section {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Section")
            .field("kind", &self.kind)
            .field("version", &self.version)
            .field("required", &self.required)
            .field("payload_len", &self.payload.len())
            .finish()
    }
}

/// An optional header extension field (F7), in the authenticated header core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderExtension {
    pub id: u16,
    pub required: bool,
    pub data: Vec<u8>,
}

/// The wrapping material of one key slot (K4): the key derived from code or
/// passphrase plus kind, salt and KDF parameters. Not the code itself.
pub struct SlotMaterial {
    kind: SlotKind,
    salt: Vec<u8>,
    params: KdfParams,
    pub(crate) wrapping_key: Zeroizing<[u8; KEY_LEN]>,
}

impl SlotMaterial {
    /// Derives the material of a secret under a new random salt and the write
    /// parameters (runs Argon2id).
    pub fn derive(secret: &BackupSecret) -> Result<SlotMaterial, BackupError> {
        Self::derive_with(&Argon2Deriver, secret, KdfParams::WRITE, None)
    }

    /// Derives the material for an existing salt (e.g. to open a file whose
    /// slot salt is known).
    pub fn derive_with_salt(
        secret: &BackupSecret,
        salt: &[u8],
        params: KdfParams,
    ) -> Result<SlotMaterial, BackupError> {
        Self::derive_with(&Argon2Deriver, secret, params, Some(salt))
    }

    /// A fresh recovery code and its slot material (K2).
    pub fn generate_code() -> Result<(RecoveryCode, SlotMaterial), BackupError> {
        let code = RecoveryCode::generate()?;
        let secret = BackupSecret::Code(code);
        let material = SlotMaterial::derive(&secret)?;
        match secret {
            BackupSecret::Code(code) => Ok((code, material)),
            BackupSecret::Passphrase(_) => Err(BackupError::InvalidInput("unreachable")),
        }
    }

    /// Slot material for a passphrase (K5: at least 12 characters).
    pub fn from_passphrase(passphrase: SecretString) -> Result<SlotMaterial, BackupError> {
        super::validate_passphrase(&passphrase)?;
        SlotMaterial::derive(&BackupSecret::Passphrase(passphrase))
    }

    pub(crate) fn derive_with(
        deriver: &dyn KeyDeriver,
        secret: &BackupSecret,
        params: KdfParams,
        salt: Option<&[u8]>,
    ) -> Result<SlotMaterial, BackupError> {
        let salt = match salt {
            Some(s) if (SALT_MIN_LEN..=SALT_MAX_LEN).contains(&s.len()) => s.to_vec(),
            Some(_) => return Err(BackupError::InvalidInput("salt length")),
            None => random_bytes::<SALT_WRITE_LEN>()?.to_vec(),
        };
        let input = secret.kdf_input();
        let wrapping_key = deriver.derive(&input, &salt, &params)?;
        Ok(SlotMaterial {
            kind: secret.kind(),
            salt,
            params,
            wrapping_key,
        })
    }

    pub fn kind(&self) -> SlotKind {
        self.kind
    }

    pub fn salt(&self) -> &[u8] {
        &self.salt
    }

    pub fn kdf_params(&self) -> KdfParams {
        self.params
    }

    /// The derived wrapping key, for the device-local backup key store (K4).
    /// Callers keep it in zeroizing types.
    pub fn wrapping_key_bytes(&self) -> &[u8; KEY_LEN] {
        &self.wrapping_key
    }

    /// Rebuilds material stored by the backup key store (K4). Rejects salts
    /// and KDF parameters that [`seal`] would not write.
    pub fn from_stored(
        kind: SlotKind,
        salt: &[u8],
        params: KdfParams,
        wrapping_key: [u8; KEY_LEN],
    ) -> Result<SlotMaterial, BackupError> {
        if !(SALT_MIN_LEN..=SALT_MAX_LEN).contains(&salt.len()) {
            return Err(BackupError::InvalidInput("salt length"));
        }
        if !params.is_acceptable() {
            return Err(BackupError::InvalidKdfParameters);
        }
        Ok(SlotMaterial {
            kind,
            salt: salt.to_vec(),
            params,
            wrapping_key: Zeroizing::new(wrapping_key),
        })
    }
}

impl fmt::Debug for SlotMaterial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SlotMaterial({:?}, <redacted>)", self.kind)
    }
}

/// Input of [`seal`].
pub struct SealInput<'a> {
    pub sections: &'a [Section],
    pub slots: &'a [SlotMaterial],
    /// Creation time (unix seconds); display only.
    pub created_at: i64,
    /// Program version that writes; display only.
    pub writer_version: &'a str,
    pub extensions: &'a [HeaderExtension],
    /// Use [`KdfParams::WRITE`] unless a test or a later release raises it.
    pub kdf: KdfParams,
}

/// Result of [`open`]: only returned after every section is authenticated.
#[derive(Debug)]
pub struct OpenedBackup {
    pub sections: Vec<Section>,
    pub created_at: i64,
    pub writer_version: String,
    /// Unknown optional sections that were skipped (F7).
    pub ignored_optional_sections: usize,
}

// ---------------------------------------------------------------- writing

/// Writes a backup file (F1–F5). A fresh random data key and fresh section
/// nonces are drawn on every call.
pub fn seal(input: &SealInput<'_>) -> Result<Vec<u8>, BackupError> {
    if input.sections.is_empty() {
        return Err(BackupError::InvalidInput(
            "a backup needs at least one section",
        ));
    }
    if input.sections.len() > MAX_SECTIONS {
        return Err(BackupError::LimitExceeded(Limit::SectionCount));
    }
    if input.slots.is_empty() {
        return Err(BackupError::InvalidInput(
            "a backup needs at least one key slot",
        ));
    }
    if input.slots.len() > MAX_KEY_SLOTS {
        return Err(BackupError::LimitExceeded(Limit::KeySlotCount));
    }
    let mut seen = [false; 2];
    for slot in input.slots {
        let i = usize::from(slot.kind.to_byte()) - 1;
        if std::mem::replace(&mut seen[i], true) {
            return Err(BackupError::InvalidInput("at most one slot per kind"));
        }
        if slot.params != input.kdf {
            return Err(BackupError::InvalidInput("slot KDF parameters differ"));
        }
    }
    if !input.kdf.is_acceptable() {
        return Err(BackupError::InvalidKdfParameters);
    }
    if input.writer_version.len() > MAX_WRITER_LEN {
        return Err(BackupError::InvalidInput("writer version too long"));
    }
    if input.extensions.len() > MAX_EXTENSIONS
        || input
            .extensions
            .iter()
            .any(|e| e.data.len() > MAX_EXTENSION_BYTES)
    {
        return Err(BackupError::InvalidInput(
            "too many or too large extensions",
        ));
    }
    let mut ids: Vec<u16> = input.extensions.iter().map(|e| e.id).collect();
    ids.sort_unstable();
    if ids.windows(2).any(|w| w[0] == w[1]) {
        return Err(BackupError::InvalidInput("duplicate extension id"));
    }

    let core = build_core(input);
    let data_key = random_bytes::<KEY_LEN>()?;

    let mut out = core.clone();
    out.push(input.slots.len() as u8);
    for slot in input.slots {
        let nonce = random_bytes::<NONCE_LEN>()?;
        let aad = slot_aad(input.kdf, slot.kind, &slot.salt);
        let wrapped = encrypt(&slot.wrapping_key, &nonce, &data_key[..], &aad)?;
        write_slot(&mut out, slot.kind, &slot.salt, &nonce[..], &wrapped);
    }
    if out.len() > MAX_HEADER_BYTES {
        return Err(BackupError::LimitExceeded(Limit::HeaderSize));
    }

    for (index, section) in input.sections.iter().enumerate() {
        let mut plain: Zeroizing<Vec<u8>> = Zeroizing::new(Vec::with_capacity(
            SECTION_PREFIX_LEN + section.payload.len(),
        ));
        plain.extend_from_slice(&section.kind.to_le_bytes());
        plain.extend_from_slice(&section.version.to_le_bytes());
        plain.push(u8::from(section.required));
        plain.extend_from_slice(&section.payload);
        let ct_len = plain.len() + TAG_LEN;
        if ct_len > MAX_SECTION_CIPHERTEXT_BYTES {
            return Err(BackupError::LimitExceeded(Limit::SectionSize));
        }
        let nonce = random_bytes::<NONCE_LEN>()?;
        let aad = section_aad(&core, index as u32, ct_len as u32);
        let ct = encrypt(&data_key, &nonce, &plain, &aad)?;
        out.extend_from_slice(&nonce[..]);
        out.extend_from_slice(&(ct_len as u32).to_le_bytes());
        out.extend_from_slice(&ct);
        if out.len() > MAX_FILE_BYTES {
            return Err(BackupError::LimitExceeded(Limit::FileSize));
        }
    }
    Ok(out)
}

fn build_core(input: &SealInput<'_>) -> Vec<u8> {
    let mut c = Vec::new();
    c.extend_from_slice(MAGIC);
    c.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    c.push(KDF_ARGON2ID);
    c.extend_from_slice(&input.kdf.m_cost_kib.to_le_bytes());
    c.extend_from_slice(&input.kdf.t_cost.to_le_bytes());
    c.extend_from_slice(&input.kdf.p_cost.to_le_bytes());
    c.extend_from_slice(&(input.sections.len() as u16).to_le_bytes());
    c.extend_from_slice(&input.created_at.to_le_bytes());
    c.push(input.writer_version.len() as u8);
    c.extend_from_slice(input.writer_version.as_bytes());
    c.push(input.extensions.len() as u8);
    for e in input.extensions {
        c.extend_from_slice(&e.id.to_le_bytes());
        c.push(u8::from(e.required));
        c.extend_from_slice(&(e.data.len() as u16).to_le_bytes());
        c.extend_from_slice(&e.data);
    }
    c
}

fn write_slot(out: &mut Vec<u8>, kind: SlotKind, salt: &[u8], nonce: &[u8], wrapped: &[u8]) {
    out.push(kind.to_byte());
    out.push(salt.len() as u8);
    out.extend_from_slice(salt);
    out.extend_from_slice(nonce);
    out.extend_from_slice(wrapped);
}

/// F2: magic, format version, KDF parameters, kind and salt of the slot.
fn slot_aad(kdf: KdfParams, kind: SlotKind, salt: &[u8]) -> Vec<u8> {
    let mut a = Vec::with_capacity(32 + salt.len());
    a.extend_from_slice(MAGIC);
    a.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    a.push(KDF_ARGON2ID);
    a.extend_from_slice(&kdf.m_cost_kib.to_le_bytes());
    a.extend_from_slice(&kdf.t_cost.to_le_bytes());
    a.extend_from_slice(&kdf.p_cost.to_le_bytes());
    a.push(kind.to_byte());
    a.push(salt.len() as u8);
    a.extend_from_slice(salt);
    a
}

/// F5: header core as stored, section index, ciphertext length.
fn section_aad(core: &[u8], index: u32, ct_len: u32) -> Vec<u8> {
    let mut a = Vec::with_capacity(core.len() + 8);
    a.extend_from_slice(core);
    a.extend_from_slice(&index.to_le_bytes());
    a.extend_from_slice(&ct_len.to_le_bytes());
    a
}

fn cipher(key: &[u8; KEY_LEN]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(&(*key).into())
}

fn encrypt(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    msg: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, BackupError> {
    let n: XNonce = (*nonce).into();
    cipher(key)
        .encrypt(&n, Payload { msg, aad })
        .map_err(|_| BackupError::InvalidInput("encryption failed"))
}

fn decrypt(
    key: &[u8; KEY_LEN],
    nonce: &[u8],
    ct: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>, BackupError> {
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| BackupError::CannotOpen)?;
    let n: XNonce = nonce.into();
    cipher(key)
        .decrypt(&n, Payload { msg: ct, aad })
        .map(Zeroizing::new)
        .map_err(|_| BackupError::CannotOpen)
}

// ---------------------------------------------------------------- reading

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    /// While reading the header: `MAX_HEADER_BYTES`; afterwards the length.
    limit: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], BackupError> {
        let end = self.pos.checked_add(n).ok_or(BackupError::CannotOpen)?;
        if end > self.buf.len() {
            return Err(BackupError::CannotOpen);
        }
        if end > self.limit {
            return Err(BackupError::LimitExceeded(Limit::HeaderSize));
        }
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, BackupError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, BackupError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, BackupError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i64(&mut self) -> Result<i64, BackupError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(i64::from_le_bytes(a))
    }
    fn bool(&mut self) -> Result<bool, BackupError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(BackupError::CannotOpen),
        }
    }
}

pub(crate) struct ParsedSlot<'a> {
    pub(crate) kind: SlotKind,
    pub(crate) salt: &'a [u8],
    pub(crate) nonce: &'a [u8],
    pub(crate) wrapped: &'a [u8],
    pub(crate) raw: &'a [u8],
}

pub(crate) struct ParsedSection<'a> {
    pub(crate) nonce: &'a [u8],
    pub(crate) ct_len: u32,
    pub(crate) ct: &'a [u8],
}

pub(crate) struct Parsed<'a> {
    pub(crate) core: &'a [u8],
    pub(crate) kdf: KdfParams,
    pub(crate) created_at: i64,
    pub(crate) writer_version: String,
    pub(crate) extensions: Vec<HeaderExtension>,
    pub(crate) slots: Vec<ParsedSlot<'a>>,
    pub(crate) header_len: usize,
    pub(crate) sections: Vec<ParsedSection<'a>>,
}

/// Strict parse of the structure. Order matters: file size, magic, format
/// version and KDF bounds are all decided **before** any key derivation, and
/// every length is checked against the data and the F12 limits before use.
pub(crate) fn parse(bytes: &[u8]) -> Result<Parsed<'_>, BackupError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(BackupError::LimitExceeded(Limit::FileSize));
    }
    if bytes.len() < MAGIC.len() {
        return Err(BackupError::CannotOpen);
    }
    if &bytes[..MAGIC.len()] != MAGIC {
        return Err(BackupError::NotABackup);
    }
    let mut r = Reader {
        buf: bytes,
        pos: MAGIC.len(),
        limit: MAX_HEADER_BYTES,
    };
    let version = r.u16()?;
    if version > FORMAT_VERSION {
        return Err(BackupError::NewerVersion {
            file: version,
            supported: FORMAT_VERSION,
        });
    }
    if version == 0 || r.u8()? != KDF_ARGON2ID {
        return Err(BackupError::CannotOpen);
    }
    let kdf = KdfParams {
        m_cost_kib: r.u32()?,
        t_cost: r.u32()?,
        p_cost: r.u32()?,
    };
    if !kdf.is_acceptable() {
        return Err(BackupError::InvalidKdfParameters);
    }
    let section_count = usize::from(r.u16()?);
    if section_count > MAX_SECTIONS {
        return Err(BackupError::LimitExceeded(Limit::SectionCount));
    }
    if section_count == 0 {
        return Err(BackupError::CannotOpen);
    }
    let created_at = r.i64()?;
    let writer_len = usize::from(r.u8()?);
    if writer_len > MAX_WRITER_LEN {
        return Err(BackupError::CannotOpen);
    }
    let writer_version = std::str::from_utf8(r.take(writer_len)?)
        .map_err(|_| BackupError::CannotOpen)?
        .to_owned();
    let ext_count = usize::from(r.u8()?);
    if ext_count > MAX_EXTENSIONS {
        return Err(BackupError::CannotOpen);
    }
    let mut extensions: Vec<HeaderExtension> = Vec::with_capacity(ext_count);
    for _ in 0..ext_count {
        let id = r.u16()?;
        let required = r.bool()?;
        let len = usize::from(r.u16()?);
        if len > MAX_EXTENSION_BYTES || extensions.iter().any(|e| e.id == id) {
            return Err(BackupError::CannotOpen);
        }
        extensions.push(HeaderExtension {
            id,
            required,
            data: r.take(len)?.to_vec(),
        });
    }
    let core = &bytes[..r.pos];

    let slot_count = usize::from(r.u8()?);
    if slot_count > MAX_KEY_SLOTS {
        return Err(BackupError::LimitExceeded(Limit::KeySlotCount));
    }
    if slot_count == 0 {
        return Err(BackupError::CannotOpen);
    }
    let mut slots = Vec::with_capacity(slot_count);
    for _ in 0..slot_count {
        let start = r.pos;
        let kind = SlotKind::from_byte(r.u8()?).ok_or(BackupError::CannotOpen)?;
        let salt_len = usize::from(r.u8()?);
        if !(SALT_MIN_LEN..=SALT_MAX_LEN).contains(&salt_len) {
            return Err(BackupError::CannotOpen);
        }
        let salt = r.take(salt_len)?;
        let nonce = r.take(NONCE_LEN)?;
        let wrapped = r.take(WRAPPED_KEY_LEN)?;
        slots.push(ParsedSlot {
            kind,
            salt,
            nonce,
            wrapped,
            raw: &bytes[start..r.pos],
        });
    }
    let header_len = r.pos;

    r.limit = bytes.len();
    let mut sections = Vec::with_capacity(section_count);
    for _ in 0..section_count {
        let nonce = r.take(NONCE_LEN)?;
        let ct_len = r.u32()?;
        let n = ct_len as usize;
        if n > MAX_SECTION_CIPHERTEXT_BYTES {
            return Err(BackupError::LimitExceeded(Limit::SectionSize));
        }
        if n < MIN_SECTION_CT {
            return Err(BackupError::CannotOpen);
        }
        // `take` compares with the real remaining data before anything is
        // allocated for the section.
        let ct = r.take(n)?;
        sections.push(ParsedSection { nonce, ct_len, ct });
    }
    if r.pos != bytes.len() {
        return Err(BackupError::CannotOpen);
    }
    Ok(Parsed {
        core,
        kdf,
        created_at,
        writer_version,
        extensions,
        slots,
        header_len,
        sections,
    })
}

/// Opens with `secret`: tries every slot of the secret's kind; a slot counts
/// only if **all** sections then authenticate. Returns the data key and the
/// decrypted sections.
fn open_inner(
    deriver: &dyn KeyDeriver,
    parsed: &Parsed<'_>,
    secret: &BackupSecret,
) -> Result<(Zeroizing<[u8; KEY_LEN]>, Vec<Section>), BackupError> {
    let input = secret.kdf_input();
    for slot in parsed.slots.iter().filter(|s| s.kind == secret.kind()) {
        let wrapping = deriver.derive(&input, slot.salt, &parsed.kdf)?;
        if let Some(found) = try_slot(parsed, slot, &wrapping) {
            return Ok(found);
        }
    }
    Err(BackupError::CannotOpen)
}

/// Unwraps the data key of `slot` under `wrapping` and authenticates every
/// section with it.
fn try_slot(
    parsed: &Parsed<'_>,
    slot: &ParsedSlot<'_>,
    wrapping: &[u8; KEY_LEN],
) -> Option<(Zeroizing<[u8; KEY_LEN]>, Vec<Section>)> {
    let aad = slot_aad(parsed.kdf, slot.kind, slot.salt);
    let key_bytes = decrypt(wrapping, slot.nonce, slot.wrapped, &aad).ok()?;
    let arr = <[u8; KEY_LEN]>::try_from(&key_bytes[..]).ok()?;
    let data_key = Zeroizing::new(arr);
    let sections = decrypt_sections(parsed, &data_key).ok()?;
    Some((data_key, sections))
}

fn decrypt_sections(parsed: &Parsed<'_>, key: &[u8; KEY_LEN]) -> Result<Vec<Section>, BackupError> {
    let mut out = Vec::with_capacity(parsed.sections.len());
    for (i, s) in parsed.sections.iter().enumerate() {
        let aad = section_aad(parsed.core, i as u32, s.ct_len);
        let plain = decrypt(key, s.nonce, s.ct, &aad)?;
        if plain.len() < SECTION_PREFIX_LEN {
            return Err(BackupError::CannotOpen);
        }
        let required = match plain[4] {
            0 => false,
            1 => true,
            _ => return Err(BackupError::CannotOpen),
        };
        out.push(Section {
            kind: u16::from_le_bytes([plain[0], plain[1]]),
            version: u16::from_le_bytes([plain[2], plain[3]]),
            required,
            payload: Zeroizing::new(plain[SECTION_PREFIX_LEN..].to_vec()),
        });
    }
    Ok(out)
}

/// Opens a backup file. `known_sections` lists the section types the caller
/// understands: an unknown *required* section (or header field) rejects the
/// file, an unknown optional one is dropped (F7). Nothing is returned unless
/// the header core and every section authenticate (U1); a wrong secret and a
/// damaged file give the same [`BackupError::CannotOpen`] (I1).
pub fn open(
    bytes: &[u8],
    secret: &BackupSecret,
    known_sections: &[u16],
) -> Result<OpenedBackup, BackupError> {
    open_with(&Argon2Deriver, bytes, secret, known_sections)
}

pub(crate) fn open_with(
    deriver: &dyn KeyDeriver,
    bytes: &[u8],
    secret: &BackupSecret,
    known_sections: &[u16],
) -> Result<OpenedBackup, BackupError> {
    let parsed = parse(bytes)?;
    let (_key, all) = open_inner(deriver, &parsed, secret)?;
    // Header extensions are authenticated now; v1 knows none.
    if parsed.extensions.iter().any(|e| e.required) {
        return Err(BackupError::UnsupportedRequired);
    }
    let mut sections = Vec::with_capacity(all.len());
    let mut ignored = 0;
    for s in all {
        if known_sections.contains(&s.kind) {
            sections.push(s);
        } else if s.required {
            return Err(BackupError::UnsupportedRequired);
        } else {
            ignored += 1;
        }
    }
    Ok(OpenedBackup {
        sections,
        created_at: parsed.created_at,
        writer_version: parsed.writer_version,
        ignored_optional_sections: ignored,
    })
}

/// Opens a file with already derived slot material (no KDF run): the check
/// an export does on the file it just wrote (X4). Every section must
/// authenticate; the same rules as [`open`] apply to unknown sections.
pub fn open_with_slot(
    bytes: &[u8],
    slot: &SlotMaterial,
    known_sections: &[u16],
) -> Result<OpenedBackup, BackupError> {
    let parsed = parse(bytes)?;
    let mut found = None;
    for s in parsed.slots.iter().filter(|s| s.kind == slot.kind) {
        if let Some((_key, sections)) = try_slot(&parsed, s, &slot.wrapping_key) {
            found = Some(sections);
            break;
        }
    }
    let all = found.ok_or(BackupError::CannotOpen)?;
    if parsed.extensions.iter().any(|e| e.required) {
        return Err(BackupError::UnsupportedRequired);
    }
    let mut sections = Vec::with_capacity(all.len());
    let mut ignored = 0;
    for s in all {
        if known_sections.contains(&s.kind) {
            sections.push(s);
        } else if s.required {
            return Err(BackupError::UnsupportedRequired);
        } else {
            ignored += 1;
        }
    }
    Ok(OpenedBackup {
        sections,
        created_at: parsed.created_at,
        writer_version: parsed.writer_version,
        ignored_optional_sections: ignored,
    })
}

/// K7: opens with `old_secret` (fully verifying the content), then adds
/// `new_slot` or replaces the slot of the same kind. Header core and all
/// section bytes stay byte-identical; only the slots are rewritten. The caller
/// writes the result with [`crate::atomic_write::write_atomically`].
pub fn rewrap(
    bytes: &[u8],
    old_secret: &BackupSecret,
    new_slot: &SlotMaterial,
) -> Result<Vec<u8>, BackupError> {
    rewrap_with(&Argon2Deriver, bytes, old_secret, new_slot)
}

pub(crate) fn rewrap_with(
    deriver: &dyn KeyDeriver,
    bytes: &[u8],
    old_secret: &BackupSecret,
    new_slot: &SlotMaterial,
) -> Result<Vec<u8>, BackupError> {
    let parsed = parse(bytes)?;
    if new_slot.params != parsed.kdf {
        return Err(BackupError::InvalidInput("slot KDF parameters differ"));
    }
    let (data_key, _sections) = open_inner(deriver, &parsed, old_secret)?;

    let kept: Vec<&ParsedSlot<'_>> = parsed
        .slots
        .iter()
        .filter(|s| s.kind != new_slot.kind)
        .collect();
    if kept.len() + 1 > MAX_KEY_SLOTS {
        return Err(BackupError::LimitExceeded(Limit::KeySlotCount));
    }
    let mut out = parsed.core.to_vec();
    out.push((kept.len() + 1) as u8);
    for s in kept {
        out.extend_from_slice(s.raw);
    }
    let nonce = random_bytes::<NONCE_LEN>()?;
    let aad = slot_aad(parsed.kdf, new_slot.kind, &new_slot.salt);
    let wrapped = encrypt(&new_slot.wrapping_key, &nonce, &data_key[..], &aad)?;
    write_slot(
        &mut out,
        new_slot.kind,
        &new_slot.salt,
        &nonce[..],
        &wrapped,
    );
    if out.len() > MAX_HEADER_BYTES {
        return Err(BackupError::LimitExceeded(Limit::HeaderSize));
    }
    out.extend_from_slice(&bytes[parsed.header_len..]);
    Ok(out)
}
