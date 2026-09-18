use globset::{Glob, GlobBuilder};
use regex::Regex;

use super::types::Pattern;

impl Pattern {
    /// Prüft, ob `cmd` (bereits whitespace-normalisiert) auf dieses Muster
    /// passt.
    ///
    /// `pub(crate)` statt `pub(super)` (Spec 0026, Abschnitt 2): `crate::risk`
    /// nutzt denselben `Pattern`-Typ für seine eigenen, containerinternen
    /// Musterlisten und braucht dieselbe Matching-Semantik — weiterhin keine
    /// Garantie für Code außerhalb dieser Crate.
    ///
    /// Ein syntaktisch ungültiges Glob-/Regex-Muster matcht nie (statt zu
    /// panicken) — eine kaputt konfigurierte Allow-Regel darf niemals
    /// versehentlich zu AutoExec führen, sondern soll folgenlos durchfallen
    /// (fail-safe defaults, Spec Abschnitt 1).
    ///
    /// **Unverändert seit Spec 0060** — bewusst NICHT die dort eingeführte
    /// striktere Pfad-Semantik (s. [`Pattern::matches_for_user_rule`]
    /// stattdessen): `crate::risk` nutzt diese Methode für hart codierte,
    /// als GANZE Kommandozeile gedachte Muster wie `"*dd*if=**of=/dev/*"`
    /// (kein „Kommando + Pfad-Argument"-Muster im Sinne von Spec 0060,
    /// sondern ein lose über die komplette Zeile gestreutes Muster, das
    /// `*`/`**` bewusst NICHT pfadsegment-weise verwendet). Ein Versuch,
    /// diese Methode hier direkt zu verschärfen, brach empirisch
    /// `test_server_risk_red_dd_to_device` (das `**` zwischen `if=` und
    /// `of=` ist in `globset` nur dann grenzüberschreitend, wenn es eine
    /// eigenständige Pfad-Komponente ist — hier ist es das nicht) und hätte
    /// eine Red-Risiko-Einstufung für einen Geräte-Überschreibbefehl
    /// stillschweigend zu „kein Risiko" herabgestuft — exakt die von
    /// CLAUDE.md verbotene Richtung („Escalation only goes one direction").
    /// Spec 0060 selbst ist außerdem explizit nur für die Filter-Engine-
    /// Regelauswertung (`crate::filter::engine`) gedacht, nicht für den
    /// Risiko-Klassifizierer.
    pub(crate) fn matches(&self, cmd: &str) -> bool {
        match self {
            Pattern::Exact(expected) => expected == cmd,
            Pattern::Glob(pattern) => Glob::new(pattern)
                .map(|glob| glob.compile_matcher().is_match(cmd))
                .unwrap_or(false),
            Pattern::Regex(pattern) => Regex::new(pattern)
                .map(|re| re.is_match(cmd))
                .unwrap_or(false),
        }
    }

