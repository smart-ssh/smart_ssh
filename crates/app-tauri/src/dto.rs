//! DTOs für die Tauri-IPC-Grenze (Spec 0007, Abschnitt 8.2). Bewusst
//! getrennt von den `core`/`persistence-sqlite`-Domänentypen: das Frontend
//! soll nie mehr sehen als es braucht (insbesondere nie `credential_ref`
//! oder gar den API-Key selbst, s. `AiProviderConfigDto`-Doc-Kommentar
//! unten) und nie an interne Persistenz-Repräsentation gekoppelt sein.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use persistence_sqlite::{
    AiProviderConfig, AiProviderConfigUpdate, ChatSessionSummary, StoredRule,
};
use ssh_manager_core::ai::{ProviderId, ProviderType};
use ssh_manager_core::filter::{
    Decision, EvalContext, EvaluationTrace, Pattern, RuleAction, RuleId, Scope,
};
use ssh_manager_core::profiles::{
    AuthMethod, CredentialRef, CredentialStore, Group, GroupId, NoteEditor, NoteRevision,
    PostIngestPolicy, Server,
};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::RemoteEntry;

use crate::events::ConnectionStatus;
use crate::server_credentials::sudo_password_credential_ref;
use crate::state::SessionId;

/// Sicht auf einen [`Server`] für Liste und Bearbeiten-Formular (Spec
/// 0007 Abschnitt 7 zunächst nur für die Liste eingeführt, Spec 0008
/// Abschnitt 4 erweitert sie um die für das Formular nötigen Felder,
/// inkl. `notes` — analog zu [`GroupDto`]s `notes`-Feld: ohne dieses
/// Feld bräuchte das Server-Formular einen eigenen Befehl nur für die
/// Notiz-Vorbefüllung, obwohl `get_server`/`list_servers` ohnehin schon
/// die volle `Server`-Struktur lesen. **Kein** Secret-Inhalt — nur
/// [`AuthMethodKind`], welche Methode aktiv ist, nie ein `CredentialRef`
/// oder gar das Secret selbst (Spec 0008 Abschnitt 4: "ServerDto ...
/// enthält keine Secret-Felder").
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDto {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub group_id: Option<String>,
    pub tags: Vec<String>,
    pub auth_kind: AuthMethodKind,
    pub jump_host: Option<String>,
    pub notes: String,
    /// Spec 0018, Abschnitt 4: ob für diesen Server ein Sudo-Passwort im
    /// `CredentialStore` hinterlegt ist — **nicht** aus `Server` selbst
    /// ableitbar (kein DB-Feld, s. `crate::server_credentials`-Doc-
    /// Kommentar), deshalb kein `From<&Server>`, sondern
    /// [`ServerDto::from_server`] mit explizitem `CredentialStore`-Zugriff.
    pub has_sudo_password: bool,
    /// Spec 0032, Abschnitt 3: `true` genau für den lokalen Pseudo-Server
    /// (`crate::local_server::LOCAL_SERVER_ID`) — steuert im Frontend, ob
    /// Host/Port/Nutzername/Auth/Jump-Host/Löschen/Verbindungstest
    /// ausgeblendet werden.
    pub is_local: bool,
    /// Spec 0039, Abschnitt 5.1.
    pub post_ingest_policy: PostIngestPolicy,
    /// Spec 0039, Abschnitt 5.2. Das Frontend gated die zugehörige
    /// Checkbox zusätzlich anhand der app-weiten Zweitmeinungs-Einstellung
    /// (`riskSettings.ts`), unabhängig von diesem Feld.
    pub ai_injection_check_enabled: bool,
}

impl ServerDto {
    pub fn from_server(server: &Server, credential_store: &dyn CredentialStore) -> Self {
        Self {
            id: server.id.0.to_string(),
            name: server.name.clone(),
            host: server.host.clone(),
            port: server.port,
            username: server.username.clone(),
            group_id: server.group_id.map(|g| g.0.to_string()),
            tags: server.tags.clone(),
            auth_kind: AuthMethodKind::from(&server.auth),
            jump_host: server.jump_host.map(|j| j.0.to_string()),
            notes: server.notes.clone(),
            has_sudo_password: credential_store
                .get(&sudo_password_credential_ref(server.id))
                .is_ok(),
            is_local: crate::local_server::is_local(server.id),
            post_ingest_policy: server.post_ingest_policy,
            ai_injection_check_enabled: server.ai_injection_check_enabled,
        }
    }
}

