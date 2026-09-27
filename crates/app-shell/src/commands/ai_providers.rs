//! Spec 0007/0025/0050/0072: KI-Provider-Verwaltung (CRUD, Modell-Discovery,
//! Zugangsdaten-Test, Attestierung) — Teil der Spec-0083-Aufteilung von
//! `commands.rs`.

use futures::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use tauri::State;

use persistence_sqlite::AiProviderConfig;
use ssh_manager_core::ai::{
    AiError, AiEvent, AiProvider, ChatMessage, MessageContent, ProviderId, Role, SessionContext,
};

use crate::ai_provider_factory::build_ai_provider;
use crate::dto::{credential_ref_for, AiProviderConfigDto, AiProviderConfigInput};
use crate::error::{keychain_aware_credential_error, CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn list_ai_providers(
    state: State<'_, AppState>,
) -> CommandResult<Vec<AiProviderConfigDto>> {
    let configs = state.ai_provider_store.list().await?;
    Ok(configs.iter().map(AiProviderConfigDto::from).collect())
}

/// Spec 0007, Abschnitt 8.2: Backend generiert eine neue `ProviderId`,
/// speichert `api_key` zuerst über den `CredentialStore`, danach erst die
/// restlichen Felder in `ai_provider_configs`.
#[tauri::command]
pub async fn add_ai_provider(
    state: State<'_, AppState>,
    config: AiProviderConfigInput,
) -> CommandResult<ProviderId> {
    // Spec 0049, Fund 1: als Erstes, bevor `api_key`/`base_url` irgendwo
    // gelesen werden.
    //
    // Spec-Reviewer-Fund (Spec 0049, Review dieses Schritts): dieser
    // Aufruf selbst ist NICHT unit-getestet — `add_ai_provider` nimmt
    // `tauri::State<'_, AppState>` direkt entgegen (anders als z. B.
    // `servers::create_server`, das für Spec 0047 extrahiert wurde) und
    // lässt sich ohne eine echte, laufende Tauri-App nicht konstruieren.
    // `AiProviderConfigInput::trimmed()` selbst ist vollständig getestet
    // (`dto.rs`); dass sie hier tatsächlich aufgerufen wird, ist bewusst
    // nur durch diesen Kommentar und nicht durch einen automatisierten
    // Test abgesichert — eine Extraktion analog zu `servers::create_server`
    // wäre der richtige, aber über Fund 1 hinausgehende nächste Schritt.
    let config = config.trimmed();
    // Spec 0065, Teil 4: vor jedem Persistieren, wie `trimmed()` oben.
    config.validate_max_tokens_override()?;
    let id = ProviderId::new();
    let credential_ref = credential_ref_for(id);
    state
        .credential_store
        .set(&credential_ref, SecretString::from(config.api_key.clone()))
        .map_err(|err| keychain_aware_credential_error(err, state.keychain))?;

    let new_config = config.into_new_config(id);
    if let Err(err) = state.ai_provider_store.create(&new_config).await {
        // Best-effort-Aufräumen: ohne diesen Rückbau bliebe bei einem
        // DB-Fehler ein verwaister Credential-Eintrag im Keychain zurück,
        // auf den keine `ai_provider_configs`-Zeile mehr verweist. Ein
        // Fehler beim Aufräumen selbst wird bewusst verschluckt (nicht per
        // `?` weitergereicht) — der eigentliche Fehler (`err`) ist die
        // relevante Information für den Aufrufer, ein sekundärer
        // Keychain-Fehler beim Aufräumversuch soll ihn nicht überdecken.
        let _ = state.credential_store.delete(&credential_ref);
        return Err(err.into());
    }
    Ok(id)
}

/// Spec 0007, Abschnitt 8.2: leeres `api_key`-Feld heißt "Credential
/// unverändert lassen", nicht "löschen". Reihenfolge bewusst umgekehrt zu
/// `add_ai_provider`/`delete_ai_provider`: erst die DB-Metadaten
/// aktualisieren (schlägt sauber mit `NotFound` fehl, falls `id` nicht
/// existiert), erst danach — nur bei nicht-leerem `api_key` — den
/// Credential überschreiben. So wird nie ein Secret für eine `id`
/// geschrieben, die sich als ungültig herausstellt.
#[tauri::command]
pub async fn update_ai_provider(
    state: State<'_, AppState>,
    id: ProviderId,
    config: AiProviderConfigInput,
) -> CommandResult<()> {
    // Spec 0049, Fund 1: siehe `add_ai_provider` — muss vor dem
    // `api_key.is_empty()`-Check unten laufen, sonst besteht ein rein aus
    // Whitespace bestehender Paste die Prüfung fälschlich und überschreibt
    // das bestehende Credential mit einem leeren Wert.
    let config = config.trimmed();
    config.validate_max_tokens_override()?;
    let api_key = config.api_key.clone();
    state
        .ai_provider_store
        .update_fields(&config.into_update(id))
        .await?;

    if !api_key.is_empty() {
        state
            .credential_store
            .set(&credential_ref_for(id), SecretString::from(api_key))
            .map_err(|err| keychain_aware_credential_error(err, state.keychain))?;
    }
    Ok(())
}

/// Spec 0007, Abschnitt 8.2/9: erst `CredentialStore::delete()`, dann die
/// DB-Zeile — aber erst, nachdem geprüft wurde, dass der Provider nicht
/// aktiv ist (Abschnitt 9: Löschen eines aktiven Providers ist verboten).
/// Würde man `is_active` nicht **vor** dem Credential-Löschen prüfen, könnte
/// ein verbotener Löschversuch trotzdem den Credential eines weiterhin
/// aktiven, in der DB unverändert bleibenden Providers entfernen.
#[tauri::command]
pub async fn delete_ai_provider(state: State<'_, AppState>, id: ProviderId) -> CommandResult<()> {
    let existing = state.ai_provider_store.get(&id).await?;
    if existing.is_active {
        return Err(
            persistence_sqlite::AiProviderStoreError::ActiveProviderDeletionForbidden(id).into(),
        );
    }

    state
        .credential_store
        .delete(&existing.credential_ref)
        .map_err(|err| keychain_aware_credential_error(err, state.keychain))?;
    state.ai_provider_store.delete(&id).await?;
    Ok(())
}

#[tauri::command]
pub async fn set_active_ai_provider(
    state: State<'_, AppState>,
    id: ProviderId,
) -> CommandResult<()> {
    state
        .ai_provider_store
        .set_active(&id, chrono::Utc::now())
        .await?;
    Ok(())
}

/// Spec 0025, Abschnitt 2 / Spec 0072, B1: `GET {base_url}/models` — läuft
/// mit den gerade im Formular eingegebenen, noch nicht gespeicherten Werten
/// (analog zu `test_connection`, Spec 0008 Abschnitt 7), nicht mit einer
/// bereits persistierten Config. `existing_provider_id` deckt denselben Fall
/// wie dort ab: ist das `api_key`-Feld leer (Bearbeiten eines gespeicherten
/// Providers, "leer = unverändert"), wird stattdessen dessen bereits
/// hinterlegtes Credential herangezogen.
///
/// Seit Spec 0072 auch für `anthropic` unterstützt — die frühere Annahme,
/// Anthropic habe "kein äquivalentes `/models`-Endpoint-Verhalten", traf
/// nicht zu (Spec 0072 §1: `GET /v1/models` existiert, ohne Beta-Header).
#[tauri::command]
pub async fn discover_models(
    state: State<'_, AppState>,
    config: AiProviderConfigInput,
    existing_provider_id: Option<ProviderId>,
) -> CommandResult<Vec<String>> {
    // Spec 0049, Fund 1: derselbe Reihenfolge-Grund wie in
    // `add_ai_provider`/`update_ai_provider` — hier zusätzlich relevant,
    // weil ein ungetrimmter `api_key` sonst direkt an den echten Provider
    // ginge und dort mit einem Auth-Fehler abgelehnt würde.
    let config = config.trimmed();
    // Spec-Reviewer-Fund (Review dieses Schritts): seit Spec 0072 listet
    // dieser `matches!` alle vier heute existierenden `ProviderType`-
    // Varianten — für den aktuellen Typ also unerreichbar, nicht mehr "die
    // OpenAI-Familie plus jetzt Anthropic gegen den Rest abgrenzen". Bewusst
    // trotzdem stehen gelassen (nicht entfernt) als Verteidigung-in-der-
    // Tiefe für eine **künftige** fünfte `ProviderType`-Variante: die käme
    // ohne Anpassung hier automatisch auf diesen klaren Fehler statt
    // stillschweigend auf einen wahrscheinlich falsch geformten Request.
    if !matches!(
        config.provider_type,
        ssh_manager_core::ai::ProviderType::OpenAi
            | ssh_manager_core::ai::ProviderType::Anthropic
            | ssh_manager_core::ai::ProviderType::GenericOpenAiCompatible
            | ssh_manager_core::ai::ProviderType::Ollama
    ) {
        return Err("Modell-Discovery wird für diesen Provider-Typ nicht unterstützt".into());
    }

    // Unabhängiger Review-Pass (Spec 0024/0025): ohne diese Prüfung fällt
    // ein fehlendes `base_url` unten stillschweigend auf
    // `DEFAULT_OPENAI_BASE_URL` zurück — bei `GenericOpenAiCompatible`/
    // `Ollama` ist das aber IMMER falsch (der ganze Sinn dieser Typen ist
    // ein eigener Endpunkt). Das Frontend erzwingt eine ausgefüllte
    // Base-URL nur bei Formular-Submit (`required`-Attribut); der
    // "Modelle laden"-Button ist `type="button"` und ruft diesen Command
    // schon VOR dem Ausfüllen auf. Ohne diese serverseitige Prüfung würde
    // der eingegebene API-Key an `api.openai.com` gehen — einen Dritten,
    // mit dem der Nutzer nie interagieren wollte. Dieselbe
    // Verteidigung-in-der-Tiefe wie die `provider_type`-Prüfung oben.
    let needs_base_url = matches!(
        config.provider_type,
        ssh_manager_core::ai::ProviderType::GenericOpenAiCompatible
            | ssh_manager_core::ai::ProviderType::Ollama
    );
    if needs_base_url && config.base_url.as_deref().unwrap_or("").trim().is_empty() {
        return Err("Base-URL erforderlich, bevor Modelle geladen werden können".into());
    }

    let api_key = if !config.api_key.is_empty() {
        config.api_key.clone()
    } else if let Some(id) = existing_provider_id {
        let existing = state.ai_provider_store.get(&id).await?;
        state
            .credential_store
            .get(&existing.credential_ref)
            .map_err(|err| keychain_aware_credential_error(err, state.keychain))?
            .expose_secret()
            .to_string()
    } else {
        return Err("API-Key erforderlich".into());
    };

    // Spec 0072, B1: derselbe provider-abhängige Default wie
    // `ai_provider_factory::build_ai_provider` — sonst ginge ein Anthropic-
    // Discovery-Aufruf ohne eingegebene `base_url` fälschlich gegen
    // `api.openai.com`.
    let default_base_url = match config.provider_type {
        ssh_manager_core::ai::ProviderType::Anthropic => {
            crate::ai_provider_factory::DEFAULT_ANTHROPIC_BASE_URL
        }
        _ => crate::ai_provider_factory::DEFAULT_OPENAI_BASE_URL,
    };
    let base_url = config.base_url.as_deref().unwrap_or(default_base_url);

    // Spec 0069, Teil A4: mit `err.code()` statt über das blanket `From<E:
    // Display>` unten (`CommandError::from`), das den Code verwirft — sonst
    // zeigt "Modelle laden" bei z. B. `AI_MODEL_NOT_FOUND`/
    // `AI_LOCAL_PROVIDER_UNREACHABLE` nur den rohen Text ohne Übersetzung.
    let models = ai_providers::discover_models(
        config.provider_type,
        base_url,
        &api_key,
        &config.extra_headers,
    )
    .await
    .map_err(|err| CommandError::with_code(err.to_string(), err.code()))?;
    Ok(models)
}

/// Spec 0050, Teil 3: Ergebnis von [`test_ai_provider_credentials`] — genau
/// die drei von der Spec verlangten, unterscheidbaren Fälle ("gültig" /
/// "Authentifizierung fehlgeschlagen" / "nicht erreichbar"), analog zu
/// [`crate::dto::TestConnectionResult`] (Spec 0008) für Server.
///
/// Mapping-Entscheidung (nicht von der Spec explizit vorgegeben, hier
/// festgehalten statt stillschweigend getroffen): `AiError::
/// AuthenticationFailed` wird zu `AuthenticationFailed`, **jeder andere**
/// `AiError` (`RateLimited`, `NetworkError`, `InvalidResponse`,
/// `ContextTooLarge`, `ProviderUnavailable` — letzteres deckt laut
/// `crate::error::map_http_status`s eigenem Design-Kommentar auch einen
/// falschen Modellnamen ab, s. Spec 0049s Nachbericht) fällt in
/// `Unreachable`. Für einen dreiwertigen Test-Button ist das die
/// pragmatischste Aufteilung — ein `RateLimited` z. B. heißt zwar
/// eigentlich "Credentials sind gültig, aber gerade gedrosselt", passt
/// aber in keinen der beiden anderen Fälle.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TestAiProviderCredentialsResult {
    Valid,
    AuthenticationFailed,
    Unreachable {
        message: String,
        /// Spec 0069, Teil A4/E3 (BL-0153): additiv, optional — ein altes
        /// Frontend ignoriert das Feld, ein unbekannter Code fällt im
        /// Frontend weiterhin auf `message` zurück. Gesetzt aus
        /// `AiError::code()`, `None` nur wenn `classify_credential_test_
        /// result` je an einer Stelle ohne `AiError` (kann heute nicht
        /// vorkommen, s. `match` dort) aufgerufen würde.
        code: Option<&'static str>,
    },
}

