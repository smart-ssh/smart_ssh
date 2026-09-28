//! Dateinamens-Ableitung für KI-generierte Dokumente (Spec 0012, Abschnitt
//! 3/4). Bewusst reine, IO-freie Funktion — der native Speichern-Dialog und
//! das eigentliche Schreiben auf die Festplatte passieren erst im
//! `export_document`-Command (`commands.rs`), damit diese Ableitung ohne
//! Tauri-Laufzeit getestet werden kann.
//!
//! Spec 0037, Abschnitt 4: der ursprüngliche Word-Export (`docx-rs`-
//! Konvertierung, "Als Word speichern"-Button) wurde komplett aus diesem
//! Modul entfernt — kein Gating, weil es dafür (noch kein privates Repo,
//! kein Lizenzschlüssel-Mechanismus) nichts freizuschalten gäbe; der
//! Word-Export wird später im privaten Repo neu aufgebaut. Übrig bleibt
//! ausschließlich der (unveränderte) Markdown-Export.

use crate::dto::DocumentFormat;

/// Leitet aus dem KI-gelieferten Dokumenttitel einen vorbelegten Dateinamen
/// für den Speichern-Dialog ab (Spec 0012, Abschnitt 3: "vorbelegt mit
/// einem aus `title` abgeleiteten Dateinamen"). Entfernt Zeichen, die in
/// Dateinamen auf keinem der unterstützten Betriebssysteme zuverlässig
/// funktionieren; ein leerer/nur aus solchen Zeichen bestehender Titel
/// fällt auf "Dokument" zurück statt einen leeren Dateinamen zu erzeugen.
///
/// `format` bleibt Teil der Signatur (statt sie zu entfernen), obwohl
/// `DocumentFormat` seit Spec 0037 nur noch die eine Variante `Markdown`
/// hat — kleinerer, klarerer Diff für den Rückbau, und `export_document`s
/// öffentliche Signatur/Frontend-Aufruf bleiben unverändert.
pub fn default_export_file_name(title: &str, format: DocumentFormat) -> String {
    let extension = match format {
        DocumentFormat::Markdown => "md",
    };
    let sanitized: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let base = if sanitized.is_empty() {
        "Dokument".to_string()
    } else {
        sanitized
    };
    format!("{base}.{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_export_file_name_uses_markdown_extension() {
        let name = default_export_file_name("Kurze Analyse", DocumentFormat::Markdown);

        assert_eq!(name, "Kurze Analyse.md");
    }

    #[test]
    fn test_default_export_file_name_sanitizes_path_unsafe_characters() {
        let name =
            default_export_file_name("Bericht: Server/Log \"prod\"", DocumentFormat::Markdown);

        assert_eq!(name, "Bericht Server Log prod.md");
    }

    #[test]
    fn test_default_export_file_name_falls_back_for_empty_title() {
        let name = default_export_file_name("   ", DocumentFormat::Markdown);

        assert_eq!(name, "Dokument.md");
    }
}
