//! Read API of the activity record for extensions (Spec 0057, issue #1449).
//!
//! Reached through [`crate::ext`] only (`list_recorded_sessions`,
//! `list_record_entries`, `record_entries_of_session`, `redact_for_server`);
//! the module itself is private. The DTOs here are independent of the table
//! layout, output is UTF-8 text (lossy). Entries are returned **as stored**,
//! i.e. as the writer redacted them; the API never reads chat messages and
//! has no write path. Anything that leaves the device must come from this
//! record and be redacted again with [`redact_for_server`].

use std::sync::Arc;

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use persistence_sqlite::{
    EntryFilter, PageRequest, RecordCursor, RecordedEntry, SessionFilter, SqliteRecordQuery,
    MAX_PAGE_SIZE,
};
use ssh_manager_core::ai::{DefaultOutputRedactor, OutputRedactor};
use ssh_manager_core::audit::{
    LedgerConnectionEvent, LedgerDecisionOutcome, LedgerEntryContent, LedgerSource,
};
use ssh_manager_core::profiles::{CredentialStore, ProfileStore};
use ssh_manager_core::shared::ServerId;

use crate::error::CommandError;
use crate::session_extension::SessionExtension;

/// Kind of a recorded session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum RecordSessionKind {
    Chat,
    Mcp,
    Terminal,
}

impl RecordSessionKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Mcp => "mcp",
            Self::Terminal => "terminal",
        }
    }
}

/// Who caused an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum RecordSource {
    User,
    Ai,
    McpAgent,
}

impl RecordSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Ai => "ai",
            Self::McpAgent => "mcp-agent",
        }
    }
}

/// Type of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum RecordEntryType {
    CommandProposed,
    Decision,
    CommandExecuted,
    AiMessage,
    FileRead,
    FileWritten,
    Connection,
    ActivityTick,
}

impl RecordEntryType {
    fn as_str(self) -> &'static str {
        match self {
            Self::CommandProposed => "command_proposed",
            Self::Decision => "decision",
            Self::CommandExecuted => "command_executed",
            Self::AiMessage => "ai_message",
            Self::FileRead => "file_read",
            Self::FileWritten => "file_written",
            Self::Connection => "connection",
            Self::ActivityTick => "activity_tick",
        }
    }
}

/// Outcome of an approval decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub enum RecordDecisionOutcome {
    Confirmed,
    Rejected,
    AutoApproved,
}

impl RecordDecisionOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "Confirmed",
            Self::Rejected => "Rejected",
            Self::AutoApproved => "AutoApproved",
        }
    }
}

/// Filter for [`list_recorded_sessions`]. Empty list / `None` = no restriction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct RecordedSessionFilter {
    /// Started at or after.
    pub started_from: Option<DateTime<Utc>>,
    /// Started before.
    pub started_until: Option<DateTime<Utc>>,
    pub server_ids: Vec<ServerId>,
    /// Group id from the snapshot taken when the session started; matches
    /// the group and every subgroup below it.
    pub group_id: Option<String>,
    pub kinds: Vec<RecordSessionKind>,
    /// Only sessions with at least one failed ledger write.
    pub has_failed_writes: bool,
}

/// Filter for [`list_record_entries`]. Empty list / `None` = no restriction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct RecordEntryFilter {
    pub from: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub session_ids: Vec<Uuid>,
    pub server_ids: Vec<ServerId>,
    pub group_id: Option<String>,
    pub sources: Vec<RecordSource>,
    pub entry_types: Vec<RecordEntryType>,
    /// Restricts to decision entries with one of these outcomes.
    pub decision_outcomes: Vec<RecordDecisionOutcome>,
    /// Executed commands with an exit code other than 0 (an unknown exit
    /// code counts as not 0).
    pub exit_code_nonzero: bool,
    /// Case-insensitive (Unicode) substring of a command text or AI text.
    pub text_contains: Option<String>,
}

/// Page request: `page_size` is clamped to 1..=[`MAX_RECORD_PAGE_SIZE`];
/// `cursor` is the `next_cursor` of the previous page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct RecordPageRequest {
    pub page_size: u32,
    pub cursor: Option<String>,
}

/// Largest page size.
pub const MAX_RECORD_PAGE_SIZE: u32 = MAX_PAGE_SIZE;

/// A page of results; `next_cursor` is `None` after the last page. With a text
/// filter a page can be short or even empty while `next_cursor` is still `Some`
/// (the scan per call is bounded): keep paging while `next_cursor` is `Some`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct RecordPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

