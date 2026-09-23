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

    // Spec 0073, I3: Der Helfer darf nie *weniger* entfernen als
    // `str::trim`. `str::trim` entfernt genau die Zeichen mit der
    // Unicode-Eigenschaft `White_Space`, also genau `char::is_whitespace`.
    //
    // Dieser Test prüft das über den **gesamten** Unicode-Bereich statt an
    // einer Handvoll Beispiele. Grund: Die übrigen Tests kommen mit Space,
    // Tab, `\r` und `\n` aus — würde jemand `c.is_whitespace()` später
    // durch das naheliegend wirkende `c.is_ascii_whitespace()` ersetzen,
    // bliebe die ganze Suite grün, während I3 gebrochen wäre. Betroffen
    // wären u. a. U+00A0 (geschütztes Leerzeichen, der klassische
    // Web-Copy-Paste), U+3000 und U+2028.
    #[test]
    fn test_i3_every_unicode_whitespace_char_is_still_trimmed() {
        let mut checked = 0_u32;
        for code in 0..=0x10_FFFF_u32 {
            let Some(c) = char::from_u32(code) else {
                continue;
            };
            if !c.is_whitespace() {
                continue;
            }
            checked += 1;
            let input = format!("{c}a{c}");
            assert_eq!(
                trim_credential_value(&input),
                "a",
                "U+{code:04X} ist Unicode-Whitespace und muss weiterhin fallen (I3)"
            );
            // Gegenprobe zur Referenz: Was `str::trim` entfernt hätte, muss
            // auch der Helfer entfernen.
            assert_eq!(trim_credential_value(&input), input.trim());
        }
        assert!(
            checked > 20,
            "es wurden nur {checked} Whitespace-Zeichen geprüft — der Test läuft ins Leere"
        );
        // Die drei namentlich genannten Fälle, damit ein Fehlschlag sie
        // sofort zeigt statt nur einen Codepunkt.
        assert_eq!(trim_credential_value("\u{00A0}sk-key\u{00A0}"), "sk-key");
        assert_eq!(trim_credential_value("\u{3000}sk-key"), "sk-key");
        assert_eq!(trim_credential_value("\u{2028}sk-key"), "sk-key");
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

    // --- Spec 0073, §6.3: adversariale Fälle --------------------------

    // Spec 0073, X1: sehr viele Randzeichen. Ergebnis ist das eine
    // Nutzzeichen — und die Laufzeit bleibt linear.
    //
    // Zur Schranke: `trim_matches` ist ein einzelner Durchlauf von beiden
    // Enden, also ~10^5 Zeichenprüfungen — im Debug-Build Mikrosekunden.
    // Ein quadratisches Abschneiden in einer Schleife (jedes Mal den Rest
    // neu kopieren) wären ~10^10 Operationen, also Minuten. Zwischen
    // beidem liegen vier Größenordnungen; die Schranke von 5 s trennt sie
    // auch auf einer stark ausgelasteten Maschine zuverlässig, ohne dass
    // dieser Test von Laufzeitschwankungen abhängt.
    #[test]
    fn test_x1_many_edge_chars_stay_linear() {
        let input = format!("{}x", "\u{200B}".repeat(100_000));

        let started = std::time::Instant::now();
        let result = trim_credential_value(&input);
        let elapsed = started.elapsed();

        assert_eq!(result, "x");
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "100 000 Randzeichen dauerten {elapsed:?} — das deutet auf ein \
             quadratisches Abschneiden hin"
        );
    }

    // Spec 0073, X2: dieselbe Länge, aber ohne Nutzzeichen — leerer
    // String, keine Panik, keine Unterlauf-Arithmetik an den Enden. Hier
    // ohne Zeitschranke: X2 verlangt sie nicht, X1 deckt die Laufzeit ab.
    #[test]
    fn test_x2_edge_chars_only_at_full_length_becomes_empty() {
        let input = "\u{200B}".repeat(100_000);
        assert_eq!(trim_credential_value(&input), "");
    }

    // Spec 0073, X3: Die Liste aus A1 ist bewusst **endlich**. U+2800
    // (Braille Blank) und U+3164 (Hangul Filler) sind ebenfalls unsichtbar,
    // stehen aber nicht darin — sie bleiben erhalten. Dieser Test hält die
    // Festlegung fest: „alles Unsichtbare" wäre geraten (§4.3). Wer die
    // Liste erweitert, muss das hier bewusst tun und melden (§8).
    #[test]
    fn test_x3_invisible_chars_outside_the_list_are_kept() {
        for c in ['\u{2800}', '\u{3164}'] {
            let code = c as u32;
            assert!(
                !INVISIBLE_CREDENTIAL_EDGE_CHARS.contains(&c),
                "U+{code:04X} gehört nicht in die Liste aus A1"
            );
            let input = format!("{c}sk-key{c}");
            assert_eq!(
                trim_credential_value(&input),
                input,
                "U+{code:04X} steht nicht in der Liste und muss erhalten bleiben"
            );
        }
    }

    // Spec 0073, X4: Der Helfer entfernt keine Satzzeichen. Ein Secret, das
    // mit `-` oder `_` beginnt oder endet, bleibt unverändert.
    #[test]
    fn test_x4_punctuation_at_the_edges_is_kept() {
        for input in ["-abc", "_abc", "abc-", "abc_", "--abc__", ".abc.", "+abc="] {
            assert_eq!(
                trim_credential_value(input),
                input,
                "Satzzeichen am Rand dürfen nicht entfernt werden"
            );
        }
    }

    // Spec 0073, X5: Im Normalfall tut die Änderung nichts. 1 000 gültige
    // Keys aus dem üblichen Zeichenvorrat gehen unverändert durch.
    //
    // Der Generator ist ein deterministischer Xorshift mit festem Startwert
    // statt einer Zufallsquelle: kein neuer Abhängigkeitsbedarf, und ein
    // Fehlschlag ist mit demselben Eingabewert reproduzierbar.
    #[test]
    fn test_x5_valid_keys_pass_through_unchanged() {
        const ALPHABET: &[u8] =
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.";
        let mut state: u64 = 0x2073_0073_2073_0073;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };

        for _ in 0..1_000 {
            let len = 8 + (next() % 57) as usize;
            let key: String = (0..len)
                .map(|_| ALPHABET[(next() % ALPHABET.len() as u64) as usize] as char)
                .collect();
            assert_eq!(
                trim_credential_value(&key),
                key,
                "ein gültiger Key darf unverändert durchgehen"
            );
        }
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
