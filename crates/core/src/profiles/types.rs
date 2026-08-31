use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::shared::ServerId;

/// Eindeutige Kennung einer Gruppe.
///
/// Bleibt (anders als [`ServerId`]) lokal in `profiles`: `filter` kennt das
/// Konzept "Gruppe" nicht, es gibt daher keinen Grund, den Typ nach
/// `crate::shared` auszulagern (s. dortiger Modul-Kommentar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId(pub Uuid);

impl GroupId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for GroupId {
    fn default() -> Self {
        Self::new()
    }
}

/// Hierarchische Gruppe zur Organisation von Servern (Spec 0003, Abschnitt
/// 2). **Kein** Ersatz für die `Tag`-Scopes der Filter-Engine (Spec 0002) —
/// Gruppen dienen der Organisation/dem Notizen-Kontext, Tags der
/// Policy-Steuerung. Ein Server kann beides unabhängig voneinander tragen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub parent_id: Option<GroupId>,
    /// Aktueller LLM-Kontext dieser Gruppe, s. Abschnitt 5.
    pub notes: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Server-Verbindungsprofil (Spec 0003, Abschnitt 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Server {
    pub id: ServerId,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub group_id: Option<GroupId>,
    // Bewusst derselbe Typ (`Vec<String>`) wie `EvalContext::tags`/
    // `Scope::Tag` in `crate::filter` (Spec 0002) — beide Specs definieren
    // Tags identisch als reinen String, ein eigener `Tag`-Newtype würde hier
    // nur verlustfreie Konvertierungen erzwingen, ohne dass irgendwo eine
    // zweite Repräsentation existiert, die zusammengeführt werden müsste.
    // Siehe `crate::shared`-Modul-Kommentar für die ausführliche Begründung.
    pub tags: Vec<String>,
    pub auth: AuthMethod,
    /// Aktueller LLM-Kontext dieses Servers, s. Abschnitt 5.
    pub notes: String,
    /// Bastion/Jump-Host-Verkettung.
    pub jump_host: Option<ServerId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Authentifizierungsmethode eines Servers (Spec 0003, Abschnitt 3).
///
/// Hält ausschließlich [`CredentialRef`]s, niemals ein Secret selbst — daher
/// ist ein `Debug`-Print von `AuthMethod`/`Server` strukturell unbedenklich
/// (s. `CredentialRef`-Kommentar).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AuthMethod {
    Password {
        credential_ref: CredentialRef,
    },
    PrivateKey {
        credential_ref: CredentialRef,
        passphrase_ref: Option<CredentialRef>,
    },
    Agent,
    Certificate {
        cert_ref: CredentialRef,
        key_ref: CredentialRef,
    },
}

/// Opaker Schlüssel ins OS-Keychain (Spec 0003, Abschnitt 4). **Kein**
/// Secret — nur ein Lookup-Key, der unbedenklich in der lokalen DB und in
/// Logs auftauchen darf; das eigentliche Secret liefert erst
/// [`crate::profiles::CredentialStore::get`] als `SecretString`. Das innere
/// Feld ist bewusst privat (die Spec nennt den Typ "opak") — Konstruktion nur
/// über [`CredentialRef::new`], Zugriff auf den Rohwert nur über
/// [`CredentialRef::as_str`] für konkrete `CredentialStore`-Implementierungen.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CredentialRef(String);

impl CredentialRef {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Ziel einer KI-Notiz-Änderung (Spec 0003, Abschnitt 5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NoteTarget {
    Server(ServerId),
    Group(GroupId),
}

/// Von der KI vorgeschlagene Aktion (Spec 0003, Abschnitt 5.2; erweitert um
/// `GenerateDocument` in Spec 0012, Abschnitt 2). Nur `SuggestCommand`
/// läuft durch die Filter-Engine (Spec 0002) — die betrifft ausschließlich
/// Shell-Kommandos. `ProposeNoteUpdate` ist immer manuell zu bestätigen
/// (Diff-Ansicht), nie automatisch übernehmbar. `GenerateDocument` läuft
/// weder durch die Filter-Engine noch durch einen Bestätigungsdialog — es
/// erzeugt reinen lokalen Inhalt, nichts wird ungefragt auf die Festplatte
/// geschrieben (Spec 0012, Abschnitt 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AiAction {
    SuggestCommand {
        command: String,
    },
    ProposeNoteUpdate {
        target: NoteTarget,
        /// Vollständiger neuer Text, nicht nur ein Diff.
        new_content: String,
    },
    GenerateDocument {
        title: String,
        content_markdown: String,
    },
}

/// Wer eine [`NoteRevision`] erzeugt hat (Spec 0003, Abschnitt 5.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NoteEditor {
    User,
    Ai { provider: String, model: String },
}

/// Ein Eintrag in der Änderungs-Historie einer Notiz (Spec 0003, Abschnitt
/// 5.3) — unabhängig vom aktuellen `notes`-Feld auf [`Server`]/[`Group`], das
/// immer nur den jeweils neuesten Stand hält.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteRevision {
    pub id: Uuid,
    pub target: NoteTarget,
    pub content: String,
    pub edited_by: NoteEditor,
    pub created_at: DateTime<Utc>,
}
