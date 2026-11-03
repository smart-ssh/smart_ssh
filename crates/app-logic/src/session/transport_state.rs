//! Connection and turn-control state of a session: terminal, status,
//! auto reconnect, pending action, chat turn, auto-continue stop.

use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::mpsc;

use crate::confirmation::ConfirmationRegistry;
use crate::events::ConnectionStatus;
use crate::state::ActionId;

use super::{ChatTurnState, TerminalCommand};

/// Issue #871: connection status and turn control of a session (terminal
/// channel, status, reconnect state, pending action, running chat turn,
/// auto-continue stop, cancellation registry).
pub struct TransportState {
    /// `Some`, sobald `open_terminal` den Aktor gestartet hat.
    /// `terminal_input`/`terminal_resize` senden darüber; `None` (noch kein
    /// `open_terminal` aufgerufen, oder Kanal bereits geschlossen) wird als
    /// Anwenderfehler zurückgemeldet statt zu blockieren.
    pub terminal: StdMutex<Option<mpsc::UnboundedSender<TerminalCommand>>>,
    /// Spec 0017, Abschnitt 2: `list_sessions()`-Statusfeld. Startet bei
    /// `Connected` (`Session` wird erst nach erfolgreichem Verbindungsaufbau
    /// konstruiert, s. `app_shell::commands::connect`) und wird von
    /// `spawn_terminal_actor` auf `Disconnected` gesetzt, sobald der
    /// Terminal-Aktor unerwartet endet (Netzwerkfehler/Verbindungsabbruch) —
    /// ein expliziter `disconnect()`-Aufruf entfernt die Session ohnehin
    /// komplett aus `SessionManager`, dort ist kein Status-Update nötig.
    /// Eigener `StdMutex` statt z. B. eines `AtomicU8`: `ConnectionStatus`
    /// ist ein einfaches Copy-Enum, aber kein Integer — ein Mutex bleibt
    /// hier die geradlinigste Wahl, analog zu `terminal` oben.
    pub status: StdMutex<ConnectionStatus>,
    /// Issue #356, Spec 0107 §8: state of the automatic reconnect.
    pub auto_reconnect: crate::auto_reconnect::AutoReconnectState,
    /// Spec 0027: derselbe `Arc` wie `AppState.running_command_
    /// cancellations` — ein billiger Klon bei `connect()`, damit
    /// `orchestration::execute_suggested_command` (die nur `&Session`
    /// bekommt, kein `AppState`) einen laufenden Abbruch registrieren kann,
    /// ohne die Signaturen von `run_chat_turn`/`run_one_round`/
    /// `handle_action_proposed` anfassen zu müssen (s. Doc-Kommentar auf
    /// `AppState.running_command_cancellations`).
    pub running_command_cancellations: Arc<ConfirmationRegistry<ActionId, ()>>,
    /// Spec 0017, Abschnitt 5: `Some(action_id)`, während diese Session auf
    /// eine `Confirm`-Bestätigung wartet (gesetzt in
    /// `crate::orchestration::handle_action_proposed`, gelöscht sobald die
    /// wartende `rx.await` dort zurückkehrt) — die Grundlage für
    /// `SessionSummaryDto.has_pending_action`, den Tab-Indikator im
    /// Frontend. **Nicht** für den Disconnect-Notiz-Vorschlag (Spec 0010)
    /// verwendet: der läuft bewusst erst, nachdem die Session bereits aus
    /// `SessionManager` entfernt wurde, und ist als App-weite Benachrichtigung
    /// (`note-update-suggested`) ohnehin nie an einen Tab gebunden.
    pub pending_action: StdMutex<Option<ActionId>>,
    /// Spec 0066, §2: ob gerade ein Chat-Turn läuft, und welche
    /// Nutzer-Nachrichten währenddessen eingereiht wurden. Beides unter
    /// EINEM Lock, damit keine Nachricht zwischen "Warteschlange leer" und
    /// "Turn beendet" verloren gehen kann.
    pub chat_turn: StdMutex<ChatTurnState>,
    /// Spec 0021, Abschnitt 5 / Spec 0066, §1: gesetzt über
    /// [`Session::request_auto_continue_stop`] (`app_shell::commands::
    /// stop_auto_continuation`). Beendet die Fortsetzungskette und bricht
    /// einen laufenden KI-Stream bzw. die Wartezeit vor dem Send sofort ab
    /// (`crate::orchestration::run_one_round`); ein offener
    /// Bestätigungsdialog und ein bereits laufendes Kommando bleiben
    /// unangetastet. Zurückgesetzt in `app_shell::commands::
    /// send_chat_message_impl` beim Start eines Turns, atomar mit
    /// `chat_turn.running`.
    pub auto_continue_stop: std::sync::atomic::AtomicBool,
    /// Spec 0066, §1: weckt eine laufende Runde auf, sobald
    /// `auto_continue_stop` gesetzt wird, damit ein laufender KI-Stream
    /// sofort abgebrochen wird statt erst an der nächsten Rundengrenze.
    /// Immer über [`Session::request_auto_continue_stop`] setzen.
    pub auto_continue_stop_notify: tokio::sync::Notify,
}
