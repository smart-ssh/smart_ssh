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
/// `docs/adr/0001-mehrzeilige-skripte-als-block.md`.
fn looks_like_heredoc_or_complex_shell_c(cmd: &str) -> bool {
    if cmd.contains("<<") {
        return true;
    }
    let mut words = cmd.split_whitespace();
    if let (Some(prog), Some(flag)) = (words.next(), words.next()) {
        let prog = prog.rsplit('/').next().unwrap_or(prog);
        if matches!(prog, "bash" | "sh" | "zsh" | "dash") && flag == "-c" {
            return true;
        }
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
