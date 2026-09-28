//! Spec 0012/0015/0016/0063/0013: Dokument-Export, Prompt-Historie,
//! strukturiertes Logging/Diagnose, Zugangsdatei-Lesen — Teil der
//! Spec-0083-Aufteilung von `commands.rs`.

use tauri::AppHandle;
use tauri::State;

use persistence_sqlite::AiProviderConfig;
use ssh_manager_core::ai::{
    fence_untrusted, ChatMessage, DefaultOutputRedactor, MessageContent, Role, UntrustedKind,
};
use ssh_manager_core::shared::ServerId;

use app_logic::dto::DocumentFormat;
use app_logic::dto::KeychainStatusDto;
use app_logic::error::CommandResult;
use app_logic::state::AppState;

// --- Spec 0012: KI-generierte Dokumente -------------------------------

/// Spec 0012, Abschnitt 3: öffnet einen nativen Speichern-unter-Dialog
/// (vorbelegt mit einem aus `title` abgeleiteten Dateinamen, s.
/// [`app_logic::document_export::default_export_file_name`]) und schreibt
/// **erst nach dessen Bestätigung** — bricht der Nutzer den Dialog ab,
/// liefert der Callback `None`, der Command kehrt dann ohne jeden
/// Seiteneffekt zurück (kein Fehler: Abbrechen ist kein Fehlerfall).
///
/// Der Dialog-Callback selbst ist nicht `async` (Tauri-Dialog-Plugin-API,
/// Abschnitt 3 der Spec nennt nur "nativer Speichern-unter-Dialog", nicht
/// welche der beiden Varianten) — er wird deshalb über einen `oneshot`-Kanal
/// an diesen `async fn`-Command zurücküberführt, statt die blockierende
/// `blocking_save_file()`-Variante zu nutzen, die den Async-Runtime-Thread
/// blockieren würde.
#[tauri::command]
pub async fn export_document(
    app: AppHandle,
    content_markdown: String,
    title: String,
    format: DocumentFormat,
) -> CommandResult<()> {
    use tauri_plugin_dialog::DialogExt;

    let file_name = app_logic::document_export::default_export_file_name(&title, format);
    let (filter_name, extension): (&str, &str) = match format {
        DocumentFormat::Markdown => ("Markdown", "md"),
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(&file_name)
        .add_filter(filter_name, &[extension])
        .save_file(move |path| {
            let _ = tx.send(path);
        });

    let Some(path) = rx.await.ok().flatten() else {
        return Ok(());
    };
    let path = path.into_path()?;

    // `std::fs::write` statt `tokio::fs`: Letzteres bräuchte das
    // ungenutzte `fs`-Feature nur für diesen einen, durch eine explizite
    // Nutzeraktion ausgelösten Einzelschreibvorgang — für eine
    // Analyse-Dokumentgröße unkritisch blockierend.
    match format {
        DocumentFormat::Markdown => std::fs::write(path, content_markdown)?,
    }

    Ok(())
}

// --- Spec 0015: Chat-Prompt-Historie ---------------------------------------

/// Spec 0015, Abschnitt 4: liefert die gespeicherten Prompts eines Servers
/// chronologisch aufsteigend (älteste zuerst) — das Frontend kehrt für die
/// Pfeiltasten-Navigation selbst um bzw. greift von hinten zu.
#[tauri::command]
pub async fn list_prompt_history(
    state: State<'_, AppState>,
    server_id: ServerId,
) -> CommandResult<Vec<String>> {
    // Spec 0040, Abschnitt 7: kein Verschlüsselungsschlüssel verfügbar ->
    // keine Prompt-Historie für diesen App-Lauf, leere Liste statt Fehler.
    let Some(store) = &state.prompt_history_store else {
        return Ok(Vec::new());
    };
    Ok(store.list(&server_id).await?)
}

// --- Spec 0016: Strukturiertes Logging & Diagnose --------------------------

/// Spec 0071, A15: Der Startdialog ist weggeklickt, sobald der Nutzer ihn
/// bestätigt hat — der Zustand muss trotzdem nachschlagbar bleiben. Liefert
/// den **bereits beim Start ermittelten** Zustand aus dem `AppState` (A16);
/// dieser Befehl probiert den Schlüsselbund nicht erneut an.
///
/// Gibt nur die Aufzählung zurück, nie einen Fehlertext (I1) — die Texte
/// dazu liegen im Frontend-Übersetzungskatalog.
#[tauri::command]
pub async fn get_keychain_status(state: State<'_, AppState>) -> CommandResult<KeychainStatusDto> {
    Ok(KeychainStatusDto::from(state.keychain))
}

/// Spec 0016, Abschnitt 5: öffnet den Log-Ordner im System-Dateimanager
/// (Finder/Explorer) — ein Klick statt manuell zum plattformspezifischen
/// Pfad navigieren zu müssen.
#[tauri::command]
pub async fn open_log_directory(app: AppHandle) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;

    let dir = app_logic::logging::default_log_dir();
    std::fs::create_dir_all(&dir)?;
    app.opener()
        .open_path(dir.to_string_lossy().into_owned(), None::<&str>)?;
    Ok(())
}