/// Welche [`AuthMethod`]-Variante aktiv ist, ohne deren Inhalt (Spec 0008
/// Abschnitt 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethodKind {
    Password,
    PrivateKey,
    Agent,
    Certificate,
}

impl From<&AuthMethod> for AuthMethodKind {
    fn from(auth: &AuthMethod) -> Self {
        match auth {
            AuthMethod::Password { .. } => AuthMethodKind::Password,
            AuthMethod::PrivateKey { .. } => AuthMethodKind::PrivateKey,
            AuthMethod::Agent => AuthMethodKind::Agent,
            AuthMethod::Certificate { .. } => AuthMethodKind::Certificate,
        }
    }
}

/// Spec 0007, Abschnitt 8.2 — **bewusst KEIN `api_key`-Feld**, der Key geht
/// nie zurück ans Frontend. `credential_ref` ist zwar selbst kein Secret,
/// bleibt aber ebenfalls draußen: reines Backend-Implementierungsdetail,
/// das das Frontend für nichts in Teil 1 braucht.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderConfigDto {
    pub id: ProviderId,
    pub provider_type: ProviderType,
    pub display_name: String,
    pub base_url: Option<String>,
    pub model: String,
    pub supports_native_tool_calling: bool,
    pub is_active: bool,
    /// Spec 0025, Abschnitt 3.
    pub extra_headers: Vec<(String, String)>,
    /// Spec 0025, Abschnitt 4.
    pub attestation_url: Option<String>,
}

impl From<&AiProviderConfig> for AiProviderConfigDto {
    fn from(config: &AiProviderConfig) -> Self {
        Self {
            id: config.id,
            provider_type: config.provider_type,
            display_name: config.display_name.clone(),
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            supports_native_tool_calling: config.supports_native_tool_calling,
            is_active: config.is_active,
            extra_headers: config.extra_headers.clone(),
            attestation_url: config.attestation_url.clone(),
        }
    }
}

/// Spec 0007, Abschnitt 8.2. `api_key` wird nie persistiert, nur an den
/// `CredentialStore` weitergereicht (`add_ai_provider`) bzw. bei
/// `update_ai_provider` interpretiert: leer = Credential unverändert
/// lassen (s. `crate::commands::update_ai_provider`-Doc-Kommentar).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderConfigInput {
    pub provider_type: ProviderType,
    pub display_name: String,
    pub base_url: Option<String>,
    pub model: String,
    pub supports_native_tool_calling: bool,
    pub api_key: String,
    /// Spec 0025, Abschnitt 3.
    pub extra_headers: Vec<(String, String)>,
    /// Spec 0025, Abschnitt 4.
    pub attestation_url: Option<String>,
}

