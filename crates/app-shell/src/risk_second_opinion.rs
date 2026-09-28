//! Auflösung des konfigurierten KI-Zweitmeinungs-Providers für die
//! Daten-Risiko-Achse (Spec 0026, Abschnitt 3) — bewusst nur diese eine
//! Achse, s. Spec-Begründung ("semantisches Einordnen ... passt besser zu
//! einer KI-Einschätzung als Server-Schaden, der sich gut musterbasiert
//! erfassen lässt").
//!
//! Spec 0084, §4 (Schnitt): Der eigentliche Abruf (`fetch_second_opinion`/
//! `fetch_injection_check`, Prompts, Parser) liegt in `app_logic::second_opinion`
//! (Tauri-frei, zieht nach `app-logic`) — dieses Modul liest nur die
//! Einstellungen (`tauri_plugin_store`) und baut daraus den konfigurierten
//! `AiProvider`, den `commands::connect` einmalig bei `connect()` auflöst
//! und als Wert im `Session` ablegt (s. `Session::risk_second_opinion_
//! provider`-Doc-Kommentar). `orchestration` (`action_exec`) bekommt diesen
//! Provider als Parameter und ruft `second_opinion::fetch_*` selbst auf —
//! dieses Modul bleibt dafür unbekannt.

use std::sync::Arc;

use ai_providers::ProviderBudgetGuard;
use tauri_plugin_store::StoreExt;

use ssh_manager_core::ai::AiProvider;

use app_logic::ai_provider_factory::build_ai_provider;
use app_logic::state::AppState;

/// Spec 0024, Abschnitt 4: derselbe `tauri-plugin-store`-Ablageort wie die
/// UI-Sprache (`frontend/src/i18n.ts`s `STORE_FILE`) — beide sind reine
/// UI-/App-Einstellungen ohne Bezug zu Server-/Gruppen-Fachdaten, keine
/// eigene SQLite-Migration nötig (Spec 0026, Abschnitt 1, Punkt 1 verlangt
/// das explizit: "keine neue SQLite-Tabelle").
const SETTINGS_STORE_FILE: &str = "settings.json";
const ENABLED_KEY: &str = "riskClassifierEnabled";
const PROVIDER_ID_KEY: &str = "riskClassifierProviderId";

/// Liest die Zweitmeinungs-Einstellungen und baut bei Bedarf den
/// konfigurierten `AiProvider` — einmalig bei `connect()` aufgerufen (s.
/// `Session::risk_second_opinion_provider`-Doc-Kommentar zur Begründung,
/// warum nicht live pro Aktionsvorschlag neu gelesen). `None`, wenn die
/// Zweitmeinung deaktiviert ist, kein Provider gewählt wurde, der gewählte
/// Provider inzwischen gelöscht wurde, oder sein Credential nicht auflösbar
/// ist — in jedem dieser Fälle bleibt Spec 0026 Abschnitt 3 Punkt 1 erfüllt
/// ("Standardmäßig deaktiviert"): lieber gar keine Zweitmeinung als eine
/// mit falscher/fehlender Konfiguration.
pub async fn resolve_second_opinion_provider(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Option<(Box<dyn AiProvider>, Arc<ProviderBudgetGuard>)> {
    let store = app.store(SETTINGS_STORE_FILE).ok()?;
    let enabled = store.get(ENABLED_KEY)?.as_bool().unwrap_or(false);
    if !enabled {
        return None;
    }
    let provider_id_raw = store.get(PROVIDER_ID_KEY)?.as_str()?.to_string();
    let provider_id =
        ssh_manager_core::ai::ProviderId(uuid::Uuid::parse_str(&provider_id_raw).ok()?);

    let config = state.ai_provider_store.get(&provider_id).await.ok()?;
    let api_key = state.credential_store.get(&config.credential_ref).ok()?;

    Some(build_ai_provider(
        &state.rate_limit_registry,
        config.provider_type,
        config.base_url.as_deref(),
        &config.model,
        api_key,
        config.supports_native_tool_calling,
        config.extra_headers.clone(),
        // Spec 0065, Teil 4: greift hier ohnehin nie — `build_second_
        // opinion_context` (in `app_logic::second_opinion`) setzt
        // `max_tokens_hint` immer explizit (`SIDE_CALL_MAX_TOKENS`), der
        // laut Rangfolge Vorrang hat.
        // Trotzdem korrekt durchgereicht statt hart `None`, für den Fall,
        // dass dieser Provider künftig noch für einen zweiten,
        // hint-losen Zweck wiederverwendet wird.
        config.max_tokens_override,
    ))
}