/// A recorded session with the server snapshot taken when it started.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct RecordedSessionInfo {
    pub id: Uuid,
    pub kind: String,
    pub mcp_client: Option<String>,
    pub server_id: String,
    pub server_name: String,
    pub server_host: String,
    pub server_port: u16,
    pub server_user: String,
    /// Root group first.
    pub group_ids: Vec<String>,
    pub group_names: Vec<String>,
    pub tags: Vec<String>,
    pub time_zone: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub last_activity_at: Option<String>,
    pub failed_writes: i64,
}

/// One entry of the record, as stored (already redacted by the writer).
/// Fields that do not apply to the entry type are `None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct RecordEntryInfo {
    pub id: String,
    pub session_id: Uuid,
    pub sequence: i64,
    pub created_at: DateTime<Utc>,
    pub source: RecordSource,
    pub entry_type: RecordEntryType,
    /// Command text (proposed / executed) or AI message text.
    pub text: Option<String>,
    pub outcome: Option<RecordDecisionOutcome>,
    pub reason: Option<String>,
    pub code: Option<String>,
    pub exit_code: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub cancelled: Option<bool>,
    pub path: Option<String>,
    pub bytes: Option<u64>,
    /// `connected`, `disconnected`, `reconnected`, `failed`.
    pub connection_event: Option<String>,
    pub detail: Option<String>,
    pub action_id: Option<String>,
    /// The writer could not redact completely.
    pub redaction_incomplete: bool,
}

/// Result of [`redact_for_server`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedText {
    pub text: String,
    /// The server no longer exists: only the generic patterns were applied.
    pub generic_only: bool,
    /// A secret lookup failed (keychain, profile store): masking may be
    /// incomplete.
    pub incomplete: bool,
}

fn parse_cursor(c: &Option<String>) -> Result<Option<RecordCursor>, CommandError> {
    c.as_deref()
        .map(|raw| {
            raw.parse::<i64>().map(RecordCursor).map_err(|_| {
                CommandError::with_code("invalid record cursor", "INVALID_RECORD_CURSOR")
            })
        })
        .transpose()
}

fn page_request(p: &RecordPageRequest) -> Result<PageRequest, CommandError> {
    Ok(PageRequest {
        limit: p.page_size,
        after: parse_cursor(&p.cursor)?,
    })
}

fn map_page<S, T>(page: persistence_sqlite::Page<S>, f: impl Fn(S) -> T) -> RecordPage<T> {
    RecordPage {
        items: page.items.into_iter().map(f).collect(),
        next_cursor: page.next.map(|c| c.0.to_string()),
    }
}

fn texts<T, U: ToString>(v: &[T], f: impl Fn(&T) -> U) -> Vec<String> {
    v.iter().map(|x| f(x).to_string()).collect()
}

/// Recorded sessions, oldest first.
pub async fn list_recorded_sessions(
    query: &SqliteRecordQuery,
    filter: &RecordedSessionFilter,
    page: &RecordPageRequest,
) -> Result<RecordPage<RecordedSessionInfo>, CommandError> {
    let f = SessionFilter {
        started_from: filter.started_from,
        started_until: filter.started_until,
        server_ids: texts(&filter.server_ids, |s| s.0),
        group_id: filter.group_id.clone(),
        kinds: texts(&filter.kinds, |k| k.as_str()),
        has_failed_writes: filter.has_failed_writes,
    };
    let page = query
        .list_sessions(&f, page_request(page)?)
        .await
        .map_err(CommandError::from)?;
    Ok(map_page(page, |s| RecordedSessionInfo {
        id: s.id,
        kind: s.kind,
        mcp_client: s.mcp_client,
        server_id: s.server_id,
        server_name: s.server_name,
        server_host: s.server_host,
        server_port: s.server_port,
        server_user: s.server_user,
        group_ids: s.group_ids,
        group_names: s.group_names,
        tags: s.tags,
        time_zone: s.time_zone,
        started_at: s.started_at,
        ended_at: s.ended_at,
        last_activity_at: s.last_activity_at,
        failed_writes: s.failed_writes,
    }))
}

/// Entries across sessions, in the order they were recorded.
pub async fn list_record_entries(
    query: &SqliteRecordQuery,
    filter: &RecordEntryFilter,
    page: &RecordPageRequest,
) -> Result<RecordPage<RecordEntryInfo>, CommandError> {
    let f = EntryFilter {
        from: filter.from,
        until: filter.until,
        session_ids: filter.session_ids.clone(),
        server_ids: texts(&filter.server_ids, |s| s.0),
        group_id: filter.group_id.clone(),
        sources: texts(&filter.sources, |s| s.as_str()),
        entry_types: texts(&filter.entry_types, |t| t.as_str()),
        decision_outcomes: texts(&filter.decision_outcomes, |o| o.as_str()),
        exit_code_nonzero: filter.exit_code_nonzero,
        text_contains: filter.text_contains.clone(),
    };
    let page = query
        .list_entries(&f, page_request(page)?)
        .await
        .map_err(CommandError::from)?;
    Ok(map_page(page, entry_info))
}

