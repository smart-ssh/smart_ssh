use std::sync::OnceLock;

use regex::Regex;

/// Ergebnis von [`split_command`] (Spec 0002, Abschnitt 4).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ParseResult {
    /// Leere/reine Whitespace-Eingabe — kein sinnvolles Kommando.
    Empty,
    /// Konnte nicht sicher zerlegt werden (unausgeglichene Quotes/Klammern,
    /// Here-Doc, komplexes `bash -c "..."`). Nie automatisch AutoExec.
    Ambiguous { reason: String },
    /// Erfolgreich an `&&`, `||`, `;`, `|` in Teilkommandos zerlegt.
    Segments(Vec<String>),
}

/// Zerlegt ein Shell-Kommando in Teilkommandos (Spec 0002, Abschnitt 4.1).
///
/// Nutzt `shell-words` als Basis-Validierung für ausgeglichene
/// Anführungszeichen (das ist alles, was `shell-words` selbst kann — es
/// tokenisiert nur, kennt aber keine Operatoren). Die eigentliche
/// Operator-Erkennung (`&&`, `||`, `;`, `|`) sowie das Erkennen von
/// Here-Docs/komplexen `bash -c`-Aufrufen kommt zusätzlich obendrauf.
pub(super) fn split_command(cmd: &str) -> ParseResult {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return ParseResult::Empty;
    }

    if cmd.chars().any(|c| {
        matches!(
            c,
            '\0' | '\x1b' | '\x00'..='\x08' | '\x0b'..='\x0c' | '\x0e'..='\x1f' | '\x7f'
        )
    }) {
        return ParseResult::Ambiguous {
            reason: "Kommando enthält nicht-druckbare Steuerzeichen".to_string(),
        };
    }

    if looks_like_heredoc_or_complex_shell_c(trimmed) {
        return ParseResult::Ambiguous {
            reason: "mehrzeiliges Skript (Here-Doc oder `... -c \"...\"`) wird als \
                     Ganzes behandelt, kein Sub-Parsing (Spec 0002, Abschnitt 7)"
                .to_string(),
        };
    }

    // shell-words schlägt bei unausgeglichenen Anführungszeichen fehl — das
    // ist exakt der "nicht sicher zerlegbar"-Fall aus Abschnitt 4.4.
    if shell_words::split(trimmed).is_err() {
        return ParseResult::Ambiguous {
            reason: "Kommando konnte nicht sicher analysiert werden \
                     (unausgeglichene Anführungszeichen)"
                .to_string(),
        };
    }

    match scan_top_level_segments(trimmed) {
        Some(segments) if !segments.is_empty() => ParseResult::Segments(segments),
        _ => ParseResult::Ambiguous {
            reason: "Kommando konnte nicht sicher analysiert werden \
                     (unausgeglichene Klammern/Anführungszeichen oder keine \
                     auswertbaren Teilkommandos)"
                .to_string(),
        },
    }
}

/// Kollabiert Whitespace-Läufe (Leerzeichen, Tabs, ...) zu je einem
/// Leerzeichen und trimmt die Enden, damit Muster unabhängig von
/// Whitespace-Varianten in der Eingabe zuverlässig matchen.
pub(super) fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Zerlegt `command` vollständig in einzeln klassifizierbare Teilstücke —
/// Top-Level-Verkettungen (`split_command`) UND, pro Teilkommando,
/// rekursiv jede darin enthaltene Command-Substitution (`strip_substitutions`,
/// gleiche Rekursionstiefe wie `engine::evaluate_segment_explained`).
///
/// `pub(crate)` statt `pub(super)` wie die übrigen Parser-Bausteine (Spec
/// 0026, Abschnitt 2: "Wiederverwende die bestehende Kommando-
/// Segmentierungsfunktion ... falls sie aktuell nicht öffentlich/
/// wiederverwendbar ist, mache sie das") — `crate::risk` braucht exakt
/// dieselbe Zerlegung wie die Filter-Engine, nur ohne deren
/// Decision-Aggregation (Hard-Blacklist/Regeln/Confirm-Eskalation bleiben
/// reine `filter`-Interna). Bei `Empty`/`Ambiguous` wird das (normalisierte)
/// Gesamtkommando als einzelnes Element zurückgegeben, statt nichts zu
/// liefern — ein Risiko-Klassifizierer soll auch bei nicht sicher
/// zerlegbaren Eingaben noch gegen die Muster prüfen können, nur eben ohne
/// Teilkommando-Auflösung.
pub(crate) fn segment_command(command: &str) -> Vec<String> {
    match split_command(command) {
        ParseResult::Empty => Vec::new(),
        ParseResult::Ambiguous { .. } => vec![normalize_whitespace(command)],
        ParseResult::Segments(segments) => {
            let mut result = Vec::new();
            for segment in segments {
                let normalized = normalize_whitespace(&segment);
                let (literal, inner_contents) = strip_substitutions(&normalized);
                result.push(normalize_whitespace(&literal));
                for inner in inner_contents {
                    result.extend(segment_command(&inner));
                }
            }
            result
        }
    }
}

