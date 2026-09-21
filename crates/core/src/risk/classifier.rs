use crate::filter::{
    resolve_effective_command, segment_command, Pattern, DEFAULT_MAX_COMMAND_LENGTH,
};

use super::patterns::{
    data_risk_patterns, secret_path_patterns, server_risk_patterns, SECRET_FILE_CANDIDATES,
    SECRET_PATH_HINTS, SECRET_READ_COMMANDS, SECRET_RELATIVE_PREFIXES,
};
use super::types::{RiskAssessment, RiskClassifier, RiskLevel};

/// Regelbasierte Umsetzung von [`RiskClassifier`] (Spec 0026, Abschnitt 2) —
/// zustandslos, hält keine eigenen Daten (die Musterlisten sind
/// modulweite `OnceLock`s, s. `patterns.rs`), deshalb kein Feld nötig.
#[derive(Debug, Default, Clone, Copy)]
pub struct RuleBasedRiskClassifier;

impl RiskClassifier for RuleBasedRiskClassifier {
    /// Zerlegt `command` mit derselben Logik wie die Filter-Engine
    /// (`crate::filter::segment_command`, Spec 0026 Abschnitt 2: "nutzt
    /// exakt dieselbe Logik wie die Filter-Engine"), klassifiziert jedes
    /// Teilkommando einzeln gegen beide Musterlisten und behält je Achse
    /// das höchste gefundene Level.
    fn classify(&self, command: &str) -> RiskAssessment {
        // Unabhängiger Review-Pass (Spec 0026): `segment_command` rekursiert
        // pro `$(...)`-Verschachtelungsebene ohne jede Tiefen-/Längenschranke
        // (anders als die Filter-Engine, die `DEFAULT_MAX_COMMAND_LENGTH`
        // bereits VOR jedem Parsing prüft, `filter::engine`) — ein
        // KI-vorgeschlagenes Kommando mit tausenden verschachtelten `$(`
        // bringt sonst den GESAMTEN Prozess per Stack-Overflow zum Absturz
        // (empirisch verifiziert), nicht nur die Session, noch bevor
        // überhaupt ein Bestätigungsdialog erscheint — erreichbar über einen
        // kompromittierten/prompt-injizierten KI-Provider oder MCP. Dieselbe
        // Schranke wie die Filter-Engine reicht laut Messung sicher aus; die
        // eigentliche Policy-Entscheidung für zu lange Kommandos trifft
        // ohnehin bereits die Filter-Engine (`FILTER_COMMAND_TOO_LONG`) —
        // hier zählt nur "nicht abstürzen", ein unklassifiziertes Ergebnis
        // ist ein akzeptabler Fail-safe.
        if command.len() > DEFAULT_MAX_COMMAND_LENGTH {
            return RiskAssessment {
                server_risk: RiskLevel::None,
                server_risk_reason: None,
                data_risk: RiskLevel::None,
                data_risk_reason: None,
                ai_reviewed: false,
            };
        }

        let mut segments = segment_command(command);
        // Zusätzlich das unzerlegte Gesamtkommando prüfen: `scan_top_level_
        // segments` (`filter::parser`) verfolgt nur `(`/`)`, keine `{`/`}` —
        // ein Muster wie die klassische Fork-Bombe `:(){ :|:& };:`, dessen
        // `|`/`;` innerhalb der `{}`-Klammern liegen, wird deshalb an
        // genau diesen Zeichen mit-aufgetrennt, obwohl es fachlich ein
        // einzelnes Kommando ist. Ein per-Segment-Match allein würde ein
        // extra/exakt formuliertes Muster für so einen Fall daher nie
        // treffen; das volle Kommando zusätzlich zu prüfen fängt das aber
        // ohne eine (hier nicht gewollte) Änderung an `filter::parser`
        // selbst auf.
        segments.push(command.to_lowercase());
        // Unabhängiger Review-Pass (Spec 0026): ohne dieselbe Normalisierung,
        // die die Filter-Engine für ihre eigene Hard-Blacklist anwendet
        // (`resolve_effective_command` — wiederholtes Entfernen von
        // `sudo`/`doas`/Wrapper-Präfixen und Variablen-Zuweisungen), sind
        // fast alle Risiko-Muster durch ein vorangestelltes `sudo`/`env`/
        // `bash -c` wirkungslos, weil sie am Anfang verankert sind (z. B.
        // die Daten-Risiko-Regexes `^(?:cat|less|head|tail|...)`,
        // Server-Risiko-Muster wie `shutdown*`/`kill *`) — empirisch
        // verifiziert: `sudo cat /etc/shadow` klassifizierte zuvor als "kein
        // Risiko" statt "Daten Rot", obwohl genau das die praktisch
        // häufigere UND gefährlichere Form ist. Zusätzlich zum rohen Segment
        // auch die aufgelöste Form prüfen — dasselbe Dual-Text-Muster wie
        // ADR 0002, hier für die Risiko-Einschätzung statt für Regeln.
        let resolved: Vec<String> = segments
            .iter()
            .map(|segment| resolve_effective_command(segment))
            .collect();
        segments.extend(resolved);

        let mut server_risk = RiskLevel::None;
        let mut server_risk_reason: Option<&'static str> = None;
        let mut data_risk = RiskLevel::None;
        let mut data_risk_reason: Option<&'static str> = None;

        for segment in &segments {
            // Wie die Hard-Blacklist der Filter-Engine (s.
            // `filter::blacklist`-Modul-Kommentar) case-insensitiv über
            // Lowercasing statt eines `case_insensitive`-Glob-Builders —
            // dieselbe, bereits etablierte Konvention.
            let lower = segment.to_lowercase();

            if let Some((level, reason)) = best_match(server_risk_patterns(), &lower) {
                if level > server_risk {
                    server_risk = level;
                    server_risk_reason = Some(reason);
                }
            }
            if let Some((level, reason)) = best_match(data_risk_patterns(), &lower) {
                if level > data_risk {
                    data_risk = level;
                    data_risk_reason = Some(reason);
                }
            }
        }

        RiskAssessment {
            server_risk,
            server_risk_reason: server_risk_reason.map(str::to_string),
            data_risk,
            data_risk_reason: data_risk_reason.map(str::to_string),
            ai_reviewed: false,
        }
    }
}

