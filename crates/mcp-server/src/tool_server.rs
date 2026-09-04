//! Die sechs MCP-Tools aus Spec 0028, Abschnitt 4, als `rmcp`-Tool-Router.
//! Übersetzt jeden aktionsauslösenden Aufruf 1:1 in eine `AiAction` und
//! reicht sie unverändert an [`McpBackend::propose_action`] weiter — diese
//! Datei selbst enthält keine Ausführungslogik, nur Protokoll-Mapping,
//! Timeout-Handling (Abschnitt 7) und Logging (Abschnitt 6).

use std::sync::Arc;
use std::time::Duration;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler};
use serde::Deserialize;
use ssh_manager_core::profiles::{AiAction, NoteTargetSelector};
use ssh_manager_core::shared::ServerId;
use uuid::Uuid;

use crate::backend::{ActionOutcome, LookupError, McpBackend};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetServerNotesArgs {
    /// Server-ID aus `list_servers`.
    pub server_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProposeCommandArgs {
    /// Server-ID aus `list_servers`.
    pub server_id: String,
    /// Das vorzuschlagende Shell-Kommando — läuft wie jeder interne
    /// KI-Vorschlag durch die Filter-Engine, landet aber (Spec 0028,
    /// Abschnitt 5) immer bei einer Bestätigung im UI.
    pub command: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadRemoteFileArgs {
    pub server_id: String,
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WriteRemoteFileArgs {
    pub server_id: String,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProposeNoteUpdateArgs {
    pub server_id: String,
    /// Vollständiger neuer Notiz-Text, kein Diff.
    pub new_content: String,
}

fn parse_server_id(raw: &str) -> Result<ServerId, McpError> {
    Uuid::parse_str(raw)
        .map(ServerId)
        .map_err(|_| McpError::invalid_params("server_id ist keine gültige UUID", None))
}

fn lookup_error_to_tool_result(err: LookupError) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(err.to_string())])
}

fn outcome_to_tool_result(outcome: ActionOutcome) -> CallToolResult {
    match outcome {
        ActionOutcome::Approved { summary } => {
            CallToolResult::success(vec![ContentBlock::text(summary)])
        }
        ActionOutcome::Rejected { reason } => {
            CallToolResult::error(vec![ContentBlock::text(format!("Abgelehnt: {reason}"))])
        }
        ActionOutcome::Failed { message } => CallToolResult::error(vec![ContentBlock::text(
            format!("Fehlgeschlagen: {message}"),
        )]),
    }
}

/// Text aus Spec 0028, Abschnitt 7 — bewusst wörtlich übernommen, damit ein
/// aufrufender Agent (z. B. Claude Code) exakt versteht, dass die Anfrage in
/// der App weiterhin offen ist, nicht verworfen wurde.
const TIMEOUT_MESSAGE: &str = "Zeitüberschreitung beim Warten auf Bestätigung — die Anfrage steht \
     weiterhin in der App zur Entscheidung offen.";

#[derive(Clone)]
pub struct SmartSshMcpServer {
    backend: Arc<dyn McpBackend>,
    confirm_timeout: Duration,
    // Wird von #[tool_handler]-generiertem Code gelesen (list_tools/call_tool),
    // nicht direkt hier — rustc erkennt das nicht als Nutzung (dasselbe
    // `#[allow]` setzt auch das rmcp-eigene Counter-Beispiel).
    #[allow(dead_code)]
    tool_router: ToolRouter<SmartSshMcpServer>,
}

#[tool_router]
impl SmartSshMcpServer {
    pub fn new(backend: Arc<dyn McpBackend>, confirm_timeout: Duration) -> Self {
        Self {
            backend,
            confirm_timeout,
            tool_router: Self::tool_router(),
        }
    }

    fn client_name(ctx: &RequestContext<RoleServer>) -> Option<String> {
        ctx.peer
            .peer_info()
            .map(|info| info.client_info.name.clone())
    }

    /// Gemeinsamer Pfad für die vier aktionsauslösenden Tools: loggt den
    /// Aufruf (`origin: "mcp"`, Spec 0028 Abschnitt 6), spawnt den
    /// eigentlichen `propose_action`-Aufruf als eigene Task (statt ihn
    /// direkt zu awaiten), damit ein Timeout beim späteren `select!` nur
    /// den *Tool-Call selbst* abbricht — die bereits laufende
    /// Bestätigungsanfrage im UI läuft unbeeinflusst weiter und wird ganz
    /// normal entschieden (Spec 0028, Abschnitt 7). Ein direktes
    /// `tokio::time::timeout(...)` um den Aufruf herum würde stattdessen
    /// die Future selbst droppen und damit den `oneshot`-Receiver im
    /// `ConfirmationRegistry`-Eintrag verwaisen lassen — ein späterer Klick
    /// auf "Genehmigen" im UI würde dann augenscheinlich nichts mehr
    /// bewirken, weil niemand mehr auf das Ergebnis wartet.
    async fn run_confirmable(
        &self,
        tool_name: &'static str,
        server_id_raw: &str,
        action: AiAction,
        client_name: Option<String>,
    ) -> Result<CallToolResult, McpError> {
        let server_id = parse_server_id(server_id_raw)?;

        tracing::info!(
            origin = "mcp",
            tool = tool_name,
            server_id = %server_id_raw,
            client = ?client_name,
            "mcp tool call received"
        );

        let backend = self.backend.clone();
        let action_for_task = action.clone();
        let client_name_for_task = client_name.clone();
        let mut join_handle = tokio::spawn(async move {
            backend
                .propose_action(server_id, action_for_task, client_name_for_task)
                .await
        });

        let result = tokio::select! {
            joined = &mut join_handle => {
                match joined {
                    Ok(Ok(outcome)) => {
                        tracing::info!(
                            origin = "mcp", tool = tool_name, server_id = %server_id_raw,
                            outcome = ?outcome, "mcp tool call completed"
                        );
                        outcome_to_tool_result(outcome)
                    }
                    Ok(Err(err)) => {
                        tracing::warn!(
                            origin = "mcp", tool = tool_name, server_id = %server_id_raw,
                            error = %err, "mcp tool call rejected: unknown server"
                        );
                        lookup_error_to_tool_result(err)
                    }
                    Err(join_err) => {
                        tracing::error!(
                            origin = "mcp", tool = tool_name, server_id = %server_id_raw,
                            error = %join_err, "mcp backend task panicked"
                        );
                        CallToolResult::error(vec![ContentBlock::text(
                            "interner Fehler bei der Verarbeitung der Anfrage",
                        )])
                    }
                }
            }
            _ = tokio::time::sleep(self.confirm_timeout) => {
                tracing::warn!(
                    origin = "mcp", tool = tool_name, server_id = %server_id_raw,
                    timeout_secs = self.confirm_timeout.as_secs(),
                    "mcp tool call timed out waiting for confirmation"
                );
                CallToolResult::error(vec![ContentBlock::text(TIMEOUT_MESSAGE)])
            }
        };

        Ok(result)
    }

    #[tool(
        description = "Listet alle Server, die über MCP ansprechbar sind (Server-Allow-Liste in den Smart-SSH-Einstellungen)."
    )]
    async fn list_servers(&self) -> Result<CallToolResult, McpError> {
        tracing::info!(
            origin = "mcp",
            tool = "list_servers",
            "mcp tool call received"
        );
        let servers = self.backend.list_servers().await;
        let json = serde_json::json!(servers
            .into_iter()
            .map(|s| serde_json::json!({ "server_id": s.id.0.to_string(), "name": s.name }))
            .collect::<Vec<_>>());
        Ok(CallToolResult::success(vec![ContentBlock::text(
            json.to_string(),
        )]))
    }

    #[tool(
        description = "Liest die effektiven Notizen eines Servers (informativ, keine Bestätigung nötig)."
    )]
    async fn get_server_notes(
        &self,
        Parameters(GetServerNotesArgs { server_id }): Parameters<GetServerNotesArgs>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!(
            origin = "mcp", tool = "get_server_notes", server_id = %server_id,
            "mcp tool call received"
        );
        let id = parse_server_id(&server_id)?;
        match self.backend.server_notes(id).await {
            Ok(notes) => Ok(CallToolResult::success(vec![ContentBlock::text(notes)])),
            Err(err) => Ok(lookup_error_to_tool_result(err)),
        }
    }

    #[tool(
        description = "Schlägt ein Shell-Kommando auf einem Server vor. Muss vom Nutzer in der App bestätigt werden, auch wenn eine passende Allow-Regel existiert."
    )]
    async fn propose_command(
        &self,
        Parameters(ProposeCommandArgs { server_id, command }): Parameters<ProposeCommandArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.run_confirmable(
            "propose_command",
            &server_id,
            AiAction::SuggestCommand { command },
            Self::client_name(&ctx),
        )
        .await
    }

    #[tool(
        description = "Liest eine Datei per SFTP von einem Server. Muss vom Nutzer in der App bestätigt werden."
    )]
    async fn read_remote_file(
        &self,
        Parameters(ReadRemoteFileArgs { server_id, path }): Parameters<ReadRemoteFileArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.run_confirmable(
            "read_remote_file",
            &server_id,
            AiAction::ReadRemoteFile { path },
            Self::client_name(&ctx),
        )
        .await
    }

    #[tool(
        description = "Schreibt eine Datei per SFTP auf einem Server (mit automatischem Backup). Muss vom Nutzer in der App bestätigt werden."
    )]
    async fn write_remote_file(
        &self,
        Parameters(WriteRemoteFileArgs {
            server_id,
            path,
            content,
        }): Parameters<WriteRemoteFileArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.run_confirmable(
            "write_remote_file",
            &server_id,
            AiAction::WriteRemoteFile { path, content },
            Self::client_name(&ctx),
        )
        .await
    }

    #[tool(
        description = "Schlägt eine aktualisierte Notiz für einen Server vor. Muss vom Nutzer in der App bestätigt werden."
    )]
    async fn propose_note_update(
        &self,
        Parameters(ProposeNoteUpdateArgs {
            server_id,
            new_content,
        }): Parameters<ProposeNoteUpdateArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.run_confirmable(
            "propose_note_update",
            &server_id,
            AiAction::ProposeNoteUpdate {
                target: NoteTargetSelector::CurrentServer,
                new_content,
            },
            Self::client_name(&ctx),
        )
        .await
    }
}