/// Spec 0050, Teil 3: analog zu `test_connection` (Spec 0008, Abschnitt 7)
/// für Server — testet die **gerade eingegebenen, noch nicht
/// gespeicherten** Formulardaten mit einem echten, minimalen Request an den
/// Provider, bevor überhaupt gespeichert wird. `existing_provider_id` deckt
/// denselben "leer = unverändert"-Fall wie `discover_models` ab.
///
/// Nutzt `build_ai_provider` — denselben Konstruktionsweg wie ein echter
/// Chat-Request (Spec 0007, Abschnitt 8.3) — statt eines eigenen,
/// separaten HTTP-Aufbaus: funktioniert dadurch einheitlich für **alle**
/// vier Provider-Typen, inkl. Anthropic. Seit Spec 0072 gilt das auch für
/// `discover_models` (davor lehnte dieser Command Anthropic mangels
/// `/models`-Endpoint-Unterstützung ab — dieser Kommentar hielt den
/// Unterschied fest, den es inzwischen nicht mehr gibt). Der Request selbst
/// ist eine
/// einzelne, minimale Nutzernachricht ("Hi") — geht durch denselben
/// Redaction-sicheren Fehler-Logging-Pfad wie jeder reguläre Chat-Request
/// (Spec 0049, Fund 2), kein Sonderfall für den Testen-Button nötig. Nur
/// das **erste** Stream-Event wird ausgewertet, danach wird der Stream
/// verworfen (nicht bis zum Ende durchlaufen) — für eine Verbindungs-/
/// Auth-Prüfung reicht das, eine vollständige generierte Antwort
/// abzuwarten wäre unnötiger Zeit-/Token-Verbrauch.
/// Spec-Reviewer-Fund (Spec 0050, Review dieses Schritts): isoliert
/// gehalten, damit sich dieser Schutz (anders als `test_ai_provider_
/// credentials` selbst, das `tauri::State` braucht) ohne Weiteres
/// unit-testen lässt — derselbe Grund/dasselbe Muster wie
/// `map_connect_result` (Spec 0047) oder `should_create_chat_session`
/// (Spec 0040) an anderer Stelle in dieser Datei.
fn missing_required_base_url(
    provider_type: ssh_manager_core::ai::ProviderType,
    base_url: Option<&str>,
) -> bool {
    let needs_base_url = matches!(
        provider_type,
        ssh_manager_core::ai::ProviderType::GenericOpenAiCompatible
            | ssh_manager_core::ai::ProviderType::Ollama
    );
    needs_base_url && base_url.unwrap_or("").trim().is_empty()
}

