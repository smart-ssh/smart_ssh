//! Spec 0014/0038/0049/0052: Plattform-/App-Info, Entitlements, Overlay-
//! Titelleiste — Teil der Spec-0083-Aufteilung von `commands.rs`.

use tauri::State;

use app_logic::dto::AppInfoDto;
use app_logic::error::CommandResult;
use app_logic::state::AppState;

/// Liefert das aktuelle Betriebssystem ("macos", "windows", "linux", "unknown")
/// zur plattformspezifischen Anpassung des UI-Paddings im Frontend (Spec 0014, Abschnitt 4).
#[tauri::command]
pub fn get_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "unknown"
    }
}

/// Spec 0052, Abschnitt 3.2/3.3: Version + Commit-Hash + Edition für den
/// Über-Dialog (Settings, Spec 0050) und die Titelzeile — ein Command für
/// beide statt zweier fast identischer, damit sie nicht auseinanderlaufen
/// können.
///
/// Generisch über `R: tauri::Runtime` (statt des impliziten `Wry`) —
/// einzig damit `tauri::test::MockRuntime` die tatsächliche
/// `tauri::State<Edition>`-Extraktion durchlaufen kann
/// (`app_info_tests::test_get_app_info_resolves_via_managed_edition_state`),
/// nicht nur die davon losgelöste `build_app_info`-Logik. `generate_handler!`
/// in `lib::run()` bindet `R` dort automatisch an `Wry` (den konkreten
/// Runtime-Typ des `tauri::Builder`), keine Änderung an der Registrierung
/// nötig.
#[tauri::command]
pub fn get_app_info<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    edition: tauri::State<'_, crate::wiring::Edition>,
) -> AppInfoDto {
    build_app_info(
        &app.package_info().version.to_string(),
        *edition,
        app_logic::version::BuildType::current(),
    )
}

/// Von der Tauri-IPC-Grenze losgelöst (dasselbe Muster wie
/// `classify_credential_test_result` oben), damit sich das eigentliche
/// Mapping ohne einen laufenden `AppHandle`/eine echte Tauri-App testen
/// lässt.
fn build_app_info(
    version: &str,
    edition: crate::wiring::Edition,
    build_type: app_logic::version::BuildType,
) -> AppInfoDto {
    AppInfoDto {
        version: version.to_string(),
        commit_hash: app_logic::version::BUILD_COMMIT_HASH.to_string(),
        version_display: app_logic::version::version_with_hash(version),
        edition: match edition {
            crate::wiring::Edition::Community => "Community".to_string(),
            crate::wiring::Edition::Official => "Official".to_string(),
        },
        build_type,
    }
}

#[cfg(test)]
mod app_info_tests {
    use tauri::Manager;

    use super::*;

    #[test]
    fn test_build_app_info_formats_version_display_per_spec() {
        let info = build_app_info(
            "0.4.1",
            crate::wiring::Edition::Community,
            app_logic::version::BuildType::Release,
        );

        assert_eq!(info.version, "0.4.1");
        assert_eq!(
            info.version_display,
            format!("0.4.1 ({})", app_logic::version::BUILD_COMMIT_HASH)
        );
        assert_eq!(info.commit_hash, app_logic::version::BUILD_COMMIT_HASH);
        assert_eq!(info.edition, "Community");
    }

    #[test]
    fn test_build_app_info_reports_build_type() {
        let dev = build_app_info(
            "0.4.1",
            crate::wiring::Edition::Community,
            app_logic::version::BuildType::Dev,
        );
        let release = build_app_info(
            "0.4.1",
            crate::wiring::Edition::Community,
            app_logic::version::BuildType::Release,
        );

        // Das Frontend liest `buildType` mit exakt diesen Werten
        // (`types.ts`) — eine Umbenennung von Feld oder Werten fiele dort
        // sonst still als "kein Build-Typ" durch.
        assert_eq!(serde_json::to_value(&dev).unwrap()["buildType"], "Dev");
        assert_eq!(
            serde_json::to_value(&release).unwrap()["buildType"],
            "Release"
        );
    }

    #[test]
    fn test_build_app_info_maps_official_edition() {
        let info = build_app_info(
            "0.4.1",
            crate::wiring::Edition::Official,
            app_logic::version::BuildType::Release,
        );

        assert_eq!(info.edition, "Official");
    }

    /// Spec-Reviewer-Fund (Spec 0052, Review dieses Schritts): die beiden
    /// Tests oben rufen `build_app_info` direkt auf und umgehen damit
    /// vollständig die `tauri::State<Edition>`-Extraktion, über die
    /// `get_app_info` tatsächlich aufgerufen wird — ein vergessenes
    /// `.manage(edition)` in `lib::run()` (oder eine falsch typisierte
    /// Registrierung) bliebe von ihnen unbemerkt und würde erst zur
    /// Laufzeit beim ersten Öffnen des Über-Dialogs als Panic auffallen
    /// ("state not managed for field"). Dieser Test baut stattdessen eine
    /// echte (gemockte) Tauri-App, managed `Edition` genauso wie
    /// `lib::run()` es tut, und ruft `get_app_info` mit einem daraus
    /// extrahierten `State<Edition>` auf — schließt damit genau diese
    /// Lücke, auch wenn er (anders als `lib::run()` selbst zu testen, was
    /// einen vollen App-Bootstrap bräuchte) nicht beweist, dass die
    /// *echte* Produktions-Wiring in `lib.rs` das `.manage()` tatsächlich
    /// aufruft — nur, dass die Befehlsfunktion korrekt funktioniert, sobald
    /// sie es tut.
    #[test]
    fn test_get_app_info_resolves_via_managed_edition_state() {
        let app = tauri::test::mock_builder()
            .manage(crate::wiring::Edition::Official)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app konnte nicht gebaut werden");
        let handle = app.handle().clone();

        let edition_state: tauri::State<'_, crate::wiring::Edition> = handle.state();
        let info = get_app_info(handle.clone(), edition_state);

        assert_eq!(info.edition, "Official");
        assert!(info.version_display.contains(&info.commit_hash));
    }
}

