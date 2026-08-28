//! Platzhalter für die Tauri-App-Schicht von ssh-manager.
//!
//! Enthält aktuell noch kein echtes Tauri-Setup (Fenster, `#[tauri::command]`s,
//! etc.) – nur das Grundgerüst mit Abhängigkeit auf `ssh_manager_core`. Diese
//! Schicht bleibt dünn: UI-Bindungen greifen ausschließlich auf Funktionalität
//! aus `ssh_manager_core` zu, keine Geschäftslogik hier.

#[cfg(test)]
mod tests {
    // Stellt sicher, dass die Abhängigkeit auf ssh_manager_core auflösbar ist.
    use ssh_manager_core as _;
}
