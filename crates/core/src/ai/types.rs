use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::profiles::AiAction;
use crate::ssh::CommandOutput;

/// Eindeutige Kennung einer gespeicherten AI-Provider-Konfiguration (Spec
/// 0007, Abschnitt 8). Bleibt (wie [`crate::profiles::GroupId`]) lokal in
/// `ai`, statt nach `crate::shared` zu wandern: kein anderes `core`-Modul
/// kennt das Konzept "AI-Provider-Konfiguration", es gibt also keinen
/// zweiten Ort, der denselben Typ bräuchte (s. `crate::shared`-Modul-
/// Kommentar für die generelle Regel).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderId(pub Uuid);

impl ProviderId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProviderId {
    fn default() -> Self {
        Self::new()
    }
}

/// Welche konkrete `AiProvider`-Implementierung (`crates/ai-providers`) eine
/// gespeicherte Konfiguration anspricht (Spec 0007, Abschnitt 8.1). Die
/// `#[serde(rename = ...)]`-Werte entsprechen exakt sowohl dem
/// `CHECK`-Constraint der `ai_provider_configs`-Tabelle als auch dem
/// JSON-Wert, den das Frontend über die Tauri-IPC-Grenze sieht — ein
/// Konstrukt, zwei Randbedingungen, die sich sonst leise auseinander
/// entwickeln könnten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderType {
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "generic_openai_compatible")]
    GenericOpenAiCompatible,
    #[serde(rename = "ollama")]
    Ollama,
}

impl ProviderType {
    /// Textform exakt wie im `CHECK`-Constraint der Migration — genutzt von
    /// `persistence-sqlite`, um den Wert als reines `TEXT` zu binden/lesen
    /// (nicht über `serde_json`, das würde umschließende Anführungszeichen
    /// mitliefern).
    pub fn as_db_str(self) -> &'static str {
        match self {
            ProviderType::OpenAi => "openai",
            ProviderType::Anthropic => "anthropic",
            ProviderType::GenericOpenAiCompatible => "generic_openai_compatible",
            ProviderType::Ollama => "ollama",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "openai" => Some(ProviderType::OpenAi),
            "anthropic" => Some(ProviderType::Anthropic),
            "generic_openai_compatible" => Some(ProviderType::GenericOpenAiCompatible),
            "ollama" => Some(ProviderType::Ollama),
            _ => None,
        }
    }
}

/// Ereignis, das ein [`super::AiProvider`] während einer Konversation streamt
/// (Spec 0006, Abschnitt 3).
#[derive(Debug, Clone, PartialEq)]
pub enum AiEvent {
    /// Chat-Text zum sofortigen Anzeigen (Streaming, wortweise).
    TextDelta(String),
    /// Strukturierter Vorschlag, s. Spec 0003 Abschnitt 5.2. Führt **nichts**
    /// aus — läuft für `SuggestCommand` unverändert durch die Filter-Engine
    /// (Spec 0002), für `ProposeNoteUpdate` immer über einen manuellen
    /// Bestätigungsdialog (Spec 0003 Abschnitt 5.2).
    ActionProposed(AiAction),
    Done,
    Error(AiError),
}

/// Kontext, den die App für eine Anfrage an den Provider zusammenstellt
/// (Spec 0006, Abschnitt 3).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionContext {
    /// `effective_notes()` aus Spec 0003 + OS/Distro-Info.
    pub system_context: String,
    pub history: Vec<ChatMessage>,
    pub available_actions: Vec<ActionSchema>,
}

/// Eine Nachricht in der Konversationshistorie (Spec 0006, Abschnitt 3).
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: MessageContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    ActionResult,
}

/// Inhalt einer [`ChatMessage`] (Spec 0006, Abschnitt 3).
#[derive(Debug, Clone, PartialEq)]
pub enum MessageContent {
    Text(String),
    /// Ergebnis eines über die Filter-Engine ausgeführten Kommandos, bereits
    /// durch einen [`super::OutputRedactor`] gelaufen (Spec 0006, Abschnitt
    /// 5) — dieses Modul selbst führt nichts aus, `output` kommt von
    /// außerhalb (SSH-Modul, Spec 0005).
    CommandResult {
        command: String,
        output: CommandOutput,
    },
}

/// Beschreibung einer Aktion, die ein Provider per Tool-/Function-Calling
/// vorschlagen kann (Spec 0006, Abschnitt 3/4) — providerunabhängige,
/// minimale Zwischenform. Jede konkrete `AiProvider`-Implementierung
/// übersetzt sie in das jeweilige Tool-Definitionsformat der Provider-API
/// (Abschnitt 4).
///
/// Nicht Teil der in der Spec explizit vorgegebenen Typen (die Spec
/// verlangt nur "eine sinnvolle minimale Form ... Name, Beschreibung,
/// Parameter-Schema"), aber deckt genau die beiden `AiAction`-Varianten aus
/// Spec 0003 ab — s. [`ActionSchema::suggest_command`]/
/// [`ActionSchema::propose_note_update`] und [`default_action_schemas`].
#[derive(Debug, Clone, PartialEq)]
pub struct ActionSchema {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ActionParameter>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActionParameter {
    pub name: String,
    pub description: String,
    pub kind: ActionParameterKind,
    pub required: bool,
}

/// Bewusst minimal (kein volles JSON-Schema): reicht aus, um beide
/// `AiAction`-Varianten aus Spec 0003 zu beschreiben, ohne die
/// Provider-Implementierungen mit unbenutzter Komplexität zu belasten.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionParameterKind {
    String,
    /// Feste Werteliste, z. B. für `ProposeNoteUpdate`s `target`
    /// (`"current_server"`/`"current_server_group"`).
    Enum(Vec<String>),
}

