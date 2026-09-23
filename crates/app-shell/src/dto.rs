//! DTOs für die Tauri-IPC-Grenze (Spec 0007, Abschnitt 8.2). Bewusst
//! getrennt von den `core`/`persistence-sqlite`-Domänentypen: das Frontend
//! soll nie mehr sehen als es braucht (insbesondere nie `credential_ref`
//! oder gar den API-Key selbst, s. `AiProviderConfigDto`-Doc-Kommentar
//! unten) und nie an interne Persistenz-Repräsentation gekoppelt sein.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use credentials_keyring::{KeychainAvailability, KeychainUnavailableReason};
use persistence_sqlite::{
    AiProviderConfig, AiProviderConfigUpdate, ChatSessionSummary, StoredRule,
};
use ssh_manager_core::ai::{ProviderId, ProviderType};
use ssh_manager_core::filter::{
    Decision, EvalContext, EvaluationTrace, Pattern, RuleAction, RuleId, Scope,
};
use ssh_manager_core::profiles::{
    trim_credential_value, AuthMethod, CredentialError, CredentialRef, CredentialStore, Group,
    GroupId, NoteEditor, NoteRevision, PostIngestPolicy, Server,
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
    /// Spec 0071, A14/I4: `true`, wenn der Schlüsselbund **nicht sagen
    /// konnte**, ob ein Sudo-Passwort hinterlegt ist (`CredentialError::
    /// Backend`). Vorher lieferte `.is_ok()` in diesem Fall `false` — die
    /// Oberfläche behauptete also "kein Sudo-Passwort hinterlegt", obwohl
    /// sie es schlicht nicht wusste. "Unbekannt" ist nicht "nein".
    ///
    /// `has_sudo_password` ist dann ebenfalls `false`; die Oberfläche muss
    /// dieses Feld zuerst prüfen und einen neutralen Zustand anzeigen,
    /// statt eine Aussage zu treffen.
    pub sudo_password_unknown: bool,
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
    /// Spec 0067, A2: `None` = automatisch.
    pub sftp_server_path: Option<String>,
}

impl ServerDto {
    pub fn from_server(server: &Server, credential_store: &dyn CredentialStore) -> Self {
        // Spec 0071, A14/X4: drei Ausgänge statt zwei. Der Unterschied
        // zwischen "es gibt keinen Eintrag" und "der Schlüsselbund konnte
        // nicht antworten" steckt bereits im Fehlertyp — es braucht dafür
        // keinen zusätzlichen Blick auf den Schlüsselbund-Zustand (A16).
        let (has_sudo_password, sudo_password_unknown) =
            match credential_store.get(&sudo_password_credential_ref(server.id)) {
                Ok(_) => (true, false),
                Err(CredentialError::NotFound(_)) => (false, false),
                Err(CredentialError::Backend(_)) => (false, true),
            };
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
            has_sudo_password,
            sudo_password_unknown,
            is_local: crate::local_server::is_local(server.id),
            post_ingest_policy: server.post_ingest_policy,
            ai_injection_check_enabled: server.ai_injection_check_enabled,
            sftp_server_path: server.sftp_server_path.clone(),
        }
    }
}

/// Spec 0071, A15: der Schlüsselbund-Zustand für die Diagnose-Ansicht.
///
/// Bewusst nur ein Flag und eine Aufzählung — **kein** Fehlertext, keine
/// D-Bus-Adresse, kein Pfad (I1/A10). Die anzeigbaren Texte liegen im
/// Frontend-Übersetzungskatalog, nicht hier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeychainStatusDto {
    pub available: bool,
    /// `None`, wenn `available` — sonst der klassifizierte Grund.
    pub reason: Option<KeychainUnavailableReasonDto>,
}

/// Serialisierbare Fassung von
/// [`credentials_keyring::KeychainUnavailableReason`]. Eigener Typ statt
/// `Serialize` auf dem Core-Enum: `credentials-keyring` soll `serde` nicht
/// kennen müssen (dieselbe Trennung wie bei allen anderen DTOs hier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeychainUnavailableReasonDto {
    NoSessionBus,
    NoSecretServiceProvider,
    Locked,
    Unknown,
}

