//! Tauri-Commands (Spec 0007, Abschnitt 4).
//!
//! Spec 0083: die ursprünglich einzelne Datei ist in thematisch geschnittene
//! Module aufgeteilt (`ai_providers`, `servers`, `groups`, `connect`,
//! `terminal`, `chat`, `notes`, `rules`, `diagnostics_export`, `app_meta`,
//! `elevation`, `sftp` — reine Umstrukturierung, keine Verhaltensänderung).
//! Diese Wurzel re-exportiert jedes Modul als Glob, damit Tauris
//! `generate_handler!` (das für jeden `#[tauri::command]` die im selben
//! Modul erzeugten Hilfsmakros unter demselben Pfad wie die Funktion
//! braucht) und bestehende `crate::commands::X`-Aufrufstellen anderswo im
//! Crate unverändert `commands::X` auflösen.

mod ai_providers;
mod app_meta;
mod chat;
mod connect;
mod diagnostics_export;
mod elevation;
mod groups;
mod notes;
mod rules;
mod servers;
mod sftp;
mod terminal;
#[cfg(test)]
mod test_support;

pub(crate) use ai_providers::*;
pub(crate) use app_meta::*;
pub(crate) use chat::*;
pub(crate) use connect::*;
pub(crate) use diagnostics_export::*;
pub(crate) use elevation::*;
pub(crate) use groups::*;
pub(crate) use notes::*;
pub(crate) use rules::*;
pub(crate) use servers::*;
pub(crate) use sftp::*;
pub(crate) use terminal::*;