/// Höchstes unter allen zutreffenden Mustern — ein Teilkommando kann
/// gleichzeitig ein Rot- und ein Gelb-Muster treffen (z. B. `rm -rf
/// /etc/shadow` träfe sowohl "rm -rf" als auch "shadow"), das strengere
/// Ergebnis soll gewinnen, nicht das zuerst in der Liste stehende.
fn best_match(
    patterns: &[(Pattern, RiskLevel, &'static str)],
    lower_segment: &str,
) -> Option<(RiskLevel, &'static str)> {
    patterns
        .iter()
        .filter(|(pattern, _, _)| pattern.matches(lower_segment))
        .map(|(_, level, reason)| (*level, *reason))
        .max_by_key(|(level, _)| *level)
}

/// Spec 0068, Teil 2: liefert eine Begründung, wenn `command` den Inhalt
/// eines Secret-Pfads in die Ausgabe bringt (oder vom Server wegträgt). Der
/// Aufrufer (`app-shell::orchestration::handle_action_proposed`) macht aus
/// `AutoExec` dann **immer** `Confirm` — auch wenn eine Allow-Regel greift
/// —, reine Eskalation wie bei Spec 0039. Eine `Deny`-Entscheidung bleibt
/// unberührt.
///
/// **Nie schwächer als die erste Fassung** (zweite Review-Runde, ERHÖHT):
/// die erste Fassung ([`first_version_secret_read_reason`]) läuft
/// unverändert weiter; die erweiterte Prüfung
/// ([`extended_secret_read_reason`]) kommt nur hinzu. Eine Umstellung der
/// erweiterten Prüfung kann so nie etwas durchlassen, das vorher
/// eskaliert wurde.
pub fn secret_path_read_reason(command: &str) -> Option<&'static str> {
    first_version_secret_read_reason(command).or_else(|| extended_secret_read_reason(command))
}

/// Erste Fassung (Commit `8817a6f`), wörtlich: Lesebefehl am Anfang eines
/// Teilkommandos + Secret-Pfad, `-exec`/`xargs` mit Secret-Hinweis,
/// Platzhalter auf Punktdateien/Secret-Hinweise.
fn first_version_secret_read_reason(command: &str) -> Option<&'static str> {
    if command.len() > DEFAULT_MAX_COMMAND_LENGTH {
        return Some("Kommando zu lang für eine Prüfung auf Secret-Pfade");
    }

    let mut segments = segment_command(command);
    segments.push(command.to_string());
    let resolved: Vec<String> = segments
        .iter()
        .map(|segment| resolve_effective_command(segment))
        .collect();
    segments.extend(resolved);

    let read_start = regex::Regex::new(&format!(r"^\s*{SECRET_READ_COMMANDS}\b"))
        .expect("eingebautes Lesebefehl-Muster ist gültig");
    let exec_read = regex::Regex::new(&format!(
        r"(?:-exec|-execdir|xargs)(?:\s+-\S+)*\s+(?:sudo\s+)?{SECRET_READ_COMMANDS}\b"
    ))
    .expect("eingebautes exec-/xargs-Muster ist gültig");

    let normalize = |text: &str| -> String {
        text.to_lowercase()
            .chars()
            .filter(|c| !matches!(c, '\'' | '"' | '\\'))
            .collect()
    };
    let full = normalize(command);

    for segment in &segments {
        let normalized = normalize(segment);
        if exec_read.is_match(&normalized) && SECRET_PATH_HINTS.iter().any(|h| full.contains(h)) {
            return Some("Liest Dateien per -exec/xargs aus einem Secret-Pfad");
        }
        if !read_start.is_match(&normalized) {
            continue;
        }
        if let Some((_, reason)) = secret_path_patterns()
            .iter()
            .find(|(pattern, _)| pattern.is_match(&normalized))
        {
            return Some(reason);
        }
        let globbed_secret = normalized.split_whitespace().skip(1).any(|arg| {
            let has_glob = arg.contains(['*', '?', '[', '{']);
            let last = arg.rsplit('/').next().unwrap_or(arg);
            has_glob
                && (last.starts_with('.')
                    || arg.starts_with('~')
                    || arg.starts_with("$home")
                    || arg.starts_with("/root")
                    || SECRET_PATH_HINTS.iter().any(|h| arg.contains(h)))
        });
        if globbed_secret {
            return Some("Lesebefehl mit Platzhalter auf einen möglichen Secret-Pfad");
        }
    }
    None
}

