//! HTTP-Fehler → [`AiError`]-Mapping, geteilt zwischen allen Providern
//! (Aufgabenstellung Teil 2, Punkt 4), damit sich das Verhalten nicht
//! zwischen `OpenAiCompatibleProvider` und `AnthropicProvider` auseinander
//! entwickelt.

use std::pin::Pin;

use futures::Stream;
use ssh_manager_core::ai::{AiError, AiEvent};

/// Spec 0072, A5 (vormals Spec 0069, Teil A2/Teil 0.2): Auffangnetz
/// **hinter** [`is_structured_model_not_found`] — Textbausteine
/// (case-insensitive), die auf "das angefragte Modell existiert nicht"
/// hindeuten, für Provider-Antworten, die kein strukturiertes Feld dafür
/// liefern. Aus den Fixtures in `tests/fixtures/model_not_found/`
/// übernommen; die Einträge für OpenAI, OpenRouter und Ollama sind nach wie
/// vor **unbelegt** (BL-0200 misst sie erst noch gegen echte Accounts, s.
/// dortige `README.md`). Anthropics echte Antwort (gemessen, Spec 0072 §1)
/// trifft keinen dieser Marker — dafür ist [`is_structured_model_not_found`]
/// zuständig.
///
/// **Warum eine Substring-Liste statt eines strikten JSON-Schemas pro
/// Provider:** die Provider-Formate unterscheiden sich strukturell zu
/// sehr (OpenAI: `error.code == "model_not_found"`; Ollama: freier
/// String; OpenRouter: `error.message`) für ein gemeinsames Schema, ohne
/// separate Parser zu bauen — ein Substring-Match ist robuster gegen kleine
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
/// Spec 0072, A1/A2: 404 **oder** 400, wenn der Body **entweder** an der
/// Antwortstruktur ([`is_structured_model_not_found`]) **oder** — als
/// Auffangnetz, additiv, nie exklusiv — an einem der
/// [`MODEL_NOT_FOUND_MARKERS`] (case-insensitive) als "Modell nicht
/// gefunden" erkennbar ist → `ModelNotFound` statt `ProviderUnavailable`.
/// 401/403 bleiben `AuthenticationFailed` auch dann, wenn "model" zufällig
/// im Body steht oder die Struktur zuträfe (Prüfreihenfolge: Status
/// zuerst, A3).
pub(crate) fn map_http_status(status: reqwest::StatusCode, body: &str) -> AiError {
    match status.as_u16() {
        401 | 403 => AiError::AuthenticationFailed,
        429 => AiError::RateLimited,
        404 | 400
            if is_structured_model_not_found(body) || contains_model_not_found_marker(body) =>
        {
            AiError::ModelNotFound(format!("HTTP {status}: {body}"))
        }
        _ => AiError::ProviderUnavailable(format!("HTTP {status}: {body}")),
    }
}