#[cfg(test)]
mod missing_required_base_url_tests {
    use super::*;

    #[test]
    fn test_generic_openai_compatible_without_base_url_is_missing() {
        assert!(missing_required_base_url(
            ssh_manager_core::ai::ProviderType::GenericOpenAiCompatible,
            None
        ));
        assert!(missing_required_base_url(
            ssh_manager_core::ai::ProviderType::GenericOpenAiCompatible,
            Some("   ")
        ));
    }

    #[test]
    fn test_ollama_without_base_url_is_missing() {
        assert!(missing_required_base_url(
            ssh_manager_core::ai::ProviderType::Ollama,
            None
        ));
    }

    #[test]
    fn test_generic_openai_compatible_with_base_url_is_not_missing() {
        assert!(!missing_required_base_url(
            ssh_manager_core::ai::ProviderType::GenericOpenAiCompatible,
            Some("https://my-gateway.example/v1")
        ));
    }

    /// Der eigentliche Spec-Reviewer-Fund: `openai`/`anthropic` haben
    /// einen festen Standard-Endpunkt (s. `ai_provider_factory.rs`) — ein
    /// fehlendes `base_url` ist dort kein Fehler, sonst könnte man diese
    /// beiden Provider gar nicht ohne eine (für sie sinnlose) Base-URL
    /// testen.
    #[test]
    fn test_openai_and_anthropic_never_require_base_url() {
        assert!(!missing_required_base_url(
            ssh_manager_core::ai::ProviderType::OpenAi,
            None
        ));
        assert!(!missing_required_base_url(
            ssh_manager_core::ai::ProviderType::Anthropic,
            None
        ));
    }
}

