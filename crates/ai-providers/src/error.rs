//! HTTP-Fehler → [`AiError`]-Mapping, geteilt zwischen allen Providern
//! (Aufgabenstellung Teil 2, Punkt 4), damit sich das Verhalten nicht
//! zwischen `OpenAiCompatibleProvider` und `AnthropicProvider` auseinander
//! entwickelt.

use std::pin::Pin;

use futures::Stream;
use ssh_manager_core::ai::{AiError, AiEvent};

/// Spec 0069, Teil A2/Teil 0.2: Textbausteine, die (case-insensitive) in
/// einem 404- oder 400-Body auf "das angefragte Modell existiert nicht"
/// hindeuten, statt auf ein generisches Server-/Routing-Problem. Aus den
/// Fixtures in `tests/fixtures/model_not_found/` übernommen (OpenAI,
/// Ollama, OpenRouter — Teil-0-Bericht: nicht per echtem Aufruf oder
/// Web-Recherche verifiziert, s. dortige `README.md`; vor dem nächsten
/// Release gegen echte Accounts abgleichen).
///
/// **Warum eine Substring-Liste statt eines strikten JSON-Schemas pro
/// Provider:** die vier Provider-Formate unterscheiden sich strukturell zu
/// sehr (OpenAI: `error.code == "model_not_found"`; Ollama: freier
/// String; OpenRouter: `error.message`; Anthropic: `error.type` +
/// `error.message`) für ein gemeinsames Schema, ohne vier separate Parser
/// zu bauen — ein Substring-Match ist robuster gegen kleine
/// Formulierungs-Änderungen der Provider als ein exaktes Feld-Mapping, auf
/// Kosten eines (durch die Wortwahl unten geprüft: gering gehaltenen)
/// False-Positive-Risikos gegen einen unrelated 404 (z. B. falsche
/// Base-URL). Jeder Marker ist deshalb eine möglichst vollständige,
/// model-spezifische Phrase statt eines einzelnen generischen Worts wie
/// "model" oder "not_found" allein (s. Negativ-Tests unten).
pub(crate) const MODEL_NOT_FOUND_MARKERS: &[&str] = &[
    "model_not_found",
    "does not exist or you do not have access",
    "not found, try pulling",
    "is not a valid model id",
    "model not found",
];

/// Bildet einen nicht-erfolgreichen HTTP-Status auf [`AiError`] ab.
///
/// Design-Entscheidung (Spec 0006 nennt nur 401 und 429 explizit,
/// Abschnitt 6): alle übrigen 4xx/5xx-Codes landen auf
/// `ProviderUnavailable`, da sie i. d. R. ein serverseitiges bzw.
/// vorübergehendes Problem anzeigen (Wartung, Überlastung, defekter
/// Endpoint) und nicht wie 401/429 einen spezifischen, für die
/// aufrufende App handlungsrelevanten Fall.
///
/// Spec 0069, Teil A2: 404 **oder** 400, wenn der Body einen der
/// [`MODEL_NOT_FOUND_MARKERS`] enthält (case-insensitive) → `ModelNotFound`
/// statt `ProviderUnavailable` — das war vorher nicht unterscheidbar
/// (jeder Nicht-401/403/429-Status landete gleich). 401/403 bleiben
/// `AuthenticationFailed` auch dann, wenn "model" zufällig im Body steht
/// (Prüfreihenfolge: Status zuerst).
pub(crate) fn map_http_status(status: reqwest::StatusCode, body: &str) -> AiError {
    match status.as_u16() {
        401 | 403 => AiError::AuthenticationFailed,
        429 => AiError::RateLimited,
        404 | 400 if contains_model_not_found_marker(body) => {
            AiError::ModelNotFound(format!("HTTP {status}: {body}"))
        }
        _ => AiError::ProviderUnavailable(format!("HTTP {status}: {body}")),
    }
}