/// Spec 0063: Best-effort-Betriebssystemversion für den Diagnose-Export
/// (§2: "OS/Plattform (Betriebssystem, Version, Architektur)") — bewusst
/// KEIN neues Cargo-Dependency (z. B. `os_info`) für dieses eine, nicht
/// sicherheitskritische Feld, sondern ein auf jeder unterstützten
/// Plattform bereits vorhandenes Systemkommando ohne Nutzer-Eingabe (feste
/// Argumente, kein Injection-Risiko). `None` bei jedem Fehler (Kommando
/// fehlt, liefert einen Fehlerstatus, o. ä.) — die OS-Version ist ein
/// "nice to have" neben OS-Familie/Architektur (`std::env::consts`, immer
/// verfügbar), kein Grund, den gesamten Export abzubrechen. Synchroner
/// Prozessaufruf direkt im `async fn`-Command statt `spawn_blocking`:
/// derselbe Grund wie bei `export_document`s `std::fs::write` — ein
/// einzelner, durch eine explizite Nutzeraktion ausgelöster Vorgang, hier
/// zusätzlich mit realistisch niedriger Laufzeit (< 50ms).
fn os_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    let output = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output();
    #[cfg(target_os = "windows")]
    let output = std::process::Command::new("cmd")
        .args(["/C", "ver"])
        .output();
    #[cfg(target_os = "linux")]
    let output = std::process::Command::new("uname").arg("-r").output();
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let output: std::io::Result<std::process::Output> =
        Err(std::io::Error::other("unbekannte Plattform"));

    let output = output.ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Spec 0063: stellt das redigierte Diagnosepaket zusammen (Version/Hash,
/// OS, Datenpfade, App-Zustand, letzte Log-Zeilen) und liefert es als
/// fertigen Text an das Frontend zurück — **kein** automatisches
/// Speichern/Versenden (Spec 0063, Invarianten). Das Frontend zeigt den
/// Text zur Durchsicht an ("Vorschau", Spec 0063 §3) und bietet erst
/// danach über [`save_diagnostics_bundle`] einen expliziten
/// Speichern-unter-Dialog an.
///
/// Sammelt hier (statt in `diagnostics::build_diagnostics_bundle`, das
/// bewusst rein/IO-frei bleibt, s. dortiger Moduldoc-Kommentar) alle
/// Eingabedaten: `AiProviderConfig`/`Server` werden auf genau die per Spec
/// erlaubten Felder reduziert (Provider-**Typ**, nicht der volle
/// `AiProviderConfig` mit `credential_ref`/`base_url`/`extra_headers`;
/// Server-**Anzahl**, nicht die `Server`-Liste selbst) — der volle,
/// potenziell sensible Datensatz verlässt diese Funktion nie.
#[tauri::command]
pub async fn generate_diagnostics_bundle<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    edition: tauri::State<'_, crate::wiring::Edition>,
) -> CommandResult<String> {
    let db_path = persistence_sqlite::default_db_path();
    let log_dir = app_logic::logging::default_log_dir();
    let host_key_path = db_path
        .parent()
        .expect("db_path hat immer ein Elternverzeichnis (s. default_db_path)")
        .join("host_keys.json");

    // Spec-reviewer-Fund (Follow-up-Review): `None` nur bei einem echten
    // Lese-/DB-Fehler, NICHT bei "null konfiguriert" — ein `unwrap_or_default`
    // hätte beides ununterscheidbar auf "keine" abgebildet (s.
    // `diagnostics::DiagnosticsInput::provider_types`-Doc-Kommentar).
    let provider_types = state.ai_provider_store.list().await.ok().map(|configs| {
        configs
            .into_iter()
            .map(|config| {
                // Erschöpfendes Destructuring OHNE `..` (spec-reviewer-
                // Vorschlag, Follow-up-Review): erzwingt einen
                // Compile-Fehler, sobald `AiProviderConfig` ein neues Feld
                // bekommt — stärkere Garantie als ein Test, dass hier
                // niemand versehentlich `credential_ref`/`base_url`/
                // `extra_headers`/`model`/`display_name` mit einschleust,
                // ohne das bewusst zu entscheiden.
                let AiProviderConfig {
                    provider_type,
                    id: _,
                    display_name: _,
                    base_url: _,
                    model: _,
                    supports_native_tool_calling: _,
                    credential_ref: _,
                    is_active: _,
                    extra_headers: _,
                    attestation_url: _,
                    // Spec 0065, Teil 4: kein Diagnose-relevanter Wert
                    // (weder Zugangsdaten noch Server-Adresse) — bewusst
                    // trotzdem im exhaustiven Destructuring aufgeführt,
                    // s. Kommentar oben.
                    max_tokens_override: _,
                    created_at: _,
                    updated_at: _,
                } = config;
                provider_type.as_db_str().to_string()
            })
            .collect()
    });
    let server_count = state
        .profile_store
        .list_servers()
        .await
        .ok()
        .map(|servers| servers.len());

    let input = app_logic::diagnostics::DiagnosticsInput {
        version_display: app_logic::version::version_with_hash(
            &app.package_info().version.to_string(),
        ),
        build_type: app_logic::version::BuildType::current().as_str(),
        edition: match *edition {
            crate::wiring::Edition::Community => "Community".to_string(),
            crate::wiring::Edition::Official => "Official".to_string(),
        },
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        os_version: os_version(),
        db_path: db_path.display().to_string(),
        log_dir: log_dir.display().to_string(),
        host_key_path: host_key_path.display().to_string(),
        provider_types,
        server_count,
    };
    let log_lines =
        app_logic::logging::read_last_log_lines(&log_dir, app_logic::diagnostics::MAX_LOG_LINES);

    Ok(app_logic::diagnostics::build_diagnostics_bundle(
        &input,
        &log_lines,
        // Spec 0063 §3: zusätzlich zur Redaction, die die Log-Zeilen beim
        // Schreiben (Spec 0016) bereits durchlaufen haben — Defense in
        // Depth, weil dieses Paket öffentlich geteilt wird. Eine frische
        // Instanz ohne nutzerdefinierte Zusatzmuster reicht hier (anders
        // als `session.redactor`, das ist an keine laufende Sitzung
        // gebunden): dieselben eingebauten Muster (Spec 0006 + Härtung)
        // wie überall sonst in der App.
        &DefaultOutputRedactor::new(),
    ))
}