/// Spec 0072, A1: erkennt "Modell nicht gefunden" an einem strukturierten
/// Feld der Provider-Antwort statt an Prosa — `error.type ==
/// "not_found_error"` (Anthropic, gemessen §1) oder `error.code ==
/// "model_not_found"` (OpenAI-kompatible Familie). Geprüft wird genau die
/// **erste** Ebene unter `error` (Spec 0072, X2) — kein rekursives Suchen
/// im Baum, das würde z. B. `{"error":{"error":{"type":
/// "not_found_error"}}}` fälschlich treffen, obwohl der äußere Fehler etwas
/// anderes bedeuten könnte.
///
/// A2: greift keine der beiden Formen — sei es, weil `body` kein gültiges
/// JSON ist, `error` fehlt oder kein Objekt ist, oder die Felder einen
/// anderen Typ als `String` haben (Spec 0072, X4) —, liefert diese
/// Funktion `false`, nie einen Fehler/Panic; der Aufrufer fällt dann auf
/// die Markerliste zurück. Rein additiv: kein Fall, den die Markerliste
/// bisher erkannte, wird durch diese Funktion "entzogen".
///
/// Restrisiko (Spec 0072, §4.2), bewusst hier dokumentiert statt versteckt —
/// **breiter als die Spec annahm, s. Spec-Reviewer-Fund (Review dieses
/// Schritts):** `error.type == "not_found_error"` ist bei Anthropic nicht
/// auf Modelle beschränkt — ein unbekannter Pfad oder eine unbekannte
/// Ressourcen-ID erzeugt denselben Typ. Innerhalb dieser Crate betrifft das
/// nicht nur `POST /v1/messages`/`GET /v1/models` (dort ist die einzige vom
/// Nutzer gesetzte Ressource der Modellname), sondern auch
/// `fetch_attestation_info`, das denselben `map_http_status` gegen eine
/// **frei vom Nutzer eingetragene** Attestierungs-URL aufruft (s.
/// `crate::discovery::fetch_attestation_info`) — ein Tippfehler dort kann
/// ebenfalls als "Modell nicht gefunden" erscheinen, obwohl der Endpunkt mit
/// Modellen nichts zu tun hat. Rein kosmetisch (Fehlertext), kein
/// Ausführungspfad/keine Filter-/Auto-Exec-Entscheidung hängt daran — aber
/// nicht mehr "nur zwei eng umrissene Aufrufstellen", wie die Spec annahm.
/// Käme später ein dritter Aufruf mit weiteren Ressourcen hinzu, ist diese
/// Einschätzung erneut zu prüfen.
fn is_structured_model_not_found(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let Some(error) = value.get("error") else {
        return false;
    };
    let error_type = error.get("type").and_then(serde_json::Value::as_str);
    let error_code = error.get("code").and_then(serde_json::Value::as_str);
    error_type == Some("not_found_error") || error_code == Some("model_not_found")
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
    //!
    //! Spec 0072, §6: `test_anthropic_model_not_found_fixture_maps_to_
    //! model_not_found` (T1) ist der Regressionstest für diesen Schritt —
    //! *Gegenbeweis*: gegen den Stand vor `is_structured_model_not_found`
    //! schlug er fehl (verifiziert: die neue, gemessene Fixture trifft
    //! keinen der `MODEL_NOT_FOUND_MARKERS`, s. `anthropic.json`-Kommentar
    //! im Item BL-0200).
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

    // --- Spec 0072, §6.1: Struktur-Erkennung, unabhängig von der Markerliste --

    /// T2 (verschärft): `error.code == "model_not_found"` trifft **auch
    /// dann**, wenn der `message`-Text keinen der `MODEL_NOT_FOUND_MARKERS`
    /// enthält — belegt, dass A1 eigenständig greift, nicht nur zufällig
    /// zusammen mit der Marker-Liste (die echte `openai.json`-Fixture
    /// träfe ohnehin auch über den Marker "does not exist or you do not
    /// have access").
    #[test]
    fn test_structured_error_code_without_any_marker_text_maps_to_model_not_found() {
        let body = r#"{"error":{"message":"nope.","type":"invalid_request_error","code":"model_not_found"}}"#;
        assert!(matches!(
            map_http_status(StatusCode::NOT_FOUND, body),
            AiError::ModelNotFound(_)
        ));
    }

    /// T5: leerer Body — kein gültiges JSON, kein Marker-Treffer, kein
    /// Absturz (A2).
    #[test]
    fn test_empty_body_stays_provider_unavailable() {
        assert!(matches!(
            map_http_status(StatusCode::NOT_FOUND, ""),
            AiError::ProviderUnavailable(_)
        ));
    }

    /// T6 (A3): `not_found_error` bei einem Status außerhalb 404/400 bleibt
    /// `ProviderUnavailable` — die Statusbedingung wird durch A1 nicht
    /// aufgeweicht.
    #[test]
    fn test_structured_not_found_error_at_500_stays_provider_unavailable() {
        let body = r#"{"type":"error","error":{"type":"not_found_error","message":"model: x"}}"#;
        assert!(matches!(
            map_http_status(StatusCode::INTERNAL_SERVER_ERROR, body),
            AiError::ProviderUnavailable(_)
        ));
    }

    /// T7 (A3): 401 geht der Struktur-Prüfung vor — derselbe Body, der bei
    /// 404 `ModelNotFound` ergäbe, bleibt bei 401 `AuthenticationFailed`.
    #[test]
    fn test_structured_not_found_error_at_401_stays_authentication_failed() {
        let body = r#"{"type":"error","error":{"type":"not_found_error","message":"model: x"}}"#;
        assert!(matches!(
            map_http_status(StatusCode::UNAUTHORIZED, body),
            AiError::AuthenticationFailed
        ));
    }

    /// T8 (A3): dasselbe für 429 — `RateLimited`, nicht `ModelNotFound`.
    #[test]
    fn test_structured_not_found_error_at_429_stays_rate_limited() {
        let body = r#"{"type":"error","error":{"type":"not_found_error","message":"model: x"}}"#;
        assert!(matches!(
            map_http_status(StatusCode::TOO_MANY_REQUESTS, body),
            AiError::RateLimited
        ));
    }

    // --- Spec 0072, §6.3: adversariale Fälle -----------------------------

    /// X2: `error.type` **eine Ebene tiefer** (`error.error.type`) darf
    /// nicht treffen — geprüft wird genau `error.type` auf der ersten
    /// Ebene unter dem Top-Level-Body, nicht "irgendwo im Baum".
    #[test]
    fn test_deeply_nested_not_found_error_does_not_match() {
        let body = r#"{"error":{"error":{"type":"not_found_error"}}}"#;
        assert!(matches!(
            map_http_status(StatusCode::NOT_FOUND, body),
            AiError::ProviderUnavailable(_)
        ));
    }

    /// X4: ein `error.type`, das kein String ist, darf nicht abstürzen —
    /// Rückfall auf die Markerliste (die hier ebenfalls nicht trifft).
    #[test]
    fn test_non_string_error_type_does_not_panic_and_falls_back_to_markers() {
        let body = r#"{"error":{"type":123}}"#;
        assert!(matches!(
            map_http_status(StatusCode::NOT_FOUND, body),
            AiError::ProviderUnavailable(_)
        ));
    }

    /// X3: ein sehr großer Body darf `is_structured_model_not_found` nicht
    /// zum Absturz bringen. Spec-Reviewer-Fund (Review dieses Schritts,
    /// „gemessen statt angenommen"): anders als eine frühere Fassung dieses
    /// Kommentars behauptete, gibt es auf dem Lesepfad
    /// (`crate::sse::read_error_body_with_timeout`) **keinen** Byte-Cap,
    /// nur einen Zeit-Timeout (`tokio::time::timeout` um `response.text()`)
    /// — ein Provider/Proxy, der einen sehr großen Fehlerbody schnell genug
    /// liefert, wird vollständig gelesen und hier vollständig geparst. Ein
    /// Byte-Cap wäre eine sinnvolle Ergänzung, ist aber nicht Teil dieser
    /// Spec (Backlog-Hinweis, kein Fix hier). Diese Funktion selbst bleibt
    /// unabhängig davon robust, wenn sie dennoch einen großen String bekommt
    /// (kein Stack-Overflow: `serde_json`s Rekursionslimit fängt tiefe
    /// Verschachtelung ab, s. X2-Test oben; linearer Zeitaufwand für einen
    /// flachen, aber langen String).
    #[test]
    fn test_very_large_body_is_handled_without_panicking() {
        let padding = "x".repeat(5 * 1024 * 1024);
        let body = format!(r#"{{"error":{{"message":"{padding}"}}}}"#);
        assert!(matches!(
            map_http_status(StatusCode::NOT_FOUND, &body),
            AiError::ProviderUnavailable(_)
        ));
    }

    /// X1: Ein Anbieter-Fehlertext ist nicht vertrauenswürdig — selbst wenn
    /// er (hier: platzhalterhaft) wie ein Secret aussieht, entscheidet er
    /// nur über den *Wert* von `ModelNotFound`, nie darüber, was geloggt
    /// wird. Die eigentliche Redaction sitzt in
    /// `request_logging::log_provider_error_response` (geprüft dort) — hier
    /// wird nur belegt, dass der volle Body (inkl. Platzhalter) unverändert
    /// in den `AiError`-Wert übernommen wird, also überhaupt etwas zu
    /// redigieren ist, bevor es geloggt werden darf.
    #[test]
    fn test_model_not_found_value_carries_the_full_body_for_downstream_redaction() {
        let body =
            r#"{"error":{"type":"not_found_error","message":"model: x (key sk-live-hunter2)"}}"#;
        let mapped = map_http_status(StatusCode::NOT_FOUND, body);
        match mapped {
            AiError::ModelNotFound(msg) => assert!(msg.contains("sk-live-hunter2")),
            other => panic!("erwartet ModelNotFound, bekam {other:?}"),
        }
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
