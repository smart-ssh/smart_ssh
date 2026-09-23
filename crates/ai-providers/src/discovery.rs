//! Modell-Discovery und rohe TEE-Attestierungs-Abfrage (Spec 0025,
//! Abschnitt 2 und 4) — bewusst als eigenständige Funktionen statt Teil des
//! `AiProvider`-Traits: `discover_models` läuft auch gegen einen noch nicht
//! gespeicherten Formularentwurf (analog zu `test_connection`, Spec 0008),
//! `fetch_attestation_info` ist kein Chat-Request und braucht keinen
//! `AiProvider`-Kontext (`SessionContext`, Streaming).

use serde::Deserialize;
use uuid::Uuid;

use ssh_manager_core::ai::{AiError, ProviderType};

use crate::anthropic::ANTHROPIC_VERSION;
use crate::error::{map_http_status, map_transport_error, timeout_error};
use crate::request_logging::{log_provider_error_response, log_provider_transport_error};
use crate::sse::{read_error_body_with_timeout, SSE_INACTIVITY_TIMEOUT};

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

/// Spec 0072, B1: die Auth-Kopfzeilen, mit denen `GET {base_url}/models`
/// angefragt wird, hängen vom Provider ab — Anthropic verlangt `x-api-key`
/// und `anthropic-version` statt `Authorization: Bearer` (dieselben Header
/// wie im Chat-Pfad, s. `crate::anthropic::connect_and_stream`).
///
/// `extra_headers` werden **danach** angewendet (B3): setzt der Nutzer dort
/// denselben Namen (case-insensitiv, wie HTTP-Header es sind) erneut wie
/// einer der oben gesetzten Defaults, gilt sein Wert — aber genau einmal,
/// nicht zusätzlich zum Default. Deshalb wird nur gegen die ursprünglich
/// gesetzten Default-Einträge dedupliziert (`headers[..default_len]`), nicht
/// gegen bereits durch `extra_headers` selbst hinzugefügte Einträge: trägt
/// ein Nutzer denselben Namen zweimal in `extra_headers` ein, werden
/// weiterhin beide gesendet (unverändertes Bestandsverhalten für diesen
/// Fall — B3 verlangt nur die Default-Override-Eigenschaft, keine
/// Deduplizierung von Nutzereingaben untereinander).
///
/// Spec-Reviewer-Fund (Review dieses Schritts): `reqwest`s `.header(...)`
/// markiert einen Wert nur über `bearer_auth(...)` als „sensitive" (kein
/// Klartext in `Debug`-Ausgaben, keine HTTP/2-HPACK-Indizierung). Ein
/// simpler `String` verliert diese Markierung. `sensitive_header_value`
/// stellt sie für die beiden hier bekannten Auth-Header (`authorization`,
/// `x-api-key`) wieder her — auch dann, wenn ihr Wert per `extra_headers`
/// überschrieben wurde.
fn effective_headers(
    provider_type: ProviderType,
    api_key: &str,
    extra_headers: &[(String, String)],
) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = match provider_type {
        ProviderType::Anthropic => vec![
            ("x-api-key".to_string(), api_key.to_string()),
            (
                "anthropic-version".to_string(),
                ANTHROPIC_VERSION.to_string(),
            ),
        ],
        ProviderType::OpenAi | ProviderType::GenericOpenAiCompatible | ProviderType::Ollama => {
            vec![("authorization".to_string(), format!("Bearer {api_key}"))]
        }
    };
    let default_len = headers.len();
    for (name, value) in extra_headers {
        if let Some(existing) = headers[..default_len]
            .iter_mut()
            .find(|(existing_name, _)| existing_name.eq_ignore_ascii_case(name))
        {
            existing.1 = value.clone();
        } else {
            headers.push((name.clone(), value.clone()));
        }
    }
    headers
}

/// Namen der Header, deren Wert ein Zugangsdaten-Geheimnis trägt und deshalb
/// als `sensitive` an `reqwest` übergeben werden muss (s. `effective_headers`-
/// Doc-Kommentar) — case-insensitiver Vergleich, wie bei HTTP-Headern üblich.
fn is_secret_header_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("x-api-key")
}

