//! Welche Zeichen ein `ssh_config`-Wert **quotet** braucht — geteilt
//! zwischen Leser und Schreiber (Spec 0075, §9/Q-1 Punkt 5).
//!
//! Der Leser (`parser::split_keyword`/`parser::tokenize`) trennt an
//! Leerraum und behandelt `"…"` als ein zusammenhängendes Wort; ein `#`
//! mitten in einer Zeile gehört zum Wert, nicht zum Kommentar (so verhält
//! sich `readconf.c`). Genau diese drei Zeichen — Leerraum, `#`, `"` — sind
//! es auch, die der Schreiber (`export::quote_value`) erkennen muss: Stünde
//! einer von ihnen ungeschützt in einem exportierten Wert, läse `ssh`
//! entweder ein anderes Wort oder bräche die Zeile am `#` ab (§4.3).
//!
//! **Warum eine eigene, geteilte Funktion statt zweier unabhängiger
//! Implementierungen:** Eine symmetrisch falsche Zeichenliste bestünde den
//! Rundlauftest (§6.3.3) trotzdem — Leser und Schreiber wären sich einig,
//! nur eben falsch einig. Deshalb ist `ssh -F … -G` (§6.3.2) der äußere
//! Zeuge, nicht der Rundlauf allein (§9/Q-1 Punkt 5).
//!
//! Bewusst **keine** Änderung an `parser.rs`: Die Leseseite ist Teil der
//! bereits geprüften Schritte 1–3 (ADR 0074) und wird hier nicht angefasst.
//! Diese Liste beschreibt nur, was der **Schreiber** quoten muss — die
//! Umkehrung (Escapes beim Lesen wieder auflösen) ist nicht Teil dieses
//! Schritts, s. `export`-Moduldoc.

/// Muss `value` in Anführungszeichen gesetzt werden, um beim Einlesen als
/// **ein** Wort zu gelten? Wahr für Leerraum, `#` und `"` selbst.
pub fn needs_quoting(value: &str) -> bool {
    value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || c == '#' || c == '"')
}

/// Ersetzt Zeilenumbrüche (`\n`, `\r`) durch ein Leerzeichen.
///
/// **Anführungszeichen schützen davor nicht** — `ssh_config` ist
/// zeilenbasiert wie unser eigener Parser: Ein echtes `\n` **innerhalb**
/// eines gequoteten Werts beendet trotzdem die Zeile, der Rest steht als
/// **eigene** Zeile in der Datei und kann eine wirksame Direktive werden
/// (spec-reviewer-Fund, Runde 1). Betrifft jeden Wert, der ungeprüft aus
/// dem Bestand kommt — `host`/`username` haben beim Anlegen von Hand
/// keinen Format-Zwang (§1.3) — und jeden `# smart-ssh:`-Kommentar
/// (Gruppenname, Schlagworte, `sftp_server_path`).
///
/// Eine allgemeine Kontrollzeichen-Politik für Server-/Gruppenfelder wäre
/// die sauberere, aber größere Lösung (an der Eingangsgrenze, nicht hier);
/// diese Funktion ist die Verteidigung im Schreiber, die für den Export
/// unabhängig davon greift.
pub fn strip_line_breaks(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

/// Setzt `value` bei Bedarf in Anführungszeichen (OpenSSH-Regel, §4.3): ein
/// enthaltenes `"` wird mit `\"` maskiert. Wird nicht gequotet, wenn es
/// nicht nötig ist — ein unnötig gequoteter Wert wäre kein Fehler, aber
/// unnötige Abweichung von dem, was ein Nutzer von Hand geschrieben hätte.
///
/// **Zeilenumbrüche werden zuerst entfernt** ([`strip_line_breaks`]), dann
/// **`\` maskiert, dann `"`** (beides spec-reviewer-Funde, Runde 1): Ohne
/// die erste Reihenfolge könnte ein `\n` im Wert die Zeile aufbrechen (s.
/// dortigen Kommentar); ohne die zweite ergab ein Wert, der auf `\` endet
/// (`a b\`), vorher `"a b\"` — das Escapezeichen verschluckte das
/// schließende Anführungszeichen, und `ssh` las den Rest der Zeile als
/// Müll.
pub fn quote_value(value: &str) -> String {
    let value = strip_line_breaks(value);
    if !needs_quoting(&value) {
        return value;
    }
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_value_is_not_quoted() {
        assert_eq!(quote_value("deploy"), "deploy");
        assert!(!needs_quoting("deploy"));
    }

    #[test]
    fn value_with_space_is_quoted() {
        assert_eq!(quote_value("max mustermann"), "\"max mustermann\"");
    }

    #[test]
    fn embedded_quote_is_escaped() {
        assert_eq!(quote_value("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn hash_forces_quoting() {
        assert!(needs_quoting("a#b"));
    }

    #[test]
    fn empty_value_is_quoted() {
        // Ein leerer Wert ohne Anführungszeichen wäre beim Lesen gar kein
        // Wort — `User ` (gefolgt von nichts) ist nicht dasselbe wie
        // `User ""`.
        assert_eq!(quote_value(""), "\"\"");
    }

    /// spec-reviewer-Fund, Runde 1: ein `\n` im Wert bricht die Zeile auf,
    /// egal ob quotiert — Anführungszeichen sind kein mehrzeiliger
    /// Container in einem zeilenbasierten Format.
    #[test]
    fn newline_in_value_does_not_break_the_line() {
        let quoted = quote_value("max\nProxyJump evil");
        assert!(
            !quoted.contains('\n'),
            "quotiert, aber trotzdem ein Zeilenumbruch: {quoted:?}"
        );
        assert_eq!(quoted, "\"max ProxyJump evil\"");
    }

    #[test]
    fn carriage_return_in_value_is_also_stripped() {
        assert!(!quote_value("a\rb").contains('\r'));
    }
}
