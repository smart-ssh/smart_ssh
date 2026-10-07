use std::sync::OnceLock;

use regex::Regex;

/// Maximale Verschachtelungstiefe für `$(...)`/`<(...)`/`>(...)`/Backtick-
/// Command-Substitution (Spec 0043, Fund B) — Filter-Engine
/// (`filter::engine::evaluate_parsed_explained`) und Risiko-Klassifizierer
/// (dieses Moduls [`segment_command`]) nutzen denselben Wert, damit kein
/// Konsument tiefer absteigt als der andere (Spec 0043, Abschnitt 5).
/// Großzügig genug für jeden legitimen Fall, weit unter jeder
/// Stack-Overflow-Schwelle für den rekursiven Abstieg in beiden
/// Konsumenten.
pub const MAX_SUBSTITUTION_DEPTH: usize = 32;

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
    segment_command_at_depth(command, 0)
}

/// Tiefenbegrenzter Abstieg hinter [`segment_command`] (Spec 0043, Fund B):
/// bricht die Rekursion ab, SOBALD `depth` den Cap erreicht — geprüft VOR
/// jedem weiteren rekursiven Aufruf, nicht erst nachdem vollständig
/// geparst wurde (das hätte dasselbe Zu-spät-Problem wie Fund A). Ab
/// `MAX_SUBSTITUTION_DEPTH` wird nicht weiter in die verbleibenden inneren
/// Substitutionen abgestiegen — dasselbe Fail-safe wie beim Längen-Cap
/// oben in [`FilterEngine`](super::engine::FilterEngine) (ein
/// unklassifiziertes/unvollständig zerlegtes Ergebnis ist hier
/// hinnehmbar, weil die eigentliche Sicherheitsentscheidung über die
/// Filter-Engine läuft, die bei Tiefenüberschreitung separat auf
/// `Confirm` eskaliert, s. `engine::evaluate_parsed_explained`).
fn segment_command_at_depth(command: &str, depth: usize) -> Vec<String> {
    match split_command(command) {
        ParseResult::Empty => Vec::new(),
        ParseResult::Ambiguous { .. } => vec![normalize_whitespace(command)],
        ParseResult::Segments(segments) => {
            let mut result = Vec::new();
            for segment in segments {
                let normalized = normalize_whitespace(&segment);
                let (literal, inner_contents) = strip_substitutions(&normalized);
                result.push(normalize_whitespace(&literal));
                if depth >= MAX_SUBSTITUTION_DEPTH {
                    continue;
                }
                for inner in inner_contents {
                    result.extend(segment_command_at_depth(&inner, depth + 1));
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

/// Extrahiert das eigentliche `-c`/`-e`/`-r`-Code-Argument aus einem von
/// [`looks_like_heredoc_or_complex_shell_c`] als Shell-/Interpreter-`-c`-
/// Aufruf erkannten Kommando (nicht für Here-Docs — deren Inhalt steht nicht
/// in einem einzelnen Token). Unabhängiger Review-Pass, Spec 0002, Abschnitt
/// 4.6, letzter Satz: "Der Inhalt des `-c`-Arguments wird als eigenes
/// Kommando ... geprüft, nicht als undurchsichtiges Argument durchgewunken"
/// — ADR 0001 behandelt den ganzen Aufruf zwar weiterhin als einen Block
/// (kein Sub-Parsing in mehrere Kommandos), aber ohne diese Funktion wäre
/// selbst eine explizite Nutzer-`Deny`-Regel oder die Hard-Blacklist über
/// `bash -c "docker rm -f prod"` bzw. `bash -c "rm -rf /"` vollständig
/// umgehbar (landet sonst blind bei `Confirm`, nie bei `Deny`) — das würde
/// die Spec-3-Garantie "Deny wird gar nicht erst zur Bestätigung angeboten"
/// verletzen. Nutzt `shell_words::split` (bereits als sicher zerlegbar
/// vorausgesetzt, s. Aufrufer) statt eigenem Tokenizing, damit Quotes/
/// Escapes im Code-Argument korrekt aufgelöst werden — dasselbe Argument,
/// das eine echte Shell auch tatsächlich als Code-String sähe.
pub(super) fn extract_shell_c_style_code(cmd: &str) -> Option<String> {
    if cmd.contains("<<") {
        return None;
    }
    let candidate = if is_complex_shell_c_invocation(cmd) {
        cmd.to_string()
    } else {
        let resolved = resolve_effective_command(cmd);
        if resolved != cmd && is_complex_shell_c_invocation(&resolved) {
            resolved
        } else {
            return None;
        }
    };
    let tokens = shell_words::split(&candidate).ok()?;
    // tokens[0] = Programm, tokens[1] = Code-Flag, tokens[2] = Code-String
    // — gilt unabhängig davon, ob das Flag exakt "-c" oder eine kombinierte
    // Kurzform wie "-xc" ist (s. `is_complex_shell_c_invocation`).
    tokens.into_iter().nth(2)
}

/// Skript-Interpreter mit einem `-c`/`-e`-artigen "führe diesen String als
/// Code aus"-Flag — dieselbe Umgehungsklasse wie `bash -c`/`sh -c`, nur mit
/// einer anderen Sprache (unabhängiger Review-Pass, Spec 0002: `python3 -c
/// "..."`/`perl -e "..."`/`ruby -e "..."`/`node -e "..."` unter einer
/// harmlosen `Allow: python3 *`-Regel wären sonst AutoExec-fähig, obwohl
/// beliebiger Code im `-c`/`-e`-Argument steht). `("python3", "-c")` heißt:
/// Interpreter `python3` erkennt das Code-Flag `-c`.
const SCRIPT_INTERPRETER_CODE_FLAGS: &[(&str, &str)] = &[
    ("python", "-c"),
    ("python3", "-c"),
    ("perl", "-e"),
    ("ruby", "-e"),
    ("node", "-e"),
    ("php", "-r"),
];

fn is_complex_shell_c_invocation(cmd: &str) -> bool {
    let mut words = cmd.split_whitespace();
    if let (Some(prog), Some(flag)) = (words.next(), words.next()) {
        // Case-insensitive (issue #13): on a case-insensitive file system
        // `BASH -c "..."` runs bash. Only widens what counts as a script
        // block, i.e. only escalates.
        let prog = prog.rsplit('/').next().unwrap_or(prog).to_ascii_lowercase();
        let prog = prog.as_str();
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
        if SCRIPT_INTERPRETER_CODE_FLAGS
            .iter()
            .any(|&(interpreter, code_flag)| prog == interpreter && flag == code_flag)
        {
            return true;
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
    "chroot", "flock", "busybox", "script", "exec", "builtin",
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
pub(crate) fn resolve_effective_command(normalized: &str) -> String {
    let mut current = normalized.to_string();
    loop {
        let stripped = strip_one_elevation_wrapper_or_assignment(&current);
        if stripped == current {
            break;
        }
        current = stripped;
    }
    strip_path_prefix_from_first_word(&unquote_first_word(&current))
}

/// Ersetzt das erste Wort durch seinen Basename (`/usr/bin/docker` bzw.
/// `./docker` → `docker`), Rest des Kommandos unverändert. Unabhängiger
/// Review-Pass (Spec 0002): ohne diesen Schritt umgeht ein absoluter oder
/// relativer Pfad zum selben Programm eine Nutzer-`Deny`-Regel wie
/// `Deny "docker *"` (`/usr/bin/docker rm -f prod` landete zuvor im
/// Default-`Confirm` statt `Deny`), obwohl die Hard-Blacklist genau diesen
/// Fall über ihre eigene Pfad-Alternation bereits abdeckt (`blacklist.rs`)
/// — Abschnitt 4.6 verlangt ausdrücklich, dass `resolve_effective_command`
/// „die Basis für jede Prüfung" ist, also keine schwächere Sicht als die
/// Blacklist haben darf.
fn strip_path_prefix_from_first_word(cmd: &str) -> String {
    let trimmed = cmd.trim_start();
    let rest_start = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let (first, rest) = trimmed.split_at(rest_start);
    let basename = first.rsplit('/').next().unwrap_or(first);
    if basename == first {
        return cmd.to_string();
    }
    format!("{basename}{rest}")
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

/// Ob `token` (das erste Wort eines Kommandos) eine Elevation
/// (`sudo`/`doas`) oder ein bekanntes durchreichendes Wrapper-Kommando (s.
/// [`PASSTHROUGH_WRAPPERS`]) ist. Öffentlich, damit Aufrufer außerhalb der
/// Blacklist-Prüfung (z. B. `crate::rule_suggestions` in `app-shell`s
/// Regel-Schnellvorschlag, Spec 0011) dieselbe Erkennung nutzen können,
/// statt eine eigene, potenziell abweichende Liste zu pflegen — unabhängiger
/// Review-Pass, Spec 0011: die Schnellvorschlag-Heuristik schlug bislang
/// unbesehen `sudo *` als Regel vor, was jedes sudo-Kommando AutoExec-fähig
/// macht.
pub fn is_elevation_or_passthrough_wrapper(token: &str) -> bool {
    token == "sudo" || token == "doas" || PASSTHROUGH_WRAPPERS.contains(&token)
}

fn strip_one_elevation_wrapper_or_assignment(cmd: &str) -> String {
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let Some(&head) = words.first() else {
        return cmd.to_string();
    };

    if is_var_assignment(head) {
        return normalize_whitespace(&words[1..].join(" "));
    }

    // Issue #13: wrappers are recognised by their lower-cased basename, so
    // `/usr/bin/env rm`, `SUDO rm` or `Env rm` (case-insensitive file
    // systems) are unwrapped like `env rm`. The result only feeds Deny/
    // Confirm rules, the hard blacklist and the risk classifier (never an
    // Allow rule, see `engine::evaluate_rules_explained`), so recognising
    // more wrappers can only escalate.
    let head_lower = head.rsplit('/').next().unwrap_or(head).to_ascii_lowercase();
    let head = head_lower.as_str();
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

// --- Fail-closed checks for opaque input (issue #13) -----------------------
//
// Each check below answers "can the engine see what the shell will run?".
// When the answer is no, the engine escalates the decision to at least
// `Confirm` (code `FILTER_PARSE_AMBIGUOUS`) — it never lowers a decision and
// it never replaces the regular evaluation, which still runs so a matching
// `Deny` rule keeps winning.

/// Unicode characters that make the displayed command differ from the
/// executed one: C1 controls (U+0080–U+009F, the C0 range is already
/// rejected in [`split_command`]), whitespace outside ASCII (a separator for
/// [`normalize_whitespace`] but part of a word for the shell, so the engine
/// would judge a different command than the one that runs), and invisible
/// format characters (zero-width characters, bidi embeddings/overrides/
/// isolates, word joiner, BOM, soft hyphen, variation selectors, tags).
fn is_deceptive_unicode(c: char) -> bool {
    if c.is_ascii() {
        return false;
    }
    if c.is_whitespace() || c.is_control() {
        return true;
    }
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{17B4}'
            | '\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFFB}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

/// Whether `text` contains an ANSI-C quoted string (`$'...'`) with a
/// backslash escape. The shell decodes those escapes (`$'\x72m'` is `rm`),
/// the engine does not, so it cannot know which word results. An ANSI-C
/// string without a backslash is a plain literal and handled by
/// [`unquote_first_word`]. Scans without quote context on purpose: a `$'`
/// inside double quotes is a false positive, which only costs a
/// confirmation.
fn contains_ansi_c_escape(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i + 1 < chars.len() {
        if chars[i] == '$' && chars[i + 1] == '\'' {
            let mut j = i + 2;
            while j < chars.len() && chars[j] != '\'' {
                if chars[j] == '\\' {
                    return true;
                }
                j += 1;
            }
            i = j + 1;
            continue;
        }
        i += 1;
    }
    false
}

/// Checks the whole raw command for encodings the engine cannot see through
/// (Unicode tricks, ANSI-C escapes). Returns the reason, or `None`.
pub(super) fn opaque_encoding_reason(cmd: &str) -> Option<&'static str> {
    if cmd.chars().any(is_deceptive_unicode) {
        return Some(
            "Kommando enthält unsichtbare Unicode-Zeichen, Unicode-Leerraum oder \
             C1-Steuerzeichen und konnte nicht sicher analysiert werden",
        );
    }
    if contains_ansi_c_escape(cmd) {
        return Some(
            "Kommando enthält ANSI-C-Quoting mit Escape-Sequenzen ($'\\x..') und \
             konnte nicht sicher analysiert werden",
        );
    }
    None
}

/// Shell reserved words: a segment starting with one of them is part of a
/// compound command (`if ...; then rm -rf /; fi`), whose real command the
/// segment-wise view does not resolve.
const SHELL_RESERVED_WORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "do", "done", "while", "until", "for", "in", "case",
    "esac", "select", "function", "coproc",
];

/// The command word of `normalized` as the shell resolves it: after the
/// same wrapper/elevation/assignment stripping and first-word unquoting as
/// [`resolve_effective_command`], but before the path prefix is cut off, so
/// glob characters in a directory part stay visible.
fn effective_command_word(normalized: &str) -> String {
    let mut current = normalized.to_string();
    loop {
        let stripped = strip_one_elevation_wrapper_or_assignment(&current);
        if stripped == current {
            break;
        }
        current = stripped;
    }
    unquote_first_word(&current)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string()
}

/// Checks whether the command word of one segment is a plain name the
/// engine can match rules against. `literal` is the whitespace-normalised
/// segment with command substitutions already removed. Opaque are:
/// non-ASCII names (fullwidth forms, homoglyphs), parameter expansion
/// (`$x`, `${IFS}`), glob and brace expansion (`/bin/r?`, `{rm,-rf,/}`),
/// subshell/group/negation syntax (`(rm`, `{`, `!`) and shell reserved
/// words. Returns the reason, or `None`.
pub(super) fn opaque_command_word_reason(literal: &str) -> Option<&'static str> {
    let word = effective_command_word(literal);
    if word.is_empty() {
        return None;
    }
    let plain = word.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '_' | '.' | '-' | '+' | ':' | '@' | '%' | ',' | '=' | '/' | '~' | '^'
            )
    });
    if !plain {
        return Some(
            "Kommandoname enthält Expansion, Shell-Syntax oder Nicht-ASCII-Zeichen und \
             konnte nicht sicher bestimmt werden",
        );
    }
    if SHELL_RESERVED_WORDS.contains(&word.to_ascii_lowercase().as_str()) {
        return Some("zusammengesetztes Shell-Konstrukt (if/for/while/case ...)");
    }
    None
}

/// Shells that read their program from stdin when started without a script
/// operand or with `-s`.
const STDIN_SHELLS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ksh", "mksh", "ash", "fish", "csh", "tcsh",
];

/// Interpreters that read their program from stdin when started without a
/// script operand or with the operand `-`.
const STDIN_INTERPRETERS: &[&str] = &["python", "python3", "perl", "ruby", "node", "php"];

/// Whether the segment starts a shell or interpreter that reads its program
/// from stdin (`... | base64 -d | sh`, `curl ... | bash -s`, `... | python3`).
/// The program text never appears in the command, so the engine cannot
/// check it. `literal` as for [`opaque_command_word_reason`].
pub(super) fn reads_program_from_stdin(literal: &str) -> bool {
    let resolved = resolve_effective_command(literal);
    let mut words = resolved.split_whitespace();
    let Some(program) = words.next() else {
        return false;
    };
    let program = program.to_ascii_lowercase();
    let is_shell = STDIN_SHELLS.contains(&program.as_str());
    let is_interpreter = STDIN_INTERPRETERS.contains(&program.as_str());
    if !is_shell && !is_interpreter {
        return false;
    }
    let mut has_operand = false;
    for word in words {
        if matches!(word, "-" | "/dev/stdin" | "/dev/fd/0" | "/proc/self/fd/0") {
            return true;
        }
        if let Some(flags) = word.strip_prefix('-') {
            if is_shell && !flags.starts_with('-') && flags.contains('s') {
                return true;
            }
            continue;
        }
        has_operand = true;
    }
    !has_operand
}

/// If the segment is an `eval` call, returns the code `eval` runs: its
/// arguments with quotes removed, joined by single spaces (what the shell
/// passes to its parser). `literal` as for [`opaque_command_word_reason`].
/// Falls back to the raw argument text when it cannot be tokenised.
pub(super) fn eval_code(literal: &str) -> Option<String> {
    let resolved = resolve_effective_command(literal);
    let trimmed = resolved.trim_start();
    let rest_start = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let (program, rest) = trimmed.split_at(rest_start);
    if !program.eq_ignore_ascii_case("eval") {
        return None;
    }
    let code = match shell_words::split(rest) {
        Ok(words) => words.join(" "),
        Err(_) => rest.trim().to_string(),
    };
    Some(code)
}