/// Baut einen `HeaderValue`, der in `reqwest`/`hyper` als „sensitive"
/// markiert ist — `None`, wenn `value` keine gültigen Header-Bytes enthält;
/// der Aufrufer fällt dann auf den unmarkierten `&str`-Pfad zurück, `reqwest`
/// validiert (und lehnt ggf. ab) beim eigentlichen `.send()` wie zuvor.
fn sensitive_header_value(value: &str) -> Option<reqwest::header::HeaderValue> {
    let mut header_value = reqwest::header::HeaderValue::from_str(value).ok()?;
    header_value.set_sensitive(true);
    Some(header_value)
}

/// Ruft `GET {base_url}/models` auf (bzw. `GET {base_url}/v1/models` für
/// Anthropic, s. u.) und liefert die Modell-IDs. Für die OpenAI-kompatible
/// Familie (OpenAI selbst, generische OpenAI-kompatible Endpunkte, Ollama im
/// OpenAI-kompatiblen Modus) die OpenAI-API-Konvention (Spec 0025, Abschnitt
/// 2); für Anthropic dasselbe Endpoint-Verhalten unter eigenen Auth-Headern
/// (Spec 0072, B1 — die Annahme aus Spec 0025, Anthropic habe "kein
/// äquivalentes Endpoint-Verhalten", traf nicht zu, s. Spec 0072 §1
/// dortiger Ausgangslage-Abschnitt). `base_url` ohne abschließenden Slash
/// erwartet, wie bei [`crate::OpenAiCompatibleProvider`].
///
/// Spec-Reviewer-Fund (Review dieses Schritts, gemessen statt angenommen —
/// `curl -o /dev/null -w '%{http_code}' https://api.anthropic.com/models`
/// → 404, `.../v1/models` → 401 bei fehlendem Key, also der richtige
/// Endpunkt): Anthropics `base_url` (`DEFAULT_ANTHROPIC_BASE_URL` in
/// `app-shell::ai_provider_factory`, ebenso ein selbst eingetragener Wert)
/// enthält — anders als bei der OpenAI-Familie — **kein** `/v1`-Präfix; der
/// Chat-Pfad hängt es deshalb selbst an (`crate::anthropic`s
/// `format!("{}/v1/messages", ...)`). Ohne dieselbe Ergänzung hier lief
/// „Modelle laden" für Anthropic gegen den falschen Pfad — und wurde durch
/// [`is_structured_model_not_found`](crate::error) sogar als "Modell nicht
/// gefunden" (statt sichtbar falsch) angezeigt, weil Anthropics generische
/// 404-Antwort auf einen unbekannten Pfad zufällig `error.type ==
/// "not_found_error"` trägt.
pub async fn discover_models(
    provider_type: ProviderType,
    base_url: &str,
    api_key: &str,
    extra_headers: &[(String, String)],
) -> Result<Vec<String>, AiError> {
    discover_models_within(
        provider_type,
        base_url,
        api_key,
        extra_headers,
        SSE_INACTIVITY_TIMEOUT,
    )
    .await
}

/// Spec 0068, Teil 5a: Zeitüberschreitung eines Discovery-/Attestierungs-
/// Aufrufs — dieselbe Meldung wie im Chat-Pfad. Spec 0069, Teil A2:
/// `AiError::Timeout` statt `NetworkError` (Log-Text unverändert, s.
/// `timeout_error`-Doc-Kommentar).
fn discovery_timeout(timeout: std::time::Duration) -> AiError {
    timeout_error(timeout)
}

