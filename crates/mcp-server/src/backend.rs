//! Die einzige Schnittstelle zwischen dem MCP-Protokoll (`tool_server.rs`)
//! und der tatsächlichen Ausführungslogik der App (Spec 0028, Abschnitt 3:
//! "Wiederverwendung statt Parallelstruktur").
//!
//! Diese Crate hängt bewusst **nicht** von `crates/app-shell` ab (das würde
//! eine zyklische Abhängigkeit erzeugen, sobald `app-shell` seinerseits
//! diese Crate einbindet, um den Server zu starten). Stattdessen definiert
//! sie nur diesen schmalen Trait; `app-shell` implementiert ihn in einem
//! eigenen Modul, das direkt `orchestration::handle_action_proposed`
//! aufruft — denselben Code-Pfad, den auch der interne Chat-Flow nutzt. Es
//! gibt dadurch strukturell exakt eine Implementierung dieses Traits im
//! Produktivbetrieb, keinen zweiten Ausführungspfad; die Trait-Grenze ist
//! reine Abhängigkeitsrichtung, keine fachliche Parallelstruktur.

use async_trait::async_trait;
use ssh_manager_core::profiles::AiAction;
use ssh_manager_core::shared::ServerId;

/// Ergebnis eines Nachschlagens per `ServerId`, das fehlschlagen kann, weil
/// der Server nicht existiert **oder** nicht auf der MCP-Allow-Liste steht
/// (Spec 0028, Abschnitt 6) — bewusst **ein** Fehlerfall für beide Ursachen,
/// damit ein MCP-Client aus der Antwort nicht ableiten kann, ob ein Server
/// mit dieser ID überhaupt existiert (kein Informationsleck über nicht
/// freigegebene Server).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupError {
    UnknownServer,
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LookupError::UnknownServer => write!(f, "unbekannter Server"),
        }
    }
}

impl std::error::Error for LookupError {}

/// Minimale, für `list_servers` benötigte Information — bewusst kein
/// vollständiges `ServerDto`, MCP-Clients brauchen nur genug, um dem Nutzer
/// einen Server zur Auswahl zu nennen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSummary {
    pub id: ServerId,
    pub name: String,
}

/// Ergebnis eines über MCP angestoßenen `propose_action`-Aufrufs, nachdem er
/// vollständig entschieden wurde (genehmigt, abgelehnt, oder bei der
/// Ausführung fehlgeschlagen) — nicht zu verwechseln mit einem Timeout beim
/// *Warten* auf diese Entscheidung (Spec 0028, Abschnitt 7), das auf einer
/// Ebene darüber (`tool_server.rs`) behandelt wird, ohne dass diese
/// Ausführung selbst abgebrochen wird.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionOutcome {
    /// Automatisch ausgeführt oder vom Nutzer genehmigt; `summary` ist der
    /// für den MCP-Client lesbare Ergebnistext (Kommando-Output,
    /// Datei-Inhalt, Schreib-Zusammenfassung, Notiz-Zusammenfassung — je
    /// nach `AiAction`-Variante).
    Approved { summary: String },
    /// Vom Nutzer abgelehnt oder von der Filter-Engine automatisch
    /// blockiert (`Decision::Deny`).
    Rejected { reason: String },
    /// Genehmigt, aber die eigentliche Ausführung ist fehlgeschlagen (z. B.
    /// SFTP-Fehler, SSH-Verbindungsfehler).
    Failed { message: String },
}

/// Schnittstelle, die `crates/app-shell` implementiert, um MCP-Tool-Calls an
/// die reale App-Logik anzubinden (Session-Verwaltung, Filter-Engine,
/// Bestätigungsdialog). Jede Methode entspricht einer Gruppe von Tools aus
/// Spec 0028, Abschnitt 4.
#[async_trait]
pub trait McpBackend: Send + Sync + 'static {
    /// Nur die Server, die auf der MCP-Allow-Liste stehen (Spec 0028,
    /// Abschnitt 6) — **nicht** alle verwalteten Server. Alles andere würde
    /// die Existenz nicht freigegebener Server bereits über dieses Tool
    /// verraten, obwohl `propose_action`/`server_notes` sie korrekt als
    /// "unbekannt" behandeln.
    async fn list_servers(&self) -> Vec<ServerSummary>;

    /// Entspricht `effective_notes()` (Spec 0003, Abschnitt 5.1) für den
    /// angegebenen Server.
    async fn server_notes(&self, server_id: ServerId) -> Result<String, LookupError>;

    /// Übersetzt einen der vier aktionsauslösenden Tool-Calls in die
    /// jeweilige `AiAction` und führt sie über denselben Orchestrierungs-
    /// Pfad wie der interne Chat-Flow aus (Spec 0028, Abschnitt 3). Läuft
    /// bis zur endgültigen Entscheidung — der Aufrufer in `tool_server.rs`
    /// ist dafür verantwortlich, diesen Aufruf mit dem Timeout aus
    /// Abschnitt 7 zu umgeben, ohne ihn dabei tatsächlich abzubrechen.
    ///
    /// `client_name` ist der optionale `clientInfo.name` aus dem
    /// MCP-Handshake (Spec 0028, Abschnitt 9a) — für die
    /// Ursprungs-Kennzeichnung im Bestätigungsdialog.
    async fn propose_action(
        &self,
        server_id: ServerId,
        action: AiAction,
        client_name: Option<String>,
    ) -> Result<ActionOutcome, LookupError>;
}