fn contains_model_not_found_marker(body: &str) -> bool {
    let lower = body.to_lowercase();
    MODEL_NOT_FOUND_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

/// Bildet einen Transport-Fehler (Verbindungsaufbau, Timeout, TLS, ...) auf
/// [`AiError`] ab.
///
/// Spec 0069, Teil A2/§4.2: `is_connect()` **und** der Host aus `err.url()`
/// ist eine Loopback-Adresse (`localhost`, `127.0.0.0/8`, `::1`) →
/// `LocalProviderUnreachable` (typischerweise: Ollama läuft nicht).
/// `is_connect()` ohne Loopback (DNS-Fehler, abgelehnt, TLS — Teil 0.5
/// belegt: `reqwest` liefert `is_connect() == true` für beides, DNS-Fehler
/// wie abgelehnte Verbindung) → weiterhin `NetworkError`. `is_timeout()` →
/// `Timeout`. **Bewusst nicht** nach Provider-*Typ* unterschieden (s.
/// Modul-Doc-Verweis auf Spec 0069 §4.2): ein Ollama auf einem anderen
/// Rechner ist kein "lokaler Dienst nicht gestartet"-Fall.
pub(crate) fn map_transport_error(err: &reqwest::Error) -> AiError {
    if err.is_connect() && is_loopback_target(err) {
        return AiError::LocalProviderUnreachable(err.to_string());
    }
    if err.is_timeout() {
        // spec-reviewer-Fund (Review dieses Schritts): `err.is_timeout()`
        // meldet nur "es gab einen Timeout", nicht welche Frist griff —
        // `reqwest::Client` bekommt in diesem Projekt aktuell nirgends ein
        // eigenes `.timeout(...)` (s. `crate::sse::build_http_client`, nur
        // `tcp_keepalive`), dieser Zweig ist also nur eine Absicherung für
        // den Fall, dass das künftig doch gesetzt wird. `SSE_INACTIVITY_
        // TIMEOUT` ist dafür die einzige im Projekt bekannte Frist — eine
        // bewusste Näherung (nicht die tatsächlich abgelaufene Zeit), kein
        // Bug: der `Display`-Text zeigt eine plausible, aber ggf. nicht
        // exakte Sekundenzahl; das Frontend zeigt ohnehin nur den
        // `AI_TIMEOUT`-Text ohne Zahl an (s. `errors.AI_TIMEOUT`).
        return AiError::Timeout {
            secs: crate::sse::SSE_INACTIVITY_TIMEOUT.as_secs(),
        };
    }
    AiError::NetworkError(err.to_string())
}

/// Spec 0069, Teil A2: eine Konstruktor-Funktion statt der fünf bisherigen
/// Kopien derselben `format!("Keine Antwort vom KI-Provider seit über {}
/// Sekunden", ...)`-Konstruktion (`anthropic.rs` ×2, `openai_compatible.rs`
/// ×2, `discovery.rs::discovery_timeout`) — der `Display`-Text von
/// `AiError::Timeout` ist bereits wörtlich identisch (s. dessen Doc-
/// Kommentar/Test in `core::ai::types`), diese Funktion baut nur noch den
/// Wert selbst.
pub(crate) fn timeout_error(timeout: std::time::Duration) -> AiError {
    AiError::Timeout {
        secs: timeout.as_secs(),
    }
}

fn is_loopback_target(err: &reqwest::Error) -> bool {
    let Some(host) = err.url().and_then(|url| url.host_str()) else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    // IPv6-Literale stehen in der URL in `[...]`-Klammern (z. B.
    // `[::1]`) — `Url::host_str` liefert sie ohne die Klammern, `parse`
    // erwartet ebenfalls das reine Adressformat, passt also zusammen.
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Ein einzelnes [`AiEvent::Error`] als fertiger Stream — für den Fall,
/// dass die Anfrage gar nicht erst erfolgreich abgeschickt/beantwortet
/// werden konnte (Verbindungsfehler, nicht-2xx-Status).
pub(crate) fn error_stream(err: AiError) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
    Box::pin(futures::stream::once(async move { AiEvent::Error(err) }))
}

#[cfg(test)]
mod map_http_status_tests {
    //! Spec 0069, Teil A2, Tests 1/2. *Gegenbeweis (s. Bericht):* vor
    //! diesem Schritt lieferte `map_http_status` für jeden dieser Fälle
    //! `ProviderUnavailable` — die Fixture-Tests unten schlugen fehl, bis
    //! `contains_model_not_found_marker` eingeführt wurde.
    use reqwest::StatusCode;

    use super::*;

    fn fixture_body(raw: &str) -> String {
        let value: serde_json::Value =
            serde_json::from_str(raw).expect("Fixture ist gültiges JSON");
        value["body"].to_string()
    }

    #[test]
    fn test_openai_model_not_found_fixture_maps_to_model_not_found() {
        let raw = include_str!("../tests/fixtures/model_not_found/openai.json");
        let body = fixture_body(raw);
        assert!(matches!(
            map_http_status(StatusCode::NOT_FOUND, &body),
            AiError::ModelNotFound(_)
        ));
    }

    #[test]
    fn test_anthropic_model_not_found_fixture_maps_to_model_not_found() {
        let raw = include_str!("../tests/fixtures/model_not_found/anthropic.json");
        let body = fixture_body(raw);
        assert!(matches!(
            map_http_status(StatusCode::NOT_FOUND, &body),
            AiError::ModelNotFound(_)
        ));
    }

    #[test]
    fn test_ollama_model_not_found_fixture_maps_to_model_not_found() {
        let raw = include_str!("../tests/fixtures/model_not_found/ollama.json");
        let body = fixture_body(raw);
        assert!(matches!(
            map_http_status(StatusCode::NOT_FOUND, &body),
            AiError::ModelNotFound(_)
        ));
    }

    #[test]
    fn test_openrouter_model_not_found_fixture_maps_to_model_not_found_with_400() {
        let raw = include_str!("../tests/fixtures/model_not_found/openrouter.json");
        let body = fixture_body(raw);
        assert!(matches!(
            map_http_status(StatusCode::BAD_REQUEST, &body),
            AiError::ModelNotFound(_)
        ));
    }

    /// Test 2 (Negativ): eine falsche Base-URL liefert typischerweise
    /// einen 404 ohne jeden Modell-Bezug (z. B. eine Proxy-/Gateway-Seite)
    /// — muss `ProviderUnavailable` bleiben, nicht fälschlich
    /// `ModelNotFound` vortäuschen.
    #[test]
    fn test_404_without_model_marker_stays_provider_unavailable() {
        let body = "<html><body>404 Not Found</body></html>";
        assert!(matches!(
            map_http_status(StatusCode::NOT_FOUND, body),
            AiError::ProviderUnavailable(_)
        ));
    }

    #[test]
    fn test_400_without_model_marker_stays_provider_unavailable() {
        let body = r#"{"error":{"message":"invalid request: missing field 'messages'"}}"#;
        assert!(matches!(
            map_http_status(StatusCode::BAD_REQUEST, body),
            AiError::ProviderUnavailable(_)
        ));
    }

    /// Test 2 (Negativ): ein 401 bleibt `AuthenticationFailed`, selbst
    /// wenn der Body zufällig das Wort "model" enthält — die
    /// Status-Prüfung geht der Marker-Prüfung vor.
    #[test]
    fn test_401_with_model_wording_still_authentication_failed() {
        let body = r#"{"error":{"message":"invalid API key for this model"}}"#;
        assert!(matches!(
            map_http_status(StatusCode::UNAUTHORIZED, body),
            AiError::AuthenticationFailed
        ));
    }

    #[test]
    fn test_5xx_stays_provider_unavailable() {
        let body = "internal error";
        assert!(matches!(
            map_http_status(StatusCode::INTERNAL_SERVER_ERROR, body),
            AiError::ProviderUnavailable(_)
        ));
    }

    #[test]
    fn test_429_stays_rate_limited_not_model_not_found() {
        assert!(matches!(
            map_http_status(StatusCode::TOO_MANY_REQUESTS, "model rate limit exceeded"),
            AiError::RateLimited
        ));
    }
}

#[cfg(test)]
mod map_transport_error_tests {
    //! Spec 0069, Teil A2, Test 3. *Gegenbeweis (s. Bericht):* vor diesem
    //! Schritt lieferte `map_transport_error` in beiden Fällen
    //! `NetworkError` — der Loopback-Test schlug vorher fehl.
    use super::*;

    /// Ein `reqwest`-Fehler gegen einen soeben geschlossenen lokalen Port
    /// — Teil 0.5 belegt: `is_connect() == true`, `url()` trägt den
    /// Ziel-Host.
    async fn connect_error_against(url: &str) -> reqwest::Error {
        reqwest::get(url).await.expect_err("darf nicht gelingen")
    }

    #[tokio::test]
    async fn test_loopback_connect_refused_maps_to_local_provider_unreachable() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let err = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            connect_error_against(&format!("http://127.0.0.1:{port}/v1/models")),
        )
        .await
        .expect("darf nicht hängen");

        assert!(matches!(
            map_transport_error(&err),
            AiError::LocalProviderUnreachable(_)
        ));
    }

    #[tokio::test]
    async fn test_localhost_hostname_also_counts_as_loopback() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let err = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            connect_error_against(&format!("http://localhost:{port}/v1/models")),
        )
        .await
        .expect("darf nicht hängen");

        assert!(matches!(
            map_transport_error(&err),
            AiError::LocalProviderUnreachable(_)
        ));
    }

    #[tokio::test]
    async fn test_non_loopback_dns_failure_stays_network_error() {
        let err = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            connect_error_against("http://this-host-does-not-exist.invalid/v1/models"),
        )
        .await
        .expect("darf nicht hängen");

        assert!(matches!(
            map_transport_error(&err),
            AiError::NetworkError(_)
        ));
    }

    #[test]
    fn test_timeout_error_carries_the_given_seconds() {
        let err = timeout_error(std::time::Duration::from_secs(42));
        assert!(matches!(err, AiError::Timeout { secs: 42 }));
    }
}
