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
///
/// `Serialize`/`Deserialize` (wie schon bei [`crate::profiles::AuthMethod`],
/// s. dortiger Kommentar): `persistence-sqlite` speichert `MessageContent`
/// gemäß Spec 0034, Abschnitt 2 als JSON in der `chat_messages.content`-
/// Spalte, direkt gegen diesen Typ serialisiert statt gegen eine separate
/// Persistenz-Spiegelstruktur — dieselbe bereits etablierte
/// Serde-auf-Core-Typen-Konvention.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: MessageContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
    ActionResult,
}

/// Inhalt einer [`ChatMessage`] (Spec 0006, Abschnitt 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MessageContent {
    Text(String),
    /// Ergebnis eines über die Filter-Engine ausgeführten Kommandos, bereits
    /// durch einen [`super::OutputRedactor`] gelaufen (Spec 0006, Abschnitt
    /// 5) — dieses Modul selbst führt nichts aus, `output` kommt von
    /// außerhalb (SSH-Modul, Spec 0005).
    CommandResult {
        command: String,
        output: CommandOutput,
        /// Spec 0027: `true`, wenn der Nutzer dieses Kommando manuell
        /// abgebrochen hat, bevor es von selbst beendet war — `output` ist
        /// dann unvollständig und `output.exit_code` immer `None`. Provider-
        /// Implementierungen (`ai-providers`) hängen bei `true` einen
        /// expliziten Hinweis an den Kontext-Block an, damit die KI den
        /// fehlenden Exit-Code nicht fälschlich als Kommandofehler liest.
        cancelled: bool,
    },
    /// Spec 0021, Abschnitt 3, Fälle 3/4: eine vorgeschlagene Aktion wurde
    /// **nicht** ausgeführt — entweder hat der Nutzer sie im
    /// Bestätigungsdialog abgelehnt ([`RejectionReason::User`]) oder die
    /// Filter-Engine hat sie automatisch blockiert
    /// ([`RejectionReason::Blocked`], mit dem `Decision::Deny`-Grund). Wie
    /// bei `CommandResult` löst auch dieser Eintrag automatisch eine neue
    /// Folgerunde aus (Spec 0021, Abschnitt 3) — die KI erfährt explizit,
    /// *dass* und *warum* nichts ausgeführt wurde, statt in einem
    /// Warte-Zustand zu verharren, der nie aufgelöst wird.
    ActionRejected {
        command: String,
        reason: RejectionReason,
    },
}

/// Warum eine Aktion nicht ausgeführt wurde (Spec 0021, Abschnitt 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RejectionReason {
    /// Der Nutzer hat im Bestätigungsdialog auf "Ablehnen" geklickt.
    User,
    /// Die Filter-Engine hat die Aktion automatisch blockiert, ohne dass
    /// überhaupt ein Bestätigungsdialog gezeigt wurde — der `String` ist
    /// der `Decision::Deny { reason }`-Grund (Spec 0002).
    Blocked(String),
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

    /// Entspricht `AiAction::ReadRemoteFile` (Spec 0020, Abschnitt 4.1).
    pub fn read_remote_file() -> Self {
        Self {
            name: "read_remote_file".to_string(),
            description: "Liest den Inhalt einer Datei auf dem verbundenen Server über SFTP. \
                Läuft wie ein Kommando durch die Filter-Engine und kann durch Nutzerregeln \
                blockiert oder bestätigungspflichtig sein. Für größere Textdateien besser \
                geeignet als ein `cat`-Kommando (kein Escaping-Risiko, klare Größenbegrenzung \
                statt eines abgeschnittenen Kommando-Outputs)."
                .to_string(),
            parameters: vec![ActionParameter {
                name: "path".to_string(),
                description: "Absoluter Pfad der zu lesenden Datei auf dem Server.".to_string(),
                kind: ActionParameterKind::String,
                required: true,
            }],
        }
    }

    /// Entspricht `AiAction::WriteRemoteFile` (Spec 0020, Abschnitt 4.2).
    pub fn write_remote_file() -> Self {
        Self {
            name: "write_remote_file".to_string(),
            description: "Schreibt eine Datei auf dem verbundenen Server über SFTP (erstellt sie \
                neu oder überschreibt eine bestehende). Läuft wie ein Kommando durch die \
                Filter-Engine, wird dem Nutzer aber IMMER zur Bestätigung mit einer \
                Änderungs-Vorschau angezeigt, nie automatisch ausgeführt. Bei bestehenden \
                Dateien wird vor dem Schreiben automatisch eine Sicherungskopie angelegt. \
                Besser geeignet für Config-Dateien als ein `cat <<EOF`-Kommando: kein \
                Shell-Quoting-Risiko bei Sonderzeichen/Anführungszeichen/`$`-Variablen, und der \
                Nutzer sieht einen echten Diff statt nur des rohen Kommandotexts."
                .to_string(),
            parameters: vec![
                ActionParameter {
                    name: "path".to_string(),
                    description: "Absoluter Pfad der zu schreibenden Datei auf dem Server."
                        .to_string(),
                    kind: ActionParameterKind::String,
                    required: true,
                },
                ActionParameter {
                    name: "content".to_string(),
                    description: "Vollständiger neuer Dateiinhalt, nicht nur ein Diff.".to_string(),
                    kind: ActionParameterKind::String,
                    required: true,
                },
            ],
        }
    }
}

