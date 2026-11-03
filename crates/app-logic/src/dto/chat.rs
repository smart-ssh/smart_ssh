//! DTOs: chat.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use persistence_sqlite::ChatSessionSummary;
use ssh_manager_core::shared::ServerId;

use crate::ids::SessionId;
use crate::status::ConnectionStatus;

/// Antwort auf `host-key-verification-needed` (Spec 0007, Abschnitt 4:
/// `confirm_host_key(session_id, decision: Trust | Reject)`).
///
/// `#[serde(tag = "decision", rename_all = "camelCase")]` statt Serdes
/// Standard-Außen-Tagging: ergibt `{"decision": "trust"}` statt
/// `"Trust"`/`{"Trust": null}` — für das TypeScript-Frontend die
/// natürlichere Form, um diesen Wert selbst zu konstruieren.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "decision",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HostKeyUserDecision {
    Trust,
    Reject,
}

/// Antwort auf `chat-action-proposed` mit `decision: Confirm` (Spec 0007,
/// Abschnitt 4/6: `respond_to_action(session_id, action_id, decision:
/// Approve | Deny | EditThenApprove { command: String })`).
#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "decision",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ActionUserDecision {
    Approve,
    Deny,
    EditThenApprove {
        command: String,
    },
    /// Issue #1019: internal deny after a reconnect; never sent by the frontend.
    #[serde(skip_deserializing)]
    DenyReconnect,
}

/// Herkunft einer vorgeschlagenen Aktion (Spec 0028, Abschnitt 6/9a) —
/// bestimmt einerseits eine zusätzliche Verschärfung der
/// Filter-Engine-Entscheidung (s. `orchestration::handle_action_proposed`),
/// andererseits die Ursprungs-Kennzeichnung im Bestätigungsdialog. Intern
/// getaggt (statt Serdes Standard-Außen-Tagging) aus demselben Grund wie
/// `HostKeyUserDecision`/`ActionUserDecision` oben — die natürlichere Form
/// für das TypeScript-Frontend, hier nur in Sende- statt Empfangsrichtung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ActionOrigin {
    /// Vorschlag aus dem eigenen Chat-Flow (Spec 0007/0021).
    Internal,
    /// Vorschlag über einen MCP-Tool-Call (Spec 0028) — `client_name` ist
    /// der optionale `clientInfo.name` aus dem MCP-Handshake, falls der
    /// verbindende Client ihn übermittelt hat.
    Mcp { client_name: Option<String> },
}

// --- Spec 0008: Server-/Gruppen-Verwaltung ------------------------------

/// Exportformat für `export_document` (Spec 0012, Abschnitt 3). Seit Spec
/// 0037, Abschnitt 4, nur noch `Markdown` — der Word-Export wurde komplett
/// entfernt (kein Gating, s. `crate::document_export`-Moduldoc), nicht auf
/// eine leere `require`-Prüfung reduziert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DocumentFormat {
    Markdown,
}

// --- Spec 0017: Multi-Tab-Sessions ---------------------------------------

/// Sicht auf eine laufende (oder auf Host-Key-Bestätigung wartende) Session
/// für die Tab-Leiste (Spec 0017, Abschnitt 2). `has_pending_action`
/// steuert den Hinweis-Indikator auf Hintergrund-Tabs (Abschnitt 5) —
/// bewusst nur ein `bool`, nicht die `ActionId` selbst: das Frontend
/// erfährt Letztere ohnehin bereits aus dem zugehörigen
/// `chat-action-proposed`-Event, sobald es zu diesem Tab wechselt und den
/// Dialog tatsächlich zeigt.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryDto {
    pub session_id: SessionId,
    pub server_id: ServerId,
    pub server_name: String,
    pub status: ConnectionStatus,
    pub has_pending_action: bool,
    /// Issue #611: current persisted chat session id (`None` for the local
    /// pseudo-server and no-persist sessions).
    pub chat_session_id: Option<String>,
    /// Spec 0104: `Some`, wenn die Sitzung einem MCP-Client gehört — das
    /// Frontend beschriftet und kennzeichnet den Tab dann als MCP-Tab, auch
    /// nach einem Reload.
    pub mcp: Option<McpSessionDto>,
}