impl AiProviderConfigInput {
    /// Baut die volle [`AiProviderConfig`] für [`persistence_sqlite::SqliteAiProviderStore::create`]
    /// — `id`/`credential_ref` werden hier frisch vergeben (ein Aufruf pro
    /// `add_ai_provider`, s. Spec Abschnitt 8.2: "Backend generiert eine
    /// neue `ProviderId`, erzeugt daraus einen `CredentialRef`").
    pub fn into_new_config(self, id: ProviderId) -> AiProviderConfig {
        let now = Utc::now();
        AiProviderConfig {
            id,
            provider_type: self.provider_type,
            display_name: self.display_name,
            base_url: self.base_url,
            model: self.model,
            supports_native_tool_calling: self.supports_native_tool_calling,
            credential_ref: credential_ref_for(id),
            is_active: false,
            extra_headers: self.extra_headers,
            attestation_url: self.attestation_url,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn into_update(self, id: ProviderId) -> AiProviderConfigUpdate {
        AiProviderConfigUpdate {
            id,
            provider_type: self.provider_type,
            display_name: self.display_name,
            base_url: self.base_url,
            model: self.model,
            supports_native_tool_calling: self.supports_native_tool_calling,
            extra_headers: self.extra_headers,
            attestation_url: self.attestation_url,
            updated_at: Utc::now(),
        }
    }
}

/// `CredentialRef`-Schema exakt wie in Spec 0007 Abschnitt 8.2 vorgegeben:
/// `"ai-provider:{id}"`.
pub fn credential_ref_for(id: ProviderId) -> CredentialRef {
    CredentialRef::new(format!("ai-provider:{}", id.0))
}

/// Antwort auf `host-key-verification-needed` (Spec 0007, Abschnitt 4:
/// `confirm_host_key(session_id, decision: Trust | Reject)`).
///
/// `#[serde(tag = "decision", rename_all = "camelCase")]` statt Serdes
/// Standard-Außen-Tagging: ergibt `{"decision": "trust"}` statt
/// `"Trust"`/`{"Trust": null}` — für das TypeScript-Frontend die
/// natürlichere Form, um diesen Wert selbst zu konstruieren.
#[derive(Debug, Clone, Deserialize)]
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
    EditThenApprove { command: String },
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

/// Flache Sicht auf eine [`Group`] (Spec 0008, Abschnitt 3 — "Baum wird im
/// Frontend gebaut"). `notes` ist dort nicht ausdrücklich erwähnt, aber
/// auch nicht ausgeschlossen — ohne dieses Feld bräuchte das
/// Gruppen-Formular einen eigenen `get_group`-Befehl nur für die
/// Notiz-Vorbefüllung, obwohl `list_groups()` die Daten ohnehin schon aus
/// derselben `Group`-Struktur liest. Kein zusätzlicher DB-Zugriff, nur ein
/// zusätzliches Feld auf einem bereits geladenen Wert.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDto {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub notes: String,
}

impl From<&Group> for GroupDto {
    fn from(group: &Group) -> Self {
        Self {
            id: group.id.0.to_string(),
            name: group.name.clone(),
            parent_id: group.parent_id.map(|p| p.0.to_string()),
            notes: group.notes.clone(),
        }
    }
}

/// Vorschau bzw. Ergebnis von `delete_group` (Spec 0008, Abschnitt 3).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteGroupResult {
    pub child_groups_to_delete: Vec<GroupDto>,
    pub servers_to_unassign: Vec<ServerDto>,
    pub executed: bool,
}

/// Eingabe für `create_server`/`update_server`/`test_connection` (Spec
/// 0008, Abschnitt 4). `group_id`/`jump_host` direkt als `GroupId`/
/// `ServerId` statt `String` — beide sind `Uuid`-Newtypes und
/// (de-)serialisieren bereits als reiner UUID-String, ein manuelles
/// Parsen im Command-Handler wäre nur Boilerplate.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInput {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub group_id: Option<GroupId>,
    pub tags: Vec<String>,
    pub auth: AuthMethodInput,
    pub jump_host: Option<ServerId>,
    /// Spec 0018, Abschnitt 4: "leer/fehlend = unverändert" bei
    /// `update_server` (s. `crate::server_credentials::resolve_sudo_password`),
    /// "nicht gesetzt" bei `create_server`. Explizites Entfernen eines
    /// bereits gesetzten Werts läuft über den eigenen
    /// `clear_server_sudo_password`-Befehl, nicht über dieses Feld.
    pub sudo_password: Option<String>,
    /// Spec 0039, Abschnitt 5.1. `#[serde(default)]`: fehlt das Feld (z. B.
    /// ein älterer Frontend-Build), gilt derselbe Default wie beim
    /// Migrations-Spaltendefault (`Balanced`), kein harter Fehler.
    #[serde(default)]
    pub post_ingest_policy: PostIngestPolicy,
    /// Spec 0039, Abschnitt 5.2. `#[serde(default)]`: fehlendes Feld ->
    /// `false`, derselbe Default wie der Migrations-Spaltendefault.
    #[serde(default)]
    pub ai_injection_check_enabled: bool,
}

/// Spec 0008, Abschnitt 4. `#[serde(tag = "kind", rename_all =
/// "camelCase")]` (wie bei [`ActionUserDecision`]/[`HostKeyUserDecision`])
/// statt Serdes Standard-Außen-Tagging — ergibt z. B. `{"kind":
/// "privateKey", "keyContent": "...", "passphrase": null}`, die
/// natürliche Form für das TypeScript-Frontend, dieses Objekt selbst zu
/// bauen.
#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AuthMethodInput {
    Password {
        value: Option<String>,
    },
    PrivateKey {
        key_content: Option<String>,
        passphrase: Option<String>,
    },
    Agent,
    Certificate {
        cert_content: Option<String>,
        key_content: Option<String>,
    },
}

