//! Schlüsselwörter, die einen Zugangsdaten-Wert in `schlüsselwort: wert` /
//! `schlüsselwort=wert` ankündigen (Spec 0013, Issue #261).
//!
//! **Eine Quelle**: Die Listen stehen je UI-Sprache hier, und der
//! Redactor wendet **immer alle** an, unabhängig von der gewählten
//! Oberflächensprache — die KI antwortet nicht verlässlich in der
//! UI-Sprache, und ein Kommando kann aus einer anderen Quelle stammen.
//! Eine neue UI-Sprache braucht hier einen Eintrag; ein Test gegen
//! `SUPPORTED_LANGUAGES` in `i18n.ts` erzwingt das.
//!
//! Abgelehnt (False Positives, `env`-/Shell-Ausgabe): `pwd` (`PWD=/home/…`
//! steht in jeder Umgebungsausgabe), `pass` und `pw` (zu kurz, matchen als
//! Teilstring in Wörtern wie `bypass=`, `pw=` in Prompt-Formaten).
//!
//! Ein Leerzeichen in einem Eintrag steht für „Leerzeichen, `_` oder `-`“.
//! Die Muster sind reine Alternationen ohne verschachtelte Quantoren.

use std::sync::LazyLock;

/// `(Sprachcode, Schlüsselwörter)` je unterstützter UI-Sprache.
pub const CREDENTIAL_KEYWORDS: &[(&str, &[&str])] = &[
    (
        "en",
        &[
            "password",
            "passwd",
            "token",
            "api_key",
            "secret",
            "passphrase",
            "credentials",
            "private key",
            "access key",
        ],
    ),
    (
        "de",
        &[
            "passwort",
            "kennwort",
            "zugangsdaten",
            "geheimnis",
            "schlüssel",
            "schluessel",
            "zugangsschlüssel",
            "zugangsschluessel",
            "token",
        ],
    ),
];

/// Sprachcodes, für die eine Liste existiert.
pub fn keyword_languages() -> Vec<&'static str> {
    CREDENTIAL_KEYWORDS.iter().map(|(l, _)| *l).collect()
}

/// Alternation (ohne Gruppe) über die Listen aller Sprachen, einmal gebaut.
pub fn keyword_alternation() -> &'static str {
    static ALT: LazyLock<String> = LazyLock::new(|| {
        let mut all: Vec<&str> = CREDENTIAL_KEYWORDS
            .iter()
            .flat_map(|(_, kws)| kws.iter().copied())
            .collect();
        all.sort_unstable();
        all.dedup();
        all.iter()
            .map(|k| regex::escape(k).replace(' ', "[ _-]"))
            .collect::<Vec<_>>()
            .join("|")
    });
    &ALT
}