#[tauri::command]
pub async fn test_ai_provider_credentials(
    state: State<'_, AppState>,
    config: AiProviderConfigInput,
    existing_provider_id: Option<ProviderId>,
) -> CommandResult<TestAiProviderCredentialsResult> {
    // Spec 0049, Fund 1: derselbe Grund wie bei `add_ai_provider`/
    // `discover_models` — vor jeder Verwendung von `api_key`/`base_url`.
    let config = config.trimmed();

    // Spec-Reviewer-Fund (Spec 0050, Review dieses Schritts): derselbe
    // Schutz wie in `discover_models` oben — ohne diese Prüfung fällt ein
    // fehlendes `base_url` bei `GenericOpenAiCompatible`/`Ollama`
    // stillschweigend auf `DEFAULT_OPENAI_BASE_URL` zurück
    // (`build_ai_provider`/`ai_provider_factory.rs`). Der "Zugangsdaten
    // testen"-Button ist wie "Modelle laden" ein `type="button"` und läuft
    // schon vor dem Formular-`required`-Attribut der Base-URL — ohne
    // diese serverseitige Prüfung würde der eingegebene API-Key an
    // `api.openai.com` gehen, einen Dritten, mit dem der Nutzer nie
    // interagieren wollte.
    if missing_required_base_url(config.provider_type, config.base_url.as_deref()) {
        return Err("Base-URL erforderlich, bevor die Zugangsdaten getestet werden können".into());
    }

    let api_key = if !config.api_key.is_empty() {
        config.api_key.clone()
    } else if let Some(id) = existing_provider_id {
        let existing = state.ai_provider_store.get(&id).await?;
        state
            .credential_store
            .get(&existing.credential_ref)
            .map_err(|err| keychain_aware_credential_error(err, state.keychain))?
            .expose_secret()
            .to_string()
    } else {
        return Err("API-Key erforderlich, bevor die Zugangsdaten getestet werden können".into());
    };

    let (provider, _budget) = build_ai_provider(
        &state.rate_limit_registry,
        config.provider_type,
        config.base_url.as_deref(),
        &config.model,
        SecretString::from(api_key),
        config.supports_native_tool_calling,
        config.extra_headers.clone(),
        config.max_tokens_override,
    );

    Ok(classify_credential_test_result(provider.as_ref()).await)
}

