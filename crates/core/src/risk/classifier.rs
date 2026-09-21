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
/// Geprüft werden alle Teilkommandos (`;`, `&&`, `|`, `$(...)`), das
/// Gesamtkommando und die von `sudo`/`env`/Zuweisungen befreite Form, jeweils
/// kleingeschrieben, ohne Quotes/Backslashes und mit lexikalisch
/// normalisierten Pfaden (`//`, `/./`, `x/../`). Eskaliert wird, wenn
/// (spec-reviewer-Fund, ERHÖHT — jeder Punkt nur zusätzlich):
/// - ein Lesebefehl **irgendwo** im Teilkommando steht (auch hinter
///   Wrappern wie `docker exec`, `sudo -iu`, `timeout -s`, als `/bin/cat`)
///   und ein Secret-Pfad vorkommt;
/// - ein Secret-Pfad per `<` umgeleitet oder nach `/dev/stdout`,
///   `/dev/fd/…`, `/proc/…/fd/…`, `/dev/tty` geschrieben wird — bei jedem
///   Befehl (`cp ~/.ssh/id_rsa /dev/stdout`);
/// - massenhaft gelesen wird (rekursives `grep`, `rg`/`ag`/`ack`,
///   `-exec`/`xargs`) — jedes Verzeichnis kann eine `.env` enthalten;
/// - ein Pfad eine Variable enthält (`cat /etc/sha$@dow`, `cat $F`) oder
///   ein `cd`-Ziel nicht prüfbar ist;
/// - ein Platzhalter im Verzeichnisteil steht oder der letzte Pfadteil einen
///   bekannten Secret-Dateinamen treffen kann (`cat /etc/sha*`, `cat *`);
/// - ein relativer Pfad zusammen mit einem `cd`-Ziel einen Secret-Pfad
///   ergibt (`cd /etc && cat shadow`);
/// - das Kommando zu lang für die Prüfung ist.
///
/// **Grenzen (ehrlich, bewusst)**: rein lexikalisch. Nicht erkannt werden
/// Symlinks (`ln -s ~/.ssh/id_rsa x; cat x` — `ln` liest nichts), Programme
/// außerhalb der Liste (Skriptsprachen mit Dateinamen, Archivierer wie
/// `tar`), Kodierungen und Umwege über eine zuvor unverdächtig kopierte
/// Datei. Inline-Code (`python -c`, `bash -c`, `$(…)`) setzt die
/// Filter-Engine ohnehin auf Bestätigung; Redaction bleibt die weitere
/// Schicht.
pub fn secret_path_read_reason(command: &str) -> Option<&'static str> {
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

    let cd_prefixes = cd_prefixes(&normalize_for_secret_check(command));

    for segment in &segments {
        let normalized = normalize_for_secret_check(segment);
        let concrete = secret_path_match(&normalized);

        if let Some(reason) = concrete {
            if normalized.contains('<')
                || STDOUT_TARGETS
                    .iter()
                    .any(|target| normalized.contains(target))
            {
                return Some(reason);
            }
        }

        let tokens: Vec<&str> = normalized.split_whitespace().collect();
        let Some(reader) = tokens.iter().position(|token| is_read_command(token)) else {
            continue;
        };
        if let Some(reason) = concrete {
            return Some(reason);
        }
        if is_bulk_read(&tokens, reader) {
            return Some(
                "Liest Dateien rekursiv bzw. per -exec/xargs – Inhalt nicht vorab prüfbar",
            );
        }
        let Some(cd_prefixes) = cd_prefixes.as_deref() else {
            return Some("Lesebefehl nach Verzeichniswechsel in ein nicht prüfbares Ziel");
        };
        // awk-Programme zerfallen an Leerzeichen (`{print $NF}`) — dort zählt
        // nur `$` in einem Pfad, nicht ein ganzes Variablen-Wort.
        let awk_program = matches!(
            tokens[reader].rsplit('/').next(),
            Some("awk" | "gawk" | "mawk" | "nawk")
        );
        for arg in candidate_args(&tokens, reader) {
            if arg.contains('`') || is_variable_path(arg, awk_program) {
                return Some("Lesebefehl mit Variable im Pfad – Ziel nicht prüfbar");
            }
            if let Some(reason) = glob_may_hit_secret(arg, cd_prefixes) {
                return Some(reason);
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
    "/dev/tty",
    "/proc/",
];

/// Kleinschreibung, ohne Quotes/Backslashes, Pfade lexikalisch normalisiert.
fn normalize_for_secret_check(text: &str) -> String {
    let stripped: String = text
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '\'' | '"' | '\\'))
        .collect();
    normalize_path(&stripped)
}