#[tool_handler]
impl ServerHandler for SmartSshMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            // `Implementation::from_build_env()` liest `CARGO_PKG_NAME` zur
            // Kompilierzeit von `rmcp` selbst aus (nicht von dieser Crate)
            // und zeigt MCP-Clients dadurch fälschlich "rmcp" statt eines
            // erkennbaren Servernamens — deshalb explizit gesetzt.
            .with_server_info(Implementation::new("smart-ssh", env!("CARGO_PKG_VERSION")))
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "Smart SSH exponiert kontrollierten SSH-Zugriff auf freigegebene Server. \
                 Jede vorgeschlagene Aktion (Kommando, Datei lesen/schreiben, Notiz \
                 aktualisieren) muss vom Nutzer in der App bestätigt werden."
                    .to_string(),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex as StdMutex;

    use super::*;
    use crate::backend::ServerSummary;

    type RecordedCall = (ServerId, AiAction, Option<String>);

    /// Test-Double für [`McpBackend`]: zeichnet jeden `propose_action`-Aufruf
    /// auf (für die Mapping-Tests) und liefert ein konfigurierbares
    /// Ergebnis — entweder sofort, mit künstlicher Verzögerung (Timeout-Test)
    /// oder als `LookupError::UnknownServer` (Allow-Listen-Test).
    struct MockBackend {
        servers: Vec<ServerSummary>,
        unknown: bool,
        outcome: ActionOutcome,
        delay: Option<Duration>,
        recorded: Arc<StdMutex<Vec<RecordedCall>>>,
        /// Wird erst NACH `delay` auf `true` gesetzt — beweist im
        /// Timeout-Test, dass der Hintergrund-Aufruf trotz
        /// Tool-Call-Timeout tatsächlich zu Ende lief (Spec 0028,
        /// Abschnitt 7), statt beim Timeout abgebrochen zu werden.
        completed: Arc<AtomicBool>,
    }

    impl MockBackend {
        fn new(outcome: ActionOutcome) -> Self {
            Self {
                servers: Vec::new(),
                unknown: false,
                outcome,
                delay: None,
                recorded: Arc::new(StdMutex::new(Vec::new())),
                completed: Arc::new(AtomicBool::new(false)),
            }
        }

        fn unknown_server() -> Self {
            Self {
                unknown: true,
                ..Self::new(ActionOutcome::Approved {
                    summary: String::new(),
                })
            }
        }

        fn with_servers(mut self, servers: Vec<ServerSummary>) -> Self {
            self.servers = servers;
            self
        }

        fn with_delay(mut self, delay: Duration) -> Self {
            self.delay = Some(delay);
            self
        }
    }

    #[async_trait::async_trait]
    impl McpBackend for MockBackend {
        async fn list_servers(&self) -> Vec<ServerSummary> {
            self.servers.clone()
        }

        async fn server_notes(&self, server_id: ServerId) -> Result<String, LookupError> {
            if self.unknown {
                return Err(LookupError::UnknownServer);
            }
            Ok(format!("Notizen für {}", server_id.0))
        }

        async fn propose_action(
            &self,
            server_id: ServerId,
            action: AiAction,
            client_name: Option<String>,
        ) -> Result<ActionOutcome, LookupError> {
            self.recorded
                .lock()
                .unwrap()
                .push((server_id, action, client_name));
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            self.completed.store(true, Ordering::SeqCst);
            if self.unknown {
                return Err(LookupError::UnknownServer);
            }
            Ok(self.outcome.clone())
        }
    }

    fn server(
        backend: MockBackend,
        confirm_timeout: Duration,
    ) -> (SmartSshMcpServer, Arc<AtomicBool>) {
        let completed = backend.completed.clone();
        (
            SmartSshMcpServer::new(Arc::new(backend), confirm_timeout),
            completed,
        )
    }

    fn some_id() -> String {
        Uuid::new_v4().to_string()
    }

    #[tokio::test]
    async fn test_propose_command_maps_to_suggest_command_action() {
        let backend = MockBackend::new(ActionOutcome::Approved {
            summary: "ok".to_string(),
        });
        let recorded = backend.recorded.clone();
        let (server, _) = server(backend, Duration::from_secs(5));
        let id = some_id();

        server
            .run_confirmable(
                "propose_command",
                &id,
                AiAction::SuggestCommand {
                    command: "uname -a".to_string(),
                },
                None,
            )
            .await
            .unwrap();

        let calls = recorded.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(matches!(
            &calls[0].1,
            AiAction::SuggestCommand { command } if command == "uname -a"
        ));
    }

    #[tokio::test]
    async fn test_read_remote_file_maps_to_read_remote_file_action() {
        let backend = MockBackend::new(ActionOutcome::Approved {
            summary: "ok".to_string(),
        });
        let recorded = backend.recorded.clone();
        let (server, _) = server(backend, Duration::from_secs(5));

        server
            .run_confirmable(
                "read_remote_file",
                &some_id(),
                AiAction::ReadRemoteFile {
                    path: "/etc/nginx/nginx.conf".to_string(),
                },
                None,
            )
            .await
            .unwrap();

        let calls = recorded.lock().unwrap();
        assert!(matches!(
            &calls[0].1,
            AiAction::ReadRemoteFile { path } if path == "/etc/nginx/nginx.conf"
        ));
    }

    #[tokio::test]
    async fn test_write_remote_file_maps_to_write_remote_file_action() {
        let backend = MockBackend::new(ActionOutcome::Approved {
            summary: "ok".to_string(),
        });
        let recorded = backend.recorded.clone();
        let (server, _) = server(backend, Duration::from_secs(5));

        server
            .run_confirmable(
                "write_remote_file",
                &some_id(),
                AiAction::WriteRemoteFile {
                    path: "/etc/nginx/nginx.conf".to_string(),
                    content: "server {}".to_string(),
                },
                None,
            )
            .await
            .unwrap();

        let calls = recorded.lock().unwrap();
        assert!(matches!(
            &calls[0].1,
            AiAction::WriteRemoteFile { path, content }
                if path == "/etc/nginx/nginx.conf" && content == "server {}"
        ));
    }

    #[tokio::test]
    async fn test_propose_note_update_maps_to_current_server_target() {
        let backend = MockBackend::new(ActionOutcome::Approved {
            summary: "ok".to_string(),
        });
        let recorded = backend.recorded.clone();
        let (server, _) = server(backend, Duration::from_secs(5));

        server
            .run_confirmable(
                "propose_note_update",
                &some_id(),
                AiAction::ProposeNoteUpdate {
                    target: NoteTargetSelector::CurrentServer,
                    new_content: "gepatcht am 2026-09-03".to_string(),
                },
                Some("Claude Code".to_string()),
            )
            .await
            .unwrap();

        let calls = recorded.lock().unwrap();
        assert!(matches!(
            &calls[0].1,
            AiAction::ProposeNoteUpdate { target: NoteTargetSelector::CurrentServer, new_content }
                if new_content == "gepatcht am 2026-09-03"
        ));
        assert_eq!(calls[0].2.as_deref(), Some("Claude Code"));
    }

    /// Spec 0028, Abschnitt 6: ein nicht auf der Allow-Liste stehender (oder
    /// tatsächlich nicht existierender) Server liefert "unbekannter
    /// Server", nie eine Formulierung, die auf "existiert, aber
    /// verweigert" hindeutet — sonst wäre die Existenz nicht freigegebener
    /// Server über die Fehlermeldung erkennbar.
    #[tokio::test]
    async fn test_unknown_server_yields_unbekannter_server_not_access_denied() {
        let (server, _) = server(MockBackend::unknown_server(), Duration::from_secs(5));

        let result = server
            .run_confirmable(
                "propose_command",
                &some_id(),
                AiAction::SuggestCommand {
                    command: "ls".to_string(),
                },
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        let text = tool_result_text(&result);
        assert!(text.contains("unbekannter Server"), "war: {text}");
        assert!(!text.to_lowercase().contains("zugriff verweigert"));
    }

    /// Spec 0028, Abschnitt 5 (Regressionstest gehört zur eigentlichen
    /// Downgrade-Durchsetzung — siehe
    /// `app-shell::orchestration::tests::test_mcp_origin_downgrades_autoexec_to_confirm`,
    /// die die reale Filter-Engine/`handle_action_proposed`-Logik prüft;
    /// hier wird nur sichergestellt, dass diese Crate selbst keinen
    /// zweiten, das Downgrade umgehenden Ausführungspfad hat — die
    /// `AiAction` geht unverändert und ohne eigene Vorab-Auswertung an
    /// [`McpBackend::propose_action`].)
    #[tokio::test]
    async fn test_confirm_timeout_returns_timeout_message_without_cancelling_backend_call() {
        let backend = MockBackend::new(ActionOutcome::Approved {
            summary: "zu spät".to_string(),
        })
        .with_delay(Duration::from_millis(150));
        let (server, completed) = server(backend, Duration::from_millis(20));

        let result = server
            .run_confirmable(
                "propose_command",
                &some_id(),
                AiAction::SuggestCommand {
                    command: "long-running-thing".to_string(),
                },
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        assert!(tool_result_text(&result).contains(TIMEOUT_MESSAGE));
        assert!(
            !completed.load(Ordering::SeqCst),
            "Backend-Aufruf sollte beim Timeout noch nicht fertig sein"
        );

        // Der Hintergrund-Aufruf läuft unabhängig vom bereits
        // zurückgegebenen Timeout weiter — nach Ablauf der künstlichen
        // Verzögerung muss er trotzdem durchgelaufen sein.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            completed.load(Ordering::SeqCst),
            "Backend-Aufruf hätte im Hintergrund trotzdem zu Ende laufen müssen"
        );
    }

    #[tokio::test]
    async fn test_list_servers_returns_configured_servers() {
        let backend = MockBackend::new(ActionOutcome::Approved {
            summary: String::new(),
        })
        .with_servers(vec![ServerSummary {
            id: ServerId(Uuid::new_v4()),
            name: "web-01".to_string(),
        }]);
        let (server, _) = server(backend, Duration::from_secs(5));

        let result = server.list_servers().await.unwrap();
        let text = tool_result_text(&result);
        assert!(text.contains("web-01"));
    }

    #[tokio::test]
    async fn test_get_server_notes_unknown_server_yields_unbekannter_server() {
        let (server, _) = server(MockBackend::unknown_server(), Duration::from_secs(5));

        let result = server
            .get_server_notes(Parameters(GetServerNotesArgs {
                server_id: some_id(),
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        assert!(tool_result_text(&result).contains("unbekannter Server"));
    }

    #[tokio::test]
    async fn test_get_server_notes_success() {
        let (server, _) = server(
            MockBackend::new(ActionOutcome::Approved {
                summary: String::new(),
            }),
            Duration::from_secs(5),
        );

        let result = server
            .get_server_notes(Parameters(GetServerNotesArgs {
                server_id: some_id(),
            }))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(false));
        assert!(tool_result_text(&result).starts_with("Notizen für"));
    }

    #[test]
    fn test_invalid_server_id_is_rejected_as_invalid_params() {
        let err = parse_server_id("not-a-uuid").unwrap_err();
        assert!(err.message.contains("UUID"));
    }

    fn tool_result_text(result: &CallToolResult) -> String {
        result
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
