//! Bearer-Token-Prüfung (Spec 0028, Abschnitt 6): jeder Aufruf gegen `/mcp`
//! ohne oder mit falschem Token wird abgelehnt, kein Teil-Zugriff — die
//! Prüfung sitzt daher als Axum-Middleware **vor** dem gesamten
//! MCP-Router, nicht pro Tool.

use std::sync::{Arc, Mutex};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

/// Hält das aktuell gültige Token hinter einem `Mutex`, damit
/// "Neu generieren" (Spec 0028, Abschnitt 9) das alte Token **sofort**
/// invalidiert — ein zur Startzeit fest konfigurierter Wert würde
/// verlangen, den HTTP-Listener für eine Token-Rotation neu zu starten,
/// was laufende Verbindungen unnötig hart trennen würde. `Arc`-geklont
/// zwischen dem Axum-`State` dieser Middleware und der Stelle, die das
/// Token bei "Neu generieren" austauscht (Spec 0028, Abschnitt 9).
#[derive(Clone, Default)]
pub struct SharedToken(Arc<Mutex<String>>);

impl SharedToken {
    pub fn new(initial: impl Into<String>) -> Self {
        Self(Arc::new(Mutex::new(initial.into())))
    }

    pub fn get(&self) -> String {
        self.0.lock().expect("SharedToken-Mutex vergiftet").clone()
    }

    /// Ersetzt das gültige Token — ab dem nächsten Aufruf wird das alte
    /// Token abgelehnt, unabhängig von bereits offenen HTTP-Verbindungen
    /// (jeder neue Tool-Call durchläuft die Middleware erneut).
    pub fn set(&self, new_token: impl Into<String>) {
        *self.0.lock().expect("SharedToken-Mutex vergiftet") = new_token.into();
    }
}

fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

pub async fn require_bearer_token(
    State(expected): State<SharedToken>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected = expected.get();
    // Issue #115: Ein leeres oder nur aus Leerzeichen bestehendes erwartetes
    // Token heißt "MCP ohne Geheimnis" — `Authorization: Bearer ` (leerer
    // Wert) wäre sonst gleich und käme durch. Ein solches Token gewährt
    // **nie** Zugriff, egal was der Client schickt; `SharedToken::default()`
    // ist genau so ein Leerstring. Ersetzt wird es beim Laden
    // (`app_logic::mcp_token::load_or_init_token`), nicht hier.
    if expected.trim().is_empty() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    match extract_bearer(&headers) {
        // Konstante Laufzeit wäre hier zusätzliche Härtung, aber die Spec
        // (Abschnitt 6) verlangt nur "kein Teil-Zugriff bei falschem
        // Token", kein Schutz gegen Timing-Seitenkanäle innerhalb von
        // 127.0.0.1 — ein einfacher Vergleich reicht für diese
        // Bedrohungslage.
        Some(token) if token == expected => Ok(next.run(request).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::middleware;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    async fn protected_ok() -> &'static str {
        "ok"
    }

    fn app(token: SharedToken) -> Router {
        Router::new()
            .route("/protected", get(protected_ok))
            .layer(middleware::from_fn_with_state(token, require_bearer_token))
    }

    #[tokio::test]
    async fn test_missing_token_is_rejected() {
        let app = app(SharedToken::new("secret-token"));
        let response = app
            .oneshot(HttpRequest::get("/protected").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_wrong_token_is_rejected() {
        let app = app(SharedToken::new("secret-token"));
        let response = app
            .oneshot(
                HttpRequest::get("/protected")
                    .header("Authorization", "Bearer wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_correct_token_is_accepted() {
        let app = app(SharedToken::new("secret-token"));
        let response = app
            .oneshot(
                HttpRequest::get("/protected")
                    .header("Authorization", "Bearer secret-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_rotated_token_invalidates_old_token_immediately() {
        let token = SharedToken::new("old-token");
        let app = app(token.clone());

        token.set("new-token");

        let old_token_response = app
            .clone()
            .oneshot(
                HttpRequest::get("/protected")
                    .header("Authorization", "Bearer old-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(old_token_response.status(), StatusCode::UNAUTHORIZED);

        let new_token_response = app
            .oneshot(
                HttpRequest::get("/protected")
                    .header("Authorization", "Bearer new-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(new_token_response.status(), StatusCode::OK);
    }

    async fn status_for(token: SharedToken, authorization: Option<&str>) -> StatusCode {
        let mut request = HttpRequest::get("/protected");
        if let Some(value) = authorization {
            request = request.header("Authorization", value);
        }
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            app(token).oneshot(request.body(Body::empty()).unwrap()),
        )
        .await
        .expect("die Middleware darf nicht hängen")
        .unwrap();
        response.status()
    }

    /// Issue #115: Ein leeres erwartetes Token darf nicht mit einem leeren
    /// `Bearer `-Wert übereinstimmen. Gegenbeweis: ohne die Leer-Prüfung in
    /// `require_bearer_token` liefert dieser Test 200.
    #[tokio::test]
    async fn test_empty_expected_token_rejects_empty_bearer() {
        assert_eq!(
            status_for(SharedToken::new(""), Some("Bearer ")).await,
            StatusCode::UNAUTHORIZED
        );
    }

    /// Issue #115: Ein nur aus Leerzeichen bestehendes erwartetes Token
    /// gewährt ebenfalls keinen Zugriff, auch nicht mit exakt gleichem Wert.
    #[tokio::test]
    async fn test_whitespace_expected_token_rejects_matching_whitespace_bearer() {
        assert_eq!(
            status_for(SharedToken::new("   "), Some("Bearer    ")).await,
            StatusCode::UNAUTHORIZED
        );
    }

    /// Issue #115: Leeres erwartetes Token und gar kein Header → 401.
    #[tokio::test]
    async fn test_empty_expected_token_rejects_missing_header() {
        assert_eq!(
            status_for(SharedToken::new(""), None).await,
            StatusCode::UNAUTHORIZED
        );
    }

    /// Issue #115: `SharedToken::default()` ist ein Leerstring und darf
    /// daher ebenfalls nichts freischalten.
    #[tokio::test]
    async fn test_default_shared_token_grants_no_access() {
        assert_eq!(
            status_for(SharedToken::default(), Some("Bearer ")).await,
            StatusCode::UNAUTHORIZED
        );
    }
}
