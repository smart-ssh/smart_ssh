//! HTTP-Fehler → [`AiError`]-Mapping, geteilt zwischen allen Providern
//! (Aufgabenstellung Teil 2, Punkt 4), damit sich das Verhalten nicht
//! zwischen `OpenAiCompatibleProvider` und `AnthropicProvider` auseinander
//! entwickelt.

use std::pin::Pin;

use futures::Stream;
use ssh_manager_core::ai::{AiError, AiEvent};

/// Bildet einen nicht-erfolgreichen HTTP-Status auf [`AiError`] ab.
///
/// Design-Entscheidung (Spec 0006 nennt nur 401 und 429 explizit,
/// Abschnitt 6): alle übrigen 4xx/5xx-Codes landen auf
/// `ProviderUnavailable`, da sie i. d. R. ein serverseitiges bzw.
/// vorübergehendes Problem anzeigen (Wartung, Überlastung, defekter
/// Endpoint) und nicht wie 401/429 einen spezifischen, für die
/// aufrufende App handlungsrelevanten Fall.
pub(crate) fn map_http_status(status: reqwest::StatusCode, body: &str) -> AiError {
    match status.as_u16() {
        401 | 403 => AiError::AuthenticationFailed,
        429 => AiError::RateLimited,
        _ => AiError::ProviderUnavailable(format!("HTTP {status}: {body}")),
    }
}

/// Bildet einen Transport-Fehler (Verbindungsaufbau, Timeout, TLS, ...) auf
/// [`AiError::NetworkError`] ab.
pub(crate) fn map_transport_error(err: &reqwest::Error) -> AiError {
    AiError::NetworkError(err.to_string())
}

/// Ein einzelnes [`AiEvent::Error`] als fertiger Stream — für den Fall,
/// dass die Anfrage gar nicht erst erfolgreich abgeschickt/beantwortet
/// werden konnte (Verbindungsfehler, nicht-2xx-Status).
pub(crate) fn error_stream(err: AiError) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
    Box::pin(futures::stream::once(async move { AiEvent::Error(err) }))
}
