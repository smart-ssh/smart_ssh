use globset::{Glob, GlobBuilder};
use regex::Regex;

use super::types::{Pattern, RuleAction};

/// Ein Muster, das sich nicht übersetzen lässt (Spec 0077, 3.1.1). Trägt den
/// Fehlertext der Bibliothek (`regex`/`globset`) unverändert, inklusive der
/// Stelle im Muster, die die Bibliothek nennt.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PatternError {
    pub message: String,
}

impl PatternError {
    fn from_library(err: impl std::fmt::Display) -> Self {
        Self {
            message: err.to_string(),
        }
    }
}

impl Pattern {
    /// Prüft, ob sich das Muster übersetzen lässt (Spec 0077, 3.1.1).
    ///
    /// Übersetzt **genau die Varianten, die die Auswertung übersetzt** —
    /// [`Pattern::matches`] (`Glob::new` bzw. `Regex::new`) und für ein
    /// pfadförmiges Glob zusätzlich den strengen Zweig aus
    /// [`Pattern::matches_for_user_rule`]. Scheitert einer davon, ist das
    /// Muster ungültig: Beim Speichern wird es abgewiesen, bei der
    /// Auswertung gemeldet (Spec 0077, 3.1.2 und 3.2.2).
    ///
    /// Ändert nichts an der Auswertung selbst — `matches` und
    /// `matches_for_user_rule` behalten ihr `unwrap_or(false)` (Spec 0077,
    /// 3.2.4).
    pub fn validate(&self) -> Result<(), PatternError> {
        match self {
            Pattern::Exact(_) => Ok(()),
            Pattern::Regex(pattern) => Regex::new(pattern)
                .map(|_| ())
                .map_err(PatternError::from_library),
            Pattern::Glob(pattern) => {
                Glob::new(pattern).map_err(PatternError::from_library)?;
                if is_path_shaped_pattern(pattern) {
                    GlobBuilder::new(&normalize_path_shaped_tokens(pattern))
                        .literal_separator(true)
                        .build()
                        .map_err(PatternError::from_library)?;
                }
                Ok(())
            }
        }
    }

