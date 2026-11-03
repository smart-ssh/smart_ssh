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

use app_logic::ai_provider_factory::build_ai_provider_from_config;
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

/// Issue #856: the security switches are written only here, never by the
/// webview directly.
pub(crate) fn write_red_risk_always_confirm<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    value: bool,
) -> app_logic::error::CommandResult<()> {
    crate::settings_store::update(app, |changes| {
        changes.set(RED_RISK_ALWAYS_CONFIRM_KEY, serde_json::json!(value))
    })
}

#[tauri::command]
pub fn set_red_risk_always_confirm(
    app: tauri::AppHandle,
    value: bool,
) -> app_logic::error::CommandResult<()> {
    write_red_risk_always_confirm(&app, value)
}

/// Validates the second-opinion settings before they are written: enabling
/// needs a provider id, and a given id must be a configured provider
/// (`known`). Returns the provider id string to store (`None` = null).
pub(crate) fn validate_second_opinion_settings(
    enabled: bool,
    provider_id: Option<&str>,
    known: impl FnOnce(&ssh_manager_core::ai::ProviderId) -> bool,
) -> app_logic::error::CommandResult<Option<String>> {
    use app_logic::error::{
        CommandError, RISK_SETTINGS_PROVIDER_REQUIRED, RISK_SETTINGS_PROVIDER_UNKNOWN,
    };
    let Some(raw) = provider_id else {
        if enabled {
            return Err(CommandError::with_code(
                "Enabling the risk second opinion requires an AI provider.",
                RISK_SETTINGS_PROVIDER_REQUIRED,
            ));
        }
        return Ok(None);
    };
    let unknown = || {
        CommandError::with_code(
            "The selected AI provider is not configured.",
            RISK_SETTINGS_PROVIDER_UNKNOWN,
        )
    };
    let id = uuid::Uuid::parse_str(raw)
        .map(ssh_manager_core::ai::ProviderId)
        .map_err(|_| unknown())?;
    if !known(&id) {
        return Err(unknown());
    }
    Ok(Some(id.0.to_string()))
}

/// Validates, then writes both keys in one atomic update. A rejected value
/// leaves the settings file untouched.
pub(crate) fn write_second_opinion_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    enabled: bool,
    provider_id: Option<&str>,
    known: impl FnOnce(&ssh_manager_core::ai::ProviderId) -> bool,
) -> app_logic::error::CommandResult<()> {
    let stored = validate_second_opinion_settings(enabled, provider_id, known)?;
    crate::settings_store::update(app, |changes| {
        changes.set(ENABLED_KEY, serde_json::json!(enabled));
        changes.set(PROVIDER_ID_KEY, serde_json::json!(stored));
    })
}

#[tauri::command]
pub async fn set_risk_second_opinion_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    enabled: bool,
    provider_id: Option<String>,
) -> app_logic::error::CommandResult<()> {
    // Look the provider up first so the validation itself stays synchronous.
    let known = match provider_id
        .as_deref()
        .and_then(|raw| uuid::Uuid::parse_str(raw).ok())
        .map(ssh_manager_core::ai::ProviderId)
    {
        Some(id) => state.ai_provider_store.get(&id).await.is_ok(),
        None => false,
    };
    write_second_opinion_settings(&app, enabled, provider_id.as_deref(), |_| known)
}

/// Ergebnis der Auflösung der Zweitmeinungs-Einstellungen (Issue #102):
/// "ausgeschaltet" und "eingeschaltet, aber nicht einrichtbar" sind getrennt,
/// damit Letzteres dem Nutzer gemeldet werden kann statt still zu enden.
pub enum SecondOpinionSetup {
    /// Zweitmeinung in den Einstellungen aus (oder Einstellung nicht lesbar
    /// bzw. nie gesetzt): bleibt still wie bisher.
    Disabled,
    /// Eingeschaltet und aufgelöst.
    Ready(
        Box<dyn AiProvider>,
        Arc<ProviderBudgetGuard>,
        app_logic::ai_usage::AiUsageSource,
    ),
    /// Eingeschaltet, aber der Provider fehlt/ist gelöscht, seine ID ist
    /// ungültig oder sein Credential ist nicht auflösbar.
    Unavailable,
}