/// Von `test_ai_provider_credentials` losgelöst, damit sich das
/// Ergebnis-Mapping (der eigentlich testenswerte Teil, s. Spec 0050,
/// Abschnitt "Testbarkeit": "gegen einen Mock-Provider") isoliert gegen
/// einen `dyn AiProvider` prüfen lässt, ohne einen echten HTTP-Request zu
/// brauchen — analog zum `Connector`-Trait-Muster in `test_connection.rs`
/// (Spec 0008). Baut selbst den minimalen `SessionContext` ("Hi"), da der
/// Inhalt für jeden Aufrufer identisch ist.
async fn classify_credential_test_result(
    provider: &dyn AiProvider,
) -> TestAiProviderCredentialsResult {
    let context = SessionContext {
        system_context: String::new(),
        history: vec![ChatMessage {
            role: Role::User,
            content: MessageContent::Text("Hi".to_string()),
        }],
        available_actions: Vec::new(),
        max_tokens_hint: None,
    };

    let mut events = provider.send(context);
    let first_event = events.next().await;
    // `events` wird hier fallen gelassen, statt den Stream zu Ende zu
    // lesen — s. Doc-Kommentar auf `test_ai_provider_credentials`.

    match first_event {
        Some(AiEvent::Error(AiError::AuthenticationFailed)) => {
            TestAiProviderCredentialsResult::AuthenticationFailed
        }
        Some(AiEvent::Error(err)) => TestAiProviderCredentialsResult::Unreachable {
            message: err.to_string(),
            code: Some(err.code()),
        },
        _ => TestAiProviderCredentialsResult::Valid,
    }
}