impl ActionSchema {
    /// Entspricht `AiAction::SuggestCommand` (Spec 0003, Abschnitt 5.2).
    pub fn suggest_command() -> Self {
        Self {
            name: "suggest_command".to_string(),
            description: "Schlägt ein einzelnes Shell-Kommando zur Ausführung vor. \
                Läuft vor jeder Ausführung durch die Filter-Engine; nichts wird \
                automatisch ausgeführt."
                .to_string(),
            parameters: vec![ActionParameter {
                name: "command".to_string(),
                description: "Das vorzuschlagende Shell-Kommando.".to_string(),
                kind: ActionParameterKind::String,
                required: true,
            }],
        }
    }

    /// Entspricht `AiAction::ProposeNoteUpdate` (Spec 0003, Abschnitt 5.2).
    /// Spec 0016, Abschnitt 6: **kein** Freitext-`target_id`-Feld mehr — die
    /// KI muss nie eine `ServerId`/`GroupId` kennen oder korrekt formatieren
    /// (Ursache des `target_id ist keine gültige UUID`-Bugfalls). `target`
    /// ist optional (Default bei Fehlen: `current_server`, s.
    /// `ai_providers::action::action_from_tool_arguments`); das Backend
    /// löst daraus die tatsächliche ID selbst aus dem Session-Kontext auf.
    pub fn propose_note_update() -> Self {
        Self {
            name: "propose_note_update".to_string(),
            description: "Schlägt eine Aktualisierung der Kontextnotiz des aktuell verbundenen \
                Servers oder dessen Gruppe vor. Wird immer als Diff zur manuellen Bestätigung \
                angezeigt, nie automatisch übernommen."
                .to_string(),
            parameters: vec![
                ActionParameter {
                    name: "target".to_string(),
                    description: "Ob sich der Vorschlag auf den aktuell verbundenen Server oder \
                        dessen Gruppe bezieht. Optional, Default: current_server."
                        .to_string(),
                    kind: ActionParameterKind::Enum(vec![
                        "current_server".to_string(),
                        "current_server_group".to_string(),
                    ]),
                    required: false,
                },
                ActionParameter {
                    name: "new_content".to_string(),
                    description: "Vollständiger neuer Notiztext, nicht nur ein Diff.".to_string(),
                    kind: ActionParameterKind::String,
                    required: true,
                },
            ],
        }
    }

    /// Entspricht `AiAction::GenerateDocument` (Spec 0012, Abschnitt 2).
    pub fn generate_document() -> Self {
        Self {
            name: "generate_document".to_string(),
            description: "Erstellt ein eigenständiges formatiertes Dokument (Bericht, Zusammenfassung, \
                Analyse, Dokumentation, Export). MUSS aufgerufen werden, wenn der Nutzer nach einem Dokument, \
                Bericht, einer Zusammenfassung als Datei, einer Analyse oder einem Word-/Markdown-Export \
                fragt, anstatt den Dokumentinhalt nur als einfachen Chattext auszugeben."
                .to_string(),
            parameters: vec![
                ActionParameter {
                    name: "title".to_string(),
                    description: "Kurzer Titel des Dokuments, dient auch als Basis für den \
                        vorgeschlagenen Dateinamen."
                        .to_string(),
                    kind: ActionParameterKind::String,
                    required: true,
                },
                ActionParameter {
                    name: "content_markdown".to_string(),
                    description: "Vollständiger Dokumentinhalt als Markdown (Überschriften, \
                        Absätze, Fett/Kursiv, Listen)."
                        .to_string(),
                    kind: ActionParameterKind::String,
                    required: true,
                },
            ],
        }
    }
}

/// Alle in Spec 0003/0012 definierten `AiAction`-Varianten als Standard-Set
/// für `SessionContext::available_actions`.
pub fn default_action_schemas() -> Vec<ActionSchema> {
    vec![
        ActionSchema::suggest_command(),
        ActionSchema::propose_note_update(),
        ActionSchema::generate_document(),
    ]
}

/// Fehler rund um Aufbau und Nutzung einer KI-Provider-Anfrage (Spec 0006,
/// Abschnitt 6).
#[derive(Debug, Clone, PartialEq)]
pub enum AiError {
    AuthenticationFailed,
    RateLimited,
    NetworkError(String),
    InvalidResponse(String),
    ContextTooLarge,
    ProviderUnavailable(String),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::AuthenticationFailed => {
                write!(f, "Authentifizierung beim KI-Provider fehlgeschlagen")
            }
            AiError::RateLimited => write!(f, "Rate-Limit des KI-Providers erreicht"),
            AiError::NetworkError(msg) => write!(f, "Netzwerkfehler: {msg}"),
            AiError::InvalidResponse(msg) => {
                write!(f, "Unerwartete Antwort des KI-Providers: {msg}")
            }
            AiError::ContextTooLarge => write!(f, "Kontext zu groß für den KI-Provider"),
            AiError::ProviderUnavailable(msg) => write!(f, "KI-Provider nicht erreichbar: {msg}"),
        }
    }
}

impl std::error::Error for AiError {}
