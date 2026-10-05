//! Spec 0101, A16: Bis zur Entsperrung ist **kein** Kommando außer
//! Entsperren, Beenden und „Neu anfangen“ erreichbar.
//!
//! **Warum ein eigenes Tor und nicht der fehlende `AppState`:** Gemessen
//! (Teil 0 Frage 3, M1/M6) beantwortet Tauri 2 ein Kommando mit nicht
//! verwaltetem `State<AppState>` mit einem Fehler, nicht mit einem Panic —
//! darauf allein ließe sich A16 also bauen. Es wäre aber ein Schutz, der an
//! der Argumentliste jedes einzelnen Kommandos hängt: `get_platform`,
//! `get_app_info` oder `create_overlay_titlebar` nehmen keinen `AppState`
//! und liefen im gesperrten Zustand weiterhin. A16 sagt „kein Kommando“,
//! nicht „kein Kommando mit Datenbankzugriff“.
//!
//! Das Tor sitzt deshalb **vor** dem von `generate_handler!` erzeugten
//! Verteiler (gemessen: M4) und entscheidet nach einer Positivliste. Ein
//! neues Kommando ist damit automatisch gesperrt, solange niemand es
//! ausdrücklich hinzufügt — die Richtung, in die ein Versehen fallen soll.
//!
//! Die ACL von Tauri liegt noch davor (gemessen: ohne Freigabe erreicht ein
//! Aufruf dieses Tor nicht). Sie ist eine zweite, unabhängige Schranke, kein
//! Ersatz.

use std::sync::atomic::{AtomicBool, Ordering};

/// Der Fehlercode, mit dem ein gesperrtes Kommando antwortet (A20).
pub const APP_LOCKED_CODE: &str = "APP_LOCKED";

/// Die **einzigen** Kommandos, die vor der Entsperrung laufen (A16).
///
/// Jedes davon entspricht einer der Wahlmöglichkeiten, die A16 und A13
/// nennen: entsperren, beenden, „Neu anfangen“ (über die Antwort auf einen
/// Startdialog), ein Master-Passwort einrichten. Keines liest Daten, keines
/// baut eine Verbindung auf, keines startet den MCP-Server.
const ALLOWED_WHILE_LOCKED: &[&str] = &[
    "get_startup_state",
    "unlock_with_master_password",
    "answer_startup_prompt",
    "quit_application",
    // Spec 0024: die Sprachwahl der Oberfläche. Die Entsperrmaske muss
    // übersetzt sein, und `get_platform` liefert nur Betriebssystem und
    // Architektur — keine Daten aus der Datenbank.
    "get_platform",
];

/// Spec 0101, A16. `Send + Sync`, weil der Verteiler sie aus jedem
/// Webview-Thread liest.
pub struct StartupGate {
    unlocked: AtomicBool,
}

impl StartupGate {
    /// `unlocked = true` im Schlüsselbund-Modus: Dort ist der `AppState`
    /// fertig, bevor das Fenster entsteht, und es gibt nichts zu sperren.
    pub fn new(unlocked: bool) -> Self {
        Self {
            unlocked: AtomicBool::new(unlocked),
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.unlocked.load(Ordering::SeqCst)
    }

    /// **Nur in eine Richtung.** Ein Tor, das sich wieder schließen ließe,
    /// bräuchte eine Antwort auf „was passiert mit den laufenden
    /// Sitzungen" — und ein Sperren nach Leerlauf ist ausdrücklich
    /// Nicht-Ziel dieser Spec (§3).
    pub fn unlock(&self) {
        self.unlocked.store(true, Ordering::SeqCst);
    }

    pub fn allows(&self, command: &str) -> bool {
        self.is_unlocked() || ALLOWED_WHILE_LOCKED.contains(&command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T18, erste Hälfte: Ein Daten-Kommando ist im gesperrten Zustand
    /// nicht erreichbar — und zwar auch dann, wenn es gar keinen `AppState`
    /// nimmt.
    #[test]
    fn test_t18_the_gate_blocks_everything_but_the_named_commands() {
        let gate = StartupGate::new(false);

        for command in [
            "list_servers",
            "connect",
            "send_chat_message",
            "get_mcp_server_settings",
            "set_mcp_server_enabled",
            // Die beiden ohne `AppState` — ohne das Tor liefen sie.
            "get_app_info",
            "create_overlay_titlebar",
        ] {
            assert!(
                !gate.allows(command),
                "{command} darf vor der Entsperrung nicht erreichbar sein (A16)"
            );
        }

        for command in ALLOWED_WHILE_LOCKED {
            assert!(gate.allows(command), "{command} muss erreichbar sein");
        }
    }

    /// Nach der Entsperrung ist alles erreichbar — sonst wäre das Tor eine
    /// dauerhafte Sperre und der Test oben bewiese nichts über den
    /// Normalbetrieb.
    #[test]
    fn test_the_gate_opens_once_and_then_allows_everything() {
        let gate = StartupGate::new(false);
        assert!(!gate.allows("list_servers"));

        gate.unlock();

        assert!(gate.is_unlocked());
        assert!(gate.allows("list_servers"));
        assert!(gate.allows("connect"));
    }

    /// Im Schlüsselbund-Modus gibt es nichts zu sperren.
    #[test]
    fn test_the_keychain_mode_starts_open() {
        let gate = StartupGate::new(true);
        assert!(gate.allows("list_servers"));
    }

    /// Die Positivliste darf nichts enthalten, was Daten liest oder
    /// schreibt. Der Test ist eine Erinnerung an die Richtung: Wer hier
    /// etwas hinzufügt, muss diese Liste bewusst ändern.
    #[test]
    fn test_the_allow_list_stays_small_and_named() {
        assert_eq!(
            ALLOWED_WHILE_LOCKED,
            &[
                "get_startup_state",
                "unlock_with_master_password",
                "answer_startup_prompt",
                "quit_application",
                "get_platform",
            ],
            "A16 nennt Entsperren, Beenden und „Neu anfangen“ — jede weitere \
             Zeile hier ist eine bewusste Entscheidung und gehört in den Bericht"
        );
    }
}
