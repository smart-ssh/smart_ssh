//! Diagnose-Export (Spec 0063): ein einzelnes, redigiertes Text-Paket
//! (Version/Hash, OS, Datenpfade, App-Zustand, letzte Log-Zeilen), das ein
//! Nutzer an den Support hängen oder in ein — oft **öffentliches** —
//! GitHub-Issue pasten kann, statt bei jedem "geht nicht" Version/OS/Log
//! einzeln erfragen zu müssen.
//!
//! Bewusst reine, IO-freie Zusammenstellung ([`build_diagnostics_bundle`])
//! getrennt von der eigentlichen Sammlung der Eingabedaten (liest DB/Log-
//! Ordner, Provider-/Server-Zahlen — passiert im `generate_diagnostics_
//! bundle`-Tauri-Command in `commands.rs`) — dasselbe Muster wie
//! `document_export.rs`: die reine Textzusammenstellung lässt sich ohne
//! Tauri-Laufzeit/echtes Dateisystem testen.
//!
//! **Sicherheitsrelevant (Spec 0063 §3, PFLICHT)**: die eingebetteten
//! Log-Zeilen laufen hier **zusätzlich** durch den [`OutputRedactor`] —
//! auch wenn Logs bereits beim Schreiben redigiert sein sollten (Spec
//! 0016), ist das hier ein zweites Sicherheitsnetz (Defense in Depth),
//! weil dieses Paket typischerweise öffentlich geteilt wird. Ebenso
//! sicherheitsrelevant: [`DiagnosticsInput`] hat schlicht **keine Felder**
//! für Keys/Passwörter/Host-Keys/Server-Adressen/Notiz-/Chat-Inhalte — was
//! nicht im Eingabe-Typ steckt, kann `build_diagnostics_bundle` strukturell
//! nicht ausgeben, unabhängig von einer Laufzeit-Filterung.

use ssh_manager_core::ai::OutputRedactor;

/// Spec 0063 §2: exakt die Felder, die ins Paket dürfen — bewusst als
/// eigener Typ statt z. B. `AppState`/`AiProviderConfig` direkt
/// durchzureichen, damit der Compiler mit erzwingt, dass niemand
/// versehentlich ein sensibles Feld (`credential_ref`, `base_url`,
/// `extra_headers`, Server-Adressen, Notiz-/Chat-Inhalte) mit einschleust.
pub struct DiagnosticsInput {
    pub version_display: String,
    pub edition: String,
    pub os: &'static str,
    pub arch: &'static str,
    /// Best-effort (Spec 0063 §2: "OS, Version") — `None`, falls die
    /// plattformspezifische Abfrage fehlschlägt (kein neues Cargo-
    /// Dependency nötig, s. `os_version`-Doc-Kommentar in `commands.rs`).
    pub os_version: Option<String>,
    pub db_path: String,
    pub log_dir: String,
    pub host_key_path: String,
    /// Spec 0063 §2: "Provider-**Typen** (nicht Keys!)" — je konfiguriertem
    /// Provider nur `ProviderType::as_db_str()`, z. B. `["anthropic",
    /// "openai"]`. Absichtlich `Vec<String>`, kein `Vec<AiProviderConfig>`
    /// (s. Modul-Doc).
    pub provider_types: Vec<String>,
    pub server_count: usize,
}

/// Spec 0063 §2: "die letzten N Log-Zeilen ... redigiert". `max_lines` wird
/// vom Aufrufer (`commands::generate_diagnostics_bundle`) über
/// `logging::read_last_log_lines` durchgereicht — hier nur noch die
/// Redaction + Formatierung.
pub const MAX_LOG_LINES: usize = 500;

/// Setzt das redigierte Text-Paket zusammen (Markdown — Spec 0063 §4:
/// "leicht in ein Issue zu pasten", eine einzelne Datei reicht, s.
/// `commands.rs`-Doc-Kommentar zur Formatwahl).
pub fn build_diagnostics_bundle(
    input: &DiagnosticsInput,
    log_lines: &[String],
    redactor: &dyn OutputRedactor,
) -> String {
    let mut out = String::new();

    out.push_str("# Smart SSH – Diagnosepaket\n\n");
    out.push_str(
        "Dieses Paket wurde lokal auf diesem Rechner zusammengestellt und **nicht** automatisch \
         versendet. Bitte kurz durchsehen, bevor du es an den Support gibst oder in ein \
         (ggf. öffentliches) Issue einfügst.\n\n",
    );

    out.push_str("## Version\n");
    out.push_str(&format!(
        "{} · {}\n\n",
        input.version_display, input.edition
    ));

    out.push_str("## System\n");
    out.push_str(&format!("OS: {} ({})\n", input.os, input.arch));
    out.push_str(&format!(
        "OS-Version: {}\n\n",
        input.os_version.as_deref().unwrap_or("unbekannt")
    ));

    out.push_str("## Datenpfade\n");
    out.push_str(&format!("Datenbank: {}\n", input.db_path));
    out.push_str(&format!("Logs: {}\n", input.log_dir));
    out.push_str(&format!("Host-Keys: {}\n\n", input.host_key_path));

    out.push_str("## App-Zustand\n");
    out.push_str(&format!(
        "Konfigurierte KI-Provider-Typen: {}\n",
        if input.provider_types.is_empty() {
            "keine".to_string()
        } else {
            input.provider_types.join(", ")
        }
    ));
    out.push_str(&format!("Server: {}\n\n", input.server_count));

    out.push_str(&format!(
        "## Letzte Log-Zeilen (redigiert, bis zu {MAX_LOG_LINES})\n```\n"
    ));
    if log_lines.is_empty() {
        out.push_str("(keine Log-Zeilen verfügbar)\n");
    } else {
        for line in log_lines {
            out.push_str(&redactor.redact_text(line));
            out.push('\n');
        }
    }
    out.push_str("```\n");

    out
}

