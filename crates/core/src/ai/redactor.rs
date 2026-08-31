use regex::Regex;

use crate::ssh::CommandOutput;

/// Läuft über einen [`CommandOutput`], bevor er als
/// `MessageContent::CommandResult` in den Kontext für die nächste
/// KI-Anfrage aufgenommen wird (Spec 0006, Abschnitt 5). Redaction passiert
/// **immer**, unabhängig vom gewählten Provider, auch bei lokalen Modellen.
pub trait OutputRedactor: Send + Sync {
    fn redact(&self, output: &CommandOutput) -> CommandOutput;
}

/// Platzhalter, der einen erkannten Treffer ersetzt.
const REDACTED_PLACEHOLDER: &str = "[REDACTED]";

/// Default-Implementierung (Spec 0006, Abschnitt 5): erkennt
/// Private-Key-Blöcke, `password=`/`token=`/`api_key=`-artige Zeilen
/// (Groß-/Kleinschreibung ignoriert) und AWS-Access-Key-Muster.
///
/// Um "leicht um nutzerdefinierte Muster erweiterbar" zu sein (Aufgabe
/// Teil 1, Punkt 2 — die Speicherung dieser Muster ist explizit noch nicht
/// Teil dieses Schritts, s. Spec Abschnitt 5), akzeptiert
/// [`DefaultOutputRedactor::with_extra_patterns`] zusätzliche `Regex`-Werte,
/// die nach den eingebauten Mustern angewendet werden.
pub struct DefaultOutputRedactor {
    patterns: Vec<Regex>,
}

impl DefaultOutputRedactor {
    pub fn new() -> Self {
        Self {
            patterns: built_in_patterns(),
        }
    }

    pub fn with_extra_patterns(extra: Vec<Regex>) -> Self {
        let mut patterns = built_in_patterns();
        patterns.extend(extra);
        Self { patterns }
    }
}

impl Default for DefaultOutputRedactor {
    fn default() -> Self {
        Self::new()
    }
}

fn built_in_patterns() -> Vec<Regex> {
    vec![
        // Private-Key-Blöcke (RSA/EC/OPENSSH/PKCS8 ...), über mehrere
        // Zeilen hinweg — `(?s)`, damit `.` auch Zeilenumbrüche matcht.
        Regex::new(
            r"(?s)-----BEGIN [A-Z0-9_\- ]+PRIVATE KEY[A-Z0-9_\- ]*-----.*?-----END [A-Z0-9_\- ]+PRIVATE KEY[A-Z0-9_\- ]*-----",
        )
        .expect("eingebautes Private-Key-Muster ist gültig"),
        // PGP Private Keys
        Regex::new(r"(?s)-----BEGIN PGP PRIVATE KEY BLOCK-----.*?-----END PGP PRIVATE KEY BLOCK-----")
            .expect("eingebautes PGP-Private-Key-Muster ist gültig"),
        // password=/token=/api_key=/secret=/passphrase=-artige Zeilen, auch mit : und Quotes
        Regex::new(
            r#"(?i)(password|token|api_key|secret|passphrase)\s*[:=]\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|\S+)"#,
        )
        .expect("eingebautes Credential-Zeilen-Muster ist gültig"),
        // Bearer Tokens
        Regex::new(r"(?i)Bearer\s+[A-Za-z0-9_\-\.]{16,}")
            .expect("eingebautes Bearer-Token-Muster ist gültig"),
        // AWS-Access-Key- & Session-Token-Muster (AKIA, ASIA)
        Regex::new(r"(AKIA|ASIA)[0-9A-Z]{16}").expect("eingebautes AWS-Key-Muster ist gültig"),
        // GitHub Personal Access Tokens (ghp_, gho_, ghu_, ghs_, ghr_)
        Regex::new(r"(ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36,}")
            .expect("eingebautes GitHub-Token-Muster ist gültig"),
    ]
}

impl OutputRedactor for DefaultOutputRedactor {
    fn redact(&self, output: &CommandOutput) -> CommandOutput {
        CommandOutput {
            stdout: redact_bytes(&output.stdout, &self.patterns),
            stderr: redact_bytes(&output.stderr, &self.patterns),
            exit_code: output.exit_code,
        }
    }
}

/// Wendet alle `patterns` nacheinander auf `data` an. Arbeitet auf einer
/// (ggf. verlustbehafteten) UTF-8-Interpretation der Bytes — Kommando-Output
/// ist praktisch immer Text, und selbst bei vereinzelten ungültigen
/// Byte-Sequenzen ist "durch Ersatzzeichen ersetzt, aber Secret trotzdem
/// erkannt" dem Fail-safe-Prinzip (Spec 0002 Abschnitt 1) angemessener als
/// die Redaction bei Nicht-UTF-8-Output ganz zu überspringen.
fn redact_bytes(data: &[u8], patterns: &[Regex]) -> Vec<u8> {
    let mut text = String::from_utf8_lossy(data).into_owned();
    for pattern in patterns {
        if pattern.is_match(&text) {
            text = pattern
                .replace_all(&text, REDACTED_PLACEHOLDER)
                .into_owned();
        }
    }
    text.into_bytes()
}
