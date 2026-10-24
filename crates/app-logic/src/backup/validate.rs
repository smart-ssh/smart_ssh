//! Validation of an opened backup before anything is applied (Spec 0109, U2,
//! U5, U6, U7, F12). A violation rejects the whole file; no row is skipped.
//!
//! Nothing here opens a file, runs a command or connects anywhere: paths and
//! names from the file are only compared and counted (U7, S-5).

use std::collections::{HashMap, HashSet};

use base64::Engine;
use chrono::{DateTime, Utc};
use persistence_sqlite::RuleRow;
use ssh_manager_core::ai::ProviderType;
use ssh_manager_core::filter::{RuleAction, RuleId, Scope};
use ssh_manager_core::profiles::AuthMethod;
use ssh_manager_core::shared::ServerId;
use uuid::Uuid;

use super::{BackupOpError, Snapshot};
use crate::dto::{PatternType, RuleInput};

const MAX_SERVERS: usize = 10_000;
const MAX_GROUPS: usize = 10_000;
const MAX_RULES: usize = 50_000;
const MAX_NOTES: usize = 200_000;
const MAX_SECRETS: usize = 50_000;
const MAX_AI_PROVIDERS: usize = 10_000;
const MAX_TAGS: usize = 200_000;
const MAX_HOST_KEYS: usize = 100_000;
const MAX_TEXT: usize = 64 * 1024;
const MAX_LONG_TEXT: usize = 4 * 1024 * 1024;
const MAX_GROUP_DEPTH: usize = 64;
const MAX_HOST_KEY_BYTES: usize = 64 * 1024;

fn bad(what: &'static str) -> BackupOpError {
    BackupOpError::Invalid(what)
}

fn ensure(ok: bool, what: &'static str) -> Result<(), BackupOpError> {
    if ok {
        Ok(())
    } else {
        Err(bad(what))
    }
}

fn text(s: &str) -> Result<(), BackupOpError> {
    ensure(s.len() <= MAX_TEXT, "a text exceeds the length limit")
}

fn long_text(s: &str) -> Result<(), BackupOpError> {
    ensure(s.len() <= MAX_LONG_TEXT, "a text exceeds the length limit")
}

fn timestamp(s: &str) -> Result<(), BackupOpError> {
    ensure(
        DateTime::parse_from_rfc3339(s).is_ok(),
        "a timestamp is not valid",
    )
}

fn unique(ids: impl Iterator<Item = Uuid>) -> Result<HashSet<Uuid>, BackupOpError> {
    let mut seen = HashSet::new();
    for id in ids {
        ensure(!id.is_nil(), "an entry has the reserved nil id")?;
        ensure(seen.insert(id), "an id occurs twice")?;
    }
    Ok(seen)
}

/// Builds the rule exactly as a user-created one would be (U6).
pub(crate) fn rule_from_row(r: &RuleRow) -> Result<persistence_sqlite::StoredRule, BackupOpError> {
    let pattern_type = match r.pattern_type.as_str() {
        "glob" => PatternType::Glob,
        "regex" => PatternType::Regex,
        "exact" => PatternType::Exact,
        _ => return Err(bad("a rule has an unknown pattern type")),
    };
    let action = match r.action.as_str() {
        "allow" => RuleAction::Allow,
        "confirm" => RuleAction::Confirm,
        "deny" => RuleAction::Deny,
        _ => return Err(bad("a rule has an unknown action")),
    };
    let scope = match (r.scope_type.as_str(), r.scope_value.as_deref()) {
        ("global", None) => Scope::Global,
        ("server", Some(v)) => Scope::Server(ServerId(
            Uuid::parse_str(v).map_err(|_| bad("a rule has an invalid scope"))?,
        )),
        ("tag", Some(v)) if !v.is_empty() => Scope::Tag(v.to_string()),
        _ => return Err(bad("a rule has an unknown scope")),
    };
    let priority = i32::try_from(r.priority).map_err(|_| bad("a rule priority is out of range"))?;
    let stamp = |s: &str| {
        DateTime::parse_from_rfc3339(s)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|_| bad("a timestamp is not valid"))
    };
    crate::filter_rules::prepare_rule(
        RuleInput {
            pattern_type,
            pattern_value: r.pattern_value.clone(),
            action,
            scope,
            priority,
        },
        RuleId(r.id.to_string()),
        stamp(&r.created_at)?,
        stamp(&r.updated_at)?,
    )
    .map_err(|_| bad("a filter rule is not valid"))
}

