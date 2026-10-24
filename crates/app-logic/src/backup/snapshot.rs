//! The typed v1 snapshot of all backup content (Spec 0109, F10, F11, C1, C2).

use base64::Engine;
use persistence_sqlite::{
    AiProviderRow, DatabaseSnapshot, GroupRow, NoteRevisionRow, RuleRow, SecretRow, ServerRow,
    SqliteBackupStore, TagRow,
};
use serde::{Deserialize, Serialize};
use ssh_manager_core::backup::Section;

use super::BackupOpError;
use crate::host_key_store::FileHostKeyStore;

/// Section types of format version 1 (F10); all are required and version 1.
const SECTION_SERVERS: u16 = 1;
const SECTION_GROUPS: u16 = 2;
const SECTION_TAGS: u16 = 3;
const SECTION_RULES: u16 = 4;
const SECTION_NOTES: u16 = 5;
const SECTION_AI_PROVIDERS: u16 = 6;
const SECTION_SECRETS: u16 = 7;
const SECTION_HOST_KEYS: u16 = 8;
const SECTION_VERSION: u16 = 1;

/// The section types this program understands (for `open`).
pub const KNOWN_SECTIONS: &[u16] = &[
    SECTION_SERVERS,
    SECTION_GROUPS,
    SECTION_TAGS,
    SECTION_RULES,
    SECTION_NOTES,
    SECTION_AI_PROVIDERS,
    SECTION_SECRETS,
    SECTION_HOST_KEYS,
];

/// A trusted host key. The key blob is base64 (JSON has no byte strings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostKeyRow {
    pub host: String,
    pub port: u16,
    pub raw_key: String,
}

/// Row counts only; safe to log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SnapshotCounts {
    pub servers: usize,
    pub groups: usize,
    pub tags: usize,
    pub rules: usize,
    pub notes: usize,
    pub ai_providers: usize,
    pub secrets: usize,
    pub host_keys: usize,
}

/// Everything a full backup holds, read at one point in time. All references
/// are UUIDs (F11). Public so that the import and other features needing a
/// consistent snapshot can reuse it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Snapshot {
    pub servers: Vec<ServerRow>,
    pub groups: Vec<GroupRow>,
    pub tags: Vec<TagRow>,
    pub rules: Vec<RuleRow>,
    pub notes: Vec<NoteRevisionRow>,
    pub ai_providers: Vec<AiProviderRow>,
    pub secrets: Vec<SecretRow>,
    pub host_keys: Vec<HostKeyRow>,
}

impl Snapshot {
    /// Reads the database in one read transaction (X2) and the trusted host
    /// keys in the same call. The host keys are read first; both reads are
    /// part of this single call and nothing else happens in between.
    pub async fn read(
        store: &SqliteBackupStore,
        host_keys: &FileHostKeyStore,
    ) -> Result<Snapshot, BackupOpError> {
        let b64 = base64::engine::general_purpose::STANDARD;
        let host_keys: Vec<HostKeyRow> = host_keys
            .entries()
            .map_err(|_| BackupOpError::HostKeys)?
            .into_iter()
            .map(|(host, port, raw)| HostKeyRow {
                host,
                port,
                raw_key: b64.encode(raw),
            })
            .collect();
        let db = store.read_snapshot().await?;
        Ok(Snapshot::from_parts(db, host_keys))
    }

    pub fn from_parts(db: DatabaseSnapshot, host_keys: Vec<HostKeyRow>) -> Snapshot {
        let mut db = db;
        Snapshot {
            servers: std::mem::take(&mut db.servers),
            groups: std::mem::take(&mut db.groups),
            tags: std::mem::take(&mut db.tags),
            rules: std::mem::take(&mut db.rules),
            notes: std::mem::take(&mut db.notes),
            ai_providers: std::mem::take(&mut db.ai_providers),
            secrets: std::mem::take(&mut db.secrets),
            host_keys,
        }
    }

    pub fn counts(&self) -> SnapshotCounts {
        SnapshotCounts {
            servers: self.servers.len(),
            groups: self.groups.len(),
            tags: self.tags.len(),
            rules: self.rules.len(),
            notes: self.notes.len(),
            ai_providers: self.ai_providers.len(),
            secrets: self.secrets.len(),
            host_keys: self.host_keys.len(),
        }
    }

    /// One section per type (F10), payload as JSON of the typed rows.
    pub fn to_sections(&self) -> Result<Vec<Section>, BackupOpError> {
        fn section<T: Serialize>(kind: u16, rows: &[T]) -> Result<Section, BackupOpError> {
            let payload = serde_json::to_vec(rows).map_err(|_| {
                BackupOpError::Format(ssh_manager_core::backup::BackupError::InvalidInput(
                    "snapshot",
                ))
            })?;
            Ok(Section::new(kind, SECTION_VERSION, true, payload))
        }
        Ok(vec![
            section(SECTION_SERVERS, &self.servers)?,
            section(SECTION_GROUPS, &self.groups)?,
            section(SECTION_TAGS, &self.tags)?,
            section(SECTION_RULES, &self.rules)?,
            section(SECTION_NOTES, &self.notes)?,
            section(SECTION_AI_PROVIDERS, &self.ai_providers)?,
            section(SECTION_SECRETS, &self.secrets)?,
            section(SECTION_HOST_KEYS, &self.host_keys)?,
        ])
    }

    /// Strict inverse of [`Self::to_sections`]: every section exactly once,
    /// version 1, unknown fields and wrong types are errors (U2).
    pub fn from_sections(sections: &[Section]) -> Result<Snapshot, BackupOpError> {
        fn parse<T: for<'de> Deserialize<'de>>(
            sections: &[Section],
            kind: u16,
        ) -> Result<Vec<T>, BackupOpError> {
            let bad = || BackupOpError::Format(ssh_manager_core::backup::BackupError::CannotOpen);
            let mut found = sections.iter().filter(|s| s.kind == kind);
            let section = found.next().ok_or_else(bad)?;
            if found.next().is_some() || section.version != SECTION_VERSION {
                return Err(bad());
            }
            serde_json::from_slice(&section.payload).map_err(|_| bad())
        }
        Ok(Snapshot {
            servers: parse(sections, SECTION_SERVERS)?,
            groups: parse(sections, SECTION_GROUPS)?,
            tags: parse(sections, SECTION_TAGS)?,
            rules: parse(sections, SECTION_RULES)?,
            notes: parse(sections, SECTION_NOTES)?,
            ai_providers: parse(sections, SECTION_AI_PROVIDERS)?,
            secrets: parse(sections, SECTION_SECRETS)?,
            host_keys: parse(sections, SECTION_HOST_KEYS)?,
        })
    }
}
