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

use ssh_manager_core::profiles::ssh_config::{build_export, ExportPlan};

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
        include_hint: format!("Include {}", path.display()),
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
/// `canonicalize()` scheiterte also am häufigsten Fall. Deshalb zuerst ein
/// rein lexikalischer Vergleich (funktioniert unabhängig davon, ob die
/// Datei existiert), zusätzlich — falls das Elternverzeichnis existiert —
/// ein Vergleich der **aufgelösten** Elternverzeichnisse, damit ein
/// symlink-verschleiertes `~/.ssh` nicht durchrutscht.
fn resolves_to_user_ssh_config(path: &Path) -> bool {
    let Some(home) = home_dir() else {
        return false;
    };
    let target = home.join(".ssh").join("config");

    if path.components().eq(target.components()) {
        return true;
    }

    match (
        path.parent().and_then(|p| std::fs::canonicalize(p).ok()),
        target.parent().and_then(|p| std::fs::canonicalize(p).ok()),
    ) {
        (Some(a), Some(b)) => a == b && path.file_name() == target.file_name(),
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
