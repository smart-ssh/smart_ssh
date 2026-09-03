//! Implementiert `mcp_server::McpBackend` (Spec 0028, Abschnitt 3) — die
//! einzige Stelle, an der MCP-Tool-Calls auf echte App-Logik treffen.
//! Übersetzt nichts selbst aus, sondern ruft für jede aktionsauslösende
//! Anfrage direkt `orchestration::handle_mcp_action_proposed` auf, denselben
//! Code-Pfad wie der interne Chat-Flow — s. `mcp_server::backend`-Moduldoc
//! zur Begründung, warum diese Implementierung hier (statt in der
//! `mcp-server`-Crate selbst) lebt.

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use mcp_server::{ActionOutcome, LookupError, McpBackend, ServerSummary};
use ssh_manager_core::profiles::AiAction;
use ssh_manager_core::shared::ServerId;

use crate::commands::connect_session;
use crate::events::{emit_mcp_action_tab_requested, ConnectionStatus, EventEmitter};
use crate::orchestration::handle_mcp_action_proposed;
use crate::session::Session;
use crate::state::{AppState, SessionId};

pub struct AppMcpBackend {
    app: AppHandle,
}

impl AppMcpBackend {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    fn is_allowed(&self, server_id: &ServerId) -> bool {
        self.state()
            .mcp
            .allowed_servers
            .lock()
            .expect("allowed_servers-Mutex vergiftet")
            .contains(server_id)
    }

    /// Liefert eine bestehende, verbundene Session für `server_id`, falls
    /// bereits ein Tab offen ist (Spec 0028, Abschnitt 9a: "existiert
    /// bereits ein Tab für diesen Server, wird dieser verwendet, kein
    /// Duplikat"), sonst baut sie über denselben `connect_session`-Pfad wie
    /// ein manueller Sidebar-Klick neu auf. In beiden Fällen wird **vor**
    /// einem eventuell wartenden Host-Key-Dialog bereits das
    /// `mcp-action-tab-requested`-Event gesendet, damit das Frontend den
    /// Tab öffnet/wechselt, bevor irgendein Dialog für diese Session
    /// erscheint — sonst liefe eine MCP-Anfrage an einen neuen Server auf
    /// einen für den Nutzer unsichtbaren, ewig wartenden Dialog hinaus.
    async fn ensure_session(
        &self,
        server_id: ServerId,
    ) -> Result<(SessionId, Arc<Session>), String> {
        let state = self.state();

        let existing = state.sessions.snapshot().into_iter().find(|entry| {
            entry.server_id == server_id && entry.status == ConnectionStatus::Connected
        });

        if let Some(entry) = existing {
            emit_mcp_action_tab_requested(&self.app, entry.session_id, server_id);
            let session = state
                .sessions
                .get(entry.session_id)
                .ok_or_else(|| "Session wurde während der Anfrage geschlossen".to_string())?;
            return Ok((entry.session_id, session));
        }

        let session_id: SessionId = uuid::Uuid::new_v4();
        emit_mcp_action_tab_requested(&self.app, session_id, server_id);

        connect_session(&self.app, &state, server_id, session_id)
            .await
            .map_err(|err| err.message)?;

        let session = state.sessions.get(session_id).ok_or_else(|| {
            "Session unmittelbar nach connect_session nicht auffindbar".to_string()
        })?;
        Ok((session_id, session))
    }

    /// Spec 0028, Abschnitt 9a: native OS-Benachrichtigung für eine
    /// wartende MCP-Bestätigung — aufdringlicher als der stille
    /// Hintergrund-Tab-Indikator aus Spec 0017, absichtlich, da eine
    /// externe Anfrage einen anderen Dringlichkeitsgrad hat als ein
    /// Ergebnis aus dem eigenen, ohnehin aktiv verfolgten Chat. Rein
    /// informativ/best-effort: schlägt das Zeigen fehl (z. B. Berechtigung
    /// verweigert), wird das nur geloggt, nie ein Fehler an den MCP-Client
    /// zurückgegeben — die eigentliche Aktion läuft unabhängig davon
    /// normal weiter.
    fn notify_pending_confirmation(&self, server_name: &str, client_name: Option<&str>) {
        use tauri_plugin_notification::NotificationExt;

        let requester = client_name.unwrap_or("Ein externes Tool (MCP)");
        let body = format!("{requester} möchte eine Aktion auf '{server_name}' ausführen.");
        if let Err(err) = self
            .app
            .notification()
            .builder()
            .title("Smart SSH: Bestätigung erforderlich")
            .body(body)
            .show()
        {
            tracing::warn!(error = %err, "mcp notification could not be shown");
        }
    }
}

