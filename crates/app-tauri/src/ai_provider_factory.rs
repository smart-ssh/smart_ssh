//! Baut die konkrete `crates/ai-providers`-Implementierung für eine
//! gespeicherte `AiProviderConfig` (Spec 0007, Abschnitt 8).

use ai_providers::{AnthropicProvider, OpenAiCompatibleProvider};
use secrecy::{ExposeSecret, SecretString};
use ssh_manager_core::ai::{AiProvider, ProviderType};

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

/// `base_url` ist in der Persistenz nur für `generic_openai_compatible`/
/// `ollama` als Pflichtfeld vorgesehen (Spec 0007, Abschnitt 8.3); für
/// `openai`/`anthropic` fällt diese Funktion auf den offiziellen
/// Standard-Endpoint zurück, falls trotzdem einer hinterlegt wurde, wird er
/// respektiert (deckt z. B. einen unternehmensinternen Azure-/Proxy-
/// Endpoint ab, ohne dass die Spec das explizit vorsehen musste).
pub fn build_ai_provider(
    provider_type: ProviderType,
    base_url: Option<&str>,
    model: &str,
    api_key: SecretString,
    supports_native_tool_calling: bool,
) -> Box<dyn AiProvider> {
    let api_key = api_key.expose_secret().to_string();
    match provider_type {
        ProviderType::OpenAi | ProviderType::GenericOpenAiCompatible | ProviderType::Ollama => {
            let resolved_base_url = base_url.unwrap_or(DEFAULT_OPENAI_BASE_URL);
            Box::new(OpenAiCompatibleProvider::new(
                resolved_base_url,
                model,
                api_key,
                supports_native_tool_calling,
            ))
        }
        ProviderType::Anthropic => {
            let resolved_base_url = base_url.unwrap_or(DEFAULT_ANTHROPIC_BASE_URL);
            Box::new(AnthropicProvider::new(
                resolved_base_url,
                model,
                api_key,
                supports_native_tool_calling,
            ))
        }
    }
}
