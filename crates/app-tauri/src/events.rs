//! Tauri-Events Richtung Frontend (Spec 0007, Abschnitt 5) und die
//! Abstraktion, über die die Orchestrierungs-Logik (`crate::orchestration`)
//! sie sendet.
//!
//! **`EventEmitter`-Trait statt direkt `tauri::AppHandle`**: die
//! Kernschleifen-Logik aus Abschnitt 6 soll laut Aufgabenstellung (Teil 1,
//! Punkt 5) gegen `MockAiProvider`/`MockSshTransport` unit-testbar sein,
//! ganz ohne echte Tauri-Runtime. Ein `dyn EventEmitter` lässt sich dafür
//! durch ein simples `TestEmitter` (unten, `#[cfg(test)]`) ersetzen, das
//! nur sammelt statt tatsächlich zu senden — `tauri::AppHandle` selbst ist
//! außerhalb einer laufenden App nicht sinnvoll konstruierbar.
//!
//! Payload-Serialisierung liegt bewusst in Value-Form im Trait (statt
//! generisch `impl Serialize`), damit der Trait dyn-kompatibel bleibt.

use serde::Serialize;

use ssh_manager_core::filter::Decision;
use ssh_manager_core::profiles::AiAction;

use crate::state::{ActionId, SessionId};

pub trait EventEmitter: Send + Sync {
    fn emit_event(&self, event: &str, payload: serde_json::Value);
}

impl EventEmitter for tauri::AppHandle {
    fn emit_event(&self, event: &str, payload: serde_json::Value) {
        // Ein Event kann höchstens dann nicht gesendet werden, wenn die App
        // gerade herunterfährt — kein Fall, den die aufrufende
        // Orchestrierungslogik als harten Fehler behandeln sollte (sie
        // würde sonst z. B. eine laufende Kommando-Ausführung abbrechen,
        // nur weil das *Benachrichtigen* des UI fehlschlug). Nur geloggt.
        if let Err(err) = tauri::Emitter::emit(self, event, payload) {
            eprintln!("Event '{event}' konnte nicht gesendet werden: {err}");
        }
    }
}

