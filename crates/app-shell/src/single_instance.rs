//! Issue #19: Ein zweiter Start desselben Builds holt die laufende Instanz
//! nach vorn und beendet sich (`tauri-plugin-single-instance`).
//!
//! **Ergänzt die Sperre auf das Datenverzeichnis, ersetzt sie nicht.** Das
//! Plugin erkennt Instanzen an der App-Kennung (`identifier` in
//! `tauri.conf.json`), nicht am Datenverzeichnis. Die Datenbank schützt
//! allein [`persistence_sqlite::DataDirLock`], genommen vor jedem Zugriff
//! (`crate::run`). Das Plugin sorgt nur dafür, dass ein Doppelstart nicht
//! in einem Fehlerdialog endet, sondern im schon offenen Fenster.
//!
//! **Nur mit dem Standard-Datenverzeichnis** (Issue #44, ADR 0121): Mit
//! gesetztem `SMART_SSH_DATA_DIR` bleibt das Plugin in diesem Prozess aus,
//! sowohl am Builder als auch in [`hand_over_to_running_instance`]. So
//! laufen Instanzen mit verschiedenen Datenverzeichnissen nebeneinander,
//! und ein zweiter Start mit demselben Override-Verzeichnis endet an der
//! Sperre mit dem Startfehler „läuft bereits".
//!
//! **Nur in Release-Builds** (ADR 0106): Debug- und Release-Build tragen
//! dieselbe Kennung, haben aber getrennte Datenverzeichnisse (ADR 0032),
//! damit beide nebeneinander laufen können. Mit dem Plugin im Debug-Build
//! würde ein `cargo tauri dev` neben einer installierten App sofort wieder
//! enden und die installierte nach vorn holen. Die Sperre gilt dagegen in
//! jedem Build.

use tauri::Manager;

/// Ob der Single-Instance-Schutz in diesem Prozess aktiv ist: nur im
/// Release-Build und nur mit dem Standard-Datenverzeichnis (Issue #44,
/// Entscheidung in [`app_logic::single_instance::focusing_applies`]).
pub(crate) fn enabled() -> bool {
    let raw_override = std::env::var(persistence_sqlite::DATA_DIR_OVERRIDE_ENV).ok();
    app_logic::single_instance::focusing_applies(raw_override.as_deref(), cfg!(debug_assertions))
}

/// Das Plugin mit dem Rückruf, der in der **laufenden** Instanz ankommt,
/// wenn eine zweite gestartet wird: Hauptfenster zeigen und nach vorn.
pub(crate) fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_single_instance::init(|app, _args, _cwd| {
        tracing::info!("a second instance was started; focusing this one (issue #19)");
        focus_main_window(app);
    })
}

/// Registriert das Plugin, wenn es in diesem Build aktiv ist. Muss das
/// **erste** Plugin am Builder sein: Seine Prüfung läuft in seinem
/// `setup`, und Plugins werden in Registrierungsreihenfolge eingerichtet —
/// eine zweite Instanz soll enden, bevor ein anderes Plugin etwas tut.
pub(crate) fn register_on(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    if enabled() {
        builder.plugin(plugin())
    } else {
        tracing::info!(
            "single-instance focusing is off (debug build or SMART_SSH_DATA_DIR set); the data \
             directory lock still applies (issues #19, #44, ADR 0106, ADR 0121)"
        );
        builder
    }
}

/// Der Weg, wenn die Sperre auf das Datenverzeichnis schon ein anderer
/// Prozess hält (`crate::run`): **Ist das eine laufende Instanz desselben
/// Builds, wird sie nach vorn geholt, und dieser Prozess endet hier mit 0**
/// — im `setup` des Plugins, bevor ein Fenster entsteht.
///
/// Kehrt die Funktion zurück, war es keine Instanz desselben Builds (etwa
/// ein anderer Build mit demselben Datenverzeichnis), oder das Plugin ist
/// in diesem Prozess aus (Debug-Build oder `SMART_SSH_DATA_DIR`, s.
/// [`enabled`]). Dann zeigt der Aufrufer den Startfehler.
///
/// Gebaut wird eine App **nur mit diesem Plugin**: `Builder::build`
/// richtet die Plugins ein, die Fenster aus der Konfiguration entstehen
/// erst beim Starten der Ereignisschleife, die hier nie läuft. Die Datenbank
/// wird nicht berührt.
pub(crate) fn hand_over_to_running_instance(context: tauri::Context<tauri::Wry>) {
    if !enabled() {
        return;
    }
    match tauri::Builder::default().plugin(plugin()).build(context) {
        Ok(app) => {
            // Keine Instanz desselben Builds hat geantwortet, also hat sich
            // dieser Prozess gerade selbst als „erste Instanz" eingetragen.
            // Das wird sofort zurückgenommen: Sonst würde eine danach
            // startende echte Instanz diesen Prozess (der nur noch seinen
            // Fehlerdialog zeigt) nach vorn holen und sich selbst beenden.
            tauri_plugin_single_instance::destroy(&app);
        }
        Err(err) => {
            tracing::warn!(error = %err, "could not check for a running instance of this build");
        }
    }
}

fn focus_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        tracing::warn!("no main window to focus for the second instance");
        return;
    };
    if let Err(err) = window.unminimize() {
        tracing::warn!(error = %err, "could not unminimize the main window");
    }
    if let Err(err) = window.show() {
        tracing::warn!(error = %err, "could not show the main window");
    }
    if let Err(err) = window.set_focus() {
        tracing::warn!(error = %err, "could not focus the main window");
    }
}