impl From<KeychainAvailability> for KeychainStatusDto {
    fn from(availability: KeychainAvailability) -> Self {
        match availability.unavailable_reason() {
            None => Self {
                available: true,
                reason: None,
            },
            Some(reason) => Self {
                available: false,
                reason: Some(match reason {
                    KeychainUnavailableReason::NoSessionBus => {
                        KeychainUnavailableReasonDto::NoSessionBus
                    }
                    KeychainUnavailableReason::NoSecretServiceProvider => {
                        KeychainUnavailableReasonDto::NoSecretServiceProvider
                    }
                    KeychainUnavailableReason::Locked => KeychainUnavailableReasonDto::Locked,
                    KeychainUnavailableReason::Unknown => KeychainUnavailableReasonDto::Unknown,
                }),
            },
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
    /// Spec 0076, A-1. Wie die übrigen Varianten **ohne** Inhalt — der
    /// Pfad reist in einem eigenen Feld des [`ServerDto`] (B-4), damit
    /// dieses Enum weiterhin nur „welche Methode" sagt.
    IdentityFile,
}

impl From<&AuthMethod> for AuthMethodKind {
    fn from(auth: &AuthMethod) -> Self {
        match auth {
            AuthMethod::Password { .. } => AuthMethodKind::Password,
            AuthMethod::PrivateKey { .. } => AuthMethodKind::PrivateKey,
            AuthMethod::Agent => AuthMethodKind::Agent,
            AuthMethod::Certificate { .. } => AuthMethodKind::Certificate,
            AuthMethod::IdentityFile { .. } => AuthMethodKind::IdentityFile,
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
    /// Spec 0065, Teil 4: `None` = „Automatisch" (Default) im Formular.
    pub max_tokens_override: Option<u32>,
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
            max_tokens_override: config.max_tokens_override,
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
    /// Spec 0065, Teil 4: `None` = „Automatisch" — validiert in
    /// [`Self::validate_max_tokens_override`] (positiv, sinnvolle
    /// Obergrenze), NICHT hier im reinen Datentyp (derselbe Grund wie bei
    /// `trimmed()`: eine
    /// Deserialize-Quelle kennt keine fachliche Validierung).
    pub max_tokens_override: Option<u32>,
}

/// Spec 0065, Teil 4: sinnvolle Obergrenze für den Override — großzügig
/// über dem höchsten heute bekannten Modell-Maximum (128K, s.
/// `ai_providers::anthropic_model_max_output_tokens`/`openai_compatible_
/// model_max_output_tokens`), damit ein zukünftig größeres Modell nicht
/// sofort an dieser UI-Grenze scheitert, aber klein genug, um einen
/// offensichtlichen Tippfehler (z. B. eine zusätzliche Null) abzufangen.
pub const MAX_TOKENS_OVERRIDE_UPPER_BOUND: u32 = 1_000_000;

impl AiProviderConfigInput {
    /// Spec 0049, Fund 1: rand-trimmt `api_key` und die Endpunkt-Felder
    /// (`base_url`, `attestation_url`) — führende/nachfolgende Whitespaces
    /// inkl. `\r`/`\n`/Tabs, wie sie ein Copy-Paste unter Windows an einen
    /// eingefügten API-Key oder eine eingefügte URL anhängt. Zentrale
    /// Stelle statt UI-verstreutem Trimmen: jeder Aufrufer von
    /// `add_ai_provider`/`update_ai_provider`/`discover_models` ruft dies
    /// als Erstes auf `config` auf, bevor der Wert irgendwo verwendet
    /// wird — greift damit unabhängig vom Eingabeweg (Paste, Tippen,
    /// später Import). Nur der Rand wird angefasst, der Inhalt (auch ein
    /// `Some("")` nach dem Trimmen) bleibt unverändert — dieselbe "leer ==
    /// unverändert lassen"-Semantik wie vor dem Trimmen gilt unangetastet
    /// weiter, das ist nicht Teil dieses Funds.
    ///
    /// Spec 0073, A3: benutzt statt `str::trim` den geteilten
    /// [`trim_credential_value`] aus `core` — dieselbe Semantik wie
    /// bisher, zusätzlich fallen unsichtbare Randzeichen ohne
    /// `White_Space`-Eigenschaft weg (BOM, Zero-Width-Space und die
    /// weiteren aus `INVISIBLE_CREDENTIAL_EDGE_CHARS`), wie sie beim
    /// Kopieren aus einer Datei mit BOM oder von einer Webseite an einem
    /// Key hängen. Die Spec nennt genau diese drei Felder; `display_name`
    /// ist ein Anzeigetext und bleibt unangetastet (T12).
    ///
    /// `model` bleibt hier ebenfalls unangetastet — das ist **kein**
    /// Ergebnis dieser Spec, sondern der Stand vor ihr. Ob ein Modellname
    /// mit Randzeichen getrimmt gehört (er geht in jeden Provider-Request
    /// und in `provider_identity_key`, ein Randzeichen ergibt also ein
    /// `AI_MODEL_NOT_FOUND` ohne sichtbaren Grund), ist eine offene
    /// Produktfrage, s. ADR 0064.
    pub fn trimmed(mut self) -> Self {
        self.api_key = trim_credential_value(&self.api_key);
        self.base_url = self.base_url.map(|v| trim_credential_value(&v));
        self.attestation_url = self.attestation_url.map(|v| trim_credential_value(&v));
        self
    }

    /// Spec 0065, Teil 4: „positive Zahl, sinnvolle Obergrenze; leer =
    /// automatisch" — `0` ist ausdrücklich UNGÜLTIG (die Anthropic-API
    /// verlangt `max_tokens >= 1`, ein `Some(0)` wäre außerdem nicht von
    /// „kein Override" unterscheidbar gewesen, hätte diese Funktion `0`
    /// stillschweigend akzeptiert). Aufrufer: `commands::add_ai_provider`/
    /// `update_ai_provider`, VOR dem Erreichen von `into_new_config`/
    /// `into_update` — ein Store-Layer, der nie ein ungültiges
    /// `max_tokens_override` sieht, statt einer Validierung tief in der
    /// Persistenz.
    pub fn validate_max_tokens_override(&self) -> Result<(), String> {
        match self.max_tokens_override {
            None => Ok(()),
            Some(0) => Err("Max. Antwortlänge muss größer als 0 sein".to_string()),
            Some(value) if value > MAX_TOKENS_OVERRIDE_UPPER_BOUND => Err(format!(
                "Max. Antwortlänge darf {MAX_TOKENS_OVERRIDE_UPPER_BOUND} Tokens nicht überschreiten"
            )),
            Some(_) => Ok(()),
        }
    }

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
            max_tokens_override: self.max_tokens_override,
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
            max_tokens_override: self.max_tokens_override,
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

/// Vorschau bzw. Ergebnis von `delete_server` (Spec 0046, Fund 1 — analog
/// zu [`DeleteGroupResult`]). `server` trägt bereits `authKind`/
/// `hasSudoPassword` (s. [`ServerDto::from_server`]), das Frontend kann
/// daraus ableiten, welche Keychain-Secrets beim Löschen entfernt würden,
/// ohne dass diese DTO die Secrets selbst benennen muss.
/// `servers_losing_jump_host` sind die anderen Server, deren `jump_host`
/// beim tatsächlichen Löschen still auf `NULL` gesetzt würde.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteServerResult {
    pub server: ServerDto,
    pub servers_losing_jump_host: Vec<ServerDto>,
    pub executed: bool,
    /// Spec 0071, A17: Secrets, die beim Löschen **nicht** aus dem
    /// Schlüsselbund entfernt werden konnten (nicht verfügbarer oder
    /// gesperrter Schlüsselbund). Der Server ist trotzdem gelöscht — das
    /// ist die bewusste Entscheidung aus A17, damit niemand auf einem
    /// unlöschbaren Server sitzen bleibt.
    ///
    /// Die Einträge sind danach **verwaist**: Sie tragen die ID eines
    /// Servers, den es nicht mehr gibt. Deshalb stehen hier die
    /// `CredentialRef`-Strings selbst — der Nutzer braucht sie, um die
    /// Einträge im Schlüsselbund-Verwaltungsprogramm wiederzufinden. Das
    /// ist **kein** Secret (s. `CredentialRef`-Doc-Kommentar), nur der
    /// Account-Name innerhalb des Service „Smart SSH".
    ///
    /// Leer im Normalfall und immer leer bei `executed: false` (dann wurde
    /// nichts gelöscht).
    pub secrets_left_behind: Vec<String>,
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
    /// Spec 0067, A2: Override für den `sftp-server`-Pfad im erhöhten
    /// Dateibrowser. Leer/fehlend = automatisch (s.
    /// [`normalize_sftp_server_path`]).
    #[serde(default)]
    pub sftp_server_path: Option<String>,
}

/// Spec 0067, A2: leerer Override = automatisch (`None`); sonst muss es ein
/// sicherer absoluter Pfad sein — er landet in einem `sudo`-Kommando und in
/// der angezeigten sudoers-Zeile.
///
// ANNAHME A-1 (Q-BL-0149-01): Diese Stelle steht in der §1-Tabelle von Spec
// 0073, ist aber kein Zugangsdaten-Wert, sondern ein Pfad-Override für den
// erhöhten Dateibrowser — er landet in einem `sudo`-Kommando. Sie bleibt
// deshalb vorerst bei `str::trim`, bis die Frage entschieden ist (ADR 0064).
//
// Der Unterschied ist **nicht** kosmetisch: `"/usr/lib/sftp-server\u{200B}"`
// wird heute abgelehnt (das ZWSP übersteht `str::trim` und fällt dann durch
// `is_plausible_sftp_server_path`), mit dem Helfer würde derselbe Pfad
// bereinigt und angenommen. Die Prüfung selbst bleibt in beiden Fassungen
// unverändert und läuft in beiden Fassungen nach dem Trimmen, der
// angenommene Endwert ist also in jedem Fall voll validiert; es ändert sich
// aber, ob eine solche Eingabe als Fehler oder als bereinigter Pfad endet.
pub fn normalize_sftp_server_path(input: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = input else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if !ssh_manager_core::ssh::elevated::is_plausible_sftp_server_path(trimmed) {
        return Err(format!(
            "Ungültiger sftp-server-Pfad „{trimmed}“ — erlaubt ist ein absoluter Pfad aus \
             Buchstaben, Ziffern und / . _ - +, der auf „sftp-server“ endet"
        ));
    }
    Ok(Some(trimmed.to_string()))
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
    /// Spec 0076, A-1/B-2. Anders als bei den übrigen Varianten ist `path`
    /// **kein** Secret und **nicht** optional: „leer = unverändert lassen"
    /// gilt für Schlüsselbund-Slots, nicht für ein Klartextfeld, das
    /// ohnehin vorbefüllt aus dem `ServerDto` zurückkommt (B-4).
    ///
    /// `passphrase` verhält sich dagegen genau wie bei
    /// [`AuthMethodInput::PrivateKey`]: leer bedeutet unverändert (A-5).
    IdentityFile {
        path: String,
        passphrase: Option<String>,
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
        /// Spec 0069, Teil A4/E3: additiv, optional — ein altes Frontend
        /// ignoriert das Feld, ein unbekannter Code fällt im Frontend auf
        /// `message` zurück. Gesetzt aus `SshError::code()`.
        code: Option<&'static str>,
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

/// Exportformat für `export_document` (Spec 0012, Abschnitt 3). Seit Spec
/// 0037, Abschnitt 4, nur noch `Markdown` — der Word-Export wurde komplett
/// entfernt (kein Gating, s. `crate::document_export`-Moduldoc), nicht auf
/// eine leere `require(Feature::DocumentExport)`-Prüfung reduziert.
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
        /// Spec 0043, Fund A — s. `ActionResultPayload::Command.truncated`.
        truncated: bool,
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
                truncated: output.truncated,
            },
            ssh_manager_core::ai::MessageContent::ActionRejected { command, reason } => {
                let reason_text = match reason {
                    ssh_manager_core::ai::RejectionReason::User => {
                        "Vom Nutzer abgelehnt.".to_string()
                    }
                    ssh_manager_core::ai::RejectionReason::Blocked(reason) => reason,
                    // Spec 0046, Fund 4: eigener Text statt "Vom Nutzer
                    // abgelehnt" — es gibt hier gerade keinen Nutzer, der
                    // aktiv abgelehnt hätte.
                    ssh_manager_core::ai::RejectionReason::Timeout => {
                        "Zeitüberschreitung — nicht innerhalb der zulässigen Zeit beantwortet, \
                         automatisch abgelehnt."
                            .to_string()
                    }
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

/// Spec 0054, Teil 3: Vorschau vor dem Löschen eines Ordners — "X Dateien,
/// Y Ordner werden gelöscht" statt einer inhaltslosen Ja/Nein-Frage, analog
/// zum zweistufigen `delete_server`. `dir_count` zählt den Ordner selbst
/// mit (s. `crate::commands::walk_dirs_and_count_files`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePreviewDto {
    pub file_count: u64,
    pub dir_count: u64,
}

/// Spec 0067, A3: Ergebnis von `sftp_elevation_enable`. Ein Fehlschlag ist
/// kein `Err`, sondern `failure` — das Frontend zeigt dazu eine
/// verständliche Erklärung samt kopierbarer sudoers-Zeile.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElevationResultDto {
    pub active: bool,
    pub target_user: String,
    /// Ermittelter bzw. konfigurierter Pfad, sofern bekannt.
    pub sftp_server_path: Option<String>,
    pub failure: Option<ElevationFailureDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElevationFailureDto {
    pub kind: ElevationFailureKind,
    /// Zugeschnittene Zeile `<login> ALL=(<nutzer>) NOPASSWD: <pfad>` —
    /// nur, wenn eine fehlende sudo-Regel die Ursache ist.
    pub sudoers_line: Option<String>,
    /// Technisches Detail (erste stderr-Zeile o. ä.), nur zur Anzeige.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ElevationFailureKind {
    /// Lokaler Pseudo-Server o. ä. — kein erhöhter Modus möglich.
    Unsupported,
    InvalidUser,
    InvalidPath,
    SftpServerNotFound,
    PasswordRequired,
    NotAllowed,
    RequireTty,
    SudoMissing,
    CheckFailed,
    StartFailed,
}

/// Spec 0067, Teil B: Ergebnis eines Downloads für die Erfolgsmeldung —
/// wohin (für „Im Finder zeigen") und wie viele Dateien (Ordner-Download).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadResultDto {
    /// Lokaler Pfad der heruntergeladenen Datei bzw. des Ordners.
    pub local_path: String,
    pub is_dir: bool,
    pub file_count: u64,
}

/// Spec 0054, Teil 3: Ergebnis von `commands::read_local_text_preview` — die
/// lokale Seite der Upload-Überschreib-Diff-Vorschau. `text: None` bei einer
/// zu großen oder nicht als UTF-8 dekodierbaren Datei; `size` ist in jedem
/// Fall gesetzt (Grundlage für den Größenvergleich-Hinweis in diesem Fall).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalFilePreviewDto {
    pub text: Option<String>,
    pub size: u64,
}

/// Spec 0054, Teil 4: Ergebnis von `commands::sftp_open_for_editing` — der
/// lokale Pfad der Bearbeitungskopie plus die Baseline für die spätere
/// "hat sich die Remote-Datei seit dem Download geändert?"-Konflikt-Prüfung
/// vor dem Hochladen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditSessionDto {
    pub local_path: String,
    pub remote_modified: Option<String>,
}

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
    /// Spec 0054, Teil 2 (Eigenschaften-Dialog: "Rechte numerisch +
    /// symbolisch") — dieselben Bits wie `permissions`, nur numerisch
    /// (`0o644`-Stil) statt als `rwxr-xr-x`-String. Auch die Grundlage für
    /// den chmod-Dialog (Teil 3), der von einem numerischen Ausgangswert
    /// aus editiert statt den `permissions`-String zurückparsen zu müssen.
    pub permissions_octal: u32,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub owner: Option<String>,
    pub group: Option<String>,
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
            permissions_octal: entry.permissions,
            uid: entry.uid,
            gid: entry.gid,
            owner: entry.owner.clone(),
            group: entry.group.clone(),
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

/// Spec 0052, Abschnitt 1/3.2/3.3: Version + Commit-Hash + Edition für
/// Über-Dialog und Titelzeile — dasselbe DTO für beide, damit sich die
/// Anzeige nicht auseinanderentwickelt: `versionDisplay` ist bereits das
/// fertig formatierte `"0.4.1 (a5b3e01)"` (`crate::version::
/// version_with_hash`), die Titelzeile übernimmt es unverändert und hängt
/// nur noch ihren eigenen "— Early Access"-Zusatz an, statt den String
/// selbst aus `version`/`commitHash` neu zusammenzusetzen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoDto {
    /// Aus `tauri.conf.json` (Spec 0048), z. B. `"0.4.1"`.
    pub version: String,
    /// Kurzer Git-Commit-Hash (`crate::version::BUILD_COMMIT_HASH`), z. B.
    /// `"a5b3e01"`, oder `"unknown"` ohne Git zur Build-Zeit.
    pub commit_hash: String,
    /// Das geteilte Anzeigeformat aus Spec 0052, Abschnitt 1:
    /// `"0.4.1 (a5b3e01)"` — vorformatiert, damit Log/Über-Dialog/
    /// Titelzeile nicht je einen eigenen `format!`-Aufruf brauchen.
    pub version_display: String,
    /// `"Community"`/`"Official"` (aus `Wiring::edition`, Spec 0038) — als
    /// String statt eines eigenen Frontend-Enums, da das Frontend damit
    /// nur anzeigt, nie verzweigt (Edition-spezifisches Verhalten wird
    /// serverseitig über `Entitlements`/`Wiring` entschieden, nie im
    /// Frontend, s. CLAUDE.md "No special-casing").
    pub edition: String,
    /// s. [`crate::version::BuildType`].
    pub build_type: crate::version::BuildType,
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
            permissions_octal: 0,
            uid: None,
            gid: None,
            owner: None,
            group: None,
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

    // --- Spec 0049, Fund 1: Rand-Trimmen (Windows-Copy-Paste-`\r\n`) -------

    fn ai_provider_config_input(api_key: &str, base_url: Option<&str>) -> AiProviderConfigInput {
        AiProviderConfigInput {
            provider_type: ProviderType::Anthropic,
            display_name: "Test".to_string(),
            base_url: base_url.map(|s| s.to_string()),
            model: "claude-sonnet-5".to_string(),
            supports_native_tool_calling: true,
            api_key: api_key.to_string(),
            extra_headers: Vec::new(),
            attestation_url: None,
            max_tokens_override: None,
        }
    }

    #[test]
    fn test_ai_provider_config_input_trimmed_strips_trailing_crlf_from_api_key() {
        let config = ai_provider_config_input("sk-ant-secret\r\n", None).trimmed();
        assert_eq!(config.api_key, "sk-ant-secret");
    }

    #[test]
    fn test_ai_provider_config_input_trimmed_keeps_interior_characters() {
        let config = ai_provider_config_input("  sk ant secret \t", None).trimmed();
        assert_eq!(
            config.api_key, "sk ant secret",
            "nur der Rand wird getrimmt, innenliegende Zeichen bleiben"
        );
    }

    #[test]
    fn test_ai_provider_config_input_trimmed_strips_whitespace_from_base_url() {
        let config =
            ai_provider_config_input("sk-key", Some(" https://example.invalid/v1\r\n ")).trimmed();
        assert_eq!(
            config.base_url.as_deref(),
            Some("https://example.invalid/v1")
        );
    }

    #[test]
    fn test_ai_provider_config_input_trimmed_leaves_none_base_url_as_none() {
        let config = ai_provider_config_input("sk-key", None).trimmed();
        assert_eq!(config.base_url, None);
    }

    // --- Spec 0073: geteilter Trim, inkl. unsichtbarer Randzeichen ------

    // Spec 0073, T9: eine BOM am `api_key` — der typische Kopierunfall aus
    // einer UTF-8-Datei mit BOM — fällt weg. Scheitert gegen den Stand vor
    // Spec 0073, weil `str::trim` U+FEFF stehen lässt.
    #[test]
    fn test_t9_ai_provider_config_input_trimmed_strips_bom_from_api_key() {
        let config = ai_provider_config_input("\u{FEFF}sk-ant-secret\u{FEFF}", None).trimmed();
        assert_eq!(config.api_key, "sk-ant-secret");
    }

    // Spec 0073, T9 (Endpunkt-Felder): dasselbe für `base_url` und
    // `attestation_url` — auch sie stehen in der §1-Tabelle.
    #[test]
    fn test_t9_ai_provider_config_input_trimmed_strips_invisible_chars_from_urls() {
        let mut input =
            ai_provider_config_input("sk-key", Some("\u{200B}https://example.invalid/v1\u{2060}"));
        input.attestation_url = Some("\u{FEFF}https://example.invalid/att \u{200B}".to_string());
        let config = input.trimmed();
        assert_eq!(
            config.base_url.as_deref(),
            Some("https://example.invalid/v1")
        );
        assert_eq!(
            config.attestation_url.as_deref(),
            Some("https://example.invalid/att")
        );
    }

    // Spec 0073, T11 / A4 / I4: ein `api_key`, der **nur** aus unsichtbaren
    // Zeichen besteht, wird leer — und leer heißt nach Spec 0007,
    // Abschnitt 8.2, „Credential unverändert lassen", nicht „löschen".
    // Genau diesen leeren String liest der `!api_key.is_empty()`-Wächter in
    // `commands::update_ai_provider` (Spec 0049, Fund 1): Er überspringt
    // dann den `credential_store.set`. Das ist beabsichtigt — wer diese
    // Semantik später umdreht, macht diesen Test rot.
    #[test]
    fn test_t11_api_key_of_only_invisible_chars_becomes_empty_meaning_unchanged() {
        let config = ai_provider_config_input("\u{FEFF}\u{200B}\u{2060}", None).trimmed();
        assert!(
            config.api_key.is_empty(),
            "ein nur aus unsichtbaren Zeichen bestehender Key muss leer werden, damit \
             `update_ai_provider` das bestehende Credential unverändert lässt"
        );
    }

    // Spec 0073, T12: Der Helfer gilt für Zugangsdaten und Endpunkte, nicht
    // für Anzeigetexte. Ein Anzeigename mit einem Zero-Width-Space bleibt
    // unverändert — auch am Rand.
    //
    // Bewusst nur `display_name`: Ob `model` getrimmt gehört, ist eine
    // offene Frage (ADR 0064) und wird hier nicht durch eine Assertion
    // vorentschieden.
    #[test]
    fn test_t12_display_name_is_not_trimmed() {
        let mut input = ai_provider_config_input("sk-key", None);
        input.display_name = "\u{200B}Mein Provider\u{200B}".to_string();
        let config = input.trimmed();
        assert_eq!(config.display_name, "\u{200B}Mein Provider\u{200B}");
    }

    // --- Spec 0065, Teil 4: max_tokens_override-Validierung -------------

    #[test]
    fn test_validate_max_tokens_override_accepts_none_automatic() {
        let mut config = ai_provider_config_input("sk-key", None);
        config.max_tokens_override = None;
        assert_eq!(config.validate_max_tokens_override(), Ok(()));
    }

    #[test]
    fn test_validate_max_tokens_override_accepts_a_positive_value() {
        let mut config = ai_provider_config_input("sk-key", None);
        config.max_tokens_override = Some(16_384);
        assert_eq!(config.validate_max_tokens_override(), Ok(()));
    }

    #[test]
    fn test_validate_max_tokens_override_rejects_zero() {
        let mut config = ai_provider_config_input("sk-key", None);
        config.max_tokens_override = Some(0);
        assert!(config.validate_max_tokens_override().is_err());
    }

    #[test]
    fn test_validate_max_tokens_override_rejects_above_the_upper_bound() {
        let mut config = ai_provider_config_input("sk-key", None);
        config.max_tokens_override = Some(MAX_TOKENS_OVERRIDE_UPPER_BOUND + 1);
        assert!(config.validate_max_tokens_override().is_err());
    }

    #[test]
    fn test_validate_max_tokens_override_accepts_exactly_the_upper_bound() {
        let mut config = ai_provider_config_input("sk-key", None);
        config.max_tokens_override = Some(MAX_TOKENS_OVERRIDE_UPPER_BOUND);
        assert_eq!(config.validate_max_tokens_override(), Ok(()));
    }
}

#[cfg(test)]
mod sudo_password_state_tests {
    //! Spec 0071, A14/X4/I4: "unbekannt" ist nicht "nein". Vor diesem
    //! Schritt fasste `credential_store.get(...).is_ok()` beide Fälle
    //! zusammen — ein nicht erreichbarer Schlüsselbund sah für die
    //! Oberfläche genauso aus wie ein Server ohne hinterlegtes
    //! Sudo-Passwort, und sie behauptete dann "kein Sudo-Passwort
    //! hinterlegt", obwohl sie es nicht wusste.

    use super::*;
    use crate::server_credentials::sudo_password_credential_ref;
    use crate::test_support::InMemoryCredentialStore;
    use chrono::Utc;
    use ssh_manager_core::profiles::PostIngestPolicy;

    fn server() -> Server {
        let now = Utc::now();
        Server {
            id: ServerId::new(),
            name: "web-01".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: AuthMethod::Agent,
            notes: String::new(),
            jump_host: None,
            post_ingest_policy: PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn test_stored_sudo_password_is_reported_as_present() {
        let server = server();
        let store = InMemoryCredentialStore::new()
            .with_secret(&sudo_password_credential_ref(server.id), "sudo-secret");

        let dto = ServerDto::from_server(&server, &store);

        assert!(dto.has_sudo_password);
        assert!(!dto.sudo_password_unknown);
    }

    #[test]
    fn test_missing_entry_is_reported_as_absent_not_unknown() {
        let server = server();
        let store = InMemoryCredentialStore::new();

        let dto = ServerDto::from_server(&server, &store);

        assert!(!dto.has_sudo_password);
        assert!(
            !dto.sudo_password_unknown,
            "ein fehlender Eintrag ist eine echte Aussage, kein Unwissen"
        );
    }

    /// Spec 0071, X4 — der eigentliche Regressionstest: Das Sudo-Passwort
    /// **ist** hinterlegt, der Schlüsselbund kann es aber nicht sagen. Am
    /// ungefixten Stand (`.is_ok()`) meldete das DTO hier
    /// `has_sudo_password: false` — eine Behauptung, die es nicht decken
    /// kann.
    #[test]
    fn test_unreadable_keychain_is_reported_as_unknown_not_as_absent() {
        let server = server();
        let store = InMemoryCredentialStore::new()
            .with_secret(&sudo_password_credential_ref(server.id), "sudo-secret")
            .with_failing_get();

        let dto = ServerDto::from_server(&server, &store);

        assert!(
            dto.sudo_password_unknown,
            "ein Backend-Fehler muss als 'unbekannt' erkennbar sein"
        );
        assert!(!dto.has_sudo_password);
    }

    /// Spec 0071, A17: Dasselbe für das Rückstandsfeld an
    /// `DeleteServerResult`. spec-reviewer-Fund: Der Schlüsselname war
    /// bisher nur im handgeschriebenen Mock von `ServerForm.test.tsx`
    /// festgehalten — ein Rename im Rust-DTO wäre von keinem Test bemerkt
    /// worden, und die Oberfläche hätte den Hinweis stillschweigend nicht
    /// mehr angezeigt.
    #[test]
    fn test_secrets_left_behind_is_serialised_as_camel_case() {
        let server = server();
        let store = InMemoryCredentialStore::new();
        let result = DeleteServerResult {
            server: ServerDto::from_server(&server, &store),
            servers_losing_jump_host: Vec::new(),
            executed: true,
            secrets_left_behind: vec!["server:abc:password".to_string()],
        };

        let json = serde_json::to_value(&result).unwrap();

        assert_eq!(
            json["secretsLeftBehind"],
            serde_json::json!(["server:abc:password"])
        );
    }

    /// Das Feld muss das Frontend auch tatsächlich erreichen — in
    /// camelCase, wie alle anderen `ServerDto`-Felder (s. `rename_all`).
    #[test]
    fn test_unknown_flag_is_serialised_as_camel_case() {
        let server = server();
        let store = InMemoryCredentialStore::new().with_failing_get();

        let json = serde_json::to_value(ServerDto::from_server(&server, &store)).unwrap();

        assert_eq!(json["sudoPasswordUnknown"], serde_json::json!(true));
        assert_eq!(json["hasSudoPassword"], serde_json::json!(false));
    }
}

#[cfg(test)]
mod sftp_server_path_tests {
    use super::normalize_sftp_server_path;

    #[test]
    fn test_empty_override_means_automatic() {
        assert_eq!(normalize_sftp_server_path(None), Ok(None));
        assert_eq!(
            normalize_sftp_server_path(Some("   ".to_string())),
            Ok(None)
        );
    }

    #[test]
    fn test_valid_override_is_trimmed_and_kept() {
        assert_eq!(
            normalize_sftp_server_path(Some(" /usr/lib/openssh/sftp-server ".to_string())),
            Ok(Some("/usr/lib/openssh/sftp-server".to_string()))
        );
    }

    #[test]
    fn test_unsafe_override_is_rejected() {
        for bad in [
            "/bin/sh",
            "sftp-server",
            "/usr/lib/x; reboot",
            "/usr/lib/$(id)",
            "/a/../b",
        ] {
            assert!(
                normalize_sftp_server_path(Some(bad.to_string())).is_err(),
                "{bad}"
            );
        }
    }
}