fn emit<T: Serialize>(emitter: &dyn EventEmitter, event: &str, payload: &T) {
    match serde_json::to_value(payload) {
        Ok(value) => emitter.emit_event(event, value),
        Err(err) => eprintln!("Event '{event}' nicht serialisierbar: {err}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    /// Spec 0017, Abschnitt 2: `list_sessions()`-Status für einen laufenden
    /// `connect()`-Aufruf, der gerade auf `confirm_host_key` wartet — diese
    /// Session existiert noch nicht in `SessionManager.sessions` (sie wird
    /// erst nach erfolgreichem Verbindungsaufbau eingefügt, s.
    /// `crate::commands::connect`), taucht aber über
    /// `SessionManager.pending_connections` trotzdem in der Momentaufnahme
    /// auf. Wird **nie** über `connection-status-changed` gesendet (dieses
    /// Event kennt nur den Übergang Connected/Disconnected) — nur als
    /// `SessionSummaryDto`-Feld relevant.
    AwaitingHostKey,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionStatusChangedPayload {
    session_id: SessionId,
    status: ConnectionStatus,
    reason: Option<String>,
}

pub fn emit_connection_status_changed(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    status: ConnectionStatus,
    reason: Option<String>,
) {
    emit(
        emitter,
        "connection-status-changed",
        &ConnectionStatusChangedPayload {
            session_id,
            status,
            reason,
        },
    );
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKeyKind {
    Unknown,
    Mismatch,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HostKeyVerificationNeededPayload {
    session_id: SessionId,
    host: String,
    port: u16,
    kind: HostKeyKind,
    fingerprint: String,
    /// Nur bei `kind: Mismatch` gesetzt. Die Spec-Skizze (Abschnitt 5)
    /// nennt nur ein einzelnes `fingerprint`-Feld — für den Mismatch-Fall
    /// (Spec 0005 Abschnitt 6, "besonders strenge Warnung") reicht das
    /// nicht, um dem Frontend alten und neuen Fingerprint nebeneinander
    /// zeigen zu lassen, deshalb hier bewusst ergänzt.
    expected_fingerprint: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn emit_host_key_verification_needed(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    host: String,
    port: u16,
    kind: HostKeyKind,
    fingerprint: String,
    expected_fingerprint: Option<String>,
) {
    emit(
        emitter,
        "host-key-verification-needed",
        &HostKeyVerificationNeededPayload {
            session_id,
            host,
            port,
            kind,
            fingerprint,
            expected_fingerprint,
        },
    );
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalOutputPayload {
    session_id: SessionId,
    /// Base64-kodiert statt als JSON-Zahlenarray: Terminal-Output kann
    /// hochfrequent/umfangreich sein (z. B. `cat` einer großen Datei) — ein
    /// JSON-Array von Bytes wäre um ein Vielfaches größer als nötig. Rohe
    /// Bytes (nicht lossy-UTF8-dekodiert) sind zudem nötig, damit
    /// mehrbyte-/Escape-Sequenzen, die zufällig an einer Chunk-Grenze
    /// zerschnitten werden, nicht korrumpiert werden.
    data: String,
}

pub fn emit_terminal_output(emitter: &dyn EventEmitter, session_id: SessionId, data: &[u8]) {
    use base64::Engine;
    emit(
        emitter,
        "terminal-output",
        &TerminalOutputPayload {
            session_id,
            data: base64::engine::general_purpose::STANDARD.encode(data),
        },
    );
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatTextDeltaPayload {
    session_id: SessionId,
    delta: String,
}

pub fn emit_chat_text_delta(emitter: &dyn EventEmitter, session_id: SessionId, delta: String) {
    emit(
        emitter,
        "chat-text-delta",
        &ChatTextDeltaPayload { session_id, delta },
    );
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatActionProposedPayload {
    session_id: SessionId,
    action_id: ActionId,
    action: AiAction,
    decision: Decision,
    /// Spec 0019, Abschnitt 3: nur bei `action: ProposeNoteUpdate` gesetzt —
    /// der aktuelle Notizinhalt des aufgelösten Ziels, damit das Frontend
    /// eine kurze Diff-Vorschau (alt/neu) zeigen kann statt nur des vollen
    /// neuen Texts. `None` für alle anderen Aktionstypen sowie wenn die
    /// Zielauflösung fehlschlägt.
    previous_note_content: Option<String>,
    /// Spec 0018, Abschnitt 7: ob beim Ausführen automatisch ein
    /// hinterlegtes Sudo-Passwort eingespeist würde — Grundlage für den
    /// Transparenz-Hinweis im Bestätigungsdialog.
    uses_stored_sudo_password: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn emit_chat_action_proposed(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    action_id: ActionId,
    action: AiAction,
    decision: Decision,
    previous_note_content: Option<String>,
    uses_stored_sudo_password: bool,
) {
    emit(
        emitter,
        "chat-action-proposed",
        &ChatActionProposedPayload {
            session_id,
            action_id,
            action,
            decision,
            previous_note_content,
            uses_stored_sudo_password,
        },
    );
}

/// Spec 0010, Abschnitt 2, Punkt 5: derselbe Vorschlags-Anlass wie
/// `chat-action-proposed` (`action` ist hier immer
/// `AiAction::ProposeNoteUpdate`, nie `SuggestCommand` — s.
/// `crate::orchestration::suggest_note_update_on_disconnect`), aber
/// **bewusst ein eigenes Event statt einer Wiederverwendung von
/// `chat-action-proposed`**: Letzteres wird im Frontend ausschließlich vom
/// `ChatPanel` einer *offenen* Session-Ansicht konsumiert (`if
/// (event.sessionId !== sessionId) return;`, an die konkrete Chat-Screen-
/// Instanz gebunden). Der Disconnect-Vorschlag muss laut Spec aber
/// **auch dann noch ankommen, wenn der Nutzer den Screen bereits verlassen
/// hat** — dafür braucht es einen App-weiten Listener statt eines an eine
/// bestimmte Screen-Instanz gebundenen. Kein `decision`-Feld (anders als
/// `chat-action-proposed`): `ProposeNoteUpdate` verlangt ohnehin immer
/// `Confirm` (Spec 0003, Abschnitt 5.2) — hier gäbe es nie einen anderen
/// Wert, das Feld wäre nur totes Gewicht. Siehe ADR-Vorschlag am Ende der
/// Aufgabe.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteUpdateSuggestedPayload {
    session_id: SessionId,
    action_id: ActionId,
    action: AiAction,
    /// Spec 0019, Abschnitt 3 — s. `ChatActionProposedPayload`-Doc-
    /// Kommentar, dieselbe Grundlage.
    previous_note_content: Option<String>,
}

pub fn emit_note_update_suggested(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    action_id: ActionId,
    action: AiAction,
    previous_note_content: Option<String>,
) {
    emit(
        emitter,
        "note-update-suggested",
        &NoteUpdateSuggestedPayload {
            session_id,
            action_id,
            action,
            previous_note_content,
        },
    );
}

/// Spec 0012, Abschnitt 3 — direkt aus `AiEvent::ActionProposed(GenerateDocument
/// { .. })` weitergereicht, ohne Umweg über `chat-action-proposed`: es gibt
/// hier keine `Decision` (kein Filter-Engine-/Bestätigungspfad, s.
/// `crate::orchestration::handle_document_generated`), das Feld wäre also
/// nur totes Gewicht.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatDocumentGeneratedPayload {
    session_id: SessionId,
    action_id: ActionId,
    title: String,
    content_markdown: String,
}

pub fn emit_chat_document_generated(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    action_id: ActionId,
    title: String,
    content_markdown: String,
) {
    emit(
        emitter,
        "chat-document-generated",
        &ChatDocumentGeneratedPayload {
            session_id,
            action_id,
            title,
            content_markdown,
        },
    );
}

/// Ergebnis einer ausgeführten Aktion. Erweitert die Spec-Skizze aus
/// Abschnitt 5 (dort nur `{ session_id, action_id, output: CommandOutput }`)
/// um eine zweite Variante: `AiAction::ProposeNoteUpdate` (Abschnitt 6,
/// letzter Punkt) hat kein `CommandOutput`, sondern nur eine
/// Erfolgsmeldung — die Spec-Skizze deckt diesen Fall nicht ab, da sie vor
/// allem den `SuggestCommand`-Pfad beschreibt. Siehe ADR-Vorschlag am Ende
/// der Aufgabe.
#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum ActionResultPayload {
    Command {
        command: String,
        stdout: String,
        stderr: String,
        exit_code: Option<i32>,
    },
    NoteUpdate {
        summary: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatActionResultPayload {
    session_id: SessionId,
    action_id: ActionId,
    result: ActionResultPayload,
}

pub fn emit_chat_action_result(
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    action_id: ActionId,
    result: ActionResultPayload,
) {
    emit(
        emitter,
        "chat-action-result",
        &ChatActionResultPayload {
            session_id,
            action_id,
            result,
        },
    );
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatErrorPayload {
    session_id: SessionId,
    message: String,
}

/// **Nicht Teil der Spec-Skizze aus Abschnitt 5** (die dort gelistete
/// Ereignistabelle kennt kein Fehler-Event) — `AiEvent::Error` (Spec 0006:
/// Auth-Fehler, Rate-Limit, Netzwerkfehler, ...) und ein fehlgeschlagenes
/// `SshTransport::execute()`/`ProfileStore::record_note_revision()` müssen
/// aber irgendwie sichtbar werden, sonst bricht ein Chat-Turn für den
/// Nutzer ohne jede Erklärung ab. Bewusst als eigenes Event statt als
/// `chat-text-delta` missbraucht (damit das Frontend Fehler visuell klar
/// vom normalen Antworttext unterscheiden kann). Siehe ADR-Vorschlag am
/// Ende der Aufgabe.
pub fn emit_chat_error(emitter: &dyn EventEmitter, session_id: SessionId, message: String) {
    emit(
        emitter,
        "chat-error",
        &ChatErrorPayload {
            session_id,
            message,
        },
    );
}

#[cfg(test)]
pub struct TestEmitter {
    pub events: std::sync::Mutex<Vec<(String, serde_json::Value)>>,
}

#[cfg(test)]
impl Default for TestEmitter {
    fn default() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl EventEmitter for TestEmitter {
    fn emit_event(&self, event: &str, payload: serde_json::Value) {
        self.events
            .lock()
            .unwrap()
            .push((event.to_string(), payload));
    }
}

#[cfg(test)]
mod tests {
    //! Regressionstest, s. ausführliche Begründung in
    //! `crate::dto::tests` — `rename_all` auf einem `#[serde(tag = ...)]`-
    //! Enum färbt nur die Tag-Werte camelCase, nicht die Feldnamen
    //! innerhalb der Varianten; `exit_code` blieb dadurch snake_case und
    //! kam im Frontend als `undefined` an (`result.exitCode`).

    use super::*;

    #[test]
    fn test_action_result_payload_command_uses_camel_case_exit_code() {
        let value = ActionResultPayload::Command {
            command: "ls".to_string(),
            stdout: "out".to_string(),
            stderr: String::new(),
            exit_code: Some(0),
        };
        let json = serde_json::to_value(&value).unwrap();

        assert_eq!(json["exitCode"], 0);
        assert!(json.get("exit_code").is_none());
    }
}
