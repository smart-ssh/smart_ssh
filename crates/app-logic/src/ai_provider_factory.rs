//! Baut die konkrete `crates/ai-providers`-Implementierung für eine
//! gespeicherte `AiProviderConfig` (Spec 0007, Abschnitt 8).

use std::sync::Arc;

use crate::ai_provider_decorator::{apply_decorators, AiProviderDecorator, AiProviderInfo};
use ai_providers::{
    provider_identity_key, AnthropicProvider, OpenAiCompatibleProvider, OpenAiResponsesProvider,
    ProviderBudgetGuard, RateLimitRegistry,
};
use persistence_sqlite::AiProviderConfig;
use secrecy::{ExposeSecret, SecretString};
use ssh_manager_core::ai::{AiProvider, ProviderType};
use ssh_manager_core::profiles::{CredentialError, CredentialStore};

pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

/// Endpoint when no base URL is set: Anthropic's own, else official OpenAI (Spec 0072, B1).
pub fn default_base_url(provider_type: ProviderType) -> &'static str {
    match provider_type {
        ProviderType::Anthropic => DEFAULT_ANTHROPIC_BASE_URL,
        _ => DEFAULT_OPENAI_BASE_URL,
    }
}

/// The official OpenAI API: provider type `openai` without a base URL or
/// with the default one (ADR 0124). Only this combination uses the
/// Responses API; custom endpoints, generic compatible endpoints and Ollama
/// keep the Chat Completions request unchanged.
pub fn is_official_openai(provider_type: ProviderType, base_url: Option<&str>) -> bool {
    provider_type == ProviderType::OpenAi
        && base_url.is_none_or(|url| url.trim().trim_end_matches('/') == DEFAULT_OPENAI_BASE_URL)
}

/// `base_url` ist in der Persistenz nur für `generic_openai_compatible`/
/// `ollama` als Pflichtfeld vorgesehen (Spec 0007, Abschnitt 8.3); für
/// `openai`/`anthropic` fällt diese Funktion auf den offiziellen
/// Standard-Endpoint zurück, falls trotzdem einer hinterlegt wurde, wird er
/// respektiert (deckt z. B. einen unternehmensinternen Azure-/Proxy-
/// Endpoint ab, ohne dass die Spec das explizit vorsehen musste).
///
/// Spec 0061: gibt zusätzlich den (ggf. mit anderen Aufrufern über
/// `registry` geteilten) [`ProviderBudgetGuard`] dieser Provider-Identität
/// zurück — der einzige Konstruktions-Trichter (Spec 0022, Abschnitt 3:
/// "der Provider-API-Key wird beim Aufbau der `AiProvider`-Instanz
/// einmalig gelesen") ist auch der einzige Ort, an dem `base_url`/`model`/
/// der entschärfte `api_key` gleichzeitig vorliegen, um
/// [`provider_identity_key`] zu bilden, BEVOR der Key als reiner `String`
/// im Provider verschwindet.
#[expect(clippy::too_many_arguments, reason = "one argument per input")]
pub fn build_ai_provider(
    registry: &RateLimitRegistry,
    provider_type: ProviderType,
    base_url: Option<&str>,
    model: &str,
    api_key: SecretString,
    supports_native_tool_calling: bool,
    // Spec 0025, Abschnitt 3 — nur für die OpenAI-kompatible Familie
    // (`OpenAiCompatibleProvider`); `AnthropicProvider` braucht wegen des
    // abweichenden Request-Formats eine eigene Erweiterung, falls das
    // später gewünscht wird (nicht Teil dieser Spec).
    extra_headers: Vec<(String, String)>,
    // Spec 0065, Teil 4: „Automatisch" (`None`) bei beiden Familien —
    // reicht unverändert an den jeweiligen Provider-Konstruktor durch, s.
    // dortigen `max_tokens_override`-Doc-Kommentar zur Rangfolge gegenüber
    // `SessionContext::max_tokens_hint`.
    max_tokens_override: Option<u32>,
    // Issue #162: serverseitige Web-Recherche — wirkt nur bei Providern
    // mit serverseitigen Web-Werkzeugen (Anthropic, offizielles OpenAI über
    // die Responses API, ADR 0127). Die OpenAI-kompatible Familie bekommt
    // nie ein Web-Werkzeug: ob ein beliebiger kompatibler Endpunkt eines
    // kennt, lässt sich nicht sicher feststellen (ADR 0117).
    web_research_enabled: bool,
) -> (Box<dyn AiProvider>, Arc<ProviderBudgetGuard>) {
    let api_key = api_key.expose_secret().to_string();
    let resolved_base_url = base_url.unwrap_or_else(|| default_base_url(provider_type));
    let budget = registry.guard_for(&provider_identity_key(resolved_base_url, model, &api_key));
    let provider: Box<dyn AiProvider> = match provider_type {
        ProviderType::Anthropic => Box::new(
            AnthropicProvider::new(
                resolved_base_url,
                model,
                api_key,
                supports_native_tool_calling,
                budget.clone(),
                max_tokens_override,
            )
            .with_web_research(web_research_enabled),
        ),
        ProviderType::OpenAi | ProviderType::GenericOpenAiCompatible | ProviderType::Ollama => {
            if is_official_openai(provider_type, base_url) {
                // Issue #168: only the official provider has the Responses
                // `web_search` tool; compatible endpoints never get one.
                Box::new(
                    OpenAiResponsesProvider::new(
                        resolved_base_url,
                        model,
                        api_key,
                        supports_native_tool_calling,
                        extra_headers,
                        max_tokens_override,
                    )
                    .with_web_research(web_research_enabled),
                )
            } else {
                Box::new(OpenAiCompatibleProvider::new(
                    resolved_base_url,
                    model,
                    api_key,
                    supports_native_tool_calling,
                    extra_headers,
                    registry.stream_options_rejections(),
                    max_tokens_override,
                ))
            }
        }
    };
    (provider, budget)
}