fn elevation_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:sudo|doas)\s+(.+)$").expect("elevation regex ist valide"))
}

/// Erkennt ein `sudo`/`doas`-Präfix und entfernt es (Spec 0002, Abschnitt
/// 4.6). `normalized` muss bereits whitespace-normalisiert sein. Gibt
/// `(elevated, rest)` zurück; `rest` ist ebenfalls whitespace-normalisiert.
pub(super) fn detect_elevation(normalized: &str) -> (bool, String) {
    match elevation_regex().captures(normalized) {
        Some(caps) => (true, normalize_whitespace(&caps[1])),
        None => (false, normalized.to_string()),
    }
}

/// Ersetzt jede `$(...)`-, `<(...)`-, `>(...)`- oder Backtick-Command-Substitution in `text` durch
/// ein einzelnes Leerzeichen und gibt zusätzlich die extrahierten inneren
/// Kommandos zurück (zur rekursiven Auswertung, Spec Abschnitt 4.5, Spec 0013 Abschnitt 2.3).
/// Verschachtelte Klammern werden über eine einfache Klammer-Tiefenzählung
/// korrekt erkannt (die innere Substitution wird unverändert als Teil des
/// extrahierten inneren Kommandos zurückgegeben und beim rekursiven Aufruf
/// erneut aufgelöst).
///
/// Arbeitet bewusst ohne Quote-Kontext (anders als
/// [`scan_top_level_segments`], das für die Operator-Erkennung Quotes
/// respektieren muss) — im Zweifel wird eine Substitution lieber zu viel als
/// zu wenig erkannt, das passt zu den Fail-safe-defaults der Spec.
pub(super) fn strip_substitutions(text: &str) -> (String, Vec<String>) {
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    let mut inner_contents = Vec::new();
    let mut i = 0usize;

    while i < chars.len() {
        if (chars[i] == '$' || chars[i] == '<' || chars[i] == '>') && chars.get(i + 1) == Some(&'(')
        {
            let start = i + 2;
            let mut depth = 1i32;
            let mut j = start;
            while j < chars.len() && depth > 0 {
                match chars[j] {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let end = if depth == 0 { j - 1 } else { chars.len() };
            inner_contents.push(chars[start..end].iter().collect());
            result.push(' ');
            i = if depth == 0 { j } else { chars.len() };
            continue;
        }
        if chars[i] == '`' {
            if let Some(offset) = chars[(i + 1)..].iter().position(|&c| c == '`') {
                let start = i + 1;
                let end = start + offset;
                inner_contents.push(chars[start..end].iter().collect());
                result.push(' ');
                i = end + 1;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    (result, inner_contents)
}

/// Here-Docs und `bash -c "..."`-artige Aufrufe komplett als einen Block
/// behandeln statt zu versuchen sie zu parsen, siehe
/// `docs/adr/0001-mehrzeilige-skripte-als-block.md`. Prüft sowohl den
/// unveränderten Text als auch die um `sudo`/Wrapper-Präfixe bereinigte
/// Fassung (`resolve_effective_command`) — sonst umgeht z. B. `sudo bash -c
/// "rm -rf /"` diese Prüfung komplett, weil das erste Wort "sudo" statt
/// "bash" ist (unabhängiger Review-Pass, Spec 0002).
fn looks_like_heredoc_or_complex_shell_c(cmd: &str) -> bool {
    if cmd.contains("<<") {
        return true;
    }
    if is_complex_shell_c_invocation(cmd) {
        return true;
    }
    let resolved = resolve_effective_command(cmd);
    resolved != cmd && is_complex_shell_c_invocation(&resolved)
}

fn is_complex_shell_c_invocation(cmd: &str) -> bool {
    let mut words = cmd.split_whitespace();
    if let (Some(prog), Some(flag)) = (words.next(), words.next()) {
        let prog = prog.rsplit('/').next().unwrap_or(prog);
        if matches!(prog, "bash" | "sh" | "zsh" | "dash") {
            // Nicht nur das exakte "-c", sondern auch kombinierte
            // Kurz-Flags, die ein "c" enthalten (`-xc`, `-lc`, `-ec`, ...)
            // — `bash -xc "..."` ist derselbe "ganzes Skript als ein Block"-
            // Fall wie `bash -c "..."`.
            let is_c_flag = flag == "-c"
                || (flag.starts_with('-') && !flag.starts_with("--") && flag.contains('c'));
            if is_c_flag {
                return true;
            }
        }
    }
    false
}

/// Bekannte "durchreichende" Kommandos — sie führen ihr letztes Argument
/// unverändert als Kommando aus, ändern selbst aber nichts an der
/// eigentlichen Aktion. Nicht erschöpfend (kein vollständiger CLI-Parser für
/// jeden Wrapper), deckt aber die in der Praxis üblichen
/// Verschleierungsversuche gegen die Hard-Blacklist ab (Spec 0002, Abschnitt
/// 3.1: "unabhängig von Nutzerregeln" — eine fest verdrahtete Blacklist, die
/// ein simples `env rm -rf /` nicht erkennt, hält dieses Versprechen nicht).
/// Erweitert um `timeout`/`xargs`/`setsid`/`stdbuf`/`ionice`/`chroot`/
/// `flock`/`busybox`/`script` (unabhängiger Review-Pass, Spec 0013).
const PASSTHROUGH_WRAPPERS: &[&str] = &[
    "env", "nice", "nohup", "time", "command", "timeout", "xargs", "setsid", "stdbuf", "ionice",
    "chroot", "flock", "busybox", "script",
];

/// Wrapper, die vor dem eigentlichen Kommando genau EIN positionales (nicht
/// mit `-` beginnendes) Pflichtargument erwarten — die Zeitdauer bei
/// `timeout`, das neue Wurzelverzeichnis bei `chroot`, die Lock-Datei bei
/// `flock`. Ohne diese Sonderbehandlung würde z. B. `timeout 5 rm -rf /`
/// das positionale `5` fälschlich als Kommandoname auffassen und die
/// Wrapper-Erkennung dort abbrechen.
const WRAPPERS_WITH_ONE_POSITIONAL_ARG: &[&str] = &["timeout", "chroot", "flock"];

/// Löst den tatsächlich auszuführenden Kommandokern heraus: entfernt
/// wiederholt (bis zum Fixpunkt, deckt z. B. `sudo sudo rm -rf /` oder
/// `FOO=1 sudo env BAR=2 rm -rf /` ab) führende
/// `NAME=wert`-Variablenzuweisungen, `sudo`/`doas`-Präfixe samt der
/// gebräuchlichsten wertetragenden Flags (`-u`/`--user`, `-g`/`--group`)
/// sowie bekannte durchreichende Wrapper-Kommandos (s.
/// [`PASSTHROUGH_WRAPPERS`]), und entfernt abschließend Anführungszeichen/
/// Escapes aus dem ersten verbleibenden Wort (`"rm" -rf /`, `r"m" -rf /`,
/// `r\m -rf /`, `$'rm' -rf /` → jeweils `rm -rf /`).
///
/// Bewusst kein vollständiger Shell-/CLI-Parser — ein Best-effort-
/// Normalisierungsschritt gegen die in der Praxis üblichen
/// Verschleierungsversuche, zusätzlich zum bisherigen einfachen
/// `detect_elevation` (das für das Dual-Text-Regel-Matching aus ADR 0002
/// unverändert bleibt — hier geht es ausschließlich um die Hard-Blacklist-
/// Prüfung, s. `engine::evaluate_segment_explained`).
pub(super) fn resolve_effective_command(normalized: &str) -> String {
    let mut current = normalized.to_string();
    loop {
        let stripped = strip_one_elevation_wrapper_or_assignment(&current);
        if stripped == current {
            break;
        }
        current = stripped;
    }
    unquote_first_word(&current)
}

/// `NAME=wert`-Präfix wie bei `FOO=1 rm -rf /` — ein Shell-typisches Muster,
/// eine Umgebungsvariable direkt vor einem einzelnen Kommando zu setzen,
/// ganz ohne Wrapper-Kommando. `name` muss mit Buchstabe/Unterstrich
/// beginnen und nur aus Alnum/Unterstrich bestehen (POSIX-Bezeichnerregel
/// für Shell-Variablen) — verhindert, dass z. B. ein Datei-Pfad mit `=`
/// darin (selten, aber möglich) fälschlich als Zuweisung erkannt wird.
fn is_var_assignment(word: &str) -> bool {
    let Some((name, _value)) = word.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn strip_one_elevation_wrapper_or_assignment(cmd: &str) -> String {
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let Some(&head) = words.first() else {
        return cmd.to_string();
    };

    if is_var_assignment(head) {
        return normalize_whitespace(&words[1..].join(" "));
    }

    let is_elevation = head == "sudo" || head == "doas";
    let is_wrapper = PASSTHROUGH_WRAPPERS.contains(&head);
    if !is_elevation && !is_wrapper {
        return cmd.to_string();
    }

    let mut i = 1;
    while i < words.len() && words[i].starts_with('-') {
        let flag = words[i];
        i += 1;
        let takes_value = (is_elevation && matches!(flag, "-u" | "--user" | "-g" | "--group"))
            || (head == "nice" && flag == "-n");
        if takes_value && i < words.len() {
            i += 1;
        }
    }
    if WRAPPERS_WITH_ONE_POSITIONAL_ARG.contains(&head) && i < words.len() {
        i += 1;
    }
    normalize_whitespace(&words[i..].join(" "))
}

/// Entfernt Anführungszeichen/Escapes aus dem ERSTEN Wort (`"rm" -rf /`,
/// `r"m" -rf /`, `r''m -rf /`, `r\m -rf /` → jeweils `rm -rf /`) — eine
/// reine Textzerlegung wie diese Engine sie betreibt (kein echtes
/// Shell-Tokenizing für die Blacklist-Prüfung) würde das Blacklist-Muster
/// sonst nie am literal quotierten/escapten ersten Wort matchen lassen,
/// obwohl eine echte Shell die Quotes/Escapes selbst entfernen und schlicht
/// `rm -rf /` ausführen würde. Entfernt bewusst JEDES `'`/`"`/`\` im ersten
/// Wort, nicht nur ein exakt umschließendes Quote-Paar (die vorherige
/// Fassung) — sonst entgeht ihr eine teilweise/versetzte Quotierung wie
/// `r"m"` oder `r''m` (unabhängiger Review-Pass, Spec 0013).
fn unquote_first_word(cmd: &str) -> String {
    let trimmed = cmd.trim_start();
    let rest_start = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let (first, rest) = trimmed.split_at(rest_start);

    // `$'...'`/`$"..."` (ANSI-C- bzw. lokalisierte Shell-Quotierung): das
    // `$` ist Teil der Quotierungssyntax selbst, kein
    // Variablen-Expansions-Präfix — ohne diesen Schritt bliebe nach dem
    // Entfernen der Anführungszeichen ein irreführendes `$rm` stehen.
    let first = if first.starts_with("$'") || first.starts_with("$\"") {
        &first[1..]
    } else {
        first
    };

    let cleaned_first: String = first
        .chars()
        .filter(|&c| !matches!(c, '\'' | '"' | '\\'))
        .collect();

    if rest.is_empty() {
        cleaned_first
    } else {
        format!("{cleaned_first}{rest}")
    }
}

/// Erkennt eine nicht in Anführungszeichen stehende Ausgabe-Umleitung (`>`,
/// `>>`, `2>`, `&>`, ...) auf Top-Level. Die Engine kannte Umleitungsziele
/// bislang überhaupt nicht — eine Allow-Regel für z. B. `ls *` ließ dadurch
/// auch `ls -la > /etc/passwd` unbestätigt durchgehen (unabhängiger
/// Review-Pass, Spec 0002 — arbiträres Datei-Überschreiben über die
/// harmloseste denkbare Whitelist-Regel). Ein `>` direkt vor `(` ist
/// Process-Substitution (`>(...)`, bereits separat über
/// `strip_substitutions`/Klammer-Tiefe in [`scan_top_level_segments`]
/// behandelt), keine Umleitung.
pub(super) fn contains_unquoted_output_redirection(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if in_single {
            if c == '\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if c == '\\' && i + 1 < chars.len() {
                i += 2;
                continue;
            }
            if c == '"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        match c {
            '\'' => in_single = true,
            '"' => in_double = true,
            '>' if chars.get(i + 1) != Some(&'(') => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Zerlegt `cmd` an den Top-Level-Operatoren `&&`, `||`, `;`, `|`, `&` sowie
/// Zeilenumbrüchen (`\n`, `\r`, `\r\n`), ohne dabei in einfache/doppelte
/// Anführungszeichen, Backticks, `$(...)`, `<(...)` oder `>(...)`
/// hineinzuspalten. Gibt `None` zurück, wenn am Ende Quotes/Klammern nicht
/// ausgeglichen sind.
fn scan_top_level_segments(cmd: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = cmd.chars().collect();
    let mut segments = Vec::new();
    let mut current_start = 0usize;
    let mut i = 0usize;
    let mut paren_depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;

    while i < chars.len() {
        let c = chars[i];

        if in_single {
            if c == '\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if c == '\\' && i + 1 < chars.len() {
                i += 2;
                continue;
            }
            if c == '"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        if in_backtick {
            if c == '`' {
                in_backtick = false;
            }
            i += 1;
            continue;
        }

        match c {
            '\'' => {
                in_single = true;
                i += 1;
                continue;
            }
            '"' => {
                in_double = true;
                i += 1;
                continue;
            }
            '`' => {
                in_backtick = true;
                i += 1;
                continue;
            }
            _ => {}
        }

        if (c == '$' || c == '<' || c == '>') && chars.get(i + 1) == Some(&'(') {
            paren_depth += 1;
            i += 2;
            continue;
        }
        if c == ')' && paren_depth > 0 {
            paren_depth -= 1;
            i += 1;
            continue;
        }

        if paren_depth == 0 {
            match c {
                '\n' | '\r' => {
                    push_segment(&chars, current_start, i, &mut segments);
                    if c == '\r' && chars.get(i + 1) == Some(&'\n') {
                        i += 1;
                    }
                    i += 1;
                    current_start = i;
                    continue;
                }
                '&' if chars.get(i + 1) == Some(&'&') => {
                    push_segment(&chars, current_start, i, &mut segments);
                    i += 2;
                    current_start = i;
                    continue;
                }
                '&' => {
                    push_segment(&chars, current_start, i, &mut segments);
                    i += 1;
                    current_start = i;
                    continue;
                }
                '|' if chars.get(i + 1) == Some(&'|') => {
                    push_segment(&chars, current_start, i, &mut segments);
                    i += 2;
                    current_start = i;
                    continue;
                }
                '|' | ';' => {
                    push_segment(&chars, current_start, i, &mut segments);
                    i += 1;
                    current_start = i;
                    continue;
                }
                _ => {}
            }
        }

        i += 1;
    }

    push_segment(&chars, current_start, chars.len(), &mut segments);

    if in_single || in_double || in_backtick || paren_depth != 0 {
        return None;
    }
    Some(segments)
}

fn push_segment(chars: &[char], start: usize, end: usize, segments: &mut Vec<String>) {
    if start >= end {
        return;
    }
    let trimmed: String = chars[start..end]
        .iter()
        .collect::<String>()
        .trim()
        .to_string();
    if !trimmed.is_empty() {
        segments.push(trimmed);
    }
}
