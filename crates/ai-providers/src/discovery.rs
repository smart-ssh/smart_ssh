//! Modell-Discovery und rohe TEE-Attestierungs-Abfrage (Spec 0025,
//! Abschnitt 2 und 4) — bewusst als eigenständige Funktionen statt Teil des
//! `AiProvider`-Traits: `discover_models` läuft auch gegen einen noch nicht
//! gespeicherten Formularentwurf (analog zu `test_connection`, Spec 0008),
//! `fetch_attestation_info` ist kein Chat-Request und braucht keinen
//! `AiProvider`-Kontext (`SessionContext`, Streaming).

use serde::Deserialize;
use uuid::Uuid;

use ssh_manager_core::ai::AiError;

use crate::error::{map_http_status, map_transport_error};
use crate::request_logging::{log_provider_error_response, log_provider_transport_error};

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
    // Spec-Reviewer-Fund (Spec 0049, Review von Fund 2): dieser Pfad
    // (der "Modelle laden"-Button im Formular) ist genau die Stelle, an
    // der ein Tester einen frisch eingefügten, falschen/untrimmten API-Key
    // oder Modellnamen zuerst bemerkt — Fund 2 nennt "Modell nicht
    // gefunden" ausdrücklich als abzudeckenden Fall. War zunächst
    // übersehen worden: nur `anthropic.rs`/`openai_compatible.rs`s
    // Chat-Pfad hatte die neuen Log-Aufrufe, dieser Discovery-Pfad nicht.
    let request_id = Uuid::new_v4();
    let secrets: Vec<&str> = std::iter::once(api_key)
        .chain(extra_headers.iter().map(|(_, value)| value.as_str()))
        .collect();

    let client = reqwest::Client::new();
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let mut request = client.get(&url).bearer_auth(api_key);
    for (name, value) in extra_headers {
        request = request.header(name, value);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            let mapped = map_transport_error(&err);
            log_provider_transport_error(request_id, &mapped, &secrets);
            return Err(mapped);
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let mapped = map_http_status(status, &text);
        log_provider_error_response(request_id, status.as_u16(), &text, &mapped, &secrets);
        return Err(mapped);
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
    // Spec 0049, Fund 2: kein `api_key`/`extra_headers` hier (s. Doc-
    // Kommentar oben) — nichts zu redigieren, daher eine leere `secrets`-
    // Liste statt eines eigenen, secret-losen Log-Pfads.
    let request_id = Uuid::new_v4();
    let client = reqwest::Client::new();
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(err) => {
            let mapped = map_transport_error(&err);
            log_provider_transport_error(request_id, &mapped, &[]);
            return Err(mapped);
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let mapped = map_http_status(status, &text);
        log_provider_error_response(request_id, status.as_u16(), &text, &mapped, &[]);
        return Err(mapped);
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

        let models = discover_models(&server.uri(), "test-key", &[])
            .await
            .unwrap();

        assert_eq!(
            models,
            vec!["gpt-4".to_string(), "gpt-3.5-turbo".to_string()]
        );
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
        assert!(
            result.is_ok(),
            "erwartet: Header korrekt gesetzt, bekam {result:?}"
        );
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

    // --- Spec-Reviewer-Fund (Spec 0049, Review von Fund 2) -----------------
    //
    // `request_logging::error_logging_tests` beweist bereits, dass die
    // Log-Funktionen selbst korrekt redigieren — was hier fehlte (und die
    // ursprüngliche Fund-2-Umsetzung in diesem Modul überhaupt verpasst
    // hatte) ist ein Beweis, dass `discover_models` sie auf dem echten
    // 401-Antwortpfad tatsächlich AUFRUFT. Nutzt `crate::test_support`
    // (nicht ein zweites, eigenes `set_global_default` — zwei globale
    // Test-Subscriber im selben Testbinary lassen sich nicht beide
    // installieren, der zweite Aufruf schlägt still fehl und die Tests
    // dieses Moduls hätten in den falschen Puffer geschrieben, s. dortiger
    // Doc-Kommentar).
    use crate::test_support::{clear_log_buffer, install_test_subscriber_once, log_buffer_text};

    #[tokio::test]
    async fn test_discover_models_logs_the_error_response_with_the_key_redacted() {
        install_test_subscriber_once();
        clear_log_buffer();

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid x-api-key"))
            .mount(&server)
            .await;

        let result = discover_models(&server.uri(), "sk-real-secret-key", &[]).await;
        assert!(matches!(result, Err(AiError::AuthenticationFailed)));

        let log_text = log_buffer_text();
        assert!(
            log_text.contains("401") && log_text.contains("AI_AUTH_FAILED"),
            "erwartet: Status + Code im Log, war: {log_text}"
        );
        assert!(
            !log_text.contains("sk-real-secret-key"),
            "der API-Key darf nicht im Log stehen: {log_text}"
        );
    }
}