/// Spec 0063 §3: expliziter Speichern-unter-Dialog für das bereits über
/// [`generate_diagnostics_bundle`] erzeugte (und vom Nutzer in der
/// Vorschau gesehene) Paket — kein eigenständiges erneutes Sammeln, `content`
/// kommt unverändert vom Frontend zurück. Dasselbe Muster wie
/// `export_document`.
#[tauri::command]
pub async fn save_diagnostics_bundle(app: AppHandle, content: String) -> CommandResult<()> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name("smart-ssh-diagnose.md")
        .add_filter("Markdown", &["md"])
        .save_file(move |path| {
            let _ = tx.send(path);
        });

    let Some(path) = rx.await.ok().flatten() else {
        return Ok(());
    };
    let path = path.into_path()?;
    std::fs::write(path, content)?;

    Ok(())
}

/// Spec 0064 (Prompt-Caching): baut die gefencte Verlaufs-Nachricht für den
/// bereits sanitisierten `uname`-Banner (s. Aufrufstelle in
/// `connect_session`) — als eigene, pure Funktion extrahiert, damit sich
/// die Fencing-Zuordnung (`UntrustedKind::RemoteOsInfo`, `Role::
/// ActionResult`) ohne einen vollen `connect()`-Durchlauf direkt testen
/// lässt (spec-reviewer-Fund, Follow-up-Review: die vorherigen Tests
/// prüften nur den System-Prompt, nie diesen konkreten Nachrichtenbau).
pub(crate) fn build_os_banner_message(sanitized_os: &str) -> ChatMessage {
    ChatMessage {
        role: Role::ActionResult,
        content: MessageContent::Text(fence_untrusted(
            UntrustedKind::RemoteOsInfo,
            "uname -a",
            sanitized_os,
        )),
    }
}