/// Erweiterte Prüfung (spec-reviewer-Funde, ERHÖHT). Pro Teilkommando
/// (dieselbe Zerlegung wie die erste Fassung), auf quote-bewusst zerlegten
/// Wörtern mit lexikalisch normalisierten Pfaden (`//`, `/./`, `x/../`).
/// Eskaliert, wenn:
/// - ein Lesebefehl an **beliebiger** Stelle steht (hinter `docker exec`,
///   `sudo -iu`, `timeout -s`, als `/bin/cat`, `(cat …)`) und ein
///   Secret-Pfad vorkommt;
/// - ein Secret-Pfad (auch per Platzhalter) per `<` umgeleitet oder nach
///   `/dev/stdout`, `/dev/fd/…`, `/dev/pts/…`, `/proc/…/fd/…`, `/dev/tty`
///   geschrieben wird — bei jedem Befehl (`cp ~/.ssh/id_rsa /dev/stdout`);
/// - massenhaft gelesen wird (rekursives `grep` auch mit abgekürzten
///   Langoptionen, `rg`/`ag`/`ack`/`rgrep`, `-exec`/`xargs`, `xargs -a`);
/// - ein Wort eine nicht in einfachen Quotes stehende Variable oder
///   Kommando-Substitution enthält, oder ein `cd`-Ziel nicht prüfbar ist;
/// - ein **ungequoteter** Platzhalter im Verzeichnisteil steht oder den
///   letzten Pfadteil auf einen bekannten Secret-Dateinamen abbilden kann
///   (gequotete Platzhalter expandiert die Shell nicht — `sed 's/a.*/b/'`
///   bleibt unbehelligt);
/// - ein relativer Pfad zusammen mit einem `cd`-Ziel einen Secret-Pfad
///   ergibt (`cd /etc && cat shadow`);
/// - das Kommando zu lang für die Prüfung ist.
///
/// **Grenzen (ehrlich, bewusst)**: rein lexikalisch und pro Aktion — ein
/// Arbeitsverzeichnis aus einer früheren Aktion ist unbekannt. Nicht
/// erkannt werden Symlinks (`ln -s ~/.ssh/id_rsa x; cat x`), Programme
/// außerhalb der Liste (Skriptsprachen mit Dateinamen, `tar`), Kodierungen
/// und Umwege über eine zuvor unverdächtig kopierte Datei. Inline-Code
/// (`python -c`, `bash -c`, `$(…)`) setzt die Filter-Engine ohnehin auf
/// Bestätigung; Redaction bleibt die weitere Schicht.
fn extended_secret_read_reason(command: &str) -> Option<&'static str> {
    if command.len() > DEFAULT_MAX_COMMAND_LENGTH {
        return Some("Kommando zu lang für eine Prüfung auf Secret-Pfade");
    }

    let mut segments = segment_command(command);
    segments.push(command.to_string());
    let resolved: Vec<String> = segments
        .iter()
        .map(|segment| resolve_effective_command(segment))
        .collect();
    segments.extend(resolved);

    let cd_prefixes = cd_prefixes(command);

    for segment in &segments {
        let lower = segment.to_lowercase();
        let stripped = strip_quotes(&lower);
        let concrete =
            secret_path_match(&normalize_path(&stripped)).or_else(|| secret_path_match(&stripped));
        let words = shell_words(&lower);

        let globbed = cd_prefixes.as_deref().and_then(|prefixes| {
            words.iter().find_map(|word| {
                word.unquoted_glob
                    .then(|| glob_may_hit_secret(word_value(&word.text), prefixes))
                    .flatten()
            })
        });
        if let Some(reason) = concrete.or(globbed) {
            if lower.contains('<')
                || STDOUT_TARGETS
                    .iter()
                    .any(|target| stripped.contains(target))
            {
                return Some(reason);
            }
        }
        if reads_file_via_xargs(&words) {
            return Some("xargs liest Argumente aus einer Datei – Inhalt nicht vorab prüfbar");
        }

        let Some(reader) = words.iter().position(|word| is_read_command(&word.text)) else {
            continue;
        };
        if let Some(reason) = concrete {
            return Some(reason);
        }
        if is_bulk_read(&words, reader) {
            return Some(
                "Liest Dateien rekursiv bzw. per -exec/xargs – Inhalt nicht vorab prüfbar",
            );
        }
        let Some(cd_prefixes) = cd_prefixes.as_deref() else {
            return Some("Lesebefehl nach Verzeichniswechsel in ein nicht prüfbares Ziel");
        };
        for (index, word) in words.iter().enumerate() {
            if index == reader {
                continue;
            }
            if word.expands || word.text.contains('`') {
                return Some("Lesebefehl mit Variable im Pfad – Ziel nicht prüfbar");
            }
            if word.text.starts_with('-') && !word.text.contains('=') {
                continue;
            }
            let arg = word_value(&word.text);
            if arg.is_empty() {
                continue;
            }
            if word.unquoted_glob {
                if let Some(reason) = glob_may_hit_secret(arg, cd_prefixes) {
                    return Some(reason);
                }
            }
            if !arg.starts_with(['/', '~']) {
                for prefix in cd_prefixes {
                    let joined = normalize_path(&format!("{prefix}{arg}"));
                    if let Some(reason) = secret_path_match(&joined) {
                        return Some(reason);
                    }
                }
            }
        }
    }
    None
}