/// All entries of one recorded session, in sequence order.
pub async fn record_entries_of_session(
    query: &SqliteRecordQuery,
    session_id: Uuid,
) -> Result<Vec<RecordEntryInfo>, CommandError> {
    let rows = query
        .entries_of_session(session_id)
        .await
        .map_err(CommandError::from)?;
    Ok(rows.into_iter().map(entry_info).collect())
}

fn entry_info(r: RecordedEntry) -> RecordEntryInfo {
    let e = r.entry;
    let mut info = RecordEntryInfo {
        id: e.id,
        session_id: r.session_id,
        sequence: r.sequence,
        created_at: e.created_at,
        source: match e.source {
            LedgerSource::User => RecordSource::User,
            LedgerSource::Ai => RecordSource::Ai,
            LedgerSource::McpAgent => RecordSource::McpAgent,
        },
        entry_type: RecordEntryType::ActivityTick,
        text: None,
        outcome: None,
        reason: None,
        code: None,
        exit_code: None,
        stdout: None,
        stderr: None,
        cancelled: None,
        path: None,
        bytes: None,
        connection_event: None,
        detail: None,
        action_id: e.meta.action_id,
        redaction_incomplete: e.meta.redaction_incomplete,
    };
    match e.content {
        LedgerEntryContent::CommandProposed { command } => {
            info.entry_type = RecordEntryType::CommandProposed;
            info.text = Some(command);
        }
        LedgerEntryContent::Decision {
            outcome,
            reason,
            code,
            ..
        } => {
            info.entry_type = RecordEntryType::Decision;
            info.outcome = Some(match outcome {
                LedgerDecisionOutcome::Confirmed => RecordDecisionOutcome::Confirmed,
                LedgerDecisionOutcome::Rejected => RecordDecisionOutcome::Rejected,
                LedgerDecisionOutcome::AutoApproved => RecordDecisionOutcome::AutoApproved,
            });
            info.reason = reason;
            info.code = code;
        }
        LedgerEntryContent::CommandExecuted {
            command,
            output,
            cancelled,
        } => {
            info.entry_type = RecordEntryType::CommandExecuted;
            info.text = Some(command);
            info.exit_code = output.exit_code;
            info.stdout = Some(String::from_utf8_lossy(&output.stdout).into_owned());
            info.stderr = Some(String::from_utf8_lossy(&output.stderr).into_owned());
            info.cancelled = Some(cancelled);
        }
        LedgerEntryContent::AiMessage { text } => {
            info.entry_type = RecordEntryType::AiMessage;
            info.text = Some(text);
        }
        LedgerEntryContent::FileRead { path, bytes, .. } => {
            info.entry_type = RecordEntryType::FileRead;
            info.path = Some(path);
            info.bytes = bytes;
        }
        LedgerEntryContent::FileWritten {
            path, bytes_after, ..
        } => {
            info.entry_type = RecordEntryType::FileWritten;
            info.path = Some(path);
            info.bytes = bytes_after;
        }
        LedgerEntryContent::Connection { event, detail } => {
            info.entry_type = RecordEntryType::Connection;
            info.connection_event = Some(
                match event {
                    LedgerConnectionEvent::Connected => "connected",
                    LedgerConnectionEvent::Disconnected => "disconnected",
                    LedgerConnectionEvent::Reconnected => "reconnected",
                    LedgerConnectionEvent::Failed => "failed",
                }
                .to_string(),
            );
            info.detail = detail;
        }
        LedgerEntryContent::ActivityTick => {}
    }
    info
}

