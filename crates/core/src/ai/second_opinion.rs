//! Längen-Cap für Inhalt, der an den optionalen Zweitmeinungs-Provider geht
//! (Spec 0043, Fund C) — sowohl die Daten-Risiko-Zweitmeinung (Spec 0026,
//! Abschnitt 3) als auch die Prompt-Injection-Fencing-Prüfung (Spec 0039,
//! Abschnitt 5.2) nutzen dieselbe "eine Infrastruktur" (s.
//! `app_shell::risk_second_opinion`-Doc-Kommentar), also auch denselben Cap.
//!
//! Reine Kürzungslogik hier in `core`, nicht in `app-shell` (Architektur-
//! Regel: `core` enthält die Domänenlogik, `app-shell` verdrahtet nur) —
//! `app_shell::risk_second_opinion::{fetch_second_opinion,
//! fetch_injection_check}` rufen [`truncate_for_second_opinion`] auf, bevor
//! sie den Provider aufrufen.

/// Default-Längen-Cap (Spec 0043, Abschnitt 4): die Zweitmeinung braucht nur
/// genug Kontext zur Einschätzung, nicht den vollen Output — reine Kosten-/
/// Timeout-Frage (Redaction läuft bereits davor, kein Secret-Leak-Risiko).
pub const DEFAULT_SECOND_OPINION_MAX_LEN: usize = 16 * 1024; // 16 KB

/// Kürzt `content` auf höchstens `max_len` Bytes (an einer UTF-8-
/// Zeichengrenze, nie mitten in einem Mehrbyte-Codepoint), falls es
/// darüber liegt. Gibt zusätzlich zurück, ob gekürzt wurde — der Aufrufer
/// hängt bei `true` einen Hinweis an den Zweitmeinungs-Prompt an (Spec
/// 0043, Abschnitt 4: "mit einem Hinweis ... dass gekürzt wurde").
///
/// Betrifft nur, was an die Zweitmeinung geht — die eigentliche Ausführung
/// und das regelbasierte Ergebnis bleiben unverändert. Da die Zweitmeinung
/// nur eskalieren kann, nie abschwächen (Spec 0026/0039), ist eine gekürzte
/// Eingabe fail-safe: im schlimmsten Fall übersieht sie etwas im
/// abgeschnittenen Teil.
pub fn truncate_for_second_opinion(content: &str, max_len: usize) -> (String, bool) {
    if content.len() <= max_len {
        return (content.to_string(), false);
    }
    let mut end = max_len;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    (content[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_under_cap_is_unchanged() {
        let (truncated, was_truncated) = truncate_for_second_opinion("short", 100);
        assert_eq!(truncated, "short");
        assert!(!was_truncated);
    }

    #[test]
    fn test_content_over_cap_is_cut_to_exactly_max_len() {
        let content = "A".repeat(200);
        let (truncated, was_truncated) = truncate_for_second_opinion(&content, 100);
        assert_eq!(truncated.len(), 100);
        assert!(was_truncated);
    }

    /// Spec 0043, Fund C: die Kürzung darf nie mitten in einem Mehrbyte-
    /// UTF-8-Codepoint schneiden (das würde `String`-Konstruktion panicken
    /// lassen) — hier ein 3-Byte-Zeichen ('€') genau an der Cap-Grenze.
    #[test]
    fn test_truncation_never_splits_a_multi_byte_utf8_character() {
        let content = format!("{}€", "A".repeat(99)); // 99 ASCII-Bytes + 3-Byte '€'
        let (truncated, was_truncated) = truncate_for_second_opinion(&content, 100);
        assert!(was_truncated);
        assert!(truncated.len() <= 100);
        assert!(truncated.is_char_boundary(truncated.len()));
    }
}
