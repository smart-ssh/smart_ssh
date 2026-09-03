//! Fest codierte Muster-Listen für die regelbasierte Risiko-Einschätzung
//! (Spec 0026, Abschnitt 2).
//!
//! **Startpunkte, kein Anspruch auf Vollständigkeit** (Spec, Abschnitt 2,
//! letzter Absatz vor "Für ReadRemoteFile/WriteRemoteFile"): anders als die
//! Hard-Blacklist der Filter-Engine (`crate::filter::hard_blacklist_patterns`)
//! sind diese Listen bewusst nicht sicherheitskritisch — sie blockieren
//! nichts, eine Lücke ist eine unvollständige Warnung, kein Sicherheitsloch.
//! Nutzer-Erweiterbarkeit ist explizit nicht Teil dieser Spec (Abschnitt 5).
//!
//! Wie bei der Hard-Blacklist (s. `filter::blacklist`-Modul-Kommentar)
//! werden Kommandos vor dem Matching lowercased (`classifier::best_match`),
//! die Muster hier sind deshalb konsequent in Kleinschreibung formuliert.

use crate::filter::Pattern;
use Pattern::{Exact, Glob, Regex};

use super::types::RiskLevel;

/// Liest-/schreibt-Kommandos, deren Ziel-Pfad Secrets enthalten könnte,
/// gemeinsam mit dem SFTP-Pseudokommando-Präfix (Spec 0020, Abschnitt 4.1 —
/// `sftp-read`/`sftp-write`, dieselbe Konvention wie die Filter-Engine).
/// Anders als z. B. `Glob("*.key*")` (würde auch auf `find -name *.key`
/// treffen, das nur nach Dateien *sucht*, ohne sie zu lesen) verlangt diese
/// Präfix-Alternation, dass tatsächlich ein Inhalt gelesen/geschrieben
/// wird — die Yellow-Muster unten (`find`/`ls`/`grep`) bleiben davon
/// unberührt.
const READ_COMMAND_PREFIX: &str = r"^(?:cat|less|head|tail|sftp-read|sftp-write)\b.*";

pub(super) fn server_risk_patterns() -> &'static [(Pattern, RiskLevel, &'static str)] {
    static PATTERNS: std::sync::OnceLock<Vec<(Pattern, RiskLevel, &'static str)>> =
        std::sync::OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            // --- Rot: destruktiv/irreversibel/dienstunterbrechend ---------
            (
                Glob("*rm*-rf*".to_string()),
                RiskLevel::Red,
                "Rekursives, erzwungenes Löschen (rm -rf)",
            ),
            (
                Glob("*rm*-fr*".to_string()),
                RiskLevel::Red,
                "Rekursives, erzwungenes Löschen (rm -fr)",
            ),
            (
                Glob("*dd*if=**of=/dev/*".to_string()),
                RiskLevel::Red,
                "Direktes Schreiben auf ein Blockgerät (dd ... of=/dev/...)",
            ),
            (
                Glob("*mkfs*".to_string()),
                RiskLevel::Red,
                "Dateisystem neu erstellen (mkfs)",
            ),
            (
                Exact(":(){ :|:& };:".to_string()),
                RiskLevel::Red,
                "Fork-Bombe",
            ),
            (
                Glob("shutdown*".to_string()),
                RiskLevel::Red,
                "Server wird heruntergefahren",
            ),
            (
                Glob("reboot*".to_string()),
                RiskLevel::Red,
                "Server wird neu gestartet",
            ),
            (
                Glob("poweroff*".to_string()),
                RiskLevel::Red,
                "Server wird ausgeschaltet",
            ),
            (
                Glob("halt*".to_string()),
                RiskLevel::Red,
                "Server wird angehalten",
            ),
            (
                Glob("iptables*-f*".to_string()),
                RiskLevel::Red,
                "Firewall-Regeln werden vollständig geleert (iptables -F)",
            ),
            (
                Glob("chmod*-r*777*/*".to_string()),
                RiskLevel::Red,
                "Rechte werden rekursiv auf 777 gesetzt",
            ),
            // --- Gelb: potenziell destruktiv, aber üblicherweise gezielt --
            (
                Glob("rm *".to_string()),
                RiskLevel::Yellow,
                "Löscht Dateien (rm)",
            ),
            (
                Glob("systemctl*stop*".to_string()),
                RiskLevel::Yellow,
                "Dienst wird gestoppt (systemctl stop)",
            ),
            (
                Glob("systemctl*restart*".to_string()),
                RiskLevel::Yellow,
                "Dienst wird neu gestartet (systemctl restart)",
            ),
            (
                Glob("apt*remove*".to_string()),
                RiskLevel::Yellow,
                "Paket wird entfernt (apt remove)",
            ),
            (
                Glob("yum*remove*".to_string()),
                RiskLevel::Yellow,
                "Paket wird entfernt (yum remove)",
            ),
            (
                Glob("git*reset*--hard*".to_string()),
                RiskLevel::Yellow,
                "Lokale Änderungen werden verworfen (git reset --hard)",
            ),
            (
                Glob("kill *".to_string()),
                RiskLevel::Yellow,
                "Prozess wird beendet (kill)",
            ),
            (
                Glob("kill*-9*".to_string()),
                RiskLevel::Yellow,
                "Prozess wird hart beendet (kill -9)",
            ),
        ]
    })
}

