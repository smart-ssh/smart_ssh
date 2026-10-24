//! Public building blocks for integrations (issue #670): a consistent read of
//! the whole data set and a validated change set applied in one transaction.
//!
//! Both work on the typed v1 structures of the backup ([`Snapshot`]). The
//! change set goes through exactly the checks an imported file goes through
//! (Spec 0109, U5 references and cycles, U6 filter rules, F12 limits): the
//! result of applying the changes is validated as a whole, so a change set
//! cannot create anything a file could not. Nothing here writes a table
//! directly.

use std::collections::BTreeMap;

use base64::Engine;
use persistence_sqlite::{
    AiProviderRow, GroupRow, NoteRevisionRow, RuleRow, SecretRow, ServerRow, SqliteBackupStore,
    TagRow,
};
use uuid::Uuid;

use super::validate::validate;
use super::{complete_pending_host_keys, BackupOpError, HostKeyRow, Snapshot, SnapshotCounts};
use crate::host_key_store::FileHostKeyStore;

/// One per-entry operation: insert-or-update the whole entry, or delete it
/// by its key.
#[derive(Debug, Clone, PartialEq)]
pub enum Change<Entry, Key> {
    /// Inserts the entry, or replaces the stored one with the same key.
    Upsert(Entry),
    /// Deletes the entry; deleting one that does not exist is a no-op.
    Delete(Key),
}

/// A set of changes per entity type. Within one type the operations apply in
/// list order. Keys: ids for servers, groups, rules, notes and AI providers;
/// server id and tag for tags; the owner reference for
/// secrets; host and port for host keys.
///
/// Deleting a server also removes its tags, notes with revisions, credentials
/// (`server:<id>:…`) and the rules bound to it, as the UI delete does; chat
/// history goes with it through the schema. Any other entry that still refers
/// to something that no longer exists makes the whole set fail.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ChangeSet {
    pub groups: Vec<Change<GroupRow, Uuid>>,
    pub servers: Vec<Change<ServerRow, Uuid>>,
    pub tags: Vec<Change<TagRow, (Uuid, String)>>,
    pub rules: Vec<Change<RuleRow, Uuid>>,
    pub notes: Vec<Change<NoteRevisionRow, Uuid>>,
    pub ai_providers: Vec<Change<AiProviderRow, Uuid>>,
    pub secrets: Vec<Change<SecretRow, String>>,
    pub host_keys: Vec<Change<HostKeyRow, (String, u16)>>,
}

/// What [`apply_changes`] reports: the counts of the content afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplySummary {
    pub counts: SnapshotCounts,
    /// The host-key file could not be written right after the commit; the
    /// next start completes it (I5).
    pub host_keys_pending: bool,
}

/// Reads all user data as one consistent snapshot: every table in a single
/// read transaction, plus the trusted host keys. The content is exactly what
/// an export holds (Spec 0109, C1, C2).
pub async fn read_snapshot(
    store: &SqliteBackupStore,
    host_keys: &FileHostKeyStore,
) -> Result<Snapshot, BackupOpError> {
    Snapshot::read(store, host_keys).await
}

/// Applies `changes` in **one** database transaction. The content after the
/// changes must pass the validation of the import; otherwise, and on any
/// failure while writing, nothing changes. Host-key changes use the pending
/// marker of the import. After the commit the data-change notification
/// (views reload) fires for the affected kinds.
pub async fn apply_changes(
    store: &SqliteBackupStore,
    host_keys: &FileHostKeyStore,
    changes: &ChangeSet,
) -> Result<ApplySummary, BackupOpError> {
    apply_changes_with(store, host_keys, changes, &mut |_| false).await
}