/// Every credential reference of the auth method must be one of the server's
/// own: the file must not be able to point a server at another secret (for
/// example the device's backup key).
fn own_refs(auth: &AuthMethod, server: Uuid) -> bool {
    let prefix = format!("server:{server}:");
    let ok = |r: &ssh_manager_core::profiles::CredentialRef| r.as_str().starts_with(&prefix);
    match auth {
        AuthMethod::Password { credential_ref } => ok(credential_ref),
        AuthMethod::PrivateKey {
            credential_ref,
            passphrase_ref,
        } => ok(credential_ref) && passphrase_ref.as_ref().is_none_or(ok),
        AuthMethod::Agent | AuthMethod::NoPassword => true,
        AuthMethod::Certificate { cert_ref, key_ref } => ok(cert_ref) && ok(key_ref),
        AuthMethod::IdentityFile { passphrase_ref, .. } => passphrase_ref.as_ref().is_none_or(ok),
    }
}

pub(crate) fn validate(s: &Snapshot) -> Result<(), BackupOpError> {
    ensure(s.servers.len() <= MAX_SERVERS, "too many servers")?;
    ensure(s.groups.len() <= MAX_GROUPS, "too many folders")?;
    ensure(s.rules.len() <= MAX_RULES, "too many rules")?;
    ensure(s.notes.len() <= MAX_NOTES, "too many note revisions")?;
    ensure(s.secrets.len() <= MAX_SECRETS, "too many credentials")?;
    ensure(
        s.ai_providers.len() <= MAX_AI_PROVIDERS,
        "too many providers",
    )?;
    ensure(s.tags.len() <= MAX_TAGS, "too many tags")?;
    ensure(s.host_keys.len() <= MAX_HOST_KEYS, "too many host keys")?;

    // Folders: unique ids, existing parents, no cycle, bounded depth.
    let group_ids = unique(s.groups.iter().map(|g| g.id))?;
    let parents: HashMap<Uuid, Option<Uuid>> =
        s.groups.iter().map(|g| (g.id, g.parent_id)).collect();
    for g in &s.groups {
        text(&g.name)?;
        long_text(&g.notes)?;
        timestamp(&g.created_at)?;
        timestamp(&g.updated_at)?;
        if let Some(p) = g.parent_id {
            ensure(
                group_ids.contains(&p),
                "a folder refers to a missing folder",
            )?;
        }
        let mut cursor = g.parent_id;
        let mut depth = 1;
        while let Some(p) = cursor {
            depth += 1;
            ensure(
                depth <= MAX_GROUP_DEPTH,
                "the folder tree is too deep or has a cycle",
            )?;
            cursor = parents.get(&p).copied().flatten();
        }
    }

    // Servers.
    let server_ids = unique(s.servers.iter().map(|x| x.id))?;
    let jumps: HashMap<Uuid, Option<Uuid>> =
        s.servers.iter().map(|x| (x.id, x.jump_host_id)).collect();
    for x in &s.servers {
        for t in [&x.name, &x.host, &x.username] {
            text(t)?;
        }
        long_text(&x.notes)?;
        text(&x.auth_method)?;
        for t in [&x.sftp_server_path, &x.start_directory]
            .into_iter()
            .flatten()
        {
            text(t)?;
        }
        timestamp(&x.created_at)?;
        timestamp(&x.updated_at)?;
        ensure(x.port != 0, "a server has an invalid port")?;
        ensure(
            matches!(
                x.post_ingest_policy.as_str(),
                "strict" | "balanced" | "standard"
            ),
            "a server has an unknown setting",
        )?;
        if let Some(g) = x.group_id {
            ensure(
                group_ids.contains(&g),
                "a server refers to a missing folder",
            )?;
        }
        if let Some(j) = x.jump_host_id {
            ensure(
                server_ids.contains(&j),
                "a server refers to a missing jump host",
            )?;
        }
        let mut cursor = x.jump_host_id;
        let mut steps = 0usize;
        while let Some(j) = cursor {
            steps += 1;
            ensure(
                j != x.id && steps <= s.servers.len(),
                "jump hosts form a cycle",
            )?;
            cursor = jumps.get(&j).copied().flatten();
        }
        let auth: AuthMethod = serde_json::from_str(&x.auth_method)
            .map_err(|_| bad("a server has an unknown login method"))?;
        ensure(
            own_refs(&auth, x.id),
            "a server refers to a credential of another owner",
        )?;
    }

    // Tags.
    let mut tag_seen = HashSet::new();
    for t in &s.tags {
        text(&t.tag)?;
        ensure(!t.tag.is_empty(), "a tag is empty")?;
        ensure(
            server_ids.contains(&t.server_id),
            "a tag refers to a missing server",
        )?;
        ensure(
            tag_seen.insert((t.server_id, t.tag.as_str())),
            "a tag occurs twice",
        )?;
    }

    // Rules: the same validation as a user-created rule (U6).
    unique(s.rules.iter().map(|r| r.id))?;
    for r in &s.rules {
        text(&r.pattern_value)?;
        if let Some(v) = &r.scope_value {
            text(v)?;
        }
        let stored = rule_from_row(r)?;
        if let Scope::Server(id) = stored.scope {
            ensure(
                server_ids.contains(&id.0),
                "a rule refers to a missing server",
            )?;
        }
    }

    // Notes.
    unique(s.notes.iter().map(|n| n.id))?;
    for n in &s.notes {
        long_text(&n.content)?;
        timestamp(&n.created_at)?;
        for t in [&n.ai_provider, &n.ai_model].into_iter().flatten() {
            text(t)?;
        }
        ensure(
            matches!(n.editor_type.as_str(), "user" | "ai"),
            "a note has an unknown editor type",
        )?;
        let known = match n.target_type.as_str() {
            "server" => server_ids.contains(&n.target_id),
            "group" => group_ids.contains(&n.target_id),
            _ => return Err(bad("a note has an unknown target type")),
        };
        ensure(known, "a note refers to a missing entry")?;
    }

    // AI providers.
    let provider_ids = unique(s.ai_providers.iter().map(|a| a.id))?;
    for a in &s.ai_providers {
        for t in [&a.display_name, &a.model, &a.extra_headers].into_iter() {
            text(t)?;
        }
        for t in [&a.base_url, &a.attestation_url].into_iter().flatten() {
            text(t)?;
        }
        timestamp(&a.created_at)?;
        timestamp(&a.updated_at)?;
        ensure(
            ProviderType::from_db_str(&a.provider_type).is_some(),
            "a provider has an unknown type",
        )?;
        ensure(
            a.credential_ref == format!("ai-provider:{}", a.id),
            "a provider refers to a credential of another owner",
        )?;
        ensure(
            a.max_tokens_override.is_none_or(|v| v > 0),
            "a provider has an invalid limit",
        )?;
    }

    // Secrets belong to an existing owner.
    let mut secret_seen = HashSet::new();
    for sec in &s.secrets {
        ensure(
            sec.owner_ref.len() <= MAX_TEXT,
            "a text exceeds the length limit",
        )?;
        long_text(&sec.value)?;
        ensure(
            secret_seen.insert(sec.owner_ref.as_str()),
            "a credential occurs twice",
        )?;
        let (kind, rest) = sec
            .owner_ref
            .split_once(':')
            .ok_or_else(|| bad("a credential has an unknown owner"))?;
        let owner = rest.split(':').next().unwrap_or("");
        let owner = Uuid::parse_str(owner).map_err(|_| bad("a credential has an unknown owner"))?;
        let known = match kind {
            "server" => server_ids.contains(&owner),
            "ai-provider" => provider_ids.contains(&owner),
            _ => false,
        };
        ensure(known, "a credential has an unknown owner")?;
    }

    // Host keys.
    let b64 = base64::engine::general_purpose::STANDARD;
    for h in &s.host_keys {
        text(&h.host)?;
        ensure(!h.host.is_empty() && h.port != 0, "a host key is invalid")?;
        let raw = b64
            .decode(&h.raw_key)
            .map_err(|_| bad("a host key is invalid"))?;
        ensure(
            !raw.is_empty() && raw.len() <= MAX_HOST_KEY_BYTES,
            "a host key is invalid",
        )?;
    }
    Ok(())
}
