//! Integrationstest (Spec 0028, Abschnitt 10): startet den echten
//! HTTP-Server lokal und spricht ihn über den `rmcp`-eigenen
//! Streamable-HTTP-Client an — kein direkt aufgerufener Rust-Code, echter
//! Netzwerk-Roundtrip über `127.0.0.1` auf einem vom OS zugewiesenen freien
//! Port (Bind-Adresse `127.0.0.1:0`).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mcp_server::{
    ActionOutcome, LookupError, McpBackend, McpServerConfig, ServerSummary, SharedToken,
};
use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use ssh_manager_core::profiles::AiAction;
use ssh_manager_core::shared::ServerId;
use uuid::Uuid;

struct TestBackend {
    known_server: ServerId,
    /// Spec 0097, K1 (F6): Ist das gesetzt, wartet `propose_action` auf
    /// dieses Signal statt auf eine feste Dauer — der Test gibt es erst
    /// frei, nachdem er geprüft hat, dass die Antwort schon da ist.
    release: Option<Arc<tokio::sync::Notify>>,
    completed: Arc<AtomicBool>,
}

#[async_trait]
impl McpBackend for TestBackend {
    async fn list_servers(&self) -> Vec<ServerSummary> {
        vec![ServerSummary {
            id: self.known_server,
            name: "integration-test-server".to_string(),
        }]
    }

    async fn server_notes(&self, server_id: ServerId) -> Result<String, LookupError> {
        if server_id == self.known_server {
            Ok("keine besonderen Notizen".to_string())
        } else {
            Err(LookupError::UnknownServer)
        }
    }

    async fn propose_action(
        &self,
        server_id: ServerId,
        _action: AiAction,
        _client_name: Option<String>,
    ) -> Result<ActionOutcome, LookupError> {
        if server_id != self.known_server {
            return Err(LookupError::UnknownServer);
        }
        if let Some(release) = &self.release {
            release.notified().await;
        }
        self.completed.store(true, Ordering::SeqCst);
        Ok(ActionOutcome::Approved {
            summary: "erfolgreich ausgeführt (Integrationstest)".to_string(),
        })
    }
}

async fn start_server(
    backend: TestBackend,
    token: &str,
    confirm_timeout: Duration,
) -> (mcp_server::McpServerHandle, SocketAddr) {
    let config = McpServerConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        confirm_timeout,
    };
    let handle = mcp_server::serve(config, Arc::new(backend), SharedToken::new(token))
        .await
        .expect("Server sollte auf einem freien Port starten");
    let addr = handle.local_addr;
    (handle, addr)
}

fn connect_transport(
    addr: SocketAddr,
    token: &str,
) -> StreamableHttpClientTransport<reqwest::Client> {
    StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!("http://{addr}/mcp"))
            .auth_header(token),
    )
}

fn client_info() -> ClientInfo {
    ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("smart-ssh-integration-test", "0.0.1"),
    )
}