#[cfg(test)]
mod tests {
    use ssh_manager_core::ai::DefaultOutputRedactor;

    use super::*;

    fn base_input() -> DiagnosticsInput {
        DiagnosticsInput {
            version_display: "0.5.0 (892001f)".to_string(),
            edition: "Community".to_string(),
            os: "macos",
            arch: "aarch64",
            os_version: Some("15.1".to_string()),
            db_path: "/Users/test/Library/Application Support/Smart SSH/smart-ssh.db".to_string(),
            log_dir: "/Users/test/Library/Logs/Smart SSH".to_string(),
            host_key_path: "/Users/test/Library/Application Support/Smart SSH/host_keys.json"
                .to_string(),
            provider_types: vec!["anthropic".to_string()],
            server_count: 3,
        }
    }

    #[test]
    fn test_bundle_contains_version_os_paths_and_state() {
        let bundle = build_diagnostics_bundle(&base_input(), &[], &DefaultOutputRedactor::new());

        assert!(bundle.contains("0.5.0 (892001f)"));
        assert!(bundle.contains("Community"));
        assert!(bundle.contains("macos"));
        assert!(bundle.contains("aarch64"));
        assert!(bundle.contains("15.1"));
        assert!(bundle.contains("smart-ssh.db"));
        assert!(bundle.contains("Smart SSH/host_keys.json") || bundle.contains("host_keys.json"));
        assert!(bundle.contains("anthropic"));
        assert!(bundle.contains("Server: 3"));
    }

    /// Der eigentliche Testfall aus der Spec: ein Fake-Secret in einer
    /// Log-Zeile darf NICHT im Klartext im Paket landen.
    #[test]
    fn test_fake_secret_in_log_line_is_redacted() {
        let log_lines = vec!["connecting with token=sk-ant-FAKE-SECRET-VALUE-123".to_string()];

        let bundle =
            build_diagnostics_bundle(&base_input(), &log_lines, &DefaultOutputRedactor::new());

        assert!(
            !bundle.contains("sk-ant-FAKE-SECRET-VALUE-123"),
            "Fake-Secret darf nicht im Klartext im Diagnosepaket landen: {bundle}"
        );
        assert!(
            bundle.contains("[REDACTED]"),
            "die Zeile muss als redigiert erkennbar bleiben: {bundle}"
        );
    }

    /// Gegenprobe zum Spec-Testfall: ein konfigurierter (Fake-)API-Key darf
    /// gar nicht erst ins Paket gelangen — hier strukturell erzwungen, weil
    /// `DiagnosticsInput` überhaupt kein Feld für Keys/Credentials hat.
    /// `build_diagnostics_bundle` bekommt den Key also nie zu Gesicht; der
    /// Test dokumentiert diese Invariante trotzdem explizit, statt sie nur
    /// implizit aus der Typdefinition abzuleiten.
    #[test]
    fn test_provider_api_key_never_appears_only_the_type_does() {
        let mut input = base_input();
        input.provider_types = vec!["anthropic".to_string()];
        let fake_api_key = "sk-ant-FAKE-CONFIGURED-KEY-456";

        let bundle = build_diagnostics_bundle(&input, &[], &DefaultOutputRedactor::new());

        assert!(bundle.contains("anthropic"));
        assert!(!bundle.contains(fake_api_key));
    }

    #[test]
    fn test_no_configured_providers_or_log_lines_degrades_gracefully() {
        let mut input = base_input();
        input.provider_types = Vec::new();

        let bundle = build_diagnostics_bundle(&input, &[], &DefaultOutputRedactor::new());

        assert!(bundle.contains("keine"));
        assert!(bundle.contains("(keine Log-Zeilen verfügbar)"));
    }
}
