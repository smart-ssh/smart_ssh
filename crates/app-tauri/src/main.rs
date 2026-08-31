// Verhindert ein zusätzliches Konsolenfenster unter Windows im
// Release-Build (Standard-Tauri-Bootstrap-Zeile, wirkungslos auf
// macOS/Linux).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ssh_manager_app_tauri::run();
}