    /// Klassifiziert ein ungültiges Muster für das Protokoll aus 3.2.2, ohne
    /// je Nutzertext zurückzugeben (Spec 0077, Klarstellung Q-BL-0249-03).
    ///
    /// `validate()` liefert den Fehlertext von `regex`/`globset` — der
    /// zitiert das Muster wörtlich, was für das Formular (3.1.4) und das DTO
    /// (3.2.3) richtig ist, in einem Protokoll aber eine neue Datensenke
    /// wäre: Ein Muster ist selbst geschriebener Text und kann ein Geheimnis
    /// enthalten (etwa eine Deny-Regel, die auf ein Passwort im Argument
    /// zielt und durch einen Tippfehler ungültig ist). Diese Methode prüft
    /// dieselben Zweige wie `validate()` (bewusst dupliziert statt aus dem
    /// `PatternError` abgeleitet, damit ein Bibliothekstext nie auch nur
    /// mittelbar hierher gelangen kann) und gibt nur einen von drei festen,
    /// textfreien Kurztexten zurück.
    pub(super) fn compile_failure_reason(&self) -> Option<&'static str> {
        match self {
            Pattern::Exact(_) => None,
            Pattern::Regex(pattern) => {
                if Regex::new(pattern).is_err() {
                    Some("regex does not compile")
                } else {
                    None
                }
            }
            Pattern::Glob(pattern) => {
                if Glob::new(pattern).is_err() {
                    Some("glob does not compile")
                } else if is_path_shaped_pattern(pattern)
                    && GlobBuilder::new(&normalize_path_shaped_tokens(pattern))
                        .literal_separator(true)
                        .build()
                        .is_err()
                {
                    Some("glob does not compile (strict branch)")
                } else {
                    None
                }
            }
        }
    }

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
    ///
    /// **spec-reviewer-Fund (ERHÖHT + adversarial, Review dieses Schritts)**:
    /// die rein lexikalische Segment-Auflösung in
    /// [`normalize_path_shaped_tokens`] vergleicht rohe Textsegmente gegen
    /// das Literal `".."` — jede Shell-Schreibweise, die *nach* der
    /// Shell-Expansion `..` ergibt (`\..`, `".."`, `'..'`, `{..,..}`,
    /// `.[.]`, …), übersteht diesen Vergleich unverändert und hebelt die
    /// Normalisierung vollständig aus, während der eigentliche Remote-Shell-
    /// Aufruf sie sehr wohl als `..` interpretiert — ein einziger
    /// Backslash reichte, um wieder `AutoExec` für einen `../`-Ausbruch zu
    /// bekommen (empirisch gegen die echte `FilterEngine` verifiziert).
    /// Eine vollständige, rein lexikalische Nachbildung jeder möglichen
    /// Shell-Expansion ist nicht erreichbar (Variablen-Substitution ist
    /// grundsätzlich nicht lexikalisch auflösbar). Deshalb, „im Zweifel
    /// strenger": enthält ein pfadförmiges Token in `cmd`
    /// Shell-Metazeichen, die Quoting/Escaping/Brace-/Bracket-Expansion
    /// auslösen könnten (s. [`path_shaped_tokens_contain_shell_metacharacters`]),
    /// gilt die strengere Prüfung als NICHT erfüllt — für `Allow` bedeutet
    /// das: kein Match (fällt sicher auf `Confirm`/niedrigere Präzedenz
    /// zurück, nie `AutoExec`). Für `Deny`/`Confirm` gilt zusätzlich (auch
    /// unabhängig von Shell-Metazeichen) `self.matches(cmd)` als
    /// Fallback/Oder-Verknüpfung — das alte, permissive Matching
    /// (`*` überquert `/`) erkennt weiterhin JEDEN Fall, den es vor Spec
    /// 0060 erkannt hätte, sodass eine bestehende Deny-Regel durch diesen
    /// Fix NIE schwächer wird (nur zusätzlich durch die neue Normalisierung
    /// verstärkt) — schließt den spec-reviewer-Fund, dass ein bestehendes
    /// `Deny: rm /home/u/*` nach dem Fix `rm /home/u/sub/file` nicht mehr
    /// gedeckt hätte.
    pub(crate) fn matches_for_user_rule(&self, cmd: &str, action: &RuleAction) -> bool {
        match self {
            Pattern::Glob(pattern) if is_path_shaped_pattern(pattern) => {
                let strict_match = if path_shaped_tokens_contain_shell_metacharacters(cmd) {
                    false
                } else {
                    // spec-reviewer-Fund: nur `cmd` zu normalisieren, nicht
                    // das MUSTER selbst, brach ein relatives Muster wie
                    // `./foo/*` (matchte `./foo/x` nicht mehr, weil `cmd`
                    // zu `foo/x` normalisiert wurde, das Muster aber
                    // `./foo/*` blieb) — und ließ einen Muster-Nachlaufslash
                    // (`/var/log/*/`) uneinheitlich zu `cmd`s entferntem
                    // Nachlaufslash stehen. Dieselbe Normalisierung auf
                    // beiden Seiten hält beide symmetrisch: `*`/`**` sind
                    // für `normalize_lexical_path` nur opake Segmente
                    // (weder `""`, `"."` noch `".."`), bleiben also
                    // unverändert erhalten.
                    let normalized_pattern = normalize_path_shaped_tokens(pattern);
                    let normalized_cmd = normalize_path_shaped_tokens(cmd);
                    GlobBuilder::new(&normalized_pattern)
                        .literal_separator(true)
                        .build()
                        .map(|glob| glob.compile_matcher().is_match(&normalized_cmd))
                        .unwrap_or(false)
                };
                match action {
                    RuleAction::Allow => strict_match,
                    RuleAction::Deny | RuleAction::Confirm => strict_match || self.matches(cmd),
                }
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
/// NUR wenn [`Pattern::matches_for_user_rule`] bereits entschieden hat,
/// dass das MUSTER pfadförmig ist.
///
/// **spec-reviewer-Fund (Review dieses Schritts)**: anders als bei der
/// Muster-Klassifizierung (`is_path_shaped_pattern`, wo die URL-Ausnahme
/// nötig ist, damit `curl http://example.com/*` nicht bricht) wird hier
/// bewusst JEDES Token mit `/` normalisiert, UNABHÄNGIG davon, ob es
/// `://` enthält. Ein `://`-Ausschluss auf der Kommando-Seite hätte eine
/// eigene Lücke geöffnet: ein Token wie `/tmp/x://../../../etc/shadow`
/// (z. B. durch ein zuvor angelegtes Verzeichnis `x:` im erlaubten Baum)
/// enthält zufällig `://`, ist aber kein echtes URL-Argument — der
/// `://`-Ausschluss hätte die `..`-Auflösung für genau dieses Token
/// übersprungen und einen `**`-Ausbruch ermöglicht (empirisch gegen die
/// echte `FilterEngine` verifiziert). Ein Angreifer-kontrolliertes Token
/// mit `/` wird deshalb immer normalisiert, unabhängig vom Inhalt.
fn normalize_path_shaped_tokens(cmd: &str) -> String {
    cmd.split_whitespace()
        .map(|token| {
            if token.contains('/') {
                normalize_lexical_path(token)
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// spec-reviewer-Fund (ERHÖHT + adversarial, Review dieses Schritts): s.
/// Doc-Kommentar bei [`Pattern::matches_for_user_rule`] — die rein
/// lexikalische `..`-Erkennung in [`normalize_lexical_path`] vergleicht
/// nur rohe Textsegmente und kann durch jede Shell-Schreibweise umgangen
/// werden, die *nach* der Shell-Expansion `..` ergibt (Backslash-Escape,
/// einfache/doppelte Anführungszeichen, Brace-Expansion, Bracket-
/// Pathname-Expansion). Statt jede dieser Schreibweisen einzeln
/// nachzubilden (prinzipiell unvollständig — Variablen-Substitution ist
/// gar nicht lexikalisch auflösbar), gilt „im Zweifel strenger": jedes
/// dieser klassischen Shell-Metazeichen in einem pfadförmigen Token macht
/// das Ergebnis der Normalisierung nicht mehr vertrauenswürdig.
fn path_shaped_tokens_contain_shell_metacharacters(cmd: &str) -> bool {
    const SHELL_METACHARACTERS: [char; 9] = ['\\', '\'', '"', '{', '}', '[', ']', '$', '`'];
    cmd.split_whitespace()
        .any(|token| token.contains('/') && token.contains(SHELL_METACHARACTERS.as_slice()))
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

    // --- Spec 0077, 3.1.1: `Pattern::validate` ---------------------------

    /// Spec 0077, T-5 (core-Teil): gültige Muster werden weiter angenommen —
    /// fängt eine zu strenge Prüfung ab.
    #[test]
    fn test_spec_0077_t5_validate_accepts_valid_patterns() {
        for pattern in [
            Pattern::Glob("*".to_string()),
            Pattern::Glob("**".to_string()),
            Pattern::Glob("systemctl *".to_string()),
            Pattern::Glob("ls [abc]*".to_string()),
            Pattern::Glob("cat {a,b}.log".to_string()),
            Pattern::Glob("cat /var/log/**".to_string()),
            Pattern::Glob("cat /var/log/*".to_string()),
            Pattern::Glob("cat ./foo/*".to_string()),
            Pattern::Glob("rm /x/[a/../b]".to_string()),
            Pattern::Glob("curl http://example.com/*".to_string()),
            Pattern::Glob("echo grüße *".to_string()),
            Pattern::Regex("^systemctl stop .*$".to_string()),
            Pattern::Regex(r"^(?:ls|cat)\s+(\S+)$".to_string()),
            Pattern::Regex("^echo [äöü]+$".to_string()),
            Pattern::Regex("a{3}".to_string()),
            Pattern::Exact("ls -la".to_string()),
            Pattern::Exact("[(".to_string()),
        ] {
            assert_eq!(pattern.validate(), Ok(()), "{pattern:?}");
        }
    }

    /// Spec 0077, 3.1.1: Syntaxfehler in Regex und Glob werden gemeldet, mit
    /// dem Fehlertext der Bibliothek.
    ///
    /// Das ungültige Regex-Literal wird aus Teilstücken zusammengesetzt:
    /// `clippy::invalid_regex` erkennt ein ungültiges Literal in
    /// `Regex::new` und bricht den Lint-Lauf ab — hier ist die Ungültigkeit
    /// aber genau der Prüfgegenstand.
    #[test]
    fn test_spec_0077_validate_rejects_invalid_regex_and_glob() {
        let invalid_regex = ["^systemctl stop ", "(", ".*"].concat();
        let err = Pattern::Regex(invalid_regex.clone())
            .validate()
            .unwrap_err();
        assert_eq!(
            err.message,
            Regex::new(&invalid_regex).unwrap_err().to_string()
        );
        let err = Pattern::Glob("systemctl [stop".to_string())
            .validate()
            .unwrap_err();
        assert_eq!(
            err.message,
            Glob::new("systemctl [stop").unwrap_err().to_string()
        );
    }

    /// Spec 0077, T-A9 (core-Teil): Ein Regex über dem Größenlimit der
    /// `regex`-Voreinstellung gilt wie ein Syntaxfehler.
    #[test]
    fn test_spec_0077_validate_rejects_regex_over_size_limit() {
        let err = Pattern::Regex("a{1000}{1000}".to_string())
            .validate()
            .unwrap_err();
        assert!(err.message.contains("size limit"), "{}", err.message);
    }

    /// Baut den strengen Zweig aus 3.1.1 genau so wie
    /// [`Pattern::validate`] und [`Pattern::matches_for_user_rule`].
    fn strict_branch_compiles(pattern: &str) -> bool {
        GlobBuilder::new(&normalize_path_shaped_tokens(pattern))
            .literal_separator(true)
            .build()
            .is_ok()
    }

    /// Spec 0077, T-4 (core-Teil): Ein pfadförmiger Glob, bei dem nur
    /// **einer** der beiden Zweige nicht übersetzt, ist ungültig — in
    /// **beiden** Richtungen (§1, Tabelle „Gemessen, zweiter Befund";
    /// Klarstellung Q-BL-0249-01).
    ///
    /// Beide Richtungen zusammen belegen, dass `validate` wirklich beide
    /// Zweige baut: Eine Fassung, die nur den strengen Zweig prüft, käme
    /// durch die erste Zeile, eine, die nur `Glob::new` prüft, durch die
    /// zweite. Welcher Zweig übersetzt, hält der Test als Zusicherung fest
    /// — sonst würde er still zu einem Test über ein durchweg ungültiges
    /// Muster, falls sich `globset` einmal anders verhält.
    #[test]
    fn test_spec_0077_t4_validate_rejects_globs_failing_in_only_one_branch() {
        // (Muster, übersetzt mit `Glob::new`, übersetzt im strengen Zweig)
        for (pattern, permissive_ok, strict_ok) in [
            // `..` löscht `b]`, die öffnende `[` bleibt ohne Gegenstück:
            // normalisiert `rm /x/[a/c`.
            ("rm /x/[a/b]/../c", true, false),
            // Umgekehrt: dieselbe Auflösung repariert das Muster zu
            // `rm /x/b`, das `Glob::new` roh nicht übersetzt.
            ("rm /x/[a/../b", false, true),
        ] {
            assert_eq!(
                Glob::new(pattern).is_ok(),
                permissive_ok,
                "Zusicherung permissiver Zweig: {pattern}"
            );
            assert_eq!(
                strict_branch_compiles(pattern),
                strict_ok,
                "Zusicherung strenger Zweig: {pattern}"
            );
            assert!(
                Pattern::Glob(pattern.to_string()).validate().is_err(),
                "muss abgewiesen werden: {pattern}"
            );
        }
    }

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
        assert_eq!(normalize_path_shaped_tokens("cat foo bar"), "cat foo bar");
    }

    /// spec-reviewer-Fund (ERHÖHT + adversarial, Review dieses Schritts):
    /// anders als die Muster-Klassifizierung wendet die Kommando-Seiten-
    /// Normalisierung KEINE URL-Ausnahme an — ein Token mit zufällig
    /// eingebettetem `://` (z. B. ein Verzeichnis namens `x:` im erlaubten
    /// Baum) wird trotzdem lexikalisch aufgelöst, sonst könnte ein
    /// `..`-Ausbruch über genau dieses Token die Normalisierung umgehen.
    #[test]
    fn test_normalize_path_shaped_tokens_normalizes_tokens_with_embedded_url_syntax_too() {
        assert_eq!(
            normalize_path_shaped_tokens("cat /tmp/x://../../../etc/shadow"),
            "cat /etc/shadow"
        );
    }
}