/// Spec 0104: Kennzeichnung einer MCP-Sitzung in [`SessionSummaryDto`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSessionDto {
    /// `clientInfo.name` aus dem MCP-Handshake, `None` ohne Namen.
    pub client_name: Option<String>,
}

// --- Spec 0034, Abschnitt 6/8: persistente Chat-Sitzungen ----------------

/// Für den Auswahl-Screen beim Verbinden (Spec 0034, Abschnitt 6): Titel,
/// Zeitpunkt, Nachrichtenanzahl je vergangener Sitzung.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSessionSummaryDto {
    pub session_id: String,
    pub title: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub message_count: i64,
}

impl From<ChatSessionSummary> for ChatSessionSummaryDto {
    fn from(s: ChatSessionSummary) -> Self {
        Self {
            session_id: s.id.to_string(),
            title: s.title,
            started_at: s.started_at,
            ended_at: s.ended_at,
            message_count: s.message_count,
        }
    }
}

/// Spec 0034, Abschnitt 8/6: die bereits geladene Historie eines Tabs
/// (nach `connect`/`resume_chat_session`, s. `commands::get_chat_history`)
/// — für die Anzeige eines wiederaufgenommenen Chats im Frontend, das
/// sonst nur über Live-Events (`chat-text-delta` etc.) befüllt wird, die
/// beim Fortsetzen einer Sitzung logischerweise nicht erneut feuern.
/// Bewusst eine eigene, schlanke Sicht statt der vollen `MessageContent`
/// (deren `CommandOutput.stdout`/`stderr` als `Vec<u8>` fürs Frontend
/// unhandlich wären) — analog zu `ActionResultPayload::Command`, dessen
/// Form hier für den `command_result`-Fall bewusst wiederverwendet wird.
/// `rename_all_fields` ist nötig, damit z. B. `exit_code` als `exitCode`
/// ankommt (Issue #167, s. Regressionstests unten).
#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum ChatHistoryEntryDto {
    Text {
        role: ChatHistoryRoleDto,
        text: String,
    },
    CommandResult {
        role: ChatHistoryRoleDto,
        command: String,
        stdout: String,
        stderr: String,
        exit_code: Option<i32>,
        cancelled: bool,
        /// Spec 0043, Fund A — s. `ActionResultPayload::Command.truncated`.
        truncated: bool,
    },
    ActionRejected {
        role: ChatHistoryRoleDto,
        command: String,
        reason: String,
    },
    /// Issue #162: eine serverseitige Web-Recherche des Providers.
    WebActivity {
        role: ChatHistoryRoleDto,
        activity: WebActivityDto,
    },
    /// Issue #504 (ADR 0141, Spec 0110 §5): extension parts of a user
    /// message — label and media class only, never content, and without
    /// asking any extension.
    ExtensionParts {
        role: ChatHistoryRoleDto,
        items: Vec<ChatHistoryExtensionPartDto>,
    },
    /// Issue #545: a clarifying question the AI asked with answer options.
    Question {
        role: ChatHistoryRoleDto,
        question_id: String,
        question: String,
        options: Vec<QuestionOptionDto>,
        multi_select: bool,
    },
}

/// Issue #545: one answer option of a question (event and history).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOptionDto {
    pub label: String,
    pub description: Option<String>,
}

impl From<&ssh_manager_core::profiles::AskUserOption> for QuestionOptionDto {
    fn from(option: &ssh_manager_core::profiles::AskUserOption) -> Self {
        Self {
            label: option.label.clone(),
            description: option.description.clone(),
        }
    }
}

/// Issue #504: one extension part in the history (label and media class).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatHistoryExtensionPartDto {
    pub label: String,
    pub media: ssh_manager_core::ai::ExtensionMedia,
}