/// Liest den Textinhalt einer vom Nutzer im nativen Dateidialog ausgewählten
/// Schlüssel-/Zertifikatsdatei (Spec 0013, SEC-06). Ersetzt globale Dateilese-
/// Berechtigungen im Frontend.
///
/// Unabhängiger Review-Pass (Spec 0013): nahm bislang einen beliebigen,
/// vom Frontend übergebenen `path: String` entgegen und las ihn ohne jede
/// Prüfung — funktional gleichbedeutend mit der pauschalen
/// Dateilese-Berechtigung, die SEC-06 gerade abschaffen sollte, da JEDER
/// Code im Webview (nicht nur der eigentliche "Datei wählen"-Button)
/// `invoke("read_credential_file", { path: "~/.ssh/id_rsa" })` aufrufen
/// konnte. Der Dialog läuft jetzt — wie bei `export_document`/
/// `sftp_download` bereits etabliert — im Backend selbst
/// (`app.dialog().file().pick_file(...)` + `oneshot`-Rückkanal, da der
/// Callback selbst nicht `async` ist): das Frontend übergibt nur noch
/// einen Anzeige-`title`, nie einen Pfad, und kann dadurch keinen
/// beliebigen Pfad mehr erzwingen — nur eine tatsächliche
/// Nutzerinteraktion mit dem nativen Dialog liefert einen Pfad.
#[tauri::command]
pub async fn read_credential_file(app: AppHandle, title: String) -> CommandResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(&title)
        .pick_file(move |path| {
            let _ = tx.send(path);
        });

    let Some(path) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = path.into_path()?;
    let content = std::fs::read_to_string(path)?;
    Ok(Some(content))
}

#[cfg(test)]
mod banner_message_tests {
    use super::*;

    /// Spec 0064: der eigentliche Regressionstest für den Banner-Umzug —
    /// prüft, dass `build_os_banner_message` tatsächlich mit
    /// `UntrustedKind::RemoteOsInfo` fenct (Tag `<remote_system>`) und als
    /// `Role::ActionResult` läuft (wie jeder andere Backend-eingefügte
    /// Systemhinweis, nicht `User`/`Assistant`).
    #[test]
    fn test_build_os_banner_message_fences_the_sanitized_os_string() {
        let message = build_os_banner_message("Linux srv1 5.10.0");

        assert_eq!(message.role, Role::ActionResult);
        match &message.content {
            MessageContent::Text(text) => {
                assert!(text.starts_with("<remote_system>"));
                assert!(text.trim_end().ends_with("</remote_system>"));
                assert!(text.contains("Linux srv1 5.10.0"));
                assert!(text.contains("<source>uname -a</source>"));
            }
            other => panic!("erwartet MessageContent::Text, war: {other:?}"),
        }
    }
}