/// Redacts `text` with the server's **current** redactor: built-in patterns,
/// the server's sudo password and the secrets the session extension knows
/// for it. For a server that no longer exists only the generic patterns
/// apply, and the result says so (`generic_only`). A failed secret lookup
/// does not fail the call but sets `incomplete`.
pub async fn redact_for_server(
    profile_store: &dyn ProfileStore,
    credential_store: Arc<dyn CredentialStore + Send + Sync>,
    extension: Option<&dyn SessionExtension>,
    server_id: ServerId,
    text: &str,
) -> RedactedText {
    let local_tags = crate::dto::is_local(server_id).then(Vec::new);
    let lookup = crate::session_extension::scope_lookup(profile_store, server_id, local_tags).await;
    if lookup.scope.is_none() {
        return RedactedText {
            text: DefaultOutputRedactor::new().redact_text(text),
            generic_only: true,
            incomplete: false,
        };
    }
    let sudo =
        crate::server_redaction::read_sudo_password_checked_async(credential_store, server_id)
            .await;
    let secrets: Vec<SecretString> =
        crate::session_extension::known_secrets(extension, lookup.scope.as_ref());
    let (redactor, incomplete) =
        crate::session_extension::redactor_for_connect(&sudo, &lookup, &secrets);
    RedactedText {
        text: redactor.redact_text(text),
        generic_only: false,
        incomplete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_extension::ServerScope;
    use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore, ServerBuilder};
    use ssh_manager_core::ai::REDACTED_PLACEHOLDER;
    use ssh_manager_core::ssh::CommandOutput;

    const GENERIC: &str = "ghp_0123456789abcdefghijABCDEFGHIJ012345";
    const EXT_SECRET: &str = "zebra-only-the-extension-knows";

    struct Ext(ServerId);

    #[async_trait::async_trait]
    impl SessionExtension for Ext {
        async fn context_section(
            &self,
            _scope: &ServerScope,
            _language: crate::system_prompt::PromptLanguage,
        ) -> Option<String> {
            None
        }
        fn known_secrets(&self, scope: &ServerScope) -> Vec<SecretString> {
            if scope.server_id == self.0 {
                vec![SecretString::from(EXT_SECRET.to_string())]
            } else {
                Vec::new()
            }
        }
    }

    fn creds() -> Arc<dyn CredentialStore + Send + Sync> {
        Arc::new(InMemoryCredentialStore::new())
    }

    #[tokio::test]
    async fn test_redact_for_server_masks_extension_secret_and_generic_pattern() {
        let srv = ServerBuilder::new().name("web").build();
        let store = InMemoryProfileStore::new().with_server(srv.clone());
        let ext = Ext(srv.id);
        let text = format!("a {EXT_SECRET} b {GENERIC} c");
        let out = redact_for_server(&store, creds(), Some(&ext), srv.id, &text).await;
        assert!(!out.text.contains(EXT_SECRET), "{}", out.text);
        assert!(!out.text.contains(GENERIC), "{}", out.text);
        assert!(out.text.contains(REDACTED_PLACEHOLDER));
        assert!(!out.generic_only);
        assert!(!out.incomplete);
        // Without the extension the literal secret stays: proves it is the
        // extension's value that masks it.
        let plain = redact_for_server(&store, creds(), None, srv.id, &text).await;
        assert!(plain.text.contains(EXT_SECRET));
    }

    #[tokio::test]
    async fn test_redact_for_deleted_server_is_generic_only() {
        let gone = ServerId::new();
        let store = InMemoryProfileStore::new();
        let ext = Ext(gone);
        let text = format!("{EXT_SECRET} {GENERIC}");
        let out = redact_for_server(&store, creds(), Some(&ext), gone, &text).await;
        assert!(out.generic_only);
        assert!(!out.text.contains(GENERIC));
        // Only generic patterns: the extension secret is not applied.
        assert!(out.text.contains(EXT_SECRET));
    }

    #[test]
    fn test_output_is_lossy_utf8_and_independent_of_table_layout() {
        use persistence_sqlite::RecordedEntry;
        let content = LedgerEntryContent::CommandExecuted {
            command: "cat x".into(),
            output: CommandOutput {
                stdout: vec![b'o', b'k', 0xFF],
                stderr: Vec::new(),
                exit_code: Some(2),
                truncated: false,
            },
            cancelled: false,
        };
        let entry = persistence_sqlite::LedgerEntry {
            id: "e1".into(),
            source: LedgerSource::User,
            content,
            created_at: Utc::now(),
            meta: Default::default(),
        };
        let info = entry_info(RecordedEntry {
            session_id: Uuid::nil(),
            sequence: 4,
            entry,
        });
        assert_eq!(info.stdout.as_deref(), Some("ok\u{FFFD}"));
        assert_eq!(info.exit_code, Some(2));
        assert_eq!(info.entry_type, RecordEntryType::CommandExecuted);
    }

    #[test]
    fn test_invalid_cursor_is_rejected() {
        let bad = RecordPageRequest {
            page_size: 10,
            cursor: Some("abc".into()),
        };
        assert!(page_request(&bad).is_err());
    }

    /// The query functions are reachable through `app_logic::ext` only: the
    /// module is private, and `ext` is the one place that names its items.
    #[test]
    fn test_record_query_is_reachable_through_ext_only() {
        let lib = include_str!("lib.rs");
        assert!(
            lib.lines().any(|l| l.trim() == "mod record_query;"),
            "record_query must stay a private module"
        );
        assert!(!lib.contains("pub mod record_query"));
        assert!(!lib.contains("pub use record_query"));
        let ext = include_str!("ext.rs");
        for f in [
            "pub async fn list_recorded_sessions",
            "pub async fn list_record_entries",
            "pub async fn record_entries_of_session",
            "pub async fn redact_for_server",
        ] {
            assert!(ext.contains(f), "missing in ext: {f}");
        }
    }
}