/// Wer eine [`NoteRevision`] erzeugt hat, für die Anzeige in der
/// Notiz-Historie (Spec 0008, Abschnitt 6: "Editor (Nutzer, oder KI inkl.
/// Provider/Modell-Name)").
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NoteEditorDto {
    User,
    Ai { provider: String, model: String },
}

impl From<&NoteEditor> for NoteEditorDto {
    fn from(editor: &NoteEditor) -> Self {
        match editor {
            NoteEditor::User => NoteEditorDto::User,
            NoteEditor::Ai { provider, model } => NoteEditorDto::Ai {
                provider: provider.clone(),
                model: model.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteRevisionDto {
    pub id: String,
    pub content: String,
    pub edited_by: NoteEditorDto,
    pub created_at: String,
}

impl From<&NoteRevision> for NoteRevisionDto {
    fn from(revision: &NoteRevision) -> Self {
        Self {
            id: revision.id.to_string(),
            content: revision.content.clone(),
            edited_by: NoteEditorDto::from(&revision.edited_by),
            created_at: revision.created_at.to_rfc3339(),
        }
    }
}

/// Spec 0008, Abschnitt 7. Weicht in zwei Punkten von der Spec-Skizze ab:
///
/// 1. `NetworkError(String)` wurde zu `NetworkError { message: String }`
///    — ein Tupel-Variant lässt sich unter internem Tagging
///    (`#[serde(tag = "kind")]`, s. o.) nicht darstellen (serde verlangt
///    dafür Struct-artige Varianten).
/// 2. `HostKeyUnknown`/`HostKeyMismatch` tragen zusätzlich `host`/`port`/
///    `raw_key` — die Spec-Skizze nennt dort nur die Fingerprints. Ohne
///    die Rohdaten hätte das Frontend keine Möglichkeit, nach einer
///    Nutzerbestätigung `trust_host_key` (neuer, in der Spec nicht
///    vorgesehener Befehl, s. Doc-Kommentar dort) aufzurufen — die Spec
///    selbst verlangt aber ausdrücklich, dass "bei Zustimmung `trust()`
///    aufgerufen" werden kann (Abschnitt 7).
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TestConnectionResult {
    Success,
    AuthenticationFailed,
    HostKeyUnknown {
        host: String,
        port: u16,
        raw_key: Vec<u8>,
        fingerprint: String,
    },
    HostKeyMismatch {
        host: String,
        port: u16,
        raw_key: Vec<u8>,
        expected_fingerprint: String,
        actual_fingerprint: String,
    },
    NetworkError {
        message: String,
    },
    Timeout,
}

// --- Spec 0009: Filter-Regel-Verwaltung ---------------------------------

/// Getrennt von `pattern_value` statt eines verschachtelten `Pattern` (Spec
/// 0009, Abschnitt 3 nennt `pattern_type`/`pattern_value` als eigene Felder
/// in `RuleInput`) — passt zum Formular aus Abschnitt 6: eine
/// Typ-Auswahl bestimmt, welches einzelne Eingabefeld angezeigt wird, zwei
/// flache Felder bilden das direkter ab als ein getaggtes Enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternType {
    Glob,
    Regex,
    Exact,
}

impl From<&Pattern> for PatternType {
    fn from(pattern: &Pattern) -> Self {
        match pattern {
            Pattern::Glob(_) => PatternType::Glob,
            Pattern::Regex(_) => PatternType::Regex,
            Pattern::Exact(_) => PatternType::Exact,
        }
    }
}

fn pattern_from_parts(pattern_type: PatternType, pattern_value: String) -> Pattern {
    match pattern_type {
        PatternType::Glob => Pattern::Glob(pattern_value),
        PatternType::Regex => Pattern::Regex(pattern_value),
        PatternType::Exact => Pattern::Exact(pattern_value),
    }
}

/// Formular-Eingabe für `create_rule`/`update_rule` (Spec 0009, Abschnitt
/// 3). `action`/`scope` sind bewusst direkt `core::filter::{RuleAction,
/// Scope}` statt separater `ScopeInput`-artiger Typen, wie die Spec-Skizze
/// nahelegt: beide sind bereits `Serialize + Deserialize` und werden
/// bereits unverändert über dieselbe IPC-Grenze gereicht (z. B. `Decision`
/// in `chat-action-proposed`) — ein strukturell identischer Zweit-Typ hätte
/// hier keinen Mehrwert. Ebenso wird die in der Spec-Skizze separat
/// benannte `ScopeFilter` (mit zusätzlichem `All`-Wert) nicht eingeführt:
/// `list_rules(scope_filter: Option<Scope>)` deckt "alle Regeln" bereits
/// über `None` ab, ein zusätzlicher `All`-Wert wäre redundant. Siehe
/// ADR-Vorschlag am Ende der Aufgabe.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleInput {
    pub pattern_type: PatternType,
    pub pattern_value: String,
    pub action: RuleAction,
    pub scope: Scope,
    pub priority: i32,
}

impl RuleInput {
    pub fn into_stored_rule(
        self,
        id: RuleId,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> StoredRule {
        StoredRule {
            id,
            pattern: pattern_from_parts(self.pattern_type, self.pattern_value),
            action: self.action,
            scope: self.scope,
            priority: self.priority,
            created_at,
            updated_at,
        }
    }
}

/// Sicht auf eine gespeicherte Regel für Liste und Bearbeiten-Formular
/// (Spec 0009, Abschnitt 3/6) — spiegelt `RuleInput`s Feldaufteilung
/// (`pattern_type`/`pattern_value` statt `pattern`), damit sich eine
/// `RuleDto` ohne Umformung direkt als Formular-Vorbefüllung eignet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleDto {
    pub id: RuleId,
    pub pattern_type: PatternType,
    pub pattern_value: String,
    pub action: RuleAction,
    pub scope: Scope,
    pub priority: i32,
}

impl From<&StoredRule> for RuleDto {
    fn from(rule: &StoredRule) -> Self {
        Self {
            id: rule.id.clone(),
            pattern_type: PatternType::from(&rule.pattern),
            pattern_value: rule.pattern.display_text().to_string(),
            action: rule.action.clone(),
            scope: rule.scope.clone(),
            priority: rule.priority,
        }
    }
}

/// Read-only-Anzeige eines Hard-Blacklist-Musters (Spec 0009, Abschnitt 3:
/// `list_hard_blacklist`). Ein flaches `{kind, value}`-Struct statt eines
/// getaggten `Pattern`-Enums — vermeidet die `rename_all_fields`-Klasse von
/// Fehlern (s. `crate::dto::tests`, `AuthMethodInput` u. a.) von vornherein,
/// ganz ohne serde-Sonderfall nötig zu machen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternDto {
    pub kind: &'static str,
    pub value: String,
}

impl From<&Pattern> for PatternDto {
    fn from(pattern: &Pattern) -> Self {
        Self {
            kind: pattern.kind_str(),
            value: pattern.display_text().to_string(),
        }
    }
}

/// Eingabe für `evaluate_explained` (Spec 0009, Abschnitt 3). Eigener Typ
/// statt direkter Wiederverwendung von `core::filter::EvalContext`: Abschnitt
/// 6 verlangt eine **optionale** Server-Simulation im Testen-Panel
/// ("optionale Scope-Simulation"), `EvalContext.server_id` ist aber nicht
/// optional (folgerichtig — in der echten Kernschleife gibt es immer einen
/// verbundenen Server). `None` wird beim Umwandeln auf eine frische
/// `ServerId` abgebildet (kann garantiert keine `Scope::Server`-Regel
/// matchen), statt `EvalContext` selbst mit dieser reinen UI-Rücksicht zu
/// belasten.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalContextInput {
    pub server_id: Option<ServerId>,
    pub tags: Vec<String>,
}

