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
use crate::settings_store::SETTINGS_STORE_FILE;
const ENABLED_KEY: &str = "riskClassifierEnabled";
const PROVIDER_ID_KEY: &str = "riskClassifierProviderId";
/// Spec 0092, A1.1.
const RED_RISK_ALWAYS_CONFIRM_KEY: &str = "redRiskAlwaysConfirm";

/// Spec 0092, A1.2/A1.3: liest die app-weite Einstellung „Bei rotem Risiko
/// immer nachfragen", einmalig bei `connect()` (s.
/// `Session::red_risk_always_confirm`).
///
/// **Fail-safe in Richtung „an"**, anders als bei der Zweitmeinung direkt
/// darunter: Dort ist „im Zweifel aus" die sichere Richtung (keine
/// halbkonfigurierte KI-Anfrage), hier ist es „im Zweifel an" (im Zweifel
/// eine Rückfrage zu viel statt eines unbestätigten roten Kommandos). Ein
/// nicht öffenbarer Store, ein fehlender Schlüssel und ein nicht-boolescher
/// Wert führen deshalb alle auf `true`.
pub(crate) fn red_risk_always_confirm<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    let Ok(store) = app.store(SETTINGS_STORE_FILE) else {
        return true;
    };
    red_risk_always_confirm_from_stored(store.get(RED_RISK_ALWAYS_CONFIRM_KEY))
}

/// Die reine Auswertung hinter [`red_risk_always_confirm`] — getrennt, damit
/// Spec 0092, T16 (Schlüssel fehlt / `false` / `"false"` als String) sie
/// ohne `tauri`-Store direkt prüfen kann.
pub(crate) fn red_risk_always_confirm_from_stored(value: Option<serde_json::Value>) -> bool {
    value.is_none_or(|value| value.as_bool().unwrap_or(true))
}

/// Ergebnis der Auflösung der Zweitmeinungs-Einstellungen (Issue #102):
/// "ausgeschaltet" und "eingeschaltet, aber nicht einrichtbar" sind getrennt,
/// damit Letzteres dem Nutzer gemeldet werden kann statt still zu enden.
pub enum SecondOpinionSetup {
    /// Zweitmeinung in den Einstellungen aus (oder Einstellung nicht lesbar
    /// bzw. nie gesetzt): bleibt still wie bisher.
    Disabled,
    /// Eingeschaltet und aufgelöst.
    Ready(Box<dyn AiProvider>, Arc<ProviderBudgetGuard>),
    /// Eingeschaltet, aber der Provider fehlt/ist gelöscht, seine ID ist
    /// ungültig oder sein Credential ist nicht auflösbar.
    Unavailable,
}

type ResolvedProvider = Option<(Box<dyn AiProvider>, Arc<ProviderBudgetGuard>)>;

impl SecondOpinionSetup {
    /// Zerlegt in den bisherigen Provider/Budget-Paarwert und das Flag
    /// "eingeschaltet, aber nicht einrichtbar".
    pub fn into_parts(self) -> (ResolvedProvider, bool) {
        match self {
            SecondOpinionSetup::Disabled => (None, false),
            SecondOpinionSetup::Ready(provider, budget) => (Some((provider, budget)), false),
            SecondOpinionSetup::Unavailable => (None, true),
        }
    }
}

/// Erster Schritt der Auflösung, rein auf den gespeicherten Werten (testbar
/// ohne Store): ist die Zweitmeinung an, und welche Provider-ID ist gewählt?
/// `Err(..)` ist bereits das Endergebnis.
pub(crate) fn parse_second_opinion_settings(
    enabled: Option<serde_json::Value>,
    provider_id: Option<serde_json::Value>,
) -> Result<ssh_manager_core::ai::ProviderId, SecondOpinionSetup> {
    let enabled = enabled.and_then(|v| v.as_bool()).unwrap_or(false);
    if !enabled {
        return Err(SecondOpinionSetup::Disabled);
    }
    provider_id
        .as_ref()
        .and_then(|v| v.as_str())
        .and_then(|raw| uuid::Uuid::parse_str(raw).ok())
        .map(ssh_manager_core::ai::ProviderId)
        .ok_or(SecondOpinionSetup::Unavailable)
}

/// Liest die Zweitmeinungs-Einstellungen und baut bei Bedarf den
/// konfigurierten `AiProvider` — einmalig bei `connect()` aufgerufen (s.
/// `Session::risk_second_opinion_provider`-Doc-Kommentar zur Begründung,
/// warum nicht live pro Aktionsvorschlag neu gelesen). Ohne Provider bleibt
/// Spec 0026 Abschnitt 3 Punkt 1 erfüllt ("Standardmäßig deaktiviert"):
/// lieber gar keine Zweitmeinung als eine mit falscher/fehlender
/// Konfiguration. Issue #102: war sie eingeschaltet, aber der gewählte
/// Provider ist gelöscht, die ID ungültig oder das Credential nicht
/// auflösbar, ist das [`SecondOpinionSetup::Unavailable`] — der Aufrufer
/// meldet es der Sitzung.
pub async fn resolve_second_opinion_provider<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
) -> SecondOpinionSetup {
    // Ein nicht öffenbarer Store verhält sich wie bisher: "aus".
    let Ok(store) = app.store(SETTINGS_STORE_FILE) else {
        return SecondOpinionSetup::Disabled;
    };
    let provider_id =
        match parse_second_opinion_settings(store.get(ENABLED_KEY), store.get(PROVIDER_ID_KEY)) {
            Ok(id) => id,
            Err(setup) => return setup,
        };

    let Ok(config) = state.ai_provider_store.get(&provider_id).await else {
        return SecondOpinionSetup::Unavailable;
    };
    let Ok(api_key) = state.credential_store.get(&config.credential_ref) else {
        return SecondOpinionSetup::Unavailable;
    };

    let (provider, budget) = build_ai_provider(
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
        // Issue #162: Nebenaufruf (Zweitmeinung/Injection-Check) — nie
        // Web-Werkzeuge.
        false,
    );
    SecondOpinionSetup::Ready(provider, budget)
}

