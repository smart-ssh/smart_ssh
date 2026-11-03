//! Normalisation of the two optional per-server path inputs (SFTP server
//! path override, Spec 0067; start directory, Spec 0102). Split out of
//! `dto` (Issue #868) because it is validation logic, not data mapping.

use ssh_manager_core::profiles::trim_credential_value;

/// Spec 0067, A2: leerer Override = automatisch (`None`); sonst muss es ein
/// sicherer absoluter Pfad sein — er landet in einem `sudo`-Kommando und in
/// der angezeigten sudoers-Zeile.
///
/// Spec 0073, A6–A8 (Q-BL-0149-01): Getrimmt wird über den geteilten
/// [`trim_credential_value`], obwohl der Pfad kein Zugangsdaten-Wert ist —
/// §1 der Spec listet diese Stelle, A3 verlangt für sie den Helfer. Der
/// Unterschied zu `str::trim` ist **nicht** kosmetisch:
/// `"/usr/lib/sftp-server\u{200B}"` wurde vorher abgelehnt (das ZWSP
/// übersteht `str::trim` und fällt dann durch
/// `is_plausible_sftp_server_path`), jetzt wird derselbe Pfad bereinigt und
/// angenommen. Geändert hat sich damit nur, ob ein eingefügter Pfad mit
/// Randzeichen als Fehler oder als bereinigter Pfad endet.
///
/// **Die Sicherheitsprüfung selbst ist wörtlich unverändert und läuft
/// weiterhin nach dem Trimmen, auf genau dem Wert, der zurückgegeben wird.**
/// Der Helfer entfernt nur Randzeichen (Spec 0073, A2/I1); ein unsichtbares
/// Zeichen *innerhalb* des Pfades überlebt ihn und wird von
/// `is_plausible_sftp_server_path` nach wie vor abgelehnt. In ein
/// `sudo`-Kommando kann also in keinem Fall ein Zeichen außerhalb des
/// erlaubten Zeichensatzes gelangen.
pub fn normalize_sftp_server_path(input: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = input else {
        return Ok(None);
    };
    let trimmed = trim_credential_value(&raw);
    if trimmed.is_empty() {
        return Ok(None);
    }
    if !ssh_manager_core::ssh::elevated::is_plausible_sftp_server_path(&trimmed) {
        let shown = crate::startup_error_messages::sanitize_text_for_display(&trimmed);
        return Err(format!(
            "Ungültiger sftp-server-Pfad „{shown}“ — erlaubt ist ein absoluter Pfad aus \
             Buchstaben, Ziffern und / . _ - +, der auf „sftp-server“ endet"
        ));
    }
    Ok(Some(trimmed))
}

/// Spec 0102: leer/fehlend = nicht gesetzt (`None`); sonst ein absoluter Pfad
/// oder `~/…`, Randleerraum entfernt (s.
/// [`ssh_manager_core::profiles::normalize_start_directory`]).
pub fn normalize_start_directory_input(input: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = input else {
        return Ok(None);
    };
    ssh_manager_core::profiles::normalize_start_directory(&raw).map_err(|err| {
        let shown = crate::startup_error_messages::sanitize_text_for_display(raw.trim());
        format!("Ungültiges Startverzeichnis „{shown}“ — {err}")
    })
}

#[cfg(test)]
mod tests_sftp_server_path;

#[cfg(test)]
mod tests_start_directory;