impl From<EvalContextInput> for EvalContext {
    fn from(input: EvalContextInput) -> Self {
        Self {
            server_id: input.server_id.unwrap_or_default(),
            tags: input.tags,
        }
    }
}

/// Sicht auf eine [`EvaluationTrace`] für das Testen-Panel (Spec 0009,
/// Abschnitt 4/6).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationTraceDto {
    pub decision: Decision,
    pub matched_rule: Option<RuleId>,
    pub matched_hard_blacklist_entry: Option<String>,
    pub sub_command_traces: Vec<EvaluationTraceDto>,
}

impl From<EvaluationTrace> for EvaluationTraceDto {
    fn from(trace: EvaluationTrace) -> Self {
        Self {
            decision: trace.decision,
            matched_rule: trace.matched_rule,
            matched_hard_blacklist_entry: trace.matched_hard_blacklist_entry,
            sub_command_traces: trace
                .sub_command_traces
                .into_iter()
                .map(EvaluationTraceDto::from)
                .collect(),
        }
    }
}

// --- Spec 0011: Regel-Schnellvorschlag ----------------------------------

/// Ein Muster-Vorschlag für den Schnellvorschlag-Dropdown im
/// Bestätigungsdialog (Spec 0011, Abschnitt 2).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternSuggestionDto {
    /// Menschenlesbar, für die Dropdown-Anzeige.
    pub label: String,
    pub pattern_type: PatternType,
    pub pattern_value: String,
}