/// Spec 0050, Teil 3 ("Testbarkeit"): "Testen-Button: gültiger Key →
/// 'gültig', falscher → 'Auth fehlgeschlagen', unerreichbar → 'nicht
/// erreichbar' (gegen einen Mock-Provider)" — genau das, gegen
/// `test_support::MockAiProvider`, ohne echten HTTP-Request.
#[cfg(test)]
mod credential_test_tests {
    use super::*;
    use crate::test_support::MockAiProvider;

    #[tokio::test]
    async fn test_valid_credentials_yield_valid() {
        let provider = MockAiProvider::new(vec![AiEvent::TextDelta("Hallo!".to_string())]);

        let result = classify_credential_test_result(&provider).await;

        assert!(matches!(result, TestAiProviderCredentialsResult::Valid));
    }

    #[tokio::test]
    async fn test_auth_failure_yields_authentication_failed() {
        let provider = MockAiProvider::new(vec![AiEvent::Error(AiError::AuthenticationFailed)]);

        let result = classify_credential_test_result(&provider).await;

        assert!(matches!(
            result,
            TestAiProviderCredentialsResult::AuthenticationFailed
        ));
    }

    #[tokio::test]
    async fn test_network_error_yields_unreachable() {
        let provider = MockAiProvider::new(vec![AiEvent::Error(AiError::NetworkError(
            "connection refused".to_string(),
        ))]);

        let result = classify_credential_test_result(&provider).await;

        match result {
            TestAiProviderCredentialsResult::Unreachable { message, code } => {
                assert!(message.contains("connection refused"));
                assert_eq!(code, Some("AI_NETWORK_ERROR"));
            }
            other => panic!("erwartet: Unreachable, war: {other:?}"),
        }
    }