pub(super) fn data_risk_patterns() -> &'static [(Pattern, RiskLevel, &'static str)] {
    static PATTERNS: std::sync::OnceLock<Vec<(Pattern, RiskLevel, &'static str)>> =
        std::sync::OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            // --- Rot: enthält mit hoher Wahrscheinlichkeit Secrets --------
            (
                Regex(format!("{READ_COMMAND_PREFIX}id_rsa")),
                RiskLevel::Red,
                "Zugriff auf eine SSH-Private-Key-Datei (id_rsa)",
            ),
            (
                Regex(format!("{READ_COMMAND_PREFIX}id_ed25519")),
                RiskLevel::Red,
                "Zugriff auf eine SSH-Private-Key-Datei (id_ed25519)",
            ),
            (
                Regex(format!(r"{READ_COMMAND_PREFIX}\.pem\b")),
                RiskLevel::Red,
                "Zugriff auf eine Zertifikat-/Key-Datei (.pem)",
            ),
            (
                Regex(format!(r"{READ_COMMAND_PREFIX}\.key\b")),
                RiskLevel::Red,
                "Zugriff auf eine Key-Datei (.key)",
            ),
            (
                Regex(format!(r"{READ_COMMAND_PREFIX}\.env\b")),
                RiskLevel::Red,
                "Zugriff auf eine .env-Datei",
            ),
            (
                Regex(format!("{READ_COMMAND_PREFIX}credentials")),
                RiskLevel::Red,
                "Zugriff auf eine Datei/einen Pfad namens \"credentials\"",
            ),
            (
                Regex(format!("{READ_COMMAND_PREFIX}shadow")),
                RiskLevel::Red,
                "Zugriff auf /etc/shadow",
            ),
            (
                Regex(format!(r"{READ_COMMAND_PREFIX}\.aws/credentials")),
                RiskLevel::Red,
                "Zugriff auf AWS-Zugangsdaten",
            ),
            (
                Exact("env".to_string()),
                RiskLevel::Red,
                "Gibt alle Umgebungsvariablen aus (env)",
            ),
            (
                Glob("env *".to_string()),
                RiskLevel::Red,
                "Gibt alle Umgebungsvariablen aus (env)",
            ),
            (
                Exact("printenv".to_string()),
                RiskLevel::Red,
                "Gibt alle Umgebungsvariablen aus (printenv)",
            ),
            (
                Glob("printenv *".to_string()),
                RiskLevel::Red,
                "Gibt Umgebungsvariablen aus (printenv)",
            ),
            (
                Glob("mysqldump*".to_string()),
                RiskLevel::Red,
                "Datenbank-Export ohne erkennbare Redaction (mysqldump)",
            ),
            (
                Glob("pg_dump*".to_string()),
                RiskLevel::Red,
                "Datenbank-Export ohne erkennbare Redaction (pg_dump)",
            ),
            (
                Glob("*select*from*user*".to_string()),
                RiskLevel::Red,
                "SQL-Abfrage auf eine user-Tabelle",
            ),
            (
                Glob("*select*from*password*".to_string()),
                RiskLevel::Red,
                "SQL-Abfrage auf eine password-Spalte/-Tabelle",
            ),
            // --- Gelb: könnte auf Secrets hindeuten, aber nicht sicher ----
            (
                Glob("find*-name*.key*".to_string()),
                RiskLevel::Yellow,
                "Sucht gezielt nach Key-Dateien (find -name *.key)",
            ),
            (
                Glob("ls*.ssh*".to_string()),
                RiskLevel::Yellow,
                "Listet den .ssh-Ordner auf",
            ),
            (
                Glob("ls*/etc*".to_string()),
                RiskLevel::Yellow,
                "Listet /etc auf",
            ),
            (
                Glob("grep*password*".to_string()),
                RiskLevel::Yellow,
                "Sucht nach dem Begriff \"password\" in Dateien",
            ),
            (
                Glob("grep*secret*".to_string()),
                RiskLevel::Yellow,
                "Sucht nach dem Begriff \"secret\" in Dateien",
            ),
            (
                Glob("grep*token*".to_string()),
                RiskLevel::Yellow,
                "Sucht nach dem Begriff \"token\" in Dateien",
            ),
        ]
    })
}