    /// Spec 0060 (Filter-Engine-Umgehung, Release-Gate C): wie
    /// [`Pattern::matches`], aber NUR für die Filter-Engine-
    /// Regelauswertung (`crate::filter::engine::evaluate_rules_explained`,
    /// Allow **und** Deny — beide werten dieselben drei Text-Varianten
    /// original/stripped/resolved aus, s. dortigen Kommentar). Bewusst eine
    /// eigene Methode statt [`Pattern::matches`] direkt zu ändern: Letztere
    /// wird auch vom Risiko-Klassifizierer für hart codierte, NICHT
    /// pfadsegment-artige Muster genutzt (s. Doc-Kommentar dort) — eine
    /// gemeinsame Änderung hätte dort eine Red-Risiko-Einstufung
    /// stillschweigend abgeschwächt.
    ///
    /// Für ein **pfadförmiges** Muster (s. [`is_path_shaped_pattern`]) wird
    /// der Glob mit `literal_separator = true` kompiliert (`*` überquert
    /// kein `/` mehr), UND jedes pfadförmige Token in `cmd` wird vor dem
    /// Matching lexikalisch normalisiert (s.
    /// [`normalize_path_shaped_tokens`]) — ohne diese Normalisierung könnte
    /// ein einzelnes `..`-Segment OHNE eingebettetes `/` (z. B.
    /// `/var/log/..`) den `*` trotz `literal_separator` weiterhin als "ein
    /// Segment" durchrutschen lassen und außerhalb des erlaubten
    /// Verzeichnisses landen. Nicht-pfadförmige Muster (z. B. ein Glob über
    /// ein URL-artiges Argument) behalten exakt das bisherige Verhalten
    /// (`Glob::new`, `*` überquert `/`) — sonst bräche der Normalfall (s.
    /// Spec 0060, „Die Design-Entscheidung"). Für `Exact`/`Regex` identisch
    /// zu [`Pattern::matches`] (die Spec betrifft nur Glob-Muster).
    pub(crate) fn matches_for_user_rule(&self, cmd: &str) -> bool {
        match self {
            Pattern::Glob(pattern) if is_path_shaped_pattern(pattern) => {
                let normalized_cmd = normalize_path_shaped_tokens(cmd);
                GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .build()
                    .map(|glob| glob.compile_matcher().is_match(&normalized_cmd))
                    .unwrap_or(false)
            }
            _ => self.matches(cmd),
        }
    }

    /// Rohes Musterliteral, unabhängig von der Variante — für Anzeigezwecke
    /// (Hard-Blacklist-Liste, `EvaluationTrace::matched_hard_blacklist_entry`,
    /// Spec 0009 Abschnitt 4/6). Anders als [`Pattern::matches`] absichtlich
    /// `pub`: reiner Lesezugriff auf den Musterinhalt, kein Teil der
    /// Matching-Semantik, die Aufrufer außerhalb von `filter` nicht kennen
    /// sollen.
    pub fn display_text(&self) -> &str {
        match self {
            Pattern::Exact(s) | Pattern::Glob(s) | Pattern::Regex(s) => s,
        }
    }

    /// Kurzbezeichner der Variante (`"glob"`/`"regex"`/`"exact"`) — für DTOs
    /// außerhalb von `core`, die Typ und Wert getrennt darstellen wollen
    /// (Spec 0009, `RuleInput.pattern_type`).
    pub fn kind_str(&self) -> &'static str {
        match self {
            Pattern::Glob(_) => "glob",
            Pattern::Regex(_) => "regex",
            Pattern::Exact(_) => "exact",
        }
    }
}

/// Spec 0060, Abschnitt 1: Erkennung eines „pfadförmigen" Glob-Musters.
///
/// **Ansatz**: das Muster wird whitespace-getrennt tokenisiert (`cmd` ist an
/// dieser Stelle bereits whitespace-normalisiert, s. [`Pattern::matches`]-
/// Doc-Kommentar — eine einfache `split_whitespace`-Tokenisierung genügt,
/// keine erneute Shell-Quoting-Auflösung nötig). Ein Muster gilt als
/// pfadförmig, wenn **mindestens ein Token ein `/` enthält** — das deckt
/// sowohl absolute (`/var/log/*`) als auch relative Pfade (`./foo/*`,
/// `foo/bar/*`) ab.
///
/// **Grenzfälle, bewusst entschieden**:
/// - Absolute Pfade (`/...`) sind der klare Fall.
/// - Explizit relative Pfade (`./...`, `../...`) werden genauso behandelt —
///   dieselbe `/`-Grenzüberschreitungs-Gefahr besteht dort identisch.
/// - **Bare** relative Pfade ohne führendes `./` (z. B. `foo/bar/*`) werden
///   ebenfalls als pfadförmig behandelt: „Im Zweifel strenger" (Spec 0060,
///   Abschnitt 1) — jedes Token mit `/` sieht potenziell wie ein Dateipfad
///   aus, ein zu strenges Muster matcht im Zweifel nur weniger (Nutzer muss
///   bestätigen), nie mehr.
/// - **Ausnahme, damit der Normalfall nicht bricht** (Spec 0060, „Die
///   Design-Entscheidung"): ein Token mit einem URL-Schema (`irgendwas://`)
///   gilt NICHT als pfadförmig — ein Kommando-Argument-Glob über eine URL
///   (z. B. `curl http://example.com/*`) muss weiterhin `*` über `/`
///   hinweg matchen können, sonst bricht genau der Fall, den die Spec
///   explizit als „Normalfall, der nicht brechen darf" nennt.
/// - Enthält EIN Token im Muster ein `/` (und ist keine URL), wird das
///   GESAMTE Muster als pfadförmig behandelt (`literal_separator` gilt für
///   den kompletten kompilierten Glob, `globset` kennt keine gemischte
///   Pro-Token-Konfiguration innerhalb eines einzelnen Musters). Ein
///   gemischtes Muster wie `wget http://x/* -O /tmp/*` würde dadurch auch
///   im URL-Teil strenger — ein seltener, rein Kommando-Argument-Glob-
///   spezifischer Fall, den „im Zweifel strenger" bewusst in Kauf nimmt
///   (Sicherheit vor Bequemlichkeit).
fn is_path_shaped_pattern(pattern: &str) -> bool {
    pattern
        .split_whitespace()
        .any(|token| token.contains('/') && !token.contains("://"))
}