/// Liefert den aktuellen Entitlement-Stand (Spec 0038, Abschnitt 4). Das
/// Frontend liest ihn per `useEntitlements()`-Hook einmalig über diesen
/// Command und hält ihn danach über das `entitlements:changed`-Event (s.
/// `app_logic::events`) aktuell, statt wiederholt zu pollen.
#[tauri::command]
pub fn get_entitlements(
    state: State<'_, AppState>,
) -> ssh_manager_core::entitlements::Entitlements {
    state.entitlements.current()
}

/// Aktiviert die Overlay-Titelleiste und konfiguriert macOS-Ampel-Insets
/// (Spec 0014, Abschnitt 3 & 6). Liefert `"custom"`, wenn die
/// plattformspezifische Overlay-Titelleiste des Plugins aktiv ist
/// (macOS-Ampel bzw. die HTML-Controls des Plugins auf Windows/Linux),
/// sonst `"native"` (Fallback auf die native Titelleiste samt deren
/// eigenen Minimieren/Maximieren/Schließen-Controls) — das Frontend nutzt
/// den Rückgabewert, um sein eigenes Layout (reservierter Platz für die
/// Plugin-Controls) entsprechend umzuschalten (s. `AppHeader.tsx`).
///
/// Spec 0049, Fund 3/4: vorher wurde das `Result` von
/// `activate_decoration()` mit `let _ =` verworfen — schlug die Aktivierung
/// fehl (z. B. auf Windows, wo die Symptome "nur ein Schließen-Button" und
/// "Fenster-Ziehen greift nicht" beobachtet wurden), blieb das Fenster in
/// einem nicht dokumentierten Zwischenzustand hängen: weder vollständig
/// nativ noch vollständig durch das Plugin decoriert, und **spurlos** —
/// nichts wurde geloggt. Jetzt: bei einem Fehler wird explizit
/// `restore_decoration()` aufgerufen (bringt die native Titelleiste
/// zuverlässig zurück, exakt das vom Plugin selbst dokumentierte
/// Recovery-Muster) und der Fehler geloggt, statt beides stillschweigend
/// zu ignorieren.
#[tauri::command]
pub async fn create_overlay_titlebar(window: tauri::WebviewWindow) -> CommandResult<&'static str> {
    use tauri_plugin_decoration::WebviewWindowExt;

    if let Err(error) = window.activate_decoration().await {
        return Ok(restore_native_decoration(&window, error).await);
    }

    #[cfg(target_os = "macos")]
    {
        // Spec 0014 Abschnitt 3 & 6: Startwert für Ampel-Positionierung.
        //
        // Spec-Reviewer-Fund (Spec 0049, Review dieses Schritts): an dieser
        // Stelle NICHT `restore_native_decoration` aufrufen. Anders als
        // beim `activate_decoration()`-Fehler oben ist die Overlay-
        // Titelleiste hier bereits erfolgreich aktiv — ein fehlgeschlagener
        // Inset-Aufruf ist rein kosmetisch (Ampel-Position leicht
        // daneben), kein Grund, die gesamte funktionierende Custom-
        // Titelleiste zurückzubauen. Das hätte außerdem exakt die einzige
        // Plattform getroffen, die diese Spec ausdrücklich unverändert
        // lassen soll (macOS) — der Fund 3/4-Fix ist für Windows/Linux
        // gedacht, nicht dafür, ein bereits funktionierendes macOS-Setup
        // bei einem harmlosen Kosmetik-Fehler zu degradieren.
        if let Err(error) = window.set_traffic_lights_inset(12.0, 16.0).await {
            tracing::warn!(
                error = %error,
                "macOS traffic-light inset failed, keeping the custom titlebar active"
            );
        }
    }

    Ok("custom")
}

/// Fallback-Pfad aus dem Plugin-Dokumentationsmuster ("Activate and
/// recover"): Aktivierung ist fehlgeschlagen, also wird explizit die
/// native Titelleiste wiederhergestellt statt das Fenster in einem
/// halb-decorierten Zustand zu belassen. `activation_error` wird geloggt
/// (nicht verschluckt) — der Startpunkt, um ein künftiges Windows-/
/// Linux-Problem tatsächlich diagnostizieren zu können, statt wie bisher
/// zu raten.
async fn restore_native_decoration(
    window: &tauri::WebviewWindow,
    activation_error: impl std::fmt::Display,
) -> &'static str {
    use tauri_plugin_decoration::WebviewWindowExt;
    match window.restore_decoration().await {
        Ok(()) => {
            tracing::warn!(
                error = %activation_error,
                "custom titlebar decoration activation failed, restored native titlebar"
            );
        }
        Err(restore_error) => {
            tracing::error!(
                error = %activation_error,
                restore_error = %restore_error,
                "custom titlebar decoration activation failed AND native restoration failed"
            );
        }
    }
    "native"
}