/// Ausgabe-Ziele, über die auch ein reines Kopieren den Inhalt ausgibt.
const STDOUT_TARGETS: &[&str] = &[
    "/dev/stdout",
    "/dev/stderr",
    "/dev/fd/",
    "/dev/pts/",
    "/dev/tty",
    "/proc/",
];

/// Ein Shell-Wort nach Quote-Auflösung (kleingeschrieben).
#[derive(Default)]
struct ShellWord {
    text: String,
    /// Ein Platzhalter außerhalb von Quotes — nur dann expandiert die Shell.
    unquoted_glob: bool,
    /// `$` außerhalb einfacher Quotes — Variable/Substitution.
    expands: bool,
}

/// Zerlegt ein (kleingeschriebenes) Teilkommando in Wörter wie die Shell:
/// Quotes und Backslashes werden aufgelöst, Trenner sind ungequotete
/// Leerzeichen und `< > | ; & ( )`.
fn shell_words(segment: &str) -> Vec<ShellWord> {
    let mut words = Vec::new();
    let mut current = ShellWord::default();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = segment.chars();
    while let Some(c) = chars.next() {
        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    current.text.push(c);
                }
            }
            Some(_) => match c {
                '"' => quote = None,
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.text.push(next);
                    }
                }
                _ => {
                    if c == '$' || c == '`' {
                        current.expands = true;
                    }
                    current.text.push(c);
                }
            },
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    in_word = true;
                }
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.text.push(next);
                    }
                    in_word = true;
                }
                c if c.is_whitespace() || matches!(c, '<' | '>' | '|' | ';' | '&' | '(' | ')') => {
                    if in_word {
                        words.push(std::mem::take(&mut current));
                        in_word = false;
                    }
                }
                _ => {
                    if matches!(c, '*' | '?' | '[' | '{') {
                        current.unquoted_glob = true;
                    }
                    if c == '$' || c == '`' {
                        current.expands = true;
                    }
                    current.text.push(c);
                    in_word = true;
                }
            },
        }
    }
    if in_word {
        words.push(current);
    }
    words
}

