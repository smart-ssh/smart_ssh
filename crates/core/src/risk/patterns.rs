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

/// Spec 0068, Teil 2: Befehle, die Dateiinhalt in die Ausgabe bringen (und
/// damit in Chat/KI-Kontext bzw. zum MCP-Client), plus die Datei-Lese-
/// Aktion als `sftp-read`-Pseudokommando. Bewusst NICHT enthalten: `cp`,
/// `install`, `mv` — genau die Wege, die die Prompt-Regel aus Spec 0066
/// empfiehlt, weil der Inhalt dabei nie in einer Ausgabe landet.
pub(super) const SECRET_READ_COMMANDS: &str = r"(?:cat|less|more|head|tail|bat|batcat|tac|nl|grep|egrep|fgrep|zgrep|rg|sed|awk|gawk|mawk|nawk|xxd|od|hexdump|strings|base64|openssl|sftp-read)";

/// Spec 0068, Teil 2: Secret-Pfade, deren Lesen immer eine Bestätigung
/// verlangt (s. `classifier::secret_path_read_reason`). Geprüft wird auf
/// dem kleingeschriebenen, von Quotes/Backslashes befreiten Teilkommando.
///
/// Grenzfälle, begründet entschieden (strenger ist die sichere Richtung):
/// - `~/.ssh/id_*` ohne `*.pub` — öffentliche Schlüssel sind nicht geheim.
///   `id_*` trifft jeden privaten Schlüssel mit eigenem Namen
///   (`id_deploy`), auch Sicherungskopien (`id_rsa.bak`).
/// - `.env.example` wird MIT eskaliert: Beispieldateien enthalten in der
///   Praxis immer wieder echte Werte, und eine Bestätigung kostet wenig.
/// - `*.pem` trifft auch öffentliche Zertifikate — lieber eine Rückfrage
///   zu viel als einen privaten Schlüssel im Chat.
pub(super) fn secret_path_patterns() -> &'static [(regex::Regex, &'static str)] {
    static PATTERNS: std::sync::OnceLock<Vec<(regex::Regex, &'static str)>> =
        std::sync::OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            // `id_<name>`, aber nicht `id_<name>.pub` (die `regex`-Crate
            // kennt kein Lookahead — daher die ausbuchstabierte Alternation
            // "Ende, anderes Zeichen, oder `.` gefolgt von etwas anderem als
            // `pub`").
            (
                r"\bid_[a-z0-9_-]+(?:$|[^a-z0-9_.-]|\.(?:$|[^p]|p(?:$|[^u])|pu(?:$|[^b])))",
                "Liest einen privaten SSH-Schlüssel (id_*)",
            ),
            (r"\.pem\b", "Liest eine Zertifikat-/Schlüsseldatei (.pem)"),
            (r"\.key\b", "Liest eine Schlüsseldatei (.key)"),
            (r"\.env\b", "Liest eine .env-Datei"),
            (r"/etc/g?shadow", "Liest /etc/shadow bzw. /etc/gshadow"),
            (r"\.aws/credentials", "Liest AWS-Zugangsdaten"),
            (
                r"\.docker/config\.json",
                "Liest Docker-Registry-Zugangsdaten",
            ),
            (
                r"\.kube/config",
                "Liest eine Kubernetes-Konfiguration mit Zugangsdaten",
            ),
            (r"\.netrc\b", "Liest ~/.netrc (Zugangsdaten)"),
            (r"\.pgpass\b", "Liest ~/.pgpass (Datenbank-Passwörter)"),
            (r"\.git-credentials\b", "Liest ~/.git-credentials"),
        ]
        .into_iter()
        .map(|(pattern, reason)| {
            (
                regex::Regex::new(pattern).expect("eingebautes Secret-Pfad-Muster ist gültig"),
                reason,
            )
        })
        .collect()
    })
}

/// Spec 0068, Teil 2: Fragmente, bei denen ein Platzhalter (`*`, `?`,
/// `[`, `{`) oder ein `-exec`/`xargs`-Lesen im Zweifel eskaliert wird.
pub(super) const SECRET_PATH_HINTS: &[&str] = &[
    ".ssh",
    ".aws",
    ".docker",
    ".kube",
    "/etc/",
    ".env",
    ".pem",
    ".key",
    "netrc",
    "pgpass",
    "git-credentials",
    "id_",
    "shadow",
];

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
            // Spec 0068, Teil 2: weitere Secret-Pfade auch in der Anzeige
            // (die eigentliche Eskalation macht `secret_path_read_reason`).
            (
                Regex(format!("{READ_COMMAND_PREFIX}/etc/gshadow")),
                RiskLevel::Red,
                "Zugriff auf /etc/gshadow",
            ),
            (
                Regex(format!(r"{READ_COMMAND_PREFIX}\.docker/config\.json")),
                RiskLevel::Red,
                "Zugriff auf Docker-Registry-Zugangsdaten",
            ),
            (
                Regex(format!(r"{READ_COMMAND_PREFIX}\.kube/config")),
                RiskLevel::Red,
                "Zugriff auf eine Kubernetes-Konfiguration",
            ),
            (
                Regex(format!(r"{READ_COMMAND_PREFIX}\.netrc\b")),
                RiskLevel::Red,
                "Zugriff auf ~/.netrc",
            ),
            (
                Regex(format!(r"{READ_COMMAND_PREFIX}\.pgpass\b")),
                RiskLevel::Red,
                "Zugriff auf ~/.pgpass",
            ),
            (
                Regex(format!(r"{READ_COMMAND_PREFIX}\.git-credentials\b")),
                RiskLevel::Red,
                "Zugriff auf ~/.git-credentials",
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
