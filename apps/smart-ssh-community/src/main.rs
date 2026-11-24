// Verhindert ein zusätzliches Konsolenfenster unter Windows im
// Release-Build (Standard-Tauri-Bootstrap-Zeile, wirkungslos auf
// macOS/Linux).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Spec 0038, Abschnitt 3: dünnes Binary, das sich von einem künftigen
// Official-Pendant nur im übergebenen `Wiring` unterscheidet. Die eigentliche
// Einstiegsfunktion liegt seit Spec 0114 in `lib.rs` (Mobile-Einstieg).
fn main() {
    smart_ssh_community_lib::run();
}