/// Wert eines Worts: `--opt=wert`/`if=wert` → `wert`, Pfad normalisiert.
fn word_value(text: &str) -> &str {
    text.split_once('=').map_or(text, |(_, value)| value)
}

fn strip_quotes(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\'' | '"' | '\\'))
        .collect()
}

/// `//` → `/`, `/./` → `/`, `x/../` → `` (wiederholt, rein lexikalisch).
/// Pfadteile enthalten keine Shell-Trenner (`credentials</../dev/null`
/// darf `credentials` nicht wegkürzen — zweite Review-Runde).
fn normalize_path(text: &str) -> String {
    static PARENT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let parent = PARENT.get_or_init(|| {
        regex::Regex::new(r"(?P<keep>^|/)(?P<name>[^/\s<>|&;()]+)/\.\.(?:/|$)")
            .expect("eingebautes Pfad-Muster ist gültig")
    });
    let mut current = text.to_string();
    loop {
        let mut next = current.replace("//", "/").replace("/./", "/");
        if let Some(caps) = parent
            .captures_iter(&next)
            .find(|caps| &caps["name"] != "..")
        {
            let whole = caps.get(0).expect("Gesamttreffer existiert");
            let keep = caps["keep"].to_string();
            next = format!("{}{keep}{}", &next[..whole.start()], &next[whole.end()..]);
        }
        if next == current {
            return current;
        }
        current = next;
    }
}

