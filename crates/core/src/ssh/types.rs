use serde::{Deserialize, Serialize};

use crate::profiles::AuthMethod;

/// Ergebnis eines ausgeführten Kommandos im Exec-Modus (Spec 0005,
/// Abschnitt 4).
///
/// `Serialize`/`Deserialize`: Teil von `MessageContent::CommandResult`
/// (Spec 0034, Abschnitt 2 — `persistence-sqlite` serialisiert die gesamte
/// `MessageContent` als JSON in `chat_messages.content`), s. dortiger
/// Kommentar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
    /// `true`, wenn `stdout` und/oder `stderr` beim Streaming am
    /// konfigurierten Output-Cap (Spec 0043, Fund A) abgeschnitten wurden —
    /// UI und KI-Kontext können das damit kenntlich machen, statt eine
    /// unvollständige Ausgabe stillschweigend als vollständig auszugeben.
    /// `#[serde(default)]`, damit bereits persistierte `chat_messages`-Zeilen
    /// (Spec 0034) ohne dieses Feld weiterhin deserialisierbar bleiben.
    #[serde(default)]
    pub truncated: bool,
}

/// Ergebnis von [`SshTransport::execute_cancellable`](super::SshTransport::execute_cancellable)
/// (Spec 0027) — trägt zusätzlich zum eigentlichen [`CommandOutput`] mit,
/// ob ein Abbruch tatsächlich gegriffen hat, bevor das Kommando von selbst
/// beendet war. Bewusst kein Feld auf `CommandOutput` selbst: der Typ wird
/// an vielen Stellen (Terminal, Tests) unverändert für regulär beendete
/// Kommandos verwendet, ein zusätzliches, dort immer `false`/irrelevantes
/// Feld wäre unnötiger Ballast — `ExecOutcome` bleibt auf den einen
/// Aufrufer beschränkt, der Abbruch überhaupt kennt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutcome {
    pub output: CommandOutput,
    pub cancelled: bool,
}

/// Terminalgröße für eine PTY-Shell (Spec 0005, Abschnitt 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
}

/// Ein einzelner Hop in einer (ggf. über Jump-Hosts verketteten)
/// SSH-Verbindung (Spec 0005, Abschnitt 5).
#[derive(Debug, Clone, PartialEq)]
pub struct Hop {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
}

/// Vollständig aufgelöste Verbindungskette (Spec 0005, Abschnitt 5): erster
/// Eintrag = erster Sprung (äußerster Jump-Host), letzter Eintrag =
/// eigentliches Ziel.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionTarget {
    pub hops: Vec<Hop>,
}

/// Ein Eintrag in einem Remote-Verzeichnis (Spec 0020, Abschnitt 3).
/// `permissions` sind die reinen Unix-Rechte-Bits (`0o755`-Stil, ohne die
/// Dateityp-Bits aus `st_mode`) — für die Dateibrowser-Anzeige (Spec 0020,
/// Abschnitt 5.1: "Rechte"-Spalte) reicht das, `is_dir` trägt die
/// Typinformation bereits separat.
/// `uid`/`gid`/`owner`/`group` (Spec 0054, Teil 2 — Eigenschaften-Dialog):
/// SFTP (v3, was praktisch jeder Server spricht) liefert nur numerische
/// `uid`/`gid` verbindlich; die Namen (`owner`/`group`) sind eine v4+-
/// Erweiterung, die längst nicht jeder Server füllt — deshalb beides als
/// eigene optionale Felder statt eines einzelnen "Besitzer"-Strings, das
/// Frontend zeigt den Namen wenn vorhanden, sonst die reine Zahl.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: u32,
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub owner: Option<String>,
    pub group: Option<String>,
}

/// Ergebnis einer Host-Key-Prüfung, Trust-on-First-Use (Spec 0005,
/// Abschnitt 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyDecision {
    Trusted,
    Unknown {
        fingerprint: String,
    },
    Mismatch {
        expected_fingerprint: String,
        actual_fingerprint: String,
    },
}