/// Alle in Spec 0003/0012/0020 definierten `AiAction`-Varianten als
/// Standard-Set für `SessionContext::available_actions`. Bewusst **ohne**
/// Löschen/Umbenennen/Verzeichnis-Anlegen (Spec 0020, Abschnitt 4.4) — dafür
/// bleibt `suggest_command()` der einzige, durch dieselbe Filter-Engine
/// samt Hard-Blacklist laufende Weg.
pub fn default_action_schemas() -> Vec<ActionSchema> {
    vec![
        ActionSchema::suggest_command(),
        ActionSchema::propose_note_update(),
        ActionSchema::generate_document(),
        ActionSchema::read_remote_file(),
        ActionSchema::write_remote_file(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec 0020, Abschnitt 4.4: der KI dürfen niemals Schemas für
    /// Löschen/Umbenennen/Verzeichnis-Anlegen angeboten werden — diese
    /// Operationen bleiben ausschließlich über `suggest_command()` (also
    /// mit voller Filter-Engine- und Hard-Blacklist-Prüfung) erreichbar.
    #[test]
    fn test_default_action_schemas_excludes_delete_rename_mkdir() {
        let schemas = default_action_schemas();
        let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"read_remote_file"));
        assert!(names.contains(&"write_remote_file"));
        for forbidden in [
            "delete_remote_file",
            "remove_remote_file",
            "rename_remote_file",
            "create_remote_dir",
            "mkdir",
        ] {
            assert!(
                !names.contains(&forbidden),
                "Schema '{forbidden}' darf der KI nicht angeboten werden (Spec 0020, Abschnitt 4.4)"
            );
        }
    }
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

impl AiError {
    /// Stabiler, sprachunabhängiger Bezeichner je Fehlerart (Spec 0024,
    /// Abschnitt 5) — fürs Frontend-Mapping auf Übersetzungs-Keys. Bleibt
    /// über Code-Änderungen hinweg stabil, anders als der `Display`-Text
    /// oben (der unverändert bleibt und weiterhin als Fallback dient, falls
    /// das Frontend einen Code nicht kennt).
    pub fn code(&self) -> &'static str {
        match self {
            AiError::AuthenticationFailed => "AI_AUTH_FAILED",
            AiError::RateLimited => "AI_RATE_LIMITED",
            AiError::NetworkError(_) => "AI_NETWORK_ERROR",
            AiError::InvalidResponse(_) => "AI_INVALID_RESPONSE",
            AiError::ContextTooLarge => "AI_CONTEXT_TOO_LARGE",
            AiError::ProviderUnavailable(_) => "AI_PROVIDER_UNAVAILABLE",
        }
    }
}

impl std::error::Error for AiError {}

#[cfg(test)]
mod ai_error_code_tests {
    use super::*;

    /// Spec 0024, Abschnitt 5: Codes müssen stabil und eindeutig sein — kein
    /// Code darf für zwei unterschiedliche Fehlerarten doppelt vergeben sein.
    #[test]
    fn test_ai_error_codes_are_unique() {
        let samples = [
            AiError::AuthenticationFailed,
            AiError::RateLimited,
            AiError::NetworkError("x".to_string()),
            AiError::InvalidResponse("x".to_string()),
            AiError::ContextTooLarge,
            AiError::ProviderUnavailable("x".to_string()),
        ];
        let codes: Vec<&'static str> = samples.iter().map(AiError::code).collect();
        let mut unique = codes.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            codes.len(),
            unique.len(),
            "doppelt vergebener AiError-Code: {codes:?}"
        );
    }

    #[test]
    fn test_ai_error_code_stable_across_payload_variation() {
        assert_eq!(
            AiError::NetworkError("a".to_string()).code(),
            AiError::NetworkError("b".to_string()).code(),
        );
    }
}