/// Spec 0092, T16: die Einstellung darf sich durch einen fehlerhaften Wert in
/// `settings.json` nie still auf „aus" bringen lassen (Angriffsrichtung
/// „Einstellung" im Umsetzungsauftrag).
// Testcode-Ausnahme zum `deny` — s. `lib.rs`, Modulkopf.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::first_run_notice::test_support::{lock, test_app};

    // Issue #102: eingeschaltet-aber-nicht-einrichtbar ist von
    // ausgeschaltet unterscheidbar.
    #[test]
    fn test_disabled_settings_stay_silent() {
        for enabled in [
            None,
            Some(serde_json::json!(false)),
            Some(serde_json::json!("true")),
        ] {
            let result =
                parse_second_opinion_settings(enabled, Some(serde_json::json!("not-a-uuid")));
            assert!(matches!(result, Err(SecondOpinionSetup::Disabled)));
        }
    }

    #[test]
    fn test_enabled_without_usable_provider_id_is_unavailable() {
        for id in [
            None,
            Some(serde_json::json!(null)),
            Some(serde_json::json!("not-a-uuid")),
            Some(serde_json::json!(42)),
        ] {
            let result = parse_second_opinion_settings(Some(serde_json::json!(true)), id);
            assert!(matches!(result, Err(SecondOpinionSetup::Unavailable)));
        }
    }

    #[test]
    fn test_enabled_with_valid_provider_id_proceeds_to_lookup() {
        let id = uuid::Uuid::new_v4();
        let result = parse_second_opinion_settings(
            Some(serde_json::json!(true)),
            Some(serde_json::json!(id.to_string())),
        );
        assert!(matches!(result, Ok(ssh_manager_core::ai::ProviderId(got)) if got == id));
    }

    #[test]
    fn test_setup_into_parts_flags_only_unavailable() {
        assert!(!SecondOpinionSetup::Disabled.into_parts().1);
        let (provider, flagged) = SecondOpinionSetup::Unavailable.into_parts();
        assert!(provider.is_none() && flagged);
    }

    #[test]
    fn test_missing_key_means_on() {
        assert!(red_risk_always_confirm_from_stored(None));
    }

    #[test]
    fn test_explicit_false_means_off() {
        assert!(!red_risk_always_confirm_from_stored(Some(
            serde_json::json!(false)
        )));
    }

    #[test]
    fn test_explicit_true_means_on() {
        assert!(red_risk_always_confirm_from_stored(Some(
            serde_json::json!(true)
        )));
    }

    /// Der entscheidende Fall: ein *String* `"false"` (oder eine `0`) ist
    /// kein boolescher Wert. `as_bool()` liefert dafür `None` — würde das auf
    /// `false` abgebildet, könnte ein einziger falsch getippter Wert in
    /// `settings.json` die Eskalation still abschalten.
    #[test]
    fn test_non_boolean_values_mean_on() {
        for value in [
            serde_json::json!("false"),
            serde_json::json!("true"),
            serde_json::json!(0),
            serde_json::json!(1),
            serde_json::json!(null),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            assert!(
                red_risk_always_confirm_from_stored(Some(value.clone())),
                "nicht-boolescher Wert {value} muss fail-safe als „an\" gelten"
            );
        }
    }

    /// Derselbe Durchlauf über den echten `tauri-plugin-store` — beweist,
    /// dass die reine Auswertung oben tatsächlich am Produktivweg hängt
    /// (`lock()`/Aufräumen: s. `first_run_notice::test_support`-Moduldoc, der
    /// Store ist prozessweit geteilt).
    #[test]
    fn test_reads_through_the_real_store() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle().clone();
        let store = handle
            .store(SETTINGS_STORE_FILE)
            .expect("Store sollte sich öffnen lassen");

        store.delete(RED_RISK_ALWAYS_CONFIRM_KEY);
        assert!(
            red_risk_always_confirm(&handle),
            "fehlender Schlüssel muss „an\" bedeuten"
        );

        store.set(RED_RISK_ALWAYS_CONFIRM_KEY, serde_json::json!(false));
        assert!(!red_risk_always_confirm(&handle));

        store.set(RED_RISK_ALWAYS_CONFIRM_KEY, serde_json::json!("false"));
        assert!(
            red_risk_always_confirm(&handle),
            "String \"false\" ist kein boolescher Wert und muss „an\" bedeuten"
        );

        store.delete(RED_RISK_ALWAYS_CONFIRM_KEY);
        let _ = store.save();
    }
}
