//! Export nach `ssh_config` (Spec 0075, §3.2, §7.4).
//!
//! Öffnet den nativen Speichern-unter-Dialog **im Backend** (dasselbe
//! Vertrauensgefälle wie beim Import, s. `ssh_config_apply`-Moduldoc: das
//! Frontend bestimmt nie einen Pfad, den wir öffnen oder beschreiben) und
//! lehnt die eigene `~/.ssh/config` als Ziel ab — **auch dann**, wenn der
//! native Dialog ein Überschreiben schon bestätigt hat (§3.2.5). Die
//! eigentliche Abbildung (Alias-Regeln, Kommentare, Quoting) liegt in
//! `ssh_manager_core::profiles::ssh_config::export` — reine Logik, kein
//! Dateisystem; dieses Modul öffnet den Dialog, prüft das Ziel und schreibt.

use std::path::{Path, PathBuf};

use ssh_manager_core::profiles::ssh_config::{build_export, quote_value, ExportPlan};

use crate::error::{CommandError, CommandResult};

const DEFAULT_EXPORT_FILE_NAME: &str = "smart-ssh-export.conf";

/// Ein Server, wie er tatsächlich exportiert wurde — Grundlage der
/// Abschlussmeldung (§3.2.7).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedServerDto {
    pub original_name: String,
    pub alias: String,
    pub renamed: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResultDto {
    pub path: String,
    pub exported: Vec<ExportedServerDto>,
    /// §3.2.5: die Zeile, mit der man die Datei in `~/.ssh/config`
    /// einbindet — `Include <pfad>`. Fertig formatiert, damit das Frontend
    /// sie unverändert anzeigen/kopieren kann.
    pub include_hint: String,
}

fn build_result_dto(plan: &ExportPlan, path: &Path) -> ExportResultDto {
    ExportResultDto {
        path: path.display().to_string(),
        exported: plan
            .exported
            .iter()
            .map(|e| ExportedServerDto {
                original_name: e.original_name.clone(),
                alias: e.alias.clone(),
                renamed: e.renamed,
            })
            .collect(),
        // spec-reviewer-Fund, Runde 1: ungequotet war die Zeile für jeden
        // Pfad mit Leerzeichen (z. B. "…/Google Drive/…") schlicht falsch —
        // `ssh` läse sie am ersten Leerzeichen ab als „garbage at end of
        // line". Dieselbe Quoting-Regel wie im Schreiber selbst (§4.3),
        // nicht neu erfunden.
        include_hint: format!("Include {}", quote_value(&path.display().to_string())),
    }
}

/// Derselbe `directories::UserDirs`-Weg wie
/// `ssh_config_import::home_dir` — ein Wert, den der Rest der App
/// ohnehin schon benutzt, keine neue Abhängigkeit.
fn home_dir() -> Option<PathBuf> {
    directories::UserDirs::new().map(|u| u.home_dir().to_path_buf())
}

/// §3.2.5: „Unabhängig davon lehnt der Export `~/.ssh/config` selbst als
/// Ziel ab — auch wenn der Nutzer sie im Dialog auswählt und bestätigt."
///
/// `~/.ssh/config` existiert auf vielen Rechnern **nicht**, ein reines
/// `canonicalize()` scheiterte also am häufigsten Fall. Deshalb mehrere
/// Prüfungen, jede für sich verschärfend, nie lockernd (spec-reviewer-Fund,
/// Runde 1 — die ersten beiden Fassungen bestanden beide adversarialen
/// Fälle unten NICHT):
///
/// 1. Rein lexikalischer Vergleich — funktioniert unabhängig davon, ob die
///    Datei existiert.
/// 2. Derselbe Vergleich, aber der **Dateiname case-insensitiv**: macOS
///    (APFS/HFS+ in der Standardeinstellung) und Windows behandeln
///    `Config`/`CONFIG` als dieselbe Datei wie `config`. Ein Nutzer, der
///    versehentlich (oder absichtlich) die Groß-/Kleinschreibung ändert,
///    überschreibt auf diesen Systemen trotzdem die echte Datei — ein
///    Treffer mehr kostet hier nichts außer einem selteneren
///    Dateinamenswunsch, ein verpasster Treffer kostet die Datei.
/// 3. **Aufgelöst**: Ist `path` selbst ein Symlink auf die tatsächliche
///    `~/.ssh/config`, erkennt das nur `canonicalize(path)` direkt — nicht
///    nur des Elternverzeichnisses. `canonicalize` löst **keine**
///    Hardlinks auf (zwei Hardlinks auf dieselbe Inode haben verschiedene
///    kanonische Pfade) — dafür ist Fall 5 da.
/// 4. Wie 2, aber auf den **aufgelösten Elternverzeichnissen** — deckt ein
///    symlink-verschleiertes `~/.ssh`-Verzeichnis ab.
/// 5. **Selbe Inode** (nur Unix): Ein Hardlink auf `~/.ssh/config` hat
///    einen anderen Pfad, aber dieselbe `(device, inode)`-Kombination —
///    `canonicalize` (Fall 3) sieht das nicht, `MetadataExt` schon
///    (spec-reviewer-Fund, Runde 2: der Doc-Kommentar hatte „oder
///    Hardlink" behauptet, ohne dass Fall 3 das je geleistet hätte).
fn resolves_to_user_ssh_config(path: &Path) -> bool {
    let Some(home) = home_dir() else {
        return false;
    };
    resolves_to_ssh_config_under(path, &home)
}

