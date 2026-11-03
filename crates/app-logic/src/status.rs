//! Leaf module (Issue #868): [`ConnectionStatus`] lives here so `dto` does
//! not depend on `events`; `events` re-exports it.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    /// Issue #355, Spec 0105: a reconnect of a lost session is running.
    /// Goes back to `Connected` or, on failure, `Disconnected`.
    Reconnecting,
    /// Spec 0017, Abschnitt 2: `list_sessions()`-Status für einen laufenden
    /// `connect()`-Aufruf, der gerade auf `confirm_host_key` wartet — diese
    /// Session existiert noch nicht in `SessionManager.sessions` (sie wird
    /// erst nach erfolgreichem Verbindungsaufbau eingefügt, s.
    /// `app_shell::commands::connect`), taucht aber über
    /// `SessionManager.pending_connections` trotzdem in der Momentaufnahme
    /// auf. Wird **nie** über `connection-status-changed` gesendet (dieses
    /// Event kennt nur den Übergang Connected/Disconnected) — nur als
    /// `SessionSummaryDto`-Feld relevant.
    AwaitingHostKey,
}