#[async_trait::async_trait]
impl McpBackend for AppMcpBackend {
    async fn list_servers(&self) -> Vec<ServerSummary> {
        let state = self.state();
        let allowed: Vec<ServerId> = state
            .mcp
            .allowed_servers
            .lock()
            .expect("allowed_servers-Mutex vergiftet")
            .iter()
            .copied()
            .collect();

        let mut summaries = Vec::with_capacity(allowed.len());
        for id in allowed {
            if let Ok(server) = state.profile_store.get_server(&id).await {
                summaries.push(ServerSummary {
                    id,
                    name: server.name,
                });
            }
        }
        summaries
    }

    async fn server_notes(&self, server_id: ServerId) -> Result<String, LookupError> {
        if !self.is_allowed(&server_id) {
            return Err(LookupError::UnknownServer);
        }
        let state = self.state();
        let server = state
            .profile_store
            .get_server(&server_id)
            .await
            .map_err(|_| LookupError::UnknownServer)?;
        ssh_manager_core::profiles::effective_notes(&server, state.profile_store.as_ref())
            .await
            .map_err(|_| LookupError::UnknownServer)
    }

    async fn propose_action(
        &self,
        server_id: ServerId,
        action: AiAction,
        client_name: Option<String>,
    ) -> Result<ActionOutcome, LookupError> {
        if !self.is_allowed(&server_id) {
            return Err(LookupError::UnknownServer);
        }

        let state = self.state();
        let server_name = state
            .profile_store
            .get_server(&server_id)
            .await
            .map(|s| s.name)
            .unwrap_or_else(|_| server_id.0.to_string());

        let (session_id, session) = self
            .ensure_session(server_id)
            .await
            .map_err(|_| LookupError::UnknownServer)?;

        self.notify_pending_confirmation(&server_name, client_name.as_deref());

        let capture = CaptureEmitter::new(&self.app);

        handle_mcp_action_proposed(
            &session,
            session_id,
            action,
            &capture,
            state.profile_store.as_ref(),
            &state.pending_action_confirmations,
            client_name,
        )
        .await;

        Ok(capture.into_outcome())
    }
}

/// Fängt genau die Events ein, die während **eines** `handle_action_proposed`
/// -Aufrufs entstehen können, um daraus das `ActionOutcome` für die
/// MCP-Antwort abzuleiten — und reicht dabei jedes Event unverändert an den
/// echten `AppHandle` weiter, damit die UI wie gewohnt reagiert (derselbe
/// Bestätigungsdialog-Mechanismus, Spec 0028, Abschnitt 3). Eine eigene
/// Instanz pro Aufruf, daher keine Verwechslungsgefahr mit gleichzeitiger,
/// unabhängiger Chat-Aktivität auf derselben Session (die läuft über den
/// `AppHandle` direkt, nicht durch diesen Wrapper).
///
/// Warum event-basiert statt den Rückgabewert von
/// `handle_action_proposed`/`handle_user_decision` (`bool`) zu nutzen: der
/// gibt nur "Folgerunde nötig" zurück (Spec 0021), nicht das tatsächliche
/// Ergebnis — und eine Ablehnung durch den Nutzer (`Confirm` → "Ablehnen")
/// erzeugt überhaupt kein Event, nur einen Kontext-Eintrag. Die
/// Ableitungsregel unten deckt daher alle vier Fälle ab: Ergebnis-Event →
/// genehmigt, Fehler-Event → fehlgeschlagen, `Deny`-Entscheidung im
/// `chat-action-proposed`-Event → von der Filter-Engine blockiert, sonst
/// (Entscheidung war `Confirm`, aber weder Ergebnis- noch Fehler-Event kam)
/// → vom Nutzer abgelehnt.
struct CaptureEmitter<'a> {
    // `&dyn EventEmitter` statt konkret `&AppHandle` — lässt sich damit in
    // Tests gegen `TestEmitter` prüfen, ohne eine echte Tauri-`AppHandle`
    // aufbauen zu müssen.
    inner: &'a dyn EventEmitter,
    decision: std::sync::Mutex<Option<serde_json::Value>>,
    result: std::sync::Mutex<Option<serde_json::Value>>,
    error: std::sync::Mutex<Option<String>>,
}

impl<'a> CaptureEmitter<'a> {
    fn new(inner: &'a dyn EventEmitter) -> Self {
        Self {
            inner,
            decision: std::sync::Mutex::new(None),
            result: std::sync::Mutex::new(None),
            error: std::sync::Mutex::new(None),
        }
    }

