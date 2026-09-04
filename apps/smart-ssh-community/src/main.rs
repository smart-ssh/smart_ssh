// Verhindert ein zusätzliches Konsolenfenster unter Windows im
// Release-Build (Standard-Tauri-Bootstrap-Zeile, wirkungslos auf
// macOS/Linux).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Spec 0038, Abschnitt 3: dünnes Binary, das sich von einem künftigen
// Official-Pendant nur im übergebenen `Wiring` unterscheidet.
//
// `tauri::generate_context!()` steht bewusst hier (nicht in `app_shell`):
// das Makro liest `tauri.conf.json` zur Kompilierzeit relativ zum
// `CARGO_MANIFEST_DIR` der Aufrufstelle, also dieser Crate — hier liegen
// `tauri.conf.json`, Icons, Capabilities und das Frontend, s.
// `app_shell::run`-Doc-Kommentar.
fn main() {
    app_shell::run(app_shell::Wiring::community(), tauri::generate_context!());
}