/// Kern von [`resolves_to_user_ssh_config`], mit injizierbarem
/// Home-Verzeichnis — testbar mit einem `tempdir()` statt dem echten `~`
/// dieser Maschine (die z. B. kein `.ssh` haben muss, damit Fall 3/4/5
/// prüfbar sind).
fn resolves_to_ssh_config_under(path: &Path, home: &Path) -> bool {
    let target = home.join(".ssh").join("config");

    // 1) Lexikalisch, exakt.
    if path.components().eq(target.components()) {
        return true;
    }

    // 2) Lexikalisch, Dateiname case-insensitiv (setzt gleiches
    // Elternverzeichnis voraus — sonst träfe es jede beliebige Datei
    // namens "config" irgendwo im Dateisystem).
    if path
        .parent()
        .is_some_and(|p| p.components().eq(target.parent().unwrap().components()))
        && filenames_match_case_insensitive(path, &target)
    {
        return true;
    }

    // 3) Aufgelöst: `path` selbst könnte ein Symlink auf die echte Datei
    // sein.
    if let Ok(canon_target) = std::fs::canonicalize(&target) {
        if let Ok(canon_path) = std::fs::canonicalize(path) {
            if canon_path == canon_target {
                return true;
            }
        }
    }

    // 4) Aufgelöste Elternverzeichnisse, Dateiname case-insensitiv.
    let via_parent = match (
        path.parent().and_then(|p| std::fs::canonicalize(p).ok()),
        target.parent().and_then(|p| std::fs::canonicalize(p).ok()),
    ) {
        (Some(a), Some(b)) => a == b && filenames_match_case_insensitive(path, &target),
        _ => false,
    };
    if via_parent {
        return true;
    }

    // 5) Dieselbe Inode (nur Unix) — fängt einen Hardlink auf
    // `~/.ssh/config`, den `canonicalize` (Fall 3) nicht auflöst.
    #[cfg(unix)]
    {
        if same_file_unix(path, &target) {
            return true;
        }
    }

    false
}

#[cfg(unix)]
fn same_file_unix(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(ma), Ok(mb)) => ma.dev() == mb.dev() && ma.ino() == mb.ino(),
        _ => false,
    }
}

fn filenames_match_case_insensitive(a: &Path, b: &Path) -> bool {
    match (
        a.file_name().and_then(|n| n.to_str()),
        b.file_name().and_then(|n| n.to_str()),
    ) {
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
        _ => false,
    }
}

/// Öffnet den Speichern-unter-Dialog, lehnt `~/.ssh/config` als Ziel ab
/// (§3.2.5) und schreibt die Server (ohne den lokalen Pseudo-Server, §3.2.4)
/// als `ssh_config`. `None`, wenn der Nutzer den Dialog abbricht — kein
/// Fehlerfall.
#[tauri::command]
pub async fn export_ssh_config(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
    title: String,
) -> CommandResult<Option<ExportResultDto>> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(&title)
        .set_file_name(DEFAULT_EXPORT_FILE_NAME)
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = picked.into_path()?;

    if resolves_to_user_ssh_config(&path) {
        return Err(CommandError::from(
            "Die eigene ~/.ssh/config wird nicht überschrieben. Bitte einen anderen Dateinamen oder Ort wählen.".to_string(),
        ));
    }

    let servers = state.profile_store.list_servers().await?;
    let groups = state.profile_store.list_groups().await?;
    let plan = build_export(&servers, &groups, crate::local_server::LOCAL_SERVER_ID);

    // `std::fs::write` statt `tokio::fs`: derselbe, durch eine explizite
    // Nutzeraktion ausgelöste Einzelschreibvorgang wie bei
    // `commands::export_document` — für eine `ssh_config`-Größe unkritisch
    // blockierend.
    std::fs::write(&path, &plan.text)?;

    Ok(Some(build_result_dto(&plan, &path)))
}

#[cfg(test)]
mod tests;
