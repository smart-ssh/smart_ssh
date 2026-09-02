//! Modell-Discovery und rohe TEE-Attestierungs-Abfrage (Spec 0025,
//! Abschnitt 2 und 4) — bewusst als eigenständige Funktionen statt Teil des
//! `AiProvider`-Traits: `discover_models` läuft auch gegen einen noch nicht
//! gespeicherten Formularentwurf (analog zu `test_connection`, Spec 0008),
//! `fetch_attestation_info` ist kein Chat-Request und braucht keinen
//! `AiProvider`-Kontext (`SessionContext`, Streaming).

use serde::Deserialize;

use ssh_manager_core::ai::AiError;

use crate::error::{map_http_status, map_transport_error};

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

/// Ruft `GET {base_url}/models` auf (OpenAI-API-Konvention, Spec 0025
/// Abschnitt 2 — von OpenAI selbst, generischen OpenAI-kompatiblen
/// Endpunkten und Ollama im OpenAI-kompatiblen Modus gleichermaßen
/// unterstützt) und liefert die Modell-IDs. `base_url` ohne abschließenden
/// Slash erwartet, wie bei [`crate::OpenAiCompatibleProvider`].
pub async fn discover_models(
    base_url: &str,
    api_key: &str,
    extra_headers: &[(String, String)],
) -> Result<Vec<String>, AiError> {
    let client = reqwest::Client::new();
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let mut request = client.get(&url).bearer_auth(api_key);
    for (name, value) in extra_headers {
        request = request.header(name, value);
    }

    let response = request
        .send()
        .await
        .map_err(|err| map_transport_error(&err))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(map_http_status(status, &text));
    }

    let parsed: ModelsResponse = response
        .json()
        .await
        .map_err(|err| AiError::InvalidResponse(err.to_string()))?;
    Ok(parsed.data.into_iter().map(|entry| entry.id).collect())
}

/// Ruft den vom Nutzer hinterlegten TEE-Attestierungs-Endpunkt ab und
/// liefert die rohe Antwort **unverändert** zurück (Spec 0025, Abschnitt 4)
/// — keine Interpretation, keine Verifikation, nur Durchreichen. Bewusst
/// ohne `api_key`/`extra_headers` des KI-Providers: ein
/// Attestierungs-Endpunkt ist konzeptionell ein eigenständiger,
/// typischerweise unauthentifizierter Nachweis-Dienst des
/// Hardware-/Anbieters (unabhängig überprüfbar), keine Ressource der
/// Chat-API selbst — ihm dieselben Zugangsdaten mitzugeben wäre eine
/// unbegründete Annahme über sein Schutzschema.
pub async fn fetch_attestation_info(url: &str) -> Result<String, AiError> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| map_transport_error(&err))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(map_http_status(status, &text));
    }

    response
        .text()
        .await
        .map_err(|err| AiError::InvalidResponse(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_discover_models_success_returns_model_ids() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list",
                "data": [
                    {"id": "gpt-4", "object": "model"},
                    {"id": "gpt-3.5-turbo", "object": "model"},
                ]
            })))
            .mount(&server)
            .await;

        let models = discover_models(&server.uri(), "test-key", &[]).await.unwrap();

        assert_eq!(models, vec!["gpt-4".to_string(), "gpt-3.5-turbo".to_string()]);
    }

    #[tokio::test]
    async fn test_discover_models_failure_yields_ai_error_not_panic() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;

        let result = discover_models(&server.uri(), "test-key", &[]).await;

        assert!(matches!(result, Err(AiError::ProviderUnavailable(_))));
    }

    #[tokio::test]
    async fn test_discover_models_sends_bearer_auth_and_extra_headers() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(header("authorization", "Bearer test-key"))
            .and(header("x-title", "Smart SSH"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"object": "list", "data": []})),
            )
            .mount(&server)
            .await;

        let result = discover_models(
            &server.uri(),
            "test-key",
            &[("X-Title".to_string(), "Smart SSH".to_string())],
        )
        .await;

        // Der wiremock-`Mock` matcht nur bei korrekt gesetzten Headern (s.
        // `.and(header(...))` oben) — ein `Err` hier bedeutete, dass kein
        // registrierter Mock traf (404 von wiremocks eigenem Fallback).
        assert!(result.is_ok(), "erwartet: Header korrekt gesetzt, bekam {result:?}");
    }

    #[tokio::test]
    async fn test_fetch_attestation_info_returns_raw_body_unmodified() {
        let server = MockServer::start().await;
        let raw_body = "{\"quote\":\"deadbeef\",\"format\":\"vendor-specific-not-json-schema\"}";
        Mock::given(method("GET"))
            .and(path("/attestation"))
            .respond_with(ResponseTemplate::new(200).set_body_string(raw_body))
            .mount(&server)
            .await;

        let result = fetch_attestation_info(&format!("{}/attestation", server.uri()))
            .await
            .unwrap();

        assert_eq!(result, raw_body);
    }

    #[tokio::test]
    async fn test_fetch_attestation_info_failure_yields_ai_error_not_panic() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/attestation"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server)
            .await;

        let result = fetch_attestation_info(&format!("{}/attestation", server.uri())).await;

        assert!(matches!(result, Err(AiError::ProviderUnavailable(_))));
    }
}