    /// Test 5 (Spec 0069, Teil A4/BL-0153): `RateLimited`, `ModelNotFound`,
    /// `LocalProviderUnreachable` müssen als `Unreachable` mit dem
    /// jeweiligen Code ankommen — *Gegenbeweis:* vor diesem Schritt hatte
    /// `Unreachable` gar kein `code`-Feld.
    #[tokio::test]
    async fn test_unreachable_carries_the_specific_ai_error_code() {
        for (err, expected_code) in [
            (AiError::RateLimited, "AI_RATE_LIMITED"),
            (
                AiError::ModelNotFound("x".to_string()),
                "AI_MODEL_NOT_FOUND",
            ),
            (
                AiError::LocalProviderUnreachable("x".to_string()),
                "AI_LOCAL_PROVIDER_UNREACHABLE",
            ),
        ] {
            let provider = MockAiProvider::new(vec![AiEvent::Error(err)]);
            let result = classify_credential_test_result(&provider).await;
            match result {
                TestAiProviderCredentialsResult::Unreachable { code, .. } => {
                    assert_eq!(code, Some(expected_code));
                }
                other => panic!("erwartet: Unreachable, war: {other:?}"),
            }
        }
    }

    /// Spec 0006, Abschnitt 6 / `crate::error::map_http_status`s eigener
    /// Design-Kommentar: ein falscher Modellname landet nicht in einem
    /// eigenen Fall, sondern kollabiert auf `ProviderUnavailable` — dieser
    /// Test hält fest, dass das hier bewusst ebenfalls als `Unreachable`
    /// gilt (s. Doc-Kommentar auf `TestAiProviderCredentialsResult`), nicht
    /// als `AuthenticationFailed`.
    #[tokio::test]
    async fn test_provider_unavailable_yields_unreachable_not_auth_failed() {
        let provider = MockAiProvider::new(vec![AiEvent::Error(AiError::ProviderUnavailable(
            "HTTP 404: model not found".to_string(),
        ))]);

        let result = classify_credential_test_result(&provider).await;

        assert!(matches!(
            result,
            TestAiProviderCredentialsResult::Unreachable { .. }
        ));
    }

    #[tokio::test]
    async fn test_empty_event_stream_yields_valid() {
        // Spec-Reviewer-Fund wäre denkbar: kein Event überhaupt (Stream
        // endet sofort) sollte nicht als Erfolg fehlinterpretiert werden
        // wie ein "es kam wenigstens etwas zurück"-Fall — dokumentiert das
        // aktuelle Verhalten (fällt auf `Valid` zurück, `_`-Arm) explizit,
        // statt es unbeobachtet zu lassen. In der Praxis unwahrscheinlich
        // (jeder reale `AiProvider` liefert entweder ein Event oder einen
        // `Error`), aber MockAiProvider mit leerem Vec macht genau das
        // reproduzierbar.
        let provider = MockAiProvider::new(vec![]);

        let result = classify_credential_test_result(&provider).await;

        assert!(matches!(result, TestAiProviderCredentialsResult::Valid));
    }
}

/// Spec 0025, Abschnitt 4: ruft den beim Provider hinterlegten
/// Attestierungs-Endpunkt ab und liefert die **rohe** Antwort unverändert
/// — anders als `discover_models` erst nach dem Speichern nutzbar
/// (`provider_id` statt Formulardaten), da der Endpunkt laut Spec "beim
/// Speichern und auf Wunsch erneut" abgerufen wird, nicht während der
/// Eingabe.
#[tauri::command]
pub async fn fetch_attestation_info(
    state: State<'_, AppState>,
    provider_id: ProviderId,
) -> CommandResult<String> {
    let existing = state.ai_provider_store.get(&provider_id).await?;
    let url = existing
        .attestation_url
        .ok_or("Kein Attestierungs-Endpunkt für diesen Provider konfiguriert")?;
    let info = ai_providers::fetch_attestation_info(&url).await?;
    Ok(info)
}

