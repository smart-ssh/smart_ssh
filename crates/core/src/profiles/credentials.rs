use std::fmt;

use secrecy::SecretString;

use super::types::CredentialRef;

/// Spec 0073, A1: Zeichen, die am **Rand** eines eingefügten Zugangsdaten-
/// Wertes praktisch immer ein Kopierunfall sind, aber die Unicode-
/// Eigenschaft `White_Space` **nicht** tragen — `str::trim` und
/// `char::is_whitespace` lassen sie also stehen.
///
/// Bewusst als benannte Konstante statt als Literal im Rumpf von
/// [`trim_credential_value`]: Die Liste ist eine fachliche Festlegung und
/// wächst erfahrungsgemäß. Sie ist ebenso bewusst **endlich** — „alles
/// Unsichtbare" wäre geraten und träfe auch Zeichen, die ein Anbieter
/// legitim verwenden könnte (Spec 0073, §4.3 und X3).
pub const INVISIBLE_CREDENTIAL_EDGE_CHARS: &[char] = &[
    '\u{FEFF}', // Byte Order Mark — Key aus einer UTF-8-Datei mit BOM kopiert
    '\u{200B}', // Zero Width Space — Kopie aus einer Webseite
    '\u{200C}', // Zero Width Non-Joiner — dito
    '\u{200D}', // Zero Width Joiner — dito
    '\u{2060}', // Word Joiner — dito
];

/// Spec 0073: die **eine** Trim-Semantik für Zugangsdaten-Werte.
///
/// Entfernt am Anfang und am Ende alles, was `char::is_whitespace()`
/// erfüllt (das ist wörtlich das bisherige Verhalten von `str::trim`),
/// **und** zusätzlich die unsichtbaren Zeichen aus
/// [`INVISIBLE_CREDENTIAL_EDGE_CHARS`]. Die beiden Bedingungen sind
/// ODER-verknüpft, damit diese Fassung per Konstruktion nicht *weniger*
/// bereinigen kann als der bisherige `.trim()` (Spec 0073, I3).
///
/// **Nur die Ränder** (A2, I1): Innerhalb des verbleibenden Wertes wird
/// kein Zeichen angefasst und nichts normalisiert — ein Secret ist eine
/// undurchsichtige Zeichenkette, und was in seiner Mitte steht, darf die
/// App nicht beurteilen. Das Ergebnis ist deshalb nie länger als die
/// Eingabe.
///
/// **Leer bleibt leer** (A4, I4): Ein Wert, der ausschließlich aus zu
/// trimmenden Zeichen besteht, ergibt den leeren String. Für einen
/// `api_key` heißt leer nach Spec 0007, Abschnitt 8.2, weiterhin
/// „Credential unverändert lassen" — nicht „löschen". Das ist
/// beabsichtigt und in `dto.rs` eigens getestet.
///
/// Arbeitet auf `&str`, gibt `String` zurück, nimmt **kein**
/// `SecretString` entgegen und loggt nichts (A5, I2): Wer den Helfer
/// benutzt, entscheidet selbst, wann ein Wert zum Secret wird.
pub fn trim_credential_value(value: &str) -> String {
    value
        .trim_matches(|c: char| c.is_whitespace() || INVISIBLE_CREDENTIAL_EDGE_CHARS.contains(&c))
        .to_string()
}

/// Fehler eines [`CredentialStore`]-Zugriffs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    NotFound(CredentialRef),
    /// Backend-spezifischer Fehler (z. B. OS-Keychain verweigert Zugriff).
    /// Nur die Fehlermeldung, nie ein Secret-Wert.
    Backend(String),
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialError::NotFound(r) => {
                write!(f, "kein Credential für Referenz '{}' gefunden", r.as_str())
            }
            CredentialError::Backend(msg) => write!(f, "Credential-Backend-Fehler: {msg}"),
        }
    }
}

impl std::error::Error for CredentialError {}

pub type CredentialResult<T> = Result<T, CredentialError>;