fn tool_result_text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| match block {
            rmcp::model::ContentBlock::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn test_wrong_token_is_rejected_over_real_http() {
    let known_server = ServerId::new();
    let (handle, addr) = start_server(
        TestBackend {
            known_server,
            release: None,
            completed: Arc::new(AtomicBool::new(false)),
        },
        "correct-token",
        Duration::from_secs(5),
    )
    .await;

    let transport = connect_transport(addr, "wrong-token");
    let result = client_info().serve(transport).await;
    assert!(
        result.is_err(),
        "Verbindungsaufbau mit falschem Token muss fehlschlagen"
    );

    handle.shutdown().await;
}

#[tokio::test]
async fn test_propose_command_round_trip_over_real_http() {
    let known_server = ServerId::new();
    let (handle, addr) = start_server(
        TestBackend {
            known_server,
            release: None,
            completed: Arc::new(AtomicBool::new(false)),
        },
        "correct-token",
        Duration::from_secs(5),
    )
    .await;

    let transport = connect_transport(addr, "correct-token");
    let client = client_info()
        .serve(transport)
        .await
        .expect("Verbindungsaufbau mit korrektem Token muss gelingen");

    let tools = client.list_tools(Default::default()).await.unwrap();
    let tool_names: Vec<_> = tools.tools.iter().map(|t| t.name.to_string()).collect();
    for expected in [
        "list_servers",
        "get_server_notes",
        "propose_command",
        "read_remote_file",
        "write_remote_file",
        "propose_note_update",
    ] {
        assert!(
            tool_names.contains(&expected.to_string()),
            "Tool '{expected}' fehlt in der Liste: {tool_names:?}"
        );
    }

    let args = serde_json::json!({
        "server_id": known_server.0.to_string(),
        "command": "uname -a",
    });
    let result = client
        .call_tool(
            CallToolRequestParams::new("propose_command")
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    assert!(tool_result_text(&result).contains("erfolgreich ausgeführt"));

    client.cancel().await.ok();
    handle.shutdown().await;
}

#[tokio::test]
async fn test_unknown_server_over_real_http() {
    let known_server = ServerId::new();
    let (handle, addr) = start_server(
        TestBackend {
            known_server,
            release: None,
            completed: Arc::new(AtomicBool::new(false)),
        },
        "correct-token",
        Duration::from_secs(5),
    )
    .await;

    let transport = connect_transport(addr, "correct-token");
    let client = client_info().serve(transport).await.unwrap();

    let args = serde_json::json!({ "server_id": Uuid::new_v4().to_string() });
    let result = client
        .call_tool(
            CallToolRequestParams::new("get_server_notes")
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();

    assert_eq!(result.is_error, Some(true));
    assert!(tool_result_text(&result).contains("unbekannter Server"));

    client.cancel().await.ok();
    handle.shutdown().await;
}

/// Spec 0028, Abschnitt 7: läuft das (hier künstlich kurze) Timeout ab,
/// bevor die Bestätigung entschieden ist, liefert der Tool-Call die
/// Zeitüberschreitungs-Antwort — geprüft über den echten HTTP-Roundtrip,
/// nicht nur die interne `run_confirmable`-Logik.
///
/// Spec 0097, K1/A3 (F6): Das Backend wartet auf eine Freigabe durch den
/// Test statt auf eine feste Verzögerung — ein enges Zeitfenster würde
/// unter Last (längerer HTTP-Rundlauf) fälschlich grün bleiben, weil das
/// Backend dann doch schon fertig wäre, bevor der Test hinschaut.
/// `propose_action` kann grundsätzlich erst zurückkehren, *nachdem* dieser
/// Test `release.notify_one()` gerufen hat — und das tut er erst, nachdem
/// er die Zeitüberschreitungs-Antwort schon geprüft hat. Dass `call_tool`
/// hier überhaupt zurückkehrt, ist deshalb selbst der Beweis, dass die
/// Antwort über den `select!`-Timeout-Zweig kam, nicht über das (noch
/// blockierte) Backend — eine eigene Prüfung des `completed`-Flags würde an
/// dieser Stelle nichts zusätzlich zeigen, da es bis zur Freigabe unter
/// jeder Implementierung `false` bleibt (spec-reviewer, Runde 1).
///
/// Die 5s-Obergrenze um `call_tool` fängt den Fall ab, dass der
/// Timeout-Zweig komplett ausfällt (z. B. `select!` durch ein direktes
/// `.await` auf den Hintergrund-Task ersetzt) — ohne sie würde der Test in
/// diesem Fall unbegrenzt hängen statt rot zu werden (spec-reviewer, Runde
/// 1; Richtwert A2).
#[tokio::test]
async fn test_confirm_timeout_over_real_http() {
    let known_server = ServerId::new();
    let completed = Arc::new(AtomicBool::new(false));
    let release = Arc::new(tokio::sync::Notify::new());
    let (handle, addr) = start_server(
        TestBackend {
            known_server,
            release: Some(release.clone()),
            completed,
        },
        "correct-token",
        Duration::from_millis(50),
    )
    .await;

    let transport = connect_transport(addr, "correct-token");
    let client = client_info().serve(transport).await.unwrap();

    let args = serde_json::json!({
        "server_id": known_server.0.to_string(),
        "command": "long-running-thing",
    });
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        client.call_tool(
            CallToolRequestParams::new("propose_command")
                .with_arguments(args.as_object().unwrap().clone()),
        ),
    )
    .await
    .expect(
        "propose_command darf nicht hängen bleiben — der Timeout-Zweig \
         der Bestätigung muss unabhängig vom (hier dauerhaft blockierten) \
         Backend zurückkehren",
    )
    .unwrap();

    assert_eq!(result.is_error, Some(true));
    assert!(tool_result_text(&result).contains("Zeitüberschreitung"));

    // Gibt den im Hintergrund weiterlaufenden Backend-Aufruf frei
    // (`run_confirmable` spawnt ihn separat und lässt ihn trotz Timeout
    // weiterlaufen, s. `tool_server.rs`), statt ihn als hängende Task
    // zurückzulassen.
    release.notify_one();

    client.cancel().await.ok();
    handle.shutdown().await;
}