// --- Spec 0012: KI-generierte Dokumente ---------------------------------

/// Exportformat für `export_document` (Spec 0012, Abschnitt 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DocumentFormat {
    Markdown,
    Word,
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
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
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
    },
    ActionRejected {
        role: ChatHistoryRoleDto,
        command: String,
        reason: String,
    },
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
            } => ChatHistoryEntryDto::CommandResult {
                role,
                command,
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: output.exit_code,
                cancelled,
            },
            ssh_manager_core::ai::MessageContent::ActionRejected { command, reason } => {
                let reason_text = match reason {
                    ssh_manager_core::ai::RejectionReason::User => {
                        "Vom Nutzer abgelehnt.".to_string()
                    }
                    ssh_manager_core::ai::RejectionReason::Blocked(reason) => reason,
                };
                ChatHistoryEntryDto::ActionRejected {
                    role,
                    command,
                    reason: reason_text,
                }
            }
        }
    }
}

// --- Spec 0020, Abschnitt 5: Manueller Dateibrowser ---------------------

/// Sicht auf einen [`RemoteEntry`] für die Dateiliste im Dateibrowser (Spec
/// 0020, Abschnitt 5.1: "Name, Größe, Rechte, Änderungsdatum"). `permissions`
/// kommt bereits hier als lesbarer `rwxr-xr-x`-String statt als rohe Bits —
/// das Frontend braucht dafür keine eigene Unix-Rechte-Formatierungslogik.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntryDto {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: String,
    pub modified: Option<String>,
}

impl From<&RemoteEntry> for RemoteEntryDto {
    fn from(entry: &RemoteEntry) -> Self {
        Self {
            name: entry.name.clone(),
            path: entry.path.clone(),
            is_dir: entry.is_dir,
            size: entry.size,
            permissions: format_unix_permissions(entry.permissions),
            modified: entry.modified.map(|dt| dt.to_rfc3339()),
        }
    }
}

/// Formatiert reine Unix-Rechte-Bits (keine Typ-Bits, s.
/// `RemoteEntry::permissions`-Doc-Kommentar in `core`) als klassischen
/// `rwxr-xr-x`-String.
fn format_unix_permissions(bits: u32) -> String {
    let triplet = |shift: u32| -> String {
        let r = if bits & (0o4 << shift) != 0 { 'r' } else { '-' };
        let w = if bits & (0o2 << shift) != 0 { 'w' } else { '-' };
        let x = if bits & (0o1 << shift) != 0 { 'x' } else { '-' };
        [r, w, x].iter().collect()
    };
    format!("{}{}{}", triplet(6), triplet(3), triplet(0))
}