/// Zugriff auf die eigentlichen Secret-Werte hinter einer [`CredentialRef`]
/// (Spec 0003, Abschnitt 4). Die lokale DB kennt nur die opaken
/// `CredentialRef`-Strings; das eigentliche Secret kommt ausschließlich über
/// diesen Trait aus dem OS-Keychain (`keyring`-Crate, s. Spec 0001).
///
/// Als Trait modelliert (analog zu `PolicyStore` in Spec 0002), damit Tests
/// eine In-Memory-Implementierung nutzen können, ohne einen echten
/// OS-Keychain zu brauchen.
pub trait CredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString>;
    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()>;
    fn delete(&self, r: &CredentialRef) -> CredentialResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Spec 0073, T1: ein normaler Wert ohne Randzeichen bleibt unverändert.
    #[test]
    fn test_t1_plain_value_is_unchanged() {
        assert_eq!(trim_credential_value("sk-ant-secret"), "sk-ant-secret");
    }

    // Spec 0073, T2: das bisherige `str::trim`-Verhalten bleibt erhalten —
    // Leerzeichen, Tabs, `\r` und `\n` am Rand fallen weg (I3).
    #[test]
    fn test_t2_whitespace_is_still_trimmed() {
        assert_eq!(trim_credential_value("  sk-key\r\n"), "sk-key");
        assert_eq!(trim_credential_value("\t\n sk-key \t\r\n"), "sk-key");
        assert_eq!(trim_credential_value("sk-key\r"), "sk-key");
    }

    // Spec 0073, T3: je ein Fall für jedes Zeichen aus A1, am Anfang, am
    // Ende und beidseitig. Scheitert gegen den Stand vor dieser Spec.
    #[test]
    fn test_t3_invisible_chars_are_trimmed_at_both_edges() {
        for c in INVISIBLE_CREDENTIAL_EDGE_CHARS {
            let code = *c as u32;
            assert_eq!(
                trim_credential_value(&format!("{c}sk-key")),
                "sk-key",
                "U+{code:04X} am Anfang muss entfernt werden"
            );
            assert_eq!(
                trim_credential_value(&format!("sk-key{c}")),
                "sk-key",
                "U+{code:04X} am Ende muss entfernt werden"
            );
            assert_eq!(
                trim_credential_value(&format!("{c}sk-key{c}")),
                "sk-key",
                "U+{code:04X} beidseitig muss entfernt werden"
            );
        }
    }

    // Spec 0073, T4: Leerzeichen und unsichtbare Zeichen in beliebiger
    // Reihenfolge am Rand — alle fallen weg.
    #[test]
    fn test_t4_mixed_whitespace_and_invisible_chars_are_trimmed() {
        assert_eq!(
            trim_credential_value(" \u{FEFF}\t\u{200B}sk-key\u{2060} \u{200D}\r\n"),
            "sk-key"
        );
        assert_eq!(
            trim_credential_value("\u{200C} \u{FEFF} sk-key \u{FEFF} \u{200C}"),
            "sk-key"
        );
    }

    // Spec 0073, T5 / I1: dieselben Zeichen **in der Mitte** bleiben
    // erhalten — ein Secret ist undurchsichtig.
    #[test]
    fn test_t5_invisible_chars_inside_the_value_are_kept() {
        for c in INVISIBLE_CREDENTIAL_EDGE_CHARS {
            let code = *c as u32;
            let input = format!("sk{c}key");
            assert_eq!(
                trim_credential_value(&input),
                input,
                "U+{code:04X} in der Mitte darf nicht angefasst werden"
            );
        }
        assert_eq!(trim_credential_value("sk key"), "sk key");
    }

    // Spec 0073, T6 / A4: ein Wert nur aus zu trimmenden Zeichen ergibt den
    // leeren String. Für einen `api_key` heißt das nach Spec 0007
    // „unverändert lassen" — beabsichtigt, s. den Test in `dto.rs`.
    #[test]
    fn test_t6_value_made_only_of_trimmable_chars_becomes_empty() {
        assert_eq!(trim_credential_value("\u{FEFF}\u{200B}\u{2060}"), "");
        assert_eq!(trim_credential_value("  \t\r\n "), "");
        assert_eq!(
            trim_credential_value(" \u{FEFF} \u{200C}\u{200D}\t\r\n"),
            ""
        );
    }

    // Spec 0073, T7.
    #[test]
    fn test_t7_empty_input_stays_empty() {
        assert_eq!(trim_credential_value(""), "");
    }

    // Spec 0073, T8: Mehrbyte-Zeichen am Rand bleiben stehen und die
    // Funktion schneidet nicht innerhalb einer UTF-8-Sequenz (keine Panik).
    #[test]
    fn test_t8_multibyte_edges_are_kept_without_panicking() {
        assert_eq!(trim_credential_value("äsk-keyö"), "äsk-keyö");
        assert_eq!(trim_credential_value("🔑sk-key🔑"), "🔑sk-key🔑");
        assert_eq!(
            trim_credential_value(" \u{FEFF}🔑key🔑\u{200B} "),
            "🔑key🔑"
        );
        assert_eq!(trim_credential_value("日本語"), "日本語");
    }

    // Spec 0073, A2: das Ergebnis ist nie länger als die Eingabe.
    #[test]
    fn test_a2_result_is_never_longer_than_the_input() {
        for input in [
            "",
            "sk-key",
            " sk-key ",
            "\u{FEFF}sk\u{200B}key\u{FEFF}",
            "🔑",
            "\u{2060}",
        ] {
            assert!(
                trim_credential_value(input).len() <= input.len(),
                "Ausgabe für {input:?} darf nicht wachsen"
            );
        }
    }
}