/// Issue #1246: passes a built provider through the registered decorators
/// (registration order). No decorator: the provider is returned unchanged.
/// Only type, effective endpoint and model reach a decorator, never the key
/// or extra headers.
pub fn decorate_provider(
    decorators: &[Arc<dyn AiProviderDecorator>],
    provider_type: ProviderType,
    base_url: Option<&str>,
    model: &str,
    provider: Box<dyn AiProvider>,
) -> Box<dyn AiProvider> {
    if decorators.is_empty() {
        return provider;
    }
    let info = AiProviderInfo {
        provider_type,
        base_url: base_url
            .unwrap_or_else(|| default_base_url(provider_type))
            .to_string(),
        model: model.to_string(),
    };
    apply_decorators(decorators, &info, provider)
}

/// Issue #249: der eine Pfad „API-Key aus dem `CredentialStore` lesen und
/// den Provider bauen“ für `connect()` und die Notiz-Kürzung. Der Key wird
/// hier genau einmal gelesen (Spec 0022, Abschnitt 3); der gebaute Provider
/// hat strukturell keinen Zugriff auf den Store, `send()` liest also nie
/// erneut. Die Anzahl der Store-Lesevorgänge ist damit eine Eigenschaft
/// dieser Funktion und wird dort getestet.
pub fn build_ai_provider_from_config(
    registry: &RateLimitRegistry,
    credential_store: &dyn CredentialStore,
    config: &AiProviderConfig,
    web_research_enabled: bool,
    decorators: &[Arc<dyn AiProviderDecorator>],
) -> Result<(Box<dyn AiProvider>, Arc<ProviderBudgetGuard>), CredentialError> {
    let api_key = credential_store.get(&config.credential_ref)?;
    let (provider, budget) = build_ai_provider(
        registry,
        config.provider_type,
        config.base_url.as_deref(),
        &config.model,
        api_key,
        config.supports_native_tool_calling,
        config.extra_headers.clone(),
        config.max_tokens_override,
        web_research_enabled,
    );
    let provider = decorate_provider(
        decorators,
        config.provider_type,
        config.base_url.as_deref(),
        &config.model,
        provider,
    );
    Ok((provider, budget))
}