    fn into_outcome(self) -> ActionOutcome {
        if let Some(result) = self.result.into_inner().expect("Mutex vergiftet") {
            return ActionOutcome::Approved {
                summary: format_action_result(&result),
            };
        }
        if let Some(message) = self.error.into_inner().expect("Mutex vergiftet") {
            return ActionOutcome::Failed { message };
        }
        let decision = self.decision.into_inner().expect("Mutex vergiftet");
        if let Some(deny_reason) = decision.as_ref().and_then(|d| d.get("Deny")?.get("reason")) {
            let reason = deny_reason.as_str().unwrap_or("blockiert").to_string();
            return ActionOutcome::Rejected {
                reason: format!("von der Filter-Engine blockiert: {reason}"),
            };
        }
        ActionOutcome::Rejected {
            reason: "vom Nutzer in der App abgelehnt".to_string(),
        }
    }
}

impl EventEmitter for CaptureEmitter<'_> {
    fn emit_event(&self, event: &str, payload: serde_json::Value) {
        match event {
            "chat-action-proposed" => {
                *self.decision.lock().expect("Mutex vergiftet") = Some(payload["decision"].clone());
            }
            "chat-action-result" => {
                *self.result.lock().expect("Mutex vergiftet") = Some(payload["result"].clone());
            }
            "chat-error" => {
                if let Some(message) = payload["message"].as_str() {
                    *self.error.lock().expect("Mutex vergiftet") = Some(message.to_string());
                }
            }
            _ => {}
        }
        self.inner.emit_event(event, payload);
    }
}

