//! Baut die konkrete `crates/ai-providers`-Implementierung für eine
//! gespeicherte `AiProviderConfig` (Spec 0007, Abschnitt 8).

use ai_providers::{AnthropicProvider, OpenAiCompatibleProvider};
use secrecy::{ExposeSecret, SecretString};
use ssh_manager_core::ai::{AiProvider, ProviderType};

pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
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
    // Spec 0025, Abschnitt 3 — nur für die OpenAI-kompatible Familie
    // (`OpenAiCompatibleProvider`); `AnthropicProvider` braucht wegen des
    // abweichenden Request-Formats eine eigene Erweiterung, falls das
    // später gewünscht wird (nicht Teil dieser Spec).
    extra_headers: Vec<(String, String)>,
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
                extra_headers,
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

#[cfg(test)]
mod tests {
    use ssh_manager_core::ai::SessionContext;
    use ssh_manager_core::profiles::{CredentialRef, CredentialStore};

    use super::*;
    use crate::test_support::InMemoryCredentialStore;

    fn empty_context() -> SessionContext {
        SessionContext {
            system_context: String::new(),
            history: Vec::new(),
            available_actions: Vec::new(),
        }
    }

    /// Spec 0022, Abschnitt 3, erster Punkt: der Provider-API-Key wird beim
    /// Aufbau der `AiProvider`-Instanz **einmalig** aus dem `CredentialStore`
    /// gelesen (hier über `store.get(...)` simuliert — exakt der Ablauf aus
    /// `crate::commands::connect`, Zeile "let api_key = state.credential_
    /// store.get(&active_config.credential_ref)?") und danach als reines
    /// `String`-Feld in die Provider-Instanz eingebettet (s.
    /// `OpenAiCompatibleProvider`/`AnthropicProvider`). Mehrere `send()`-
    /// Aufrufe (mehrere Chat-Runden über dieselbe Session) dürfen den Store
    /// nicht erneut ansprechen — `send()` selbst nimmt strukturell gar
    /// keinen `CredentialStore`-Parameter entgegen, kann ihn also gar nicht
    /// erreichen; dieser Test macht diese Garantie trotzdem explizit und
    /// ausführbar, statt sie nur implizit im Typsystem zu verstecken.
    #[test]
    fn test_provider_api_key_read_once_regardless_of_send_call_count() {
        let credential_ref = CredentialRef::new("ai-provider:test");
        let store = InMemoryCredentialStore::new().with_secret(&credential_ref, "sk-test-key");

        let api_key = store.get(&credential_ref).expect("Key muss auflösbar sein");
        assert_eq!(store.get_calls(), 1);

        let provider =
            build_ai_provider(ProviderType::OpenAi, None, "gpt-4o", api_key, true, Vec::new());

        // Fünf "Chat-Runden" — `send()` liefert nur einen (nicht gepollten)
        // Stream zurück, es geschieht keine echte Netzwerk-I/O, aber jeder
        // Aufruf würde einen erneuten Store-Zugriff sofort sichtbar machen,
        // falls der Key doch nicht gecacht wäre.
        for _ in 0..5 {
            let _ = provider.send(empty_context());
        }

        assert_eq!(
            store.get_calls(),
            1,
            "der Provider-API-Key darf über mehrere send()-Aufrufe hinweg nicht erneut aus dem CredentialStore gelesen werden"
        );
    }
}
