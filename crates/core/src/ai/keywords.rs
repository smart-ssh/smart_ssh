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
    (
        "fr",
        &[
            "mot de passe",
            "mots de passe",
            "phrase secrète",
            "phrase secrete",
            "phrase de passe",
            "jeton",
            "clé api",
            "cle api",
            "clé privée",
            "cle privee",
            "clé d'accès",
            "cle d'acces",
            "clé secrète",
            "cle secrete",
            "identifiants",
            "code pin",
        ],
    ),
    (
        "es",
        &[
            "contraseña",
            "contrasena",
            "clave",
            "frase de contraseña",
            "frase de contrasena",
            "secreto",
            "clave api",
            "clave privada",
            "clave de acceso",
            "clave secreta",
            "credenciales",
            "código pin",
            "codigo pin",
        ],
    ),
    (
        "it",
        &[
            "parola d'ordine",
            "parola chiave",
            "frase di accesso",
            "frase segreta",
            "segreto",
            "chiave api",
            "chiave privata",
            "chiave di accesso",
            "chiave segreta",
            "credenziali",
            "codice pin",
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
        // Zusätzlich die NFD-Form jedes Eintrags mit Umlaut (`u` + U+0308),
        // damit zerlegte Eingaben (macOS, Zwischenablage) ebenfalls greifen.
        // Der Eingabetext wird nicht normalisiert, Offsets bleiben gleich.
        let mut variants: Vec<String> = all.iter().map(|k| k.to_string()).collect();
        for k in &all {
            let d = decompose_umlauts(k);
            if d != *k {
                variants.push(d);
            }
        }
        variants
            .iter()
            .map(|k| regex::escape(k).replace(' ', "[ _-]"))
            .collect::<Vec<_>>()
            .join("|")
    });
    &ALT
}

/// Ersetzt vorkomponierte Umlaute (`ä`, `ö`, `ü`) durch Grundbuchstabe +
/// kombinierendes Trema (U+0308), also die NFD-Form.
fn decompose_umlauts(k: &str) -> String {
    let mut out = String::with_capacity(k.len() + 2);
    for c in k.chars() {
        match c {
            'ä' => out.push_str("a\u{308}"),
            'ö' => out.push_str("o\u{308}"),
            'ü' => out.push_str("u\u{308}"),
            _ => out.push(c),
        }
    }
    out
}