type ResolvedProvider = Option<(
    Box<dyn AiProvider>,
    Arc<ProviderBudgetGuard>,
    app_logic::ai_usage::AiUsageSource,
)>;

impl SecondOpinionSetup {
    /// Zerlegt in den bisherigen Provider/Budget-Paarwert und das Flag
    /// "eingeschaltet, aber nicht einrichtbar".
    pub fn into_parts(self) -> (ResolvedProvider, bool) {
        match self {
            SecondOpinionSetup::Disabled => (None, false),
            SecondOpinionSetup::Ready(provider, budget, source) => {
                (Some((provider, budget, source)), false)
            }
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
    // Side call (second opinion / injection check): never web tools (#162).
    let Ok((provider, budget)) = build_ai_provider_from_config(
        &state.rate_limit_registry,
        state.credential_store.as_ref(),
        &config,
        false,
    ) else {
        return SecondOpinionSetup::Unavailable;
    };
    let source = app_logic::ai_usage::AiUsageSource {
        provider_config_id: config.id.0.to_string(),
        provider_type: config.provider_type,
        configured_model: config.model.clone(),
    };
    SecondOpinionSetup::Ready(provider, budget, source)
}

/// Spec 0092, T16: die Einstellung darf sich durch einen fehlerhaften Wert in
/// `settings.json` nie still auf „aus" bringen lassen (Angriffsrichtung
/// „Einstellung" im Umsetzungsauftrag).
// Testcode-Ausnahme zum `deny` — s. `lib.rs`, Modulkopf.
#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code")]
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
    fn test_validate_second_opinion_settings() {
        let id = uuid::Uuid::new_v4().to_string();
        let code = |r: app_logic::error::CommandResult<Option<String>>| r.unwrap_err().code;
        assert_eq!(
            validate_second_opinion_settings(true, Some(&id), |_| true).unwrap(),
            Some(id.clone())
        );
        assert_eq!(
            validate_second_opinion_settings(false, None, |_| false).unwrap(),
            None
        );
        assert_eq!(
            code(validate_second_opinion_settings(true, None, |_| true)),
            Some("RISK_SETTINGS_PROVIDER_REQUIRED")
        );
        assert_eq!(
            code(validate_second_opinion_settings(true, Some(&id), |_| false)),
            Some("RISK_SETTINGS_PROVIDER_UNKNOWN")
        );
        assert_eq!(
            code(validate_second_opinion_settings(
                false,
                Some("junk"),
                |_| true
            )),
            Some("RISK_SETTINGS_PROVIDER_UNKNOWN")
        );
    }

    #[test]
    fn test_write_second_opinion_settings_writes_keys() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle();
        let id = uuid::Uuid::new_v4().to_string();
        write_second_opinion_settings(handle, true, Some(&id), |_| true).unwrap();
        let store = handle.store(SETTINGS_STORE_FILE).unwrap();
        assert_eq!(store.get(ENABLED_KEY), Some(serde_json::json!(true)));
        assert_eq!(store.get(PROVIDER_ID_KEY), Some(serde_json::json!(id)));
        write_second_opinion_settings(handle, false, None, |_| false).unwrap();
        assert_eq!(store.get(ENABLED_KEY), Some(serde_json::json!(false)));
        assert_eq!(store.get(PROVIDER_ID_KEY), Some(serde_json::json!(null)));

        // Rejected values leave the file unchanged.
        let before = (store.get(ENABLED_KEY), store.get(PROVIDER_ID_KEY));
        assert!(write_second_opinion_settings(handle, true, Some(&id), |_| false).is_err());
        assert!(write_second_opinion_settings(handle, true, None, |_| true).is_err());
        assert_eq!(before, (store.get(ENABLED_KEY), store.get(PROVIDER_ID_KEY)));
    }

    #[test]
    fn test_write_red_risk_always_confirm_round_trips_through_reader() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle();
        write_red_risk_always_confirm(handle, false).unwrap();
        assert!(!red_risk_always_confirm(handle));
        write_red_risk_always_confirm(handle, true).unwrap();
        assert!(red_risk_always_confirm(handle));
        let store = handle.store(SETTINGS_STORE_FILE).unwrap();
        assert_eq!(
            store.get(RED_RISK_ALWAYS_CONFIRM_KEY),
            Some(serde_json::json!(true))
        );
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
