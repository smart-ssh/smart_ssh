// Spec 0114, ADR 0148: Die App liegt als Bibliothek vor, damit Tauri-Mobile
// (Android) sie laden kann; das Desktop-Binary (`main.rs`) ruft dieselbe
// Funktion auf.
//
// `tauri::generate_context!()` steht bewusst hier (nicht in `app_shell`):
// das Makro liest `tauri.conf.json` zur Kompilierzeit relativ zum
// `CARGO_MANIFEST_DIR` der Aufrufstelle, also dieser Crate — hier liegen
// `tauri.conf.json`, Icons, Capabilities und das Frontend, s.
// `app_shell::run`-Doc-Kommentar.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app_shell::run(app_shell::Wiring::community(), tauri::generate_context!());
}