/// As [`apply_changes`]; `abort_after` is asked after the steps "upsert" and
/// "delete" and fails the transaction when it returns `true` (tests).
pub async fn apply_changes_with(
    store: &SqliteBackupStore,
    host_keys: &FileHostKeyStore,
    changes: &ChangeSet,
    abort_after: &mut (dyn FnMut(&'static str) -> bool + Send),
) -> Result<ApplySummary, BackupOpError> {
    let result = run(store, host_keys, changes, abort_after).await;
    if let Err(err) = &result {
        tracing::warn!(result = ?err, "change set rejected; data unchanged");
    }
    result
}

async fn run(
    store: &SqliteBackupStore,
    host_keys: &FileHostKeyStore,
    changes: &ChangeSet,
    abort_after: &mut (dyn FnMut(&'static str) -> bool + Send),
) -> Result<ApplySummary, BackupOpError> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let current_keys: Vec<HostKeyRow> = host_keys
        .entries()
        .map_err(|_| BackupOpError::HostKeys)?
        .into_iter()
        .map(|(host, port, raw)| HostKeyRow {
            host,
            port,
            raw_key: b64.encode(raw),
        })
        .collect();
    let target_keys = apply(current_keys.clone(), &changes.host_keys, |k| {
        (k.host.clone(), k.port)
    });
    let host_keys_changed = target_keys != current_keys;

    let mut counts = None;
    store
        .apply_changes_with(
            |current| {
                let current = Snapshot::from_parts(current.clone(), current_keys.clone());
                let target = target_snapshot(current, changes, target_keys.clone());
                validate(&target)?;
                let pending = host_keys_changed
                    .then(|| serde_json::to_string(&target.host_keys))
                    .transpose()
                    .map_err(|_| BackupOpError::HostKeys)?;
                counts = Some(target.counts());
                Ok::<_, BackupOpError>((target.to_database(), pending))
            },
            &chrono::Utc::now().to_rfc3339(),
            abort_after,
        )
        .await?;

    let host_keys_pending = if host_keys_changed {
        match complete_pending_host_keys(store, host_keys).await {
            Ok(_) => false,
            Err(err) => {
                tracing::warn!(result = ?err, "host keys will be completed at the next start");
                true
            }
        }
    } else {
        false
    };
    Ok(ApplySummary {
        counts: counts.unwrap_or_default(),
        host_keys_pending,
    })
}

/// Applies per-entry operations to `rows`, keyed by `key`; the result is
/// ordered by key.
fn apply<T, K: Ord + Clone>(rows: Vec<T>, changes: &[Change<T, K>], key: impl Fn(&T) -> K) -> Vec<T>
where
    T: Clone,
{
    let mut map: BTreeMap<K, T> = rows.into_iter().map(|r| (key(&r), r)).collect();
    for c in changes {
        match c {
            Change::Upsert(row) => {
                map.insert(key(row), row.clone());
            }
            Change::Delete(k) => {
                map.remove(k);
            }
        }
    }
    map.into_values().collect()
}

/// The complete content after the change set, including the cascade of
/// deleted servers.
fn target_snapshot(current: Snapshot, changes: &ChangeSet, host_keys: Vec<HostKeyRow>) -> Snapshot {
    let old_servers: Vec<Uuid> = current.servers.iter().map(|s| s.id).collect();
    let mut t = Snapshot {
        groups: apply(current.groups, &changes.groups, |g| g.id),
        servers: apply(current.servers, &changes.servers, |s| s.id),
        tags: apply(current.tags, &changes.tags, |t| {
            (t.server_id, t.tag.clone())
        }),
        rules: apply(current.rules, &changes.rules, |r| r.id),
        notes: apply(current.notes, &changes.notes, |n| n.id),
        ai_providers: apply(current.ai_providers, &changes.ai_providers, |a| a.id),
        secrets: apply(current.secrets, &changes.secrets, |s| s.owner_ref.clone()),
        host_keys,
    };
    let alive: std::collections::HashSet<Uuid> = t.servers.iter().map(|s| s.id).collect();
    let gone: std::collections::HashSet<Uuid> = old_servers
        .into_iter()
        .filter(|id| !alive.contains(id))
        .collect();
    if !gone.is_empty() {
        t.tags.retain(|x| !gone.contains(&x.server_id));
        t.notes
            .retain(|n| !(n.target_type == "server" && gone.contains(&n.target_id)));
        t.rules.retain(|r| {
            !(r.scope_type == "server"
                && r.scope_value
                    .as_deref()
                    .and_then(|v| Uuid::parse_str(v).ok())
                    .is_some_and(|id| gone.contains(&id)))
        });
        t.secrets.retain(|s| {
            !gone
                .iter()
                .any(|id| s.owner_ref.starts_with(&format!("server:{id}:")))
        });
    }
    t
}
