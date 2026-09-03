//! Startet den lokalen MCP-HTTP-Server (Spec 0028, Abschnitt 8: Streamable
//! HTTP, gebunden an `127.0.0.1`, konfigurierbarer Port).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;
use rmcp::transport::StreamableHttpServerConfig;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::auth::{require_bearer_token, SharedToken};
use crate::backend::McpBackend;
use crate::tool_server::SmartSshMcpServer;

/// Spec 0028, Abschnitt 8: "Port konfigurierbar (Default z. B. `47823`,
/// außerhalb üblicher Kollisionsbereiche)".
pub const DEFAULT_PORT: u16 = 47823;

/// Spec 0028, Abschnitt 7: "konfigurierbares Timeout (Default 5 Minuten)".
pub const DEFAULT_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub bind_addr: SocketAddr,
    pub confirm_timeout: Duration,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)),
            confirm_timeout: DEFAULT_CONFIRM_TIMEOUT,
        }
    }
}

/// Griff auf einen laufenden Server — hält die tatsächlich gebundene
/// Adresse (relevant für Tests, die mit Port `0` einen freien Port vom OS
/// zuweisen lassen) und erlaubt sauberes Beenden.
pub struct McpServerHandle {
    pub local_addr: SocketAddr,
    shutdown: CancellationToken,
    join_handle: tokio::task::JoinHandle<()>,
}

impl McpServerHandle {
    /// Beendet den HTTP-Listener und wartet, bis er tatsächlich
    /// geschlossen ist. Laufende `propose_action`-Aufrufe im Hintergrund
    /// (Spec 0028, Abschnitt 7) sind davon unabhängig — sie werden über den
    /// `McpBackend`, nicht über diesen Server-Prozess, zu Ende geführt.
    pub async fn shutdown(self) {
        self.shutdown.cancel();
        let _ = self.join_handle.await;
    }
}

/// Startet den Server; `token` wird dabei nicht kopiert, sondern nur
/// geklont (`Arc`-basiert), damit ein späteres `token.set(...)` (Spec 0028,
/// "Neu generieren") auch diesen bereits laufenden Server sofort betrifft.
pub async fn serve(
    config: McpServerConfig,
    backend: Arc<dyn McpBackend>,
    token: SharedToken,
) -> std::io::Result<McpServerHandle> {
    let listener = TcpListener::bind(config.bind_addr).await?;
    let local_addr = listener.local_addr()?;

    let cancellation = CancellationToken::new();
    let confirm_timeout = config.confirm_timeout;

    let mcp_service: StreamableHttpService<SmartSshMcpServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(SmartSshMcpServer::new(backend.clone(), confirm_timeout)),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default()
                .with_cancellation_token(cancellation.child_token()),
        );

    let router = axum::Router::new().nest_service("/mcp", mcp_service).layer(
        axum::middleware::from_fn_with_state(token, require_bearer_token),
    );

    let shutdown_signal = cancellation.clone();
    let join_handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown_signal.cancelled().await })
            .await;
    });

    Ok(McpServerHandle {
        local_addr,
        shutdown: cancellation,
        join_handle,
    })
}