/// Kein eigener `get_active`-Query in `SqliteAiProviderStore` (s. dortige
/// API) — bei der zu erwartenden geringen Providerzahl reicht ein Filter
/// über `list()`, ein zusätzlicher SQL-Pfad nur für diesen einen Aufrufer
/// wäre unnötige API-Fläche.
pub(super) async fn active_ai_provider_config(state: &AppState) -> CommandResult<AiProviderConfig> {
    pick_active_provider(state.ai_provider_store.list().await?)
}

/// Reine Auswahl-Logik aus `active_ai_provider_config` herausgezogen (wie
/// `missing_required_base_url`/`map_connect_result` an anderer Stelle in
/// dieser Datei), damit sich der "kein aktiver Provider"-Fehlerfall ohne
/// eine vollständige `AppState` (u. a. `ProfileStore`, `CredentialStore`,
/// `HostKeyStore`, ...) testen lässt.
///
/// Spec 0069, Teil A4: Code statt eines rohen, hart-deutschen Strings —
/// die englische UI zeigte diesen Text vorher unübersetzt an (§1 der Spec,
/// Zeile "Kein aktiver KI-Provider beim Verbinden").
fn pick_active_provider(configs: Vec<AiProviderConfig>) -> CommandResult<AiProviderConfig> {
    configs.into_iter().find(|c| c.is_active).ok_or_else(|| {
        CommandError::with_code(
            "kein aktiver AI-Provider konfiguriert — bitte zuerst in den Einstellungen einrichten",
            "AI_NO_ACTIVE_PROVIDER",
        )
    })
}

#[cfg(test)]
mod pick_active_provider_tests {
    use super::*;
    use ssh_manager_core::profiles::CredentialRef;

    /// Test 9 (Spec 0069, Teil A4): kein aktiver Provider → Code
    /// `AI_NO_ACTIVE_PROVIDER`. *Gegenbeweis:* vor diesem Schritt lieferte
    /// dieser Zweig einen `CommandError` ohne `code` (über den blanket
    /// `From<E: Display>`).
    #[test]
    fn test_no_active_provider_yields_ai_no_active_provider_code() {
        let err = pick_active_provider(Vec::new()).expect_err("keine Configs → Fehler erwartet");
        assert_eq!(err.code, Some("AI_NO_ACTIVE_PROVIDER"));
    }

    #[test]
    fn test_no_provider_is_active_yields_ai_no_active_provider_code() {
        let inactive = test_ai_provider_config(false);
        let err = pick_active_provider(vec![inactive])
            .expect_err("keine aktive Config → Fehler erwartet");
        assert_eq!(err.code, Some("AI_NO_ACTIVE_PROVIDER"));
    }

    #[test]
    fn test_active_provider_among_several_is_returned() {
        let active = test_ai_provider_config(true);
        let id = active.id;
        let configs = vec![
            test_ai_provider_config(false),
            active,
            test_ai_provider_config(false),
        ];
        let picked = pick_active_provider(configs).expect("aktive Config vorhanden");
        assert_eq!(picked.id, id);
    }

    fn test_ai_provider_config(is_active: bool) -> AiProviderConfig {
        let now = chrono::Utc::now();
        AiProviderConfig {
            id: ProviderId::new(),
            display_name: "test".to_string(),
            provider_type: ssh_manager_core::ai::ProviderType::Anthropic,
            base_url: None,
            model: "claude-test".to_string(),
            credential_ref: CredentialRef::new("test:ai"),
            is_active,
            supports_native_tool_calling: true,
            extra_headers: Vec::new(),
            max_tokens_override: None,
            attestation_url: None,
            created_at: now,
            updated_at: now,
        }
    }
}