/// Konkreter Secret-Pfad im Text (Musterliste + Dateien in `~/.ssh`).
fn secret_path_match(text: &str) -> Option<&'static str> {
    if let Some((_, reason)) = secret_path_patterns()
        .iter()
        .find(|(pattern, _)| pattern.is_match(text))
    {
        return Some(reason);
    }
    // Private Schlüssel in `~/.ssh` heißen nicht immer `id_*` (`deploy_key`,
    // `github_ed25519`) — jede Datei dort außer den bekannt öffentlichen.
    text.match_indices(".ssh/").find_map(|(index, _)| {
        let rest = &text[index + ".ssh/".len()..];
        let name: String = rest
            .chars()
            .take_while(|c| {
                !c.is_whitespace() && !matches!(c, ';' | '|' | '&' | ')' | '<' | '>' | '\'' | '"')
            })
            .collect();
        let harmless = name.is_empty()
            || name.contains(['*', '?', '[', '{'])
            || name.ends_with(".pub")
            || matches!(
                name.as_str(),
                "known_hosts"
                    | "known_hosts.old"
                    | "authorized_keys"
                    | "authorized_keys2"
                    | "config"
            );
        (!harmless).then_some("Liest eine Datei aus ~/.ssh (möglicher privater Schlüssel)")
    })
}

fn is_read_command(word: &str) -> bool {
    static READ: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let read = READ.get_or_init(|| {
        regex::Regex::new(&format!(r"^{SECRET_READ_COMMANDS}$"))
            .expect("eingebautes Lesebefehl-Muster ist gültig")
    });
    let trimmed = word.trim_start_matches(['{', '!']);
    let name = trimmed.rsplit('/').next().unwrap_or(trimmed);
    read.is_match(name)
}

/// `xargs -a datei` / `--arg-file=datei` liest eine Datei.
fn reads_file_via_xargs(words: &[ShellWord]) -> bool {
    words
        .iter()
        .any(|word| word.text.rsplit('/').next() == Some("xargs"))
        && words.iter().any(|word| {
            word.text == "-a"
                || word.text.starts_with("--arg")
                || (word.text.starts_with("-a") && !word.text.starts_with("--"))
        })
}

/// Rekursives oder per `-exec`/`xargs` verteiltes Lesen.
fn is_bulk_read(words: &[ShellWord], reader: usize) -> bool {
    if words.iter().any(|word| {
        matches!(
            word.text.rsplit('/').next(),
            Some("xargs" | "-exec" | "-execdir" | "-ok" | "-okdir")
        )
    }) {
        return true;
    }
    let text = &words[reader].text;
    let name = text.rsplit('/').next().unwrap_or(text);
    if matches!(name, "rg" | "ag" | "ack" | "rgrep") {
        return true;
    }
    matches!(name, "grep" | "egrep" | "fgrep" | "zgrep" | "ugrep")
        && words[reader + 1..]
            .iter()
            .enumerate()
            .any(|(offset, word)| {
                let token = word.text.as_str();
                // getopt akzeptiert eindeutige Präfixe (`--rec`, `--dir=rec`,
                // `--deref`) — im Zweifel als rekursiv werten.
                token.starts_with("--rec")
                    || token.starts_with("--der")
                    || token.starts_with("--dir")
                    || (token == "-d"
                        && words
                            .get(reader + 2 + offset)
                            .is_some_and(|next| next.text.starts_with("rec")))
                    || (token.starts_with('-')
                        && !token.starts_with("--")
                        && token[1..].chars().all(|c| c.is_ascii_alphanumeric())
                        && token.contains('r'))
            })
}

/// Platzhalter in einem Pfad: im Verzeichnisteil immer verdächtig; im
/// letzten Teil, wenn er einen bekannten Secret-Dateinamen trifft.
fn glob_may_hit_secret(arg: &str, cd_prefixes: &[String]) -> Option<&'static str> {
    const GLOB: [char; 4] = ['*', '?', '[', '{'];
    if !arg.contains(GLOB) {
        return None;
    }
    let arg_owned = normalize_path(arg);
    let arg = arg_owned.as_str();
    let (dir, last) = match arg.rsplit_once('/') {
        Some((dir, last)) => (Some(dir), last),
        None => (None, arg),
    };
    if glob_has_secret_hint(arg) {
        return Some("Lesebefehl mit Platzhalter auf einen möglichen Secret-Pfad");
    }
    if dir.is_some_and(|dir| dir.contains(GLOB)) {
        return Some("Lesebefehl mit Platzhalter im Verzeichnispfad");
    }
    let Some(last_regex) = glob_to_regex(last) else {
        return Some("Lesebefehl mit nicht prüfbarem Platzhalter");
    };
    let dir_part = dir.map(|dir| format!("{dir}/")).unwrap_or_default();
    let prefixes: Vec<String> = if arg.starts_with('/') {
        vec![dir_part]
    } else {
        cd_prefixes
            .iter()
            .map(String::as_str)
            .chain(SECRET_RELATIVE_PREFIXES.iter().copied())
            .map(|prefix| format!("{prefix}{dir_part}"))
            .collect()
    };
    SECRET_FILE_CANDIDATES
        .iter()
        .filter(|candidate| last_regex.is_match(candidate))
        .find_map(|candidate| {
            prefixes.iter().find_map(|prefix| {
                secret_path_match(&normalize_path(&format!("{prefix}{candidate}")))
            })
        })
}