/// `//` → `/`, `/./` → `/`, `x/../` → `` (wiederholt, rein lexikalisch).
fn normalize_path(text: &str) -> String {
    static PARENT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let parent = PARENT.get_or_init(|| {
        regex::Regex::new(r"(?P<keep>^|/)(?P<name>[^/\s]+)/\.\.(?:/|$)")
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
            .take_while(|c| !c.is_whitespace() && !matches!(c, ';' | '|' | '&' | ')' | '<' | '>'))
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

fn is_read_command(token: &str) -> bool {
    static READ: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let read = READ.get_or_init(|| {
        regex::Regex::new(&format!(r"^{SECRET_READ_COMMANDS}$"))
            .expect("eingebautes Lesebefehl-Muster ist gültig")
    });
    let trimmed = token.trim_start_matches(['(', '{', '!', '<']);
    let name = trimmed.rsplit('/').next().unwrap_or(trimmed);
    read.is_match(name)
}

/// Rekursives oder per `-exec`/`xargs` verteiltes Lesen.
fn is_bulk_read(tokens: &[&str], reader: usize) -> bool {
    if tokens
        .iter()
        .any(|token| matches!(*token, "xargs" | "-exec" | "-execdir" | "-ok" | "-okdir"))
    {
        return true;
    }
    let name = tokens[reader].rsplit('/').next().unwrap_or(tokens[reader]);
    if matches!(name, "rg" | "ag" | "ack") {
        return true;
    }
    matches!(name, "grep" | "egrep" | "fgrep" | "zgrep")
        && tokens[reader + 1..]
            .iter()
            .enumerate()
            .any(|(offset, token)| {
                matches!(
                    *token,
                    "--recursive" | "--dereference-recursive" | "--directories=recurse"
                ) || (*token == "-d" && tokens.get(reader + 2 + offset) == Some(&"recurse"))
                    || (token.starts_with('-')
                        && !token.starts_with("--")
                        && token[1..].chars().all(|c| c.is_ascii_alphanumeric())
                        && token.contains('r'))
            })
}

/// Pfad-Kandidaten eines Teilkommandos: alle Wörter außer dem Lesebefehl
/// und Optionen; `--opt=wert`/`if=wert` liefern ihren Wert, Umleitungen ihr
/// Ziel. Bei Musterbefehlen (grep, sed, awk, jq, …) ist das erste Argument
/// das Muster/Programm und wird übersprungen — außer es gibt `-e`/`-f`,
/// dann ist das erste Argument schon eine Datei.
fn candidate_args<'a>(tokens: &[&'a str], reader: usize) -> Vec<&'a str> {
    let name = tokens[reader].rsplit('/').next().unwrap_or(tokens[reader]);
    let takes_pattern = matches!(
        name,
        "grep"
            | "egrep"
            | "fgrep"
            | "zgrep"
            | "sed"
            | "awk"
            | "gawk"
            | "mawk"
            | "nawk"
            | "jq"
            | "yq"
    ) && !tokens[reader + 1..].iter().any(|token| {
        matches!(*token, "-e" | "-f" | "--regexp" | "--file" | "--expression")
            || token.starts_with("--regexp=")
            || token.starts_with("--file=")
            || token.starts_with("--expression=")
            || token.starts_with("--from-file")
    });
    let mut pattern_skipped = !takes_pattern;
    let mut args = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if index == reader {
            continue;
        }
        let mut arg = *token;
        if let Some(position) = arg.rfind(['<', '>']) {
            arg = &arg[position + 1..];
        }
        if let Some((_, value)) = arg.split_once('=') {
            arg = value;
        } else if arg.starts_with('-') {
            continue;
        }
        let arg = arg.trim_matches(['(', ')', '{', '}', ';']);
        if arg.is_empty() {
            continue;
        }
        if index > reader && !pattern_skipped {
            pattern_skipped = true;
            continue;
        }
        args.push(arg);
    }
    args
}

/// `$VAR`, `${VAR}`, `$@`, `$*` als ganzes Argument, oder `$` in einem
/// Pfad — Ziel lexikalisch nicht bestimmbar. Bewusst nicht: `$1}` o. ä. aus
/// awk-Programmen.
fn is_variable_path(arg: &str, path_only: bool) -> bool {
    static VARIABLE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let variable = VARIABLE.get_or_init(|| {
        regex::Regex::new(r"^\$(?:\{[^}]*\}|[a-z_][a-z0-9_]*|[@*])$")
            .expect("eingebautes Variablen-Muster ist gültig")
    });
    arg.contains('$') && (arg.contains('/') || (!path_only && variable.is_match(arg)))
}

/// Platzhalter in einem Pfad: im Verzeichnisteil immer verdächtig; im
/// letzten Teil, wenn er einen bekannten Secret-Dateinamen trifft.
fn glob_may_hit_secret(arg: &str, cd_prefixes: &[String]) -> Option<&'static str> {
    const GLOB: [char; 4] = ['*', '?', '[', '{'];
    if !arg.contains(GLOB) {
        return None;
    }
    let (dir, last) = match arg.rsplit_once('/') {
        Some((dir, last)) => (Some(dir), last),
        None => (None, arg),
    };
    if last.starts_with('.')
        || arg.starts_with('~')
        || arg.starts_with("$home")
        || arg.starts_with("/root")
        || SECRET_PATH_HINTS.iter().any(|hint| arg.contains(hint))
    {
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

/// Ziele von `cd`/`pushd` im Kommando als Präfixe (`"/etc/"`); ohne Ziel
/// bzw. `~` das Home-Verzeichnis. `None`, wenn ein Ziel nicht prüfbar ist
/// (Variable, Platzhalter, Kommando-Substitution).
fn cd_prefixes(normalized_command: &str) -> Option<Vec<String>> {
    let tokens: Vec<&str> = normalized_command
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')'))
        .filter(|token| !token.is_empty())
        .collect();
    let mut prefixes = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !matches!(*token, "cd" | "pushd") {
            continue;
        }
        let target = tokens[index + 1..]
            .iter()
            .find(|next| !next.starts_with('-') || **next == "-")
            .copied();
        match target {
            None | Some("~") => prefixes.push("~/".to_string()),
            Some("-") => {}
            Some(target) if target.contains(['$', '`', '*', '?', '[', '{']) => return None,
            Some(target) => prefixes.push(format!("{}/", target.trim_end_matches('/'))),
        }
    }
    Some(prefixes)
}