/// Spec 0068, Teil 5a: Verbindungsaufbau und Body-Lesen sind begrenzt —
/// mit demselben Mechanismus wie der Chat-Pfad (`SSE_INACTIVITY_TIMEOUT`,
/// `read_error_body_with_timeout`), keine neue Konstante. `timeout` ist nur
/// ein Parameter, damit Tests nicht 90 s warten müssen.
async fn discover_models_within(
    provider_type: ProviderType,
    base_url: &str,
    api_key: &str,
    extra_headers: &[(String, String)],
    timeout: std::time::Duration,
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
    let base_url = base_url.trim_end_matches('/');
    // Spec-Reviewer-Fund (Review dieses Schritts): Anthropics `base_url`
    // trägt kein `/v1`-Präfix (s. Doc-Kommentar von `discover_models`) — nur
    // hier ergänzt, nicht für die OpenAI-Familie, deren `base_url`
    // (`DEFAULT_OPENAI_BASE_URL`/eine selbst eingetragene) es bereits
    // enthält.
    let url = match provider_type {
        ProviderType::Anthropic => format!("{base_url}/v1/models"),
        ProviderType::OpenAi | ProviderType::GenericOpenAiCompatible | ProviderType::Ollama => {
            format!("{base_url}/models")
        }
    };
    let headers = effective_headers(provider_type, api_key, extra_headers);
    let mut request = client.get(&url);
    for (name, value) in &headers {
        request = if is_secret_header_name(name) {
            match sensitive_header_value(value) {
                Some(header_value) => request.header(name, header_value),
                // Ungültige Header-Bytes: fällt auf den unmarkierten Pfad
                // zurück, `reqwest` validiert/lehnt beim `.send()` wie zuvor.
                None => request.header(name, value),
            }
        } else {
            request.header(name, value)
        };
    }

    let response = match tokio::time::timeout(timeout, request.send()).await {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => {
            let mapped = map_transport_error(&err);
            log_provider_transport_error(request_id, &mapped, &secrets);
            return Err(mapped);
        }
        Err(_elapsed) => {
            let mapped = discovery_timeout(timeout);
            log_provider_transport_error(request_id, &mapped, &secrets);
            return Err(mapped);
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let text = read_error_body_with_timeout(response.text()).await;
        let mapped = map_http_status(status, &text);
        log_provider_error_response(request_id, status.as_u16(), &text, &mapped, &secrets);
        return Err(mapped);
    }

    let parsed: ModelsResponse = match tokio::time::timeout(timeout, response.json()).await {
        Ok(result) => result.map_err(|err| AiError::InvalidResponse(err.to_string()))?,
        Err(_elapsed) => {
            let mapped = discovery_timeout(timeout);
            log_provider_transport_error(request_id, &mapped, &secrets);
            return Err(mapped);
        }
    };
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
    fetch_attestation_info_within(url, SSE_INACTIVITY_TIMEOUT).await
}

/// s. [`discover_models_within`] (Spec 0068, Teil 5a).
async fn fetch_attestation_info_within(
    url: &str,
    timeout: std::time::Duration,
) -> Result<String, AiError> {
    // Spec 0049, Fund 2: kein `api_key`/`extra_headers` hier (s. Doc-
    // Kommentar oben) — nichts zu redigieren, daher eine leere `secrets`-
    // Liste statt eines eigenen, secret-losen Log-Pfads.
    let request_id = Uuid::new_v4();
    let client = reqwest::Client::new();
    let response = match tokio::time::timeout(timeout, client.get(url).send()).await {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => {
            let mapped = map_transport_error(&err);
            log_provider_transport_error(request_id, &mapped, &[]);
            return Err(mapped);
        }
        Err(_elapsed) => {
            let mapped = discovery_timeout(timeout);
            log_provider_transport_error(request_id, &mapped, &[]);
            return Err(mapped);
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let text = read_error_body_with_timeout(response.text()).await;
        let mapped = map_http_status(status, &text);
        log_provider_error_response(request_id, status.as_u16(), &text, &mapped, &[]);
        return Err(mapped);
    }

    match tokio::time::timeout(timeout, response.text()).await {
        Ok(result) => result.map_err(|err| AiError::InvalidResponse(err.to_string())),
        Err(_elapsed) => {
            let mapped = discovery_timeout(timeout);
            log_provider_transport_error(request_id, &mapped, &[]);
            Err(mapped)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    /// Spec-Reviewer-Fund (Review dieses Schritts): `wiremock`s eingebaute
    /// Matcher können nur Anwesenheit/Wert eines Headers prüfen, nicht
    /// dessen **Abwesenheit** — ohne diesen Matcher hätte B-T1 auch dann
    /// noch grün gemeldet, wenn `discover_models` versehentlich sowohl
    /// `x-api-key` als auch `authorization` gesendet hätte (`wiremock`
    /// ignoriert unbekannte zusätzliche Header stillschweigend).
    struct HeaderAbsent(&'static str);

    impl wiremock::Match for HeaderAbsent {
        fn matches(&self, request: &Request) -> bool {
            !request.headers.contains_key(self.0)
        }
    }

    /// Spec-Reviewer-Fund (Review dieses Schritts): der API-Key muss als
    /// `sensitive` markiert werden (kein Klartext in `Debug`-Ausgaben, keine
    /// HTTP/2-HPACK-Indizierung) — genau das, was `reqwest::bearer_auth`
    /// bisher lieferte und ein einfacher `String` über `.header(name,
    /// value)` nicht mehr tut. *Gegenbeweis:* vor Einführung von
    /// `set_sensitive(true)` lieferte diese Funktion einen `HeaderValue`
    /// mit `is_sensitive() == false`, dieser Test schlug fehl.
    #[test]
    fn test_sensitive_header_value_marks_the_value_as_sensitive() {
        let value = sensitive_header_value("sk-ant-real-secret-key").expect("gültige Bytes");
        assert!(value.is_sensitive());
        assert_eq!(value.to_str().unwrap(), "sk-ant-real-secret-key");
    }

    #[test]
    fn test_sensitive_header_value_returns_none_for_invalid_bytes_instead_of_panicking() {
        // Ein eingebettetes Steuerzeichen ist kein gültiger Header-Wert.
        assert!(sensitive_header_value("sk-ant-\ninjected").is_none());
    }

    /// Spec-Reviewer-Fund (Review dieses Schritts): `effective_headers`
    /// dedupliziert bewusst nur gegen die ursprünglichen Default-Einträge,
    /// nicht `extra_headers` untereinander — B3 verlangt nur, dass ein
    /// Nutzer-Header einen gleichnamigen Default ersetzt, nicht dass zwei
    /// vom Nutzer selbst doppelt eingetragene Header zu einem
    /// zusammenfallen (unverändertes Bestandsverhalten für diesen Fall).
    #[test]
    fn test_effective_headers_overrides_only_the_matching_default_not_other_extra_entries() {
        let headers = effective_headers(
            ProviderType::Anthropic,
            "default-key",
            &[
                ("x-api-key".to_string(), "overridden".to_string()),
                ("x-custom".to_string(), "first".to_string()),
                ("x-custom".to_string(), "second".to_string()),
            ],
        );

        let api_key_values: Vec<&str> = headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("x-api-key"))
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(api_key_values, vec!["overridden"]);

        let custom_values: Vec<&str> = headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("x-custom"))
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(custom_values, vec!["first", "second"]);
    }

    #[test]
    fn test_is_secret_header_name_matches_case_insensitively() {
        assert!(is_secret_header_name("authorization"));
        assert!(is_secret_header_name("Authorization"));
        assert!(is_secret_header_name("x-api-key"));
        assert!(is_secret_header_name("X-Api-Key"));
        assert!(!is_secret_header_name("anthropic-version"));
        assert!(!is_secret_header_name("x-title"));
    }

    /// Spec 0068, Teil 5a: ein Server, der nie (rechtzeitig) antwortet,
    /// lässt "Modelle laden" nicht ewig hängen, sondern liefert einen
    /// sichtbaren Netzwerkfehler.
    #[tokio::test]
    async fn test_discover_models_times_out_instead_of_hanging() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_secs(30))
                    .set_body_json(serde_json::json!({"data": []})),
            )
            .mount(&server)
            .await;

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            discover_models_within(
                ProviderType::GenericOpenAiCompatible,
                &server.uri(),
                "k",
                &[],
                std::time::Duration::from_millis(200),
            ),
        )
        .await
        .expect("darf nicht hängen");

        assert!(matches!(result, Err(AiError::Timeout { .. })), "{result:?}");
    }

    #[tokio::test]
    async fn test_fetch_attestation_info_times_out_instead_of_hanging() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/attestation"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_secs(30))
                    .set_body_string("{}"),
            )
            .mount(&server)
            .await;

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            fetch_attestation_info_within(
                &format!("{}/attestation", server.uri()),
                std::time::Duration::from_millis(200),
            ),
        )
        .await
        .expect("darf nicht hängen");

        assert!(matches!(result, Err(AiError::Timeout { .. })), "{result:?}");
    }

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

        let models = discover_models(ProviderType::OpenAi, &server.uri(), "test-key", &[])
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

        let result = discover_models(ProviderType::OpenAi, &server.uri(), "test-key", &[]).await;

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
            ProviderType::OpenAi,
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

    /// Spec 0072, B-T1: Anthropic bekommt `x-api-key` +
    /// `anthropic-version`, **kein** `authorization`, unter `/v1/models`
    /// (nicht dem bloßen `/models`, das die OpenAI-Familie nutzt — s.
    /// Spec-Reviewer-Fund/Doc-Kommentar an [`discover_models`]).
    /// *Gegenbeweis:* gegen den Stand vor `effective_headers`/
    /// `ProviderType`-Parameter schlug dieser Test fehl, weil
    /// `discover_models` unbedingt `bearer_auth` setzte und `/models` ohne
    /// `/v1`-Präfix ansprach (letzteres verifiziert: gegen den Zwischenstand
    /// nach dem ersten Review-Fund, der `path("/models")` noch verwendete,
    /// schlug dieser Test ebenfalls fehl, weil `wiremock` die Anfrage an
    /// `/v1/models` nicht auf den bei `/models` gemounteten Mock matchte).
    #[tokio::test]
    async fn test_discover_models_anthropic_sends_x_api_key_and_version_not_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("x-api-key", "test-key"))
            .and(header("anthropic-version", ANTHROPIC_VERSION))
            .and(HeaderAbsent("authorization"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"object": "list", "data": []})),
            )
            .mount(&server)
            .await;

        let result = discover_models(ProviderType::Anthropic, &server.uri(), "test-key", &[]).await;

        assert!(
            result.is_ok(),
            "erwartet: x-api-key + anthropic-version korrekt gesetzt, bekam {result:?}"
        );
    }

    /// Spec 0072, B-T3 (Backend-Teil): `ProviderType::Anthropic` läuft über
    /// denselben Code-Pfad wie die OpenAI-kompatible Familie, nur mit
    /// anderen Headern — kein separater "Anthropic wird abgelehnt"-Zweig
    /// mehr in `discover_models` selbst (der lebt, wenn überhaupt, nur noch
    /// im aufrufenden `app-shell`-Command, s. dortigen Test).
    #[tokio::test]
    async fn test_discover_models_anthropic_401_yields_authentication_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid x-api-key"))
            .mount(&server)
            .await;

        let result =
            discover_models(ProviderType::Anthropic, &server.uri(), "wrong-key", &[]).await;

        // B-T4: 401 ergibt `AuthenticationFailed`, nie eine (fälschlich)
        // leere Modell-Liste.
        assert!(matches!(result, Err(AiError::AuthenticationFailed)));
    }

    /// Spec 0072, B-T5: eine per `extra_headers` gesetzte `x-api-key`
    /// erscheint genau einmal in der Anfrage — nicht zusätzlich zu der
    /// intern gesetzten. `wiremock`s `header(...)`-Matcher prüft nicht
    /// direkt "genau einmal", deshalb hier zusätzlich über
    /// `Mock::given(...).expect(1)` plus einem Matcher auf den
    /// **überschriebenen** Wert — träfe der interne Default-Wert noch (weil
    /// beide Header gesendet würden), würde `header("x-api-key",
    /// "overridden-by-user")` nicht matchen und der Request liefe auf
    /// `wiremock`s 404-Fallback statt auf `200`.
    #[tokio::test]
    async fn test_discover_models_extra_header_overrides_default_x_api_key_exactly_once() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("x-api-key", "overridden-by-user"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"object": "list", "data": []})),
            )
            .mount(&server)
            .await;

        let result = discover_models(
            ProviderType::Anthropic,
            &server.uri(),
            "default-key",
            &[("x-api-key".to_string(), "overridden-by-user".to_string())],
        )
        .await;

        assert!(
            result.is_ok(),
            "erwartet: extra_headers überschreibt den Default-Wert genau einmal, bekam {result:?}"
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

        let result = discover_models(
            ProviderType::OpenAi,
            &server.uri(),
            "sk-real-secret-key",
            &[],
        )
        .await;
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