/// [`build_ai_provider_from_config`] for async callers: the key is read on
/// the blocking pool (issue #898), then the provider is built as before.
pub async fn build_ai_provider_from_config_async(
    registry: &RateLimitRegistry,
    credential_store: Arc<dyn CredentialStore + Send + Sync>,
    config: &AiProviderConfig,
    web_research_enabled: bool,
    decorators: &[Arc<dyn AiProviderDecorator>],
) -> Result<(Box<dyn AiProvider>, Arc<ProviderBudgetGuard>), CredentialError> {
    let credential_ref = config.credential_ref.clone();
    let api_key = persistence_sqlite::runtime_bridge::spawn_blocking_io(move || {
        credential_store.get(&credential_ref)
    })
    .await?;
    let (provider, budget) = build_ai_provider(
        registry,
        config.provider_type,
        config.base_url.as_deref(),
        &config.model,
        api_key,
        config.supports_native_tool_calling,
        config.extra_headers.clone(),
        config.max_tokens_override,
        web_research_enabled,
    );
    let provider = decorate_provider(
        decorators,
        config.provider_type,
        config.base_url.as_deref(),
        &config.model,
        provider,
    );
    Ok((provider, budget))
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_the_official_openai_endpoint_uses_the_responses_api() {
        use ssh_manager_core::ai::ProviderType as P;
        assert!(is_official_openai(P::OpenAi, None));
        assert!(is_official_openai(P::OpenAi, Some(DEFAULT_OPENAI_BASE_URL)));
        assert!(is_official_openai(
            P::OpenAi,
            Some("https://api.openai.com/v1/")
        ));
        // Custom base URL (proxy), generic compatible endpoints and Ollama
        // stay on Chat Completions, even with the official URL.
        assert!(!is_official_openai(
            P::OpenAi,
            Some("https://proxy.example.com/v1")
        ));
        assert!(!is_official_openai(P::GenericOpenAiCompatible, None));
        assert!(!is_official_openai(
            P::GenericOpenAiCompatible,
            Some(DEFAULT_OPENAI_BASE_URL)
        ));
        assert!(!is_official_openai(
            P::Ollama,
            Some("http://localhost:11434/v1")
        ));
        assert!(!is_official_openai(P::Anthropic, None));
    }

    use ssh_manager_core::ai::SessionContext;
    use ssh_manager_core::profiles::CredentialRef;

    use super::*;
    use crate::test_support::InMemoryCredentialStore;

    fn empty_context() -> SessionContext {
        SessionContext {
            system_context: String::new(),
            history: Vec::new(),
            available_actions: Vec::new(),
            max_tokens_hint: None,
        }
    }

    fn test_config(credential_ref: CredentialRef) -> AiProviderConfig {
        let now = chrono::Utc::now();
        AiProviderConfig {
            id: ssh_manager_core::ai::ProviderId::new(),
            provider_type: ProviderType::OpenAi,
            display_name: "Test".to_string(),
            base_url: None,
            model: "gpt-4o".to_string(),
            supports_native_tool_calling: true,
            credential_ref,
            is_active: true,
            extra_headers: Vec::new(),
            attestation_url: None,
            max_tokens_override: None,
            web_research_enabled: false,
            created_at: now,
            updated_at: now,
        }
    }

    /// Spec 0022, Abschnitt 3, erster Punkt (Issue #249): der Provider-API-
    /// Key wird beim Aufbau der `AiProvider`-Instanz **einmalig** aus dem
    /// `CredentialStore` gelesen. Der Test geht über den Produktionspfad
    /// (`build_ai_provider_from_config`, den `connect()` und die Notiz-
    /// Kürzung nutzen) und zählt die Store-Zugriffe über mehrere `send()`-
    /// Aufrufe hinweg. Gegenprobe: liest der Helper den Store zweimal (oder
    /// bei jedem Aufruf), schlägt die Zählung fehl.
    #[test]
    fn test_provider_api_key_read_once_regardless_of_send_call_count() {
        let credential_ref = CredentialRef::new("ai-provider:test");
        let store = InMemoryCredentialStore::new().with_secret(&credential_ref, "sk-test-key");
        let config = test_config(credential_ref);

        let registry = RateLimitRegistry::new();
        let (provider, _budget) =
            build_ai_provider_from_config(&registry, &store, &config, false, &[])
                .expect("Key muss auflösbar sein");
        assert_eq!(store.get_calls(), 1);

        for _ in 0..5 {
            let _ = provider.send(ssh_manager_core::ai::RedactedContext::for_tests(
                empty_context(),
            ));
        }

        assert_eq!(
            store.get_calls(),
            1,
            "der Provider-API-Key darf über mehrere send()-Aufrufe hinweg nicht erneut aus dem CredentialStore gelesen werden"
        );
    }

    /// Ein fehlender Key ist ein Fehler des Helpers, kein stiller Fallback.
    #[test]
    fn test_build_ai_provider_from_config_fails_without_stored_key() {
        let store = InMemoryCredentialStore::new();
        let config = test_config(CredentialRef::new("ai-provider:missing"));
        let registry = RateLimitRegistry::new();

        let result = build_ai_provider_from_config(&registry, &store, &config, false, &[]);

        assert!(matches!(result, Err(CredentialError::NotFound(_))));
    }

    /// Spec 0061, Abschnitt 2 (die Kern-Entscheidung): zwei Aufrufe mit
    /// identischem Key/Endpunkt/Modell (z. B. Haupt-Provider und
    /// Zweitmeinungs-Provider, wenn ein Nutzer denselben Provider für beide
    /// wählt) müssen sich EIN Budget teilen, nicht zwei unabhängige
    /// bekommen.
    #[test]
    fn test_build_ai_provider_shares_one_budget_for_the_same_identity() {
        let registry = RateLimitRegistry::new();
        let (_provider_a, budget_a) = build_ai_provider(
            &registry,
            ProviderType::Anthropic,
            None,
            "claude-test",
            SecretString::from("sk-same-key".to_string()),
            true,
            Vec::new(),
            None,
            false,
        );
        let (_provider_b, budget_b) = build_ai_provider(
            &registry,
            ProviderType::Anthropic,
            None,
            "claude-test",
            SecretString::from("sk-same-key".to_string()),
            true,
            Vec::new(),
            None,
            false,
        );

        assert!(
            std::sync::Arc::ptr_eq(&budget_a, &budget_b),
            "gleicher Key/Endpunkt/Modell muss denselben Budget-Wächter liefern"
        );
    }

    /// Gegenprobe: ein anderer Key (andere Provider-Identität) bekommt
    /// einen eigenständigen Wächter — sonst würden fremde Accounts
    /// versehentlich ein Budget teilen.
    #[test]
    fn test_build_ai_provider_uses_separate_budgets_for_different_identities() {
        let registry = RateLimitRegistry::new();
        let (_provider_a, budget_a) = build_ai_provider(
            &registry,
            ProviderType::Anthropic,
            None,
            "claude-test",
            SecretString::from("sk-key-one".to_string()),
            true,
            Vec::new(),
            None,
            false,
        );
        let (_provider_b, budget_b) = build_ai_provider(
            &registry,
            ProviderType::Anthropic,
            None,
            "claude-test",
            SecretString::from("sk-key-two".to_string()),
            true,
            Vec::new(),
            None,
            false,
        );

        assert!(
            !std::sync::Arc::ptr_eq(&budget_a, &budget_b),
            "unterschiedliche Keys dürfen sich kein Budget teilen"
        );
    }

    // --- Issue #1246: provider decorators --------------------------------

    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use futures::Stream;
    use ssh_manager_core::ai::{AiEvent, RedactedContext};

    struct Inner {
        web_disabled: Arc<AtomicBool>,
    }
    impl AiProvider for Inner {
        fn send(&self, _c: RedactedContext) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
            Box::pin(futures::stream::empty())
        }
        fn disable_web_research(&self) {
            self.web_disabled.store(true, Ordering::SeqCst);
        }
        fn last_request_id(&self) -> Option<uuid::Uuid> {
            Some(uuid::Uuid::nil())
        }
    }

    /// Wrapper that forwards everything, as the trait docs demand.
    struct Wrap(Box<dyn AiProvider>);
    impl AiProvider for Wrap {
        fn send(&self, c: RedactedContext) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
            self.0.send(c)
        }
        fn disable_web_research(&self) {
            self.0.disable_web_research();
        }
        fn last_request_id(&self) -> Option<uuid::Uuid> {
            self.0.last_request_id()
        }
    }

    struct Recorder {
        name: &'static str,
        log: Arc<Mutex<Vec<(String, AiProviderInfo)>>>,
    }
    impl AiProviderDecorator for Recorder {
        fn decorate(
            &self,
            info: &AiProviderInfo,
            provider: Box<dyn AiProvider>,
        ) -> Box<dyn AiProvider> {
            self.log
                .lock()
                .unwrap()
                .push((self.name.to_string(), info.clone()));
            Box::new(Wrap(provider))
        }
    }

    fn recorder(
        name: &'static str,
        log: &Arc<Mutex<Vec<(String, AiProviderInfo)>>>,
    ) -> Arc<dyn AiProviderDecorator> {
        Arc::new(Recorder {
            name,
            log: log.clone(),
        })
    }

    #[test]
    fn test_no_decorator_returns_provider_unchanged() {
        let flag = Arc::new(AtomicBool::new(false));
        let provider: Box<dyn AiProvider> = Box::new(Inner { web_disabled: flag });
        let raw = &*provider as *const dyn AiProvider as *const ();
        let out = decorate_provider(&[], ProviderType::OpenAi, None, "m", provider);
        assert_eq!(&*out as *const dyn AiProvider as *const (), raw);
    }

    /// Covers the connect / notes / second-opinion path (all use the async
    /// from-config builder): the decorator wraps and sees no key.
    #[tokio::test]
    async fn test_from_config_async_applies_decorators_in_order_without_key() {
        let credential_ref = CredentialRef::new("ai-provider:deco");
        let store = Arc::new(
            InMemoryCredentialStore::new().with_secret(&credential_ref, "sk-secret-value"),
        );
        let config = test_config(credential_ref);
        let log = Arc::new(Mutex::new(Vec::new()));
        let decorators = vec![recorder("first", &log), recorder("second", &log)];

        let registry = RateLimitRegistry::new();
        let _ = build_ai_provider_from_config_async(&registry, store, &config, false, &decorators)
            .await
            .expect("key resolvable");

        let log = log.lock().unwrap();
        let names: Vec<_> = log.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["first", "second"]);
        assert_eq!(log[0].1.model, "gpt-4o");
        assert_eq!(log[0].1.base_url, DEFAULT_OPENAI_BASE_URL);
        assert!(!format!("{:?}", log[0].1).contains("sk-secret-value"));
    }

    #[test]
    fn test_from_config_sync_applies_decorator() {
        let credential_ref = CredentialRef::new("ai-provider:deco2");
        let store = InMemoryCredentialStore::new().with_secret(&credential_ref, "sk-x");
        let config = test_config(credential_ref);
        let log = Arc::new(Mutex::new(Vec::new()));
        let registry = RateLimitRegistry::new();
        let _ = build_ai_provider_from_config(
            &registry,
            &store,
            &config,
            false,
            &[recorder("only", &log)],
        )
        .unwrap();
        assert_eq!(log.lock().unwrap().len(), 1);
    }

    /// Credential-test path: `decorate_provider` on a directly built provider.
    #[test]
    fn test_decorate_provider_for_credential_test_path() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let registry = RateLimitRegistry::new();
        let (provider, _b) = build_ai_provider(
            &registry,
            ProviderType::Anthropic,
            None,
            "claude-x",
            SecretString::from("sk-y"),
            true,
            Vec::new(),
            None,
            false,
        );
        let _ = decorate_provider(
            &[recorder("cred", &log)],
            ProviderType::Anthropic,
            None,
            "claude-x",
            provider,
        );
        let log = log.lock().unwrap();
        assert_eq!(log[0].1.base_url, DEFAULT_ANTHROPIC_BASE_URL);
        assert_eq!(log[0].1.provider_type, ProviderType::Anthropic);
    }

    #[test]
    fn test_forwarding_wrapper_keeps_web_research_and_request_id() {
        let flag = Arc::new(AtomicBool::new(false));
        let inner: Box<dyn AiProvider> = Box::new(Inner {
            web_disabled: flag.clone(),
        });
        let log = Arc::new(Mutex::new(Vec::new()));
        let out = decorate_provider(
            &[recorder("w", &log)],
            ProviderType::OpenAi,
            None,
            "m",
            inner,
        );
        out.disable_web_research();
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(out.last_request_id(), Some(uuid::Uuid::nil()));
    }
}
