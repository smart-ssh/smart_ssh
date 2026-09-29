//! Kernschleife eines Chat-Turns (Spec 0007, Abschnitt 6) — bewusst als
//! reine, von Tauri unabhängige `async fn` gehalten (nimmt `&Session` +
//! `&dyn EventEmitter` + `&dyn ProfileStore`, keinen `tauri::AppHandle`
//! direkt), damit sie ohne laufende Tauri-Runtime gegen
//! `MockAiProvider`/`MockSshTransport` testbar ist (Aufgabenstellung Teil
//! 2, Punkt 5).
//!
//! `run_chat_turn` besteht aus 1..n Runden gegen `AiProvider::send()`
//! (`run_one_round`, je genau eine KI-Antwort: 0..n `TextDelta`s, 0..n
//! `ActionProposed`s, dann `Done`/`Error`). **Wurde in einer Runde
//! mindestens eine vorgeschlagene Aktion zu einem der vier Ausgänge aus
//! Spec 0021, Abschnitt 3 geführt** — tatsächlich ausgeführt (`AutoExec`
//! oder vom Nutzer bestätigt, inkl. `EditThenApprove`), vom Nutzer im
//! Bestätigungsdialog abgelehnt, oder automatisch durch die Filter-Engine
//! blockiert —, folgt automatisch eine weitere Runde mit dem inzwischen um
//! einen entsprechenden `MessageContent`-Eintrag (`CommandResult`/
//! Notiz-Zusammenfassung/`ActionRejected`) erweiterten Kontext. Ohne diesen
//! Automatismus bekäme die KI nach einer Ablehnung nie mit, dass (und
//! warum) nichts passiert ist, und der Nutzer bekäme nie eine Reaktion
//! darauf (Spec 0021, Abschnitt 1 — das war der gemeldete Bug: "nach
//! Ablehnen passiert nichts mehr"). Ursprünglich (vor Spec 0021) galt das
//! nur für tatsächlich ausgeführte Aktionen — s.
//! `docs/adr/0014-automatic-followup-round-after-executed-action.md` für die
//! Historie dieses Mechanismus, den Spec 0021 auf alle vier Ausgänge
//! erweitert, aber nicht grundlegend verändert.
//!
//! Das widerspricht nicht der im Projekt durchgehaltenen
//! Transparenz-/Bestätigungs-Philosophie (Spec 0002, Spec 0007 Abschnitt 5:
//! selbst `AutoExec`/`Deny` werden dem Nutzer nur *angezeigt*, nie verborgen
//! weitergesponnen): jede in einer Folgerunde neu vorgeschlagene Aktion
//! durchläuft erneut dieselbe Filter-Engine/Bestätigungslogik wie jede
//! andere auch — "automatisch weiterdenken" heißt nur, dass die KI
//! Ergebnisse automatisch sieht, nie, dass künftige Kommandos automatisch
//! ausgeführt werden (Spec 0021, Abschnitt 2). Begrenzt auf
//! [`chat_turn::MAX_AUTO_FOLLOWUP_ROUNDS`] Runden (Spec 0021, Abschnitt 4)
//! sowie jederzeit manuell abbrechbar über `Session::auto_continue_stop`
//! (Spec 0021, Abschnitt 5, `app_shell::commands::stop_auto_continuation`). Seit
//! Spec 0066 bricht der Stopp auch einen laufenden KI-Stream und die
//! Wartezeit vor dem Send sofort ab (`run_one_round`); ein bereits offener
//! Bestätigungsdialog und ein bereits laufendes Kommando bleiben
//! unangetastet.
//!
//! Spec 0083: die ursprünglich einzelne Datei ist in thematisch
//! geschnittene Module aufgeteilt (`chat_turn`, `action_exec`, `notes`,
//! `remote_files` — reine Umstrukturierung, keine Verhaltensänderung).
//! Diese Wurzel re-exportiert, was andere Module von `app-shell` bisher
//! unter `orchestration::…` erreicht haben, damit deren Aufrufstellen
//! unverändert bleiben.

mod action_exec;
mod chat_turn;
mod notes;
/// Spec 0088, A1: das Warten auf eine Bestätigung als Wert mit `Drop`.
mod pending_confirmation;
mod remote_files;
#[cfg(test)]
mod test_support;

pub use chat_turn::run_chat_turn;
// `push_history`/`PENDING_ACTION_CONFIRM_TIMEOUT`: auch von `app-shell`
// gebraucht (`commands::chat`/`commands::connect`), deshalb `pub` statt
// `pub(crate)` (Spec 0084, A6.4).
pub use chat_turn::{push_history, PENDING_ACTION_CONFIRM_TIMEOUT};
pub(crate) use chat_turn::{
    reapply_redaction_for_send, wait_for_ai_request_slot, wait_for_rate_limit_budget,
    SIDE_CALL_MAX_TOKENS,
};

// `app-shell::mcp_backend` ruft dies direkt auf.
pub use action_exec::handle_mcp_action_proposed;

pub use notes::{
    execute_note_shrink_request, generate_session_title_on_disconnect, should_suggest_note_shrink,
    suggest_note_shrink_on_disconnect, suggest_note_update_on_disconnect, NoteShrinkTarget,
    ProfileStoreNoteShrinkTarget,
};
// `app-shell::commands::{chat, notes}` brauchen beide direkt.
pub use notes::{propose_note_from_chat_content, LARGE_NOTE_DIALOG_THRESHOLD_CHARS};

// `app-shell::commands::elevation` ruft dies direkt auf.
pub use remote_files::ensure_sftp_open;

/// Spec 0084, §4 (Schnitt `orchestration` → `commands::sanitize_uname_output`):
/// hierher verschoben, weil `orchestration` (Tauri-frei, zieht nach
/// `app-logic`) diese Funktion in Tests braucht (Spec 0013, SEC-02, T5) und
/// die Funktion selbst Tauri-frei ist — sie gehörte vorher zu
/// `commands::diagnostics_export` (Tauri-gebunden, bleibt in `app-shell`),
/// das `commands::connect` weiterhin über diesen Pfad aufruft.
///
/// Validiert und bereinigt `uname -a` Output vor der Aufnahme in den
/// privilegierten System-Prompt: max 256 Zeichen, nur erlaubte Zeichen
/// (alphanumerisch, . _ - # : space tab), keine Steuerzeichen oder
/// Zeilenumbrüche.
pub fn sanitize_uname_output(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 256 {
        return None;
    }
    if trimmed.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || c == '.'
            || c == '_'
            || c == '-'
            || c == ' '
            || c == '\t'
            || c == '#'
            || c == ':'
    }) {
        Some(trimmed.to_string())
    } else {
        None
    }
}