/// Issue #162: Anzeige einer Web-Recherche im Chat (Live-Event
/// `chat-web-activity` und wiederaufgenommene Historie) — Suchanfrage bzw.
/// URL, Treffer, zitierte Quellen, ggf. Fehlercode. Der gelesene
/// Seitentext selbst geht bewusst nicht ans Frontend (nur Anzeige der
/// Quellen, s. Spec 0105).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebActivityDto {
    pub kind: WebActivityKindDto,
    pub input: String,
    pub results: Vec<WebSourceDto>,
    pub cited: Vec<WebSourceDto>,
    pub content_truncated: bool,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WebActivityKindDto {
    Search,
    Fetch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSourceDto {
    pub title: String,
    pub url: String,
}

impl From<&ssh_manager_core::ai::WebActivity> for WebActivityDto {
    fn from(activity: &ssh_manager_core::ai::WebActivity) -> Self {
        let sources = |list: &[ssh_manager_core::ai::WebSource]| {
            list.iter()
                .map(|s| WebSourceDto {
                    title: s.title.clone(),
                    url: s.url.clone(),
                })
                .collect()
        };
        Self {
            kind: match activity.kind {
                ssh_manager_core::ai::WebActivityKind::Search => WebActivityKindDto::Search,
                ssh_manager_core::ai::WebActivityKind::Fetch => WebActivityKindDto::Fetch,
            },
            input: activity.input.clone(),
            results: sources(&activity.results),
            cited: sources(&activity.cited),
            content_truncated: activity.content_truncated,
            error_code: activity.error_code.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatHistoryRoleDto {
    User,
    Assistant,
    ActionResult,
}

impl From<ssh_manager_core::ai::Role> for ChatHistoryRoleDto {
    fn from(role: ssh_manager_core::ai::Role) -> Self {
        match role {
            ssh_manager_core::ai::Role::User => ChatHistoryRoleDto::User,
            ssh_manager_core::ai::Role::Assistant => ChatHistoryRoleDto::Assistant,
            ssh_manager_core::ai::Role::ActionResult => ChatHistoryRoleDto::ActionResult,
        }
    }
}

impl From<ssh_manager_core::ai::ChatMessage> for ChatHistoryEntryDto {
    fn from(message: ssh_manager_core::ai::ChatMessage) -> Self {
        let role = ChatHistoryRoleDto::from(message.role);
        match message.content {
            ssh_manager_core::ai::MessageContent::Text(text) => {
                ChatHistoryEntryDto::Text { role, text }
            }
            ssh_manager_core::ai::MessageContent::CommandResult {
                command,
                output,
                cancelled,
                ..
            } => ChatHistoryEntryDto::CommandResult {
                role,
                command,
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: output.exit_code,
                cancelled,
                truncated: output.truncated,
            },
            ssh_manager_core::ai::MessageContent::ActionRejected { command, reason } => {
                let reason_text = crate::confirmation_settled::rejection_reason_text(reason);
                ChatHistoryEntryDto::ActionRejected {
                    role,
                    command,
                    reason: reason_text,
                }
            }
            ssh_manager_core::ai::MessageContent::WebActivity(activity) => {
                ChatHistoryEntryDto::WebActivity {
                    role,
                    activity: WebActivityDto::from(&activity),
                }
            }
            // Issue #504: label and media only, one entry per stored part.
            ssh_manager_core::ai::MessageContent::Extension(part) => {
                ChatHistoryEntryDto::ExtensionParts {
                    role,
                    items: vec![ChatHistoryExtensionPartDto {
                        label: part.label,
                        media: part.media,
                    }],
                }
            }
            ssh_manager_core::ai::MessageContent::Question {
                question_id,
                question,
                options,
                multi_select,
            } => ChatHistoryEntryDto::Question {
                role,
                question_id: question_id.to_string(),
                question,
                options: options.iter().map(QuestionOptionDto::from).collect(),
                multi_select,
            },
            // Issue #505: sealed content exists only in a send copy and
            // never reaches the history view; show label only if it does.
            ssh_manager_core::ai::MessageContent::Sealed(sealed) => {
                ChatHistoryEntryDto::ExtensionParts {
                    role,
                    items: vec![ChatHistoryExtensionPartDto {
                        label: sealed.label().to_string(),
                        media: ssh_manager_core::ai::ExtensionMedia::Text,
                    }],
                }
            }
        }
    }
}

// --- Spec 0020, Abschnitt 5: Manueller Dateibrowser ---------------------
