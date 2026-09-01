use crate::profiles::AuthMethod;

/// Ergebnis eines ausgeführten Kommandos im Exec-Modus (Spec 0005,
/// Abschnitt 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
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
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: u32,
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
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