/// Wandelt den `result`-Wert eines `chat-action-result`-Events
/// (`ActionResultPayload`, intern per `kind` getaggt — s.
/// `crate::events`-Moduldoc) in einen für den MCP-Client lesbaren Text um.
fn format_action_result(result: &serde_json::Value) -> String {
    match result["kind"].as_str() {
        Some("command") => {
            let stdout = result["stdout"].as_str().unwrap_or_default();
            let stderr = result["stderr"].as_str().unwrap_or_default();
            let exit_code = result["exitCode"].as_i64();
            let cancelled = result["cancelled"].as_bool().unwrap_or(false);
            let mut text = if cancelled {
                "Kommando wurde vom Nutzer abgebrochen, bevor es beendet war.\n\n".to_string()
            } else {
                match exit_code {
                    Some(code) => format!("Exit-Code: {code}\n\n"),
                    None => String::new(),
                }
            };
            text.push_str(&format!("stdout:\n{stdout}"));
            if !stderr.is_empty() {
                text.push_str(&format!("\n\nstderr:\n{stderr}"));
            }
            text
        }
        Some("noteUpdate") => result["summary"].as_str().unwrap_or_default().to_string(),
        Some("fileRead") => {
            let path = result["path"].as_str().unwrap_or_default();
            let content = result["content"].as_str().unwrap_or_default();
            format!("Inhalt von '{path}':\n\n{content}")
        }
        Some("fileWrite") => {
            let path = result["path"].as_str().unwrap_or_default();
            match result["backupPath"].as_str() {
                Some(backup) => format!("Datei '{path}' geschrieben (Backup: '{backup}')."),
                None => format!("Datei '{path}' neu angelegt."),
            }
        }
        _ => result.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::TestEmitter;

    #[test]
    fn test_format_action_result_command_success() {
        let result = serde_json::json!({
            "kind": "command",
            "command": "ls -la",
            "stdout": "total 0",
            "stderr": "",
            "exitCode": 0,
            "cancelled": false,
        });
        let text = format_action_result(&result);
        assert!(text.contains("Exit-Code: 0"));
        assert!(text.contains("stdout:\ntotal 0"));
        assert!(!text.contains("stderr:"));
    }

    #[test]
    fn test_format_action_result_command_with_stderr() {
        let result = serde_json::json!({
            "kind": "command", "command": "false", "stdout": "", "stderr": "boom",
            "exitCode": 1, "cancelled": false,
        });
        let text = format_action_result(&result);
        assert!(text.contains("stderr:\nboom"));
    }

    #[test]
    fn test_format_action_result_cancelled_command_omits_exit_code() {
        let result = serde_json::json!({
            "kind": "command", "command": "journalctl -f", "stdout": "line1", "stderr": "",
            "exitCode": null, "cancelled": true,
        });
        let text = format_action_result(&result);
        assert!(text.contains("abgebrochen"));
        assert!(!text.contains("Exit-Code"));
    }

    #[test]
    fn test_format_action_result_file_read() {
        let result = serde_json::json!({
            "kind": "fileRead", "path": "/etc/hosts", "content": "127.0.0.1 localhost",
        });
        let text = format_action_result(&result);
        assert!(text.contains("/etc/hosts"));
        assert!(text.contains("127.0.0.1 localhost"));
    }

    #[test]
    fn test_format_action_result_file_write_with_backup() {
        let result = serde_json::json!({
            "kind": "fileWrite", "path": "/etc/nginx/nginx.conf",
            "backupPath": "/etc/nginx/nginx.conf.smartssh-backup-20260101", "usedSudoPassword": false,
        });
        let text = format_action_result(&result);
        assert!(text.contains("geschrieben"));
        assert!(text.contains("smartssh-backup"));
    }

    #[test]
    fn test_format_action_result_file_write_new_file_has_no_backup_mention() {
        let result = serde_json::json!({
            "kind": "fileWrite", "path": "/home/deploy/new.txt", "backupPath": null, "usedSudoPassword": false,
        });
        let text = format_action_result(&result);
        assert!(text.contains("neu angelegt"));
        assert!(!text.contains("Backup"));
    }

    #[test]
    fn test_format_action_result_note_update() {
        let result = serde_json::json!({ "kind": "noteUpdate", "summary": "Notiz für Server web-01 aktualisiert." });
        assert_eq!(
            format_action_result(&result),
            "Notiz für Server web-01 aktualisiert."
        );
    }

    #[test]
    fn test_capture_emitter_approved_on_result_event() {
        let inner = TestEmitter::default();
        let capture = CaptureEmitter::new(&inner);
        capture.emit_event(
            "chat-action-proposed",
            serde_json::json!({ "decision": "AutoExec" }),
        );
        capture.emit_event(
            "chat-action-result",
            serde_json::json!({ "result": { "kind": "noteUpdate", "summary": "erledigt" } }),
        );
        match capture.into_outcome() {
            ActionOutcome::Approved { summary } => assert_eq!(summary, "erledigt"),
            other => panic!("erwartete Approved, war: {other:?}"),
        }
    }

    #[test]
    fn test_capture_emitter_failed_on_error_event() {
        let inner = TestEmitter::default();
        let capture = CaptureEmitter::new(&inner);
        capture.emit_event(
            "chat-error",
            serde_json::json!({ "sessionId": "x", "message": "SFTP-Fehler: No such file" }),
        );
        match capture.into_outcome() {
            ActionOutcome::Failed { message } => assert!(message.contains("No such file")),
            other => panic!("erwartete Failed, war: {other:?}"),
        }
    }

    #[test]
    fn test_capture_emitter_rejected_when_filter_engine_denies() {
        let inner = TestEmitter::default();
        let capture = CaptureEmitter::new(&inner);
        capture.emit_event(
            "chat-action-proposed",
            serde_json::json!({ "decision": { "Deny": { "reason": "auf der Blacklist", "code": "X" } } }),
        );
        match capture.into_outcome() {
            ActionOutcome::Rejected { reason } => assert!(reason.contains("auf der Blacklist")),
            other => panic!("erwartete Rejected, war: {other:?}"),
        }
    }

    /// Spec 0028, Abschnitt 5: kein Ereignis entsteht, wenn der Nutzer im
    /// Bestätigungsdialog "Ablehnen" klickt (`handle_user_decision`s
    /// `Deny`-Zweig pusht nur in `session.context.history`, s. dortiger
    /// Kommentar) — genau der Fall, den `into_outcome`s letzter
    /// Rückfall-Zweig abdecken muss.
    #[test]
    fn test_capture_emitter_rejected_when_user_denies_no_event_fires() {
        let inner = TestEmitter::default();
        let capture = CaptureEmitter::new(&inner);
        capture.emit_event(
            "chat-action-proposed",
            serde_json::json!({ "decision": { "Confirm": { "reason": "r", "code": "c" } } }),
        );
        match capture.into_outcome() {
            ActionOutcome::Rejected { reason } => assert!(reason.contains("Nutzer")),
            other => panic!("erwartete Rejected, war: {other:?}"),
        }
    }

    #[test]
    fn test_capture_emitter_forwards_every_event_to_wrapped_emitter() {
        let inner = TestEmitter::default();
        {
            let capture = CaptureEmitter::new(&inner);
            capture.emit_event(
                "chat-action-proposed",
                serde_json::json!({ "decision": "AutoExec" }),
            );
            capture.emit_event(
                "chat-action-result",
                serde_json::json!({ "result": { "kind": "noteUpdate", "summary": "ok" } }),
            );
        }
        let events = inner.events.lock().unwrap();
        let names: Vec<&str> = events.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["chat-action-proposed", "chat-action-result"]);
    }
}
