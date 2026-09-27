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
//! (Spec 0021, Abschnitt 5, `crate::commands::stop_auto_continuation`). Seit
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
mod remote_files;
#[cfg(test)]
mod test_support;

pub use chat_turn::run_chat_turn;
pub(crate) use chat_turn::{
    push_history, reapply_redaction_for_send, wait_for_ai_request_slot, wait_for_rate_limit_budget,
    PENDING_ACTION_CONFIRM_TIMEOUT, SIDE_CALL_MAX_TOKENS,
};

pub(crate) use action_exec::handle_mcp_action_proposed;

pub use notes::{
    execute_note_shrink_request, generate_session_title_on_disconnect, should_suggest_note_shrink,
    suggest_note_shrink_on_disconnect, suggest_note_update_on_disconnect, NoteShrinkTarget,
    ProfileStoreNoteShrinkTarget,
};
pub(crate) use notes::{propose_note_from_chat_content, LARGE_NOTE_DIALOG_THRESHOLD_CHARS};

pub(crate) use remote_files::ensure_sftp_open;