/// Heuristik der ersten Fassung: Platzhalter in einem Wort, das auf eine
/// Punktdatei, ein Home-Verzeichnis oder einen Secret-Hinweis zeigt.
fn glob_has_secret_hint(word: &str) -> bool {
    let last = word.rsplit('/').next().unwrap_or(word);
    word.contains(['*', '?', '[', '{'])
        && (last.starts_with('.')
            || word.starts_with('~')
            || word.starts_with("$home")
            || word.starts_with("/root")
            || SECRET_PATH_HINTS.iter().any(|hint| word.contains(hint)))
}

/// Shell-Glob eines Pfadteils als verankerte Regex (`*`, `?`, `[…]`,
/// `{a,b}`); `None` bei nicht auswertbarer Syntax (→ Aufrufer eskaliert).
fn glob_to_regex(glob: &str) -> Option<regex::Regex> {
    let mut pattern = String::from("^");
    let mut chars = glob.chars().peekable();
    let mut brace_depth = 0usize;
    while let Some(c) = chars.next() {
        match c {
            '*' => pattern.push_str("[^/]*"),
            '?' => pattern.push_str("[^/]"),
            '[' => {
                pattern.push('[');
                if matches!(chars.peek(), Some('!' | '^')) {
                    chars.next();
                    pattern.push('^');
                }
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == ']' {
                        closed = true;
                        break;
                    }
                    if matches!(inner, '\\' | '[' | '&' | '~') {
                        pattern.push('\\');
                    }
                    pattern.push(inner);
                }
                if !closed {
                    return None;
                }
                pattern.push(']');
            }
            '{' => {
                brace_depth += 1;
                pattern.push_str("(?:");
            }
            ',' if brace_depth > 0 => pattern.push('|'),
            '}' if brace_depth > 0 => {
                brace_depth -= 1;
                pattern.push(')');
            }
            other => pattern.push_str(&regex::escape(&other.to_string())),
        }
    }
    if brace_depth > 0 {
        return None;
    }
    pattern.push('$');
    regex::Regex::new(&pattern).ok()
}

/// Ziele von `cd`/`pushd` als Präfixe (`"/etc/"`), je Teilkommando; ohne
/// Ziel bzw. `~` das Home-Verzeichnis. `None`, wenn ein Ziel nicht prüfbar
/// ist (Variable, Platzhalter, Kommando-Substitution).
fn cd_prefixes(command: &str) -> Option<Vec<String>> {
    let mut prefixes = Vec::new();
    for segment in segment_command(command) {
        let words = shell_words(&segment.to_lowercase());
        let Some(first) = words.first() else { continue };
        if !matches!(first.text.as_str(), "cd" | "pushd") {
            continue;
        }
        let target = words[1..]
            .iter()
            .find(|word| !word.text.starts_with('-') || word.text == "-");
        match target {
            None => prefixes.push("~/".to_string()),
            Some(word) if word.expands || word.unquoted_glob || word.text.contains('`') => {
                return None;
            }
            Some(word) if word.text == "~" => prefixes.push("~/".to_string()),
            Some(word) if word.text == "-" => {}
            Some(word) => prefixes.push(format!(
                "{}/",
                normalize_path(word.text.trim_end_matches('/'))
            )),
        }
    }
    Some(prefixes)
}