/// Sortiert Verzeichniseinträge für die Anzeige: Verzeichnisse zuerst, dann
/// alphabetisch nach Name (case-insensitiv) — Spec 0020 macht dazu keine
/// Vorgabe, das ist die in Dateibrowsern übliche Konvention.
pub fn sort_remote_entries(entries: &mut [RemoteEntryDto]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[cfg(test)]
mod tests {
    //! Regressionstest für einen tatsächlich aufgetretenen Bug: serdes
    //! `#[serde(tag = "...", rename_all = "camelCase")]` auf einem Enum
    //! wandelt nur die Varianten-/Tag-Namen in camelCase um, **nicht** die
    //! Feldnamen innerhalb der Struct-artigen Varianten (empirisch
    //! verifiziert — `rename_all` ohne `rename_all_fields` lässt
    //! `raw_key`/`key_content`/`exit_code` etc. unverändert). Das führte im
    //! echten Betrieb dazu, dass `trust_host_key` nach einem
    //! `test_connection`-Host-Key-Ereignis mit "missing required key
    //! rawKey" fehlschlug, weil das Frontend `rawKey` erwartete, die
    //! Payload aber `raw_key` enthielt. Jeder betroffene Typ braucht
    //! zusätzlich `rename_all_fields = "camelCase"` — diese Tests fixieren
    //! die tatsächliche JSON-Form, damit eine künftige Änderung so einen
    //! Typ nicht wieder lautlos in diesen Zustand zurückfallen lässt.

    use super::*;

    #[test]
    fn test_test_connection_result_host_key_unknown_uses_camel_case_fields() {
        let value = TestConnectionResult::HostKeyUnknown {
            host: "example.invalid".to_string(),
            port: 22,
            raw_key: vec![1, 2, 3],
            fingerprint: "SHA256:abc".to_string(),
        };
        let json = serde_json::to_value(&value).unwrap();

        assert_eq!(json["kind"], "hostKeyUnknown");
        assert_eq!(json["rawKey"], serde_json::json!([1, 2, 3]));
        assert!(
            json.get("raw_key").is_none(),
            "raw_key darf nicht mehr im snake_case vorkommen"
        );
    }

    #[test]
    fn test_test_connection_result_host_key_mismatch_uses_camel_case_fields() {
        let value = TestConnectionResult::HostKeyMismatch {
            host: "example.invalid".to_string(),
            port: 22,
            raw_key: vec![9],
            expected_fingerprint: "SHA256:old".to_string(),
            actual_fingerprint: "SHA256:new".to_string(),
        };
        let json = serde_json::to_value(&value).unwrap();

        assert_eq!(json["expectedFingerprint"], "SHA256:old");
        assert_eq!(json["actualFingerprint"], "SHA256:new");
    }

    #[test]
    fn test_auth_method_input_private_key_deserializes_from_camel_case() {
        let json = serde_json::json!({
            "kind": "privateKey",
            "keyContent": "key-data",
            "passphrase": null,
        });

        let input: AuthMethodInput = serde_json::from_value(json).unwrap();

        assert!(matches!(
            input,
            AuthMethodInput::PrivateKey { key_content: Some(k), passphrase: None } if k == "key-data"
        ));
    }

    #[test]
    fn test_auth_method_input_certificate_deserializes_from_camel_case() {
        let json = serde_json::json!({
            "kind": "certificate",
            "certContent": "cert-data",
            "keyContent": "key-data",
        });

        let input: AuthMethodInput = serde_json::from_value(json).unwrap();

        assert!(matches!(
            input,
            AuthMethodInput::Certificate { cert_content: Some(c), key_content: Some(k) }
                if c == "cert-data" && k == "key-data"
        ));
    }

    #[test]
    fn test_format_unix_permissions_formats_rwx_triplets() {
        assert_eq!(format_unix_permissions(0o755), "rwxr-xr-x");
        assert_eq!(format_unix_permissions(0o644), "rw-r--r--");
        assert_eq!(format_unix_permissions(0o600), "rw-------");
        assert_eq!(format_unix_permissions(0), "---------");
    }

    fn dummy_entry(name: &str, is_dir: bool) -> RemoteEntryDto {
        RemoteEntryDto {
            name: name.to_string(),
            path: format!("/{name}"),
            is_dir,
            size: 0,
            permissions: String::new(),
            modified: None,
        }
    }

    #[test]
    fn test_sort_remote_entries_lists_directories_first_then_alphabetically() {
        let mut entries = vec![
            dummy_entry("zebra.txt", false),
            dummy_entry("Apps", true),
            dummy_entry("alpha.txt", false),
            dummy_entry("bin", true),
        ];

        sort_remote_entries(&mut entries);

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Apps", "bin", "alpha.txt", "zebra.txt"]);
    }
}