/// Spec 0060, Abschnitt 2: Zusammenspiel mit dem lexikalischen Pfad-
/// Normalizer.
///
/// Der bestehende SFTP-Traversal-Normalizer
/// (`app_shell::orchestration::normalize_remote_path`) normalisiert genau
/// EIN Feld (den SFTP-Pfad) VOR der Filter-Auswertung — für die beiden
/// SFTP-Pseudo-Kommandos ist `cmd` an dieser Stelle also bereits
/// `.`/`..`-frei, diese Funktion hier ist für diesen Fall ein
/// (idempotenter) No-op, kein Widerspruch.
///
/// Für JEDE andere Shell-Kommandozeile (die nie durch den SFTP-Normalizer
/// läuft) reicht `literal_separator` allein NICHT: `*` überquert zwar kein
/// `/` mehr, aber ein einzelnes `..`-Segment OHNE eingebettetes `/` (z. B.
/// `/var/log/..`) enthält selbst kein `/` und würde von einem einzelnen
/// `*` trotzdem als „ein Segment" akzeptiert — `/var/log/*` würde
/// `/var/log/..` matchen, was lexikalisch `/var` ist, außerhalb des
/// erlaubten Verzeichnisses. Deshalb: **erst normalisieren, dann matchen**
/// (exakt der von Spec 0060 Abschnitt 2 geforderte Ansatz), mit derselben
/// rein lexikalischen `.`/`..`-Auflösung wie der bestehende SFTP-
/// Normalizer (kein Dateisystemzugriff, keine Symlink-Auflösung — dieselbe
/// bewusste Grenze wie dort).
///
/// Normalisiert wird NUR das jeweilige Token, nicht der gesamte `cmd`-Text
/// (der Kommandoname und andere Argumente sind keine Pfadsegmente) — und
/// NUR wenn [`Pattern::matches`] bereits entschieden hat, dass das Muster
/// pfadförmig ist (dieselbe `is_path_shaped_pattern`-Heuristik, hier auf
/// `cmd` statt auf das Muster angewendet — ein Angreifer-kontrolliertes
/// Token mit `/`, das keine URL ist, wird immer normalisiert, unabhängig
/// davon, ob es „zufällig" schon sauber aussieht).
fn normalize_path_shaped_tokens(cmd: &str) -> String {
    cmd.split_whitespace()
        .map(|token| {
            if token.contains('/') && !token.contains("://") {
                normalize_lexical_path(token)
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Löst `.`/`..`-Segmente rein lexikalisch auf und kollabiert wiederholte
/// `/` — bewusst dieselbe Semantik wie
/// `app_shell::orchestration::normalize_remote_path` (Spec 0020, SFTP-
/// Traversal-Entschärfung), hier unabhängig dupliziert: `core` darf laut
/// Architektur-Regel nicht von `app-shell` abhängen (`app-shell` hängt von
/// `core` ab, nie umgekehrt), ein Teilen des Codes ist also nur in dieser
/// Richtung (app-shell → core) möglich, nicht andersherum — nicht Teil
/// dieser Spec (Spec 0060, „Nicht Teil dieser Spec"), hier nur als
/// eigenständige, für die Filter-Engine unabhängig getestete Kopie.
fn normalize_lexical_path(path: &str) -> String {
    let is_absolute = path.starts_with('/');
    let mut stack: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if matches!(stack.last(), Some(&last) if last != "..") {
                    stack.pop();
                } else if !is_absolute {
                    stack.push("..");
                }
                // Absoluter Pfad: `..` über der Wurzel hinaus wird verworfen
                // (kann nicht höher als `/`).
            }
            other => stack.push(other),
        }
    }
    let joined = stack.join("/");
    if is_absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

#[cfg(test)]
mod path_glob_tests {
    use super::*;

    #[test]
    fn test_is_path_shaped_recognizes_absolute_paths() {
        assert!(is_path_shaped_pattern("/var/log/*"));
        assert!(is_path_shaped_pattern("cat /var/log/*"));
    }

    #[test]
    fn test_is_path_shaped_recognizes_explicit_relative_paths() {
        assert!(is_path_shaped_pattern("./foo/*"));
        assert!(is_path_shaped_pattern("cat ../foo/*"));
    }

    #[test]
    fn test_is_path_shaped_recognizes_bare_relative_paths_when_in_doubt() {
        // Spec 0060: "im Zweifel strenger" — auch ein Token ohne führendes
        // `./` gilt als pfadförmig, sobald es ein `/` enthält.
        assert!(is_path_shaped_pattern("cat foo/bar/*"));
    }

    #[test]
    fn test_is_path_shaped_excludes_url_like_tokens() {
        // Der Normalfall, der laut Spec 0060 NICHT brechen darf.
        assert!(!is_path_shaped_pattern("curl http://example.com/*"));
        assert!(!is_path_shaped_pattern("curl https://example.com/api/*"));
    }

    #[test]
    fn test_is_path_shaped_returns_false_for_no_slash_at_all() {
        assert!(!is_path_shaped_pattern("rm -rf *"));
        assert!(!is_path_shaped_pattern("*"));
    }

    #[test]
    fn test_normalize_lexical_path_resolves_traversal_and_redundant_segments() {
        assert_eq!(
            normalize_lexical_path("/var/log/../etc/shadow"),
            "/var/etc/shadow"
        );
        assert_eq!(
            normalize_lexical_path("/var/log/../log/../../etc/passwd"),
            "/etc/passwd"
        );
        assert_eq!(normalize_lexical_path("/var//log/foo"), "/var/log/foo");
        assert_eq!(normalize_lexical_path("/var/log/./x"), "/var/log/x");
        // Einzelnes `..`-Segment OHNE eingebettetes `/` — genau der Fall,
        // den `literal_separator` allein NICHT abfängt (s. Doc-Kommentar
        // oben).
        assert_eq!(normalize_lexical_path("/var/log/.."), "/var");
        // `..` über die Wurzel hinaus wird verworfen, nicht negativ.
        assert_eq!(normalize_lexical_path("/../../etc/shadow"), "/etc/shadow");
    }

    #[test]
    fn test_normalize_path_shaped_tokens_only_touches_path_like_tokens() {
        assert_eq!(
            normalize_path_shaped_tokens("cat /var/log/../etc/shadow"),
            "cat /var/etc/shadow"
        );
        // URL-Token bleibt unverändert (keine Pfad-Normalisierung für
        // Nicht-Dateipfade).
        assert_eq!(
            normalize_path_shaped_tokens("curl http://example.com/../x"),
            "curl http://example.com/../x"
        );
    }
}
