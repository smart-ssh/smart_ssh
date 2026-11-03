//! DTOs: sftp.

use serde::Serialize;

/// Spec 0054, Teil 3: Vorschau vor dem Löschen eines Ordners — "X Dateien,
/// Y Ordner werden gelöscht" statt einer inhaltslosen Ja/Nein-Frage, analog
/// zum zweistufigen `delete_server`. `dir_count` zählt den Ordner selbst
/// mit (s. `app_shell::commands::walk_dirs_and_count_files`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePreviewDto {
    pub file_count: u64,
    pub dir_count: u64,
}

/// Spec 0067, Teil B: Ergebnis eines Downloads für die Erfolgsmeldung —
/// wohin (für „Im Finder zeigen") und wie viele Dateien (Ordner-Download).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadResultDto {
    /// Lokaler Pfad der heruntergeladenen Datei bzw. des Ordners.
    pub local_path: String,
    pub is_dir: bool,
    pub file_count: u64,
}

/// Spec 0054, Teil 3: Ergebnis von `commands::read_local_text_preview` — die
/// lokale Seite der Upload-Überschreib-Diff-Vorschau. `text: None` bei einer
/// zu großen oder nicht als UTF-8 dekodierbaren Datei; `size` ist in jedem
/// Fall gesetzt (Grundlage für den Größenvergleich-Hinweis in diesem Fall).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalFilePreviewDto {
    pub text: Option<String>,
    pub size: u64,
}

/// Spec 0054, Teil 4: Ergebnis von `commands::sftp_open_for_editing` — der
/// lokale Pfad der Bearbeitungskopie plus die Baseline für die spätere
/// "hat sich die Remote-Datei seit dem Download geändert?"-Konflikt-Prüfung
/// vor dem Hochladen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditSessionDto {
    pub local_path: String,
    pub remote_modified: Option<String>,
}
