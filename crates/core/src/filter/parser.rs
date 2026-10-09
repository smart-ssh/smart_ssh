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
///
/// Issue #13: besides that third token, the code found by [`program_source`]
/// is returned as well (`sh -c -- CODE`, `bash -c -o x CODE`, `perl -w -e
/// CODE -e CODE2`). Both are evaluated, so the result can only get
/// stricter than with the third token alone.
pub(crate) fn extract_shell_c_style_codes(cmd: &str) -> Vec<String> {
    let mut codes: Vec<String> = extract_shell_c_style_code(cmd).into_iter().collect();
    if codes.is_empty() {
        return codes;
    }
    if let Some(ProgramSource::Code(found)) = program_source(&normalize_whitespace(cmd)) {
        for code in found {
            if !codes.contains(&code) {
                codes.push(code);
            }
        }
    }
    codes
}

fn extract_shell_c_style_code(cmd: &str) -> Option<String> {
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

/// Where a shell, interpreter or `source` call takes the program it runs
/// from — see [`program_source`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ProgramSource {
    /// Code passed as an argument (`sh -c CODE`, `fish --command CODE`,
    /// `python3 -c CODE`, `perl -e CODE`, ...), in the order given.
    Code(Vec<String>),
    /// The program is read from stdin (`... | sh`, `bash -s`, `python3 -`,
    /// `source /dev/stdin`); its text never appears in the command.
    Stdin,
    /// A script or module operand the engine can see (`bash deploy.sh`).
    Operand,
    /// The arguments could not be analysed with confidence (an unknown long
    /// option, untokenisable quoting, `-c` without its code argument).
    Opaque,
}

/// How a program parses its options, as far as [`program_source`] needs to
/// know. Characters are short options; long options are listed without the
/// leading `--`. A long option that is in none of the lists makes the call
/// [`ProgramSource::Opaque`] (fail closed): the engine cannot tell whether
/// it swallows the next word, and guessing "no" would let an option value
/// pass for the script operand (`| bash -o errexit`).
struct OptionSpec {
    /// Short options that say "the first operand is the code" (POSIX
    /// shells' `-c`, also inside clusters such as `-xc`).
    code_operand: &'static str,
    /// Short options that make a shell read its program from stdin (`-s`).
    stdin_flag: &'static str,
    /// Short options whose value is code (`perl -e CODE`, `fish -c CODE`).
    code_value: &'static str,
    /// Short options whose value is a script or module to run (`python3 -m`).
    script_value: &'static str,
    /// Short options that take a value, attached (the rest of the cluster)
    /// or as the next word. When unsure whether an option takes a value, it
    /// belongs here: swallowing one word too many can only make a script
    /// operand look missing, which reads as "stdin" and escalates.
    value: &'static str,
    /// Short options that always take the *next word* as their value, even
    /// inside a cluster, while the rest of the cluster is still read as
    /// options (`bash -oc errexit CODE` runs `CODE`).
    next_word_value: &'static str,
    /// Options (among `value`) whose value names a shell option and may be
    /// that option's single letter: ksh93 reads `-oc`, `-o c` as `-c`. A
    /// one-letter value that is a code or stdin option counts as that
    /// option; a negated (`+o c`, `-o noc`) or case-folded one, or one
    /// naming a value-taking option, makes the call
    /// [`ProgramSource::Opaque`] (fail closed). The value is normalised
    /// first (`-`/`_` dropped, so `-o c_` is `-c`), and a multi-letter
    /// value must be a known option name, else the call is `Opaque` too
    /// (see [`letter_option`]).
    letter_name_value: &'static str,
    /// Short options followed by an optional run of digits, after which the
    /// rest of the cluster is still read as options (`perl -l0e CODE`,
    /// `ruby -W2e CODE`). A hexadecimal value (`perl -0x1ff`) makes the
    /// call [`ProgramSource::Opaque`]: hex digits include `e`.
    digit_value: &'static str,
    /// Short options whose value can only be attached and whose end is not
    /// known (`perl -Mstrict`, `ruby -Ku`). If the rest of the cluster
    /// contains a code, stdin or script option character, the call is
    /// [`ProgramSource::Opaque`] (fail closed): the program might read it
    /// as that option.
    attached_value: &'static str,
    /// Whether `+x`-style options exist (`sh +x`, `bash +o history`).
    plus_options: bool,
    long_code: &'static [&'static str],
    long_script: &'static [&'static str],
    long_value: &'static [&'static str],
    long_flag: &'static [&'static str],
}

/// bash, dash and busybox ash: `-o`/`-O` always take the *next word* as
/// the option name, and the rest of the cluster is still read as options
/// (`bash -oc errexit CODE` runs `CODE`).
const BASH_LIKE_SHELL_OPTIONS: OptionSpec = OptionSpec {
    code_operand: "c",
    stdin_flag: "s",
    code_value: "",
    script_value: "",
    value: "",
    next_word_value: "oO",
    letter_name_value: "",
    digit_value: "",
    attached_value: "",
    plus_options: true,
    long_code: &[],
    long_script: &[],
    long_value: &["rcfile", "init-file"],
    long_flag: &[
        "norc",
        "noprofile",
        "login",
        "posix",
        "noediting",
        "restricted",
        "verbose",
        "debugger",
        "dump-strings",
        "dump-po-strings",
        "version",
        "help",
    ],
};

/// zsh and the ksh family: `-o`/`-O` take the *rest of the cluster* as the
/// option name if it is non-empty (`zsh -oerrexit -c CODE`), and the next
/// word only when they stand alone.
const KSH_LIKE_SHELL_OPTIONS: OptionSpec = OptionSpec {
    code_operand: "c",
    stdin_flag: "s",
    code_value: "",
    script_value: "",
    value: "oO",
    next_word_value: "",
    letter_name_value: "oO",
    digit_value: "",
    attached_value: "",
    plus_options: true,
    long_code: &[],
    long_script: &[],
    long_value: &["rcfile", "init-file"],
    long_flag: &[
        "norc",
        "noprofile",
        "login",
        "posix",
        "noediting",
        "restricted",
        "verbose",
        "debugger",
        "dump-strings",
        "dump-po-strings",
        "version",
        "help",
    ],
};

const FISH_OPTIONS: OptionSpec = OptionSpec {
    code_operand: "",
    stdin_flag: "",
    code_value: "cC",
    script_value: "",
    value: "dof",
    next_word_value: "",
    letter_name_value: "",
    digit_value: "",
    attached_value: "",
    plus_options: false,
    long_code: &["command", "init-command"],
    long_script: &[],
    long_value: &[
        "debug",
        "debug-output",
        "features",
        "profile",
        "profile-startup",
    ],
    long_flag: &[
        "interactive",
        "login",
        "no-execute",
        "no-config",
        "private",
        "print-rusage-self",
        "print-debug-categories",
        "version",
        "help",
    ],
};

const CSH_OPTIONS: OptionSpec = OptionSpec {
    code_operand: "c",
    stdin_flag: "s",
    code_value: "",
    script_value: "",
    value: "",
    next_word_value: "",
    letter_name_value: "",
    digit_value: "",
    attached_value: "",
    plus_options: false,
    long_code: &[],
    long_script: &[],
    long_value: &[],
    long_flag: &["version", "help"],
};

const PYTHON_OPTIONS: OptionSpec = OptionSpec {
    code_operand: "",
    stdin_flag: "",
    code_value: "c",
    script_value: "m",
    value: "WXQ",
    next_word_value: "",
    letter_name_value: "",
    digit_value: "",
    attached_value: "",
    plus_options: false,
    long_code: &[],
    long_script: &[],
    long_value: &["check-hash-based-pycs"],
    long_flag: &["version", "help", "help-env", "help-xoptions", "help-all"],
};

const PERL_OPTIONS: OptionSpec = OptionSpec {
    code_operand: "",
    stdin_flag: "",
    code_value: "eE",
    script_value: "",
    value: "I",
    next_word_value: "",
    letter_name_value: "",
    digit_value: "0l",
    attached_value: "CdDimMVx",
    plus_options: false,
    long_code: &[],
    long_script: &[],
    long_value: &[],
    long_flag: &["version", "help"],
};

const RUBY_OPTIONS: OptionSpec = OptionSpec {
    code_operand: "",
    stdin_flag: "",
    code_value: "e",
    script_value: "",
    value: "CEIr",
    next_word_value: "",
    letter_name_value: "",
    digit_value: "0TW",
    attached_value: "FKx",
    plus_options: false,
    long_code: &[],
    long_script: &[],
    long_value: &[
        "enable",
        "disable",
        "encoding",
        "external-encoding",
        "internal-encoding",
        "dump",
        "backtrace-limit",
        "crash-report",
        "parser",
    ],
    long_flag: &["version", "help", "verbose", "copyright", "yjit", "jit"],
};

const NODE_OPTIONS: OptionSpec = OptionSpec {
    code_operand: "",
    stdin_flag: "",
    code_value: "ep",
    script_value: "",
    value: "Cr",
    next_word_value: "",
    letter_name_value: "",
    digit_value: "",
    attached_value: "",
    plus_options: false,
    long_code: &["eval", "print"],
    long_script: &[],
    long_value: &[
        "require",
        "import",
        "loader",
        "experimental-loader",
        "input-type",
        "conditions",
        "env-file",
        "title",
    ],
    long_flag: &[
        "version",
        "help",
        "check",
        "interactive",
        "no-warnings",
        "no-deprecation",
        "trace-warnings",
        "enable-source-maps",
    ],
};

const PHP_OPTIONS: OptionSpec = OptionSpec {
    code_operand: "",
    stdin_flag: "",
    code_value: "BERr",
    script_value: "Ff",
    value: "cdzSt",
    next_word_value: "",
    letter_name_value: "",
    digit_value: "",
    attached_value: "",
    plus_options: false,
    long_code: &[],
    long_script: &[],
    long_value: &[],
    long_flag: &["version", "help", "info", "ini"],
};

/// Shells with [`BASH_LIKE_SHELL_OPTIONS`]: `-c` makes the first operand
/// the code, `-s` reads stdin, `-o`/`+o` take the next word.
const BASH_LIKE_SHELLS: &[&str] = &["bash", "rbash", "dash", "ash"];

/// Shells with [`KSH_LIKE_SHELL_OPTIONS`]: like [`BASH_LIKE_SHELLS`], but
/// `-o`/`+o` take an attached value. Issue #60 adds the restricted variants
/// `rksh` (ksh93) and `rzsh` (zsh) and `oksh` (OpenBSD ksh, a pdksh
/// descendant).
const KSH_LIKE_SHELLS: &[&str] = &[
    "zsh", "rzsh", "ksh", "ksh93", "rksh", "mksh", "lksh", "pdksh", "oksh",
];

/// POSIX shells whose `-o` syntax is not known for certain: `sh` can be any
/// of the shells above, depending on the system. Both readings are
/// analysed and merged by [`merge_program_sources`] (fail closed).
const UNKNOWN_POSIX_SHELLS: &[&str] = &["sh", "yash", "posh"];

/// What kind of program `program` (lower-cased basename) is.
enum ProgramKind {
    WithOptions(&'static OptionSpec, bool),
    /// A shell whose option syntax may be either of the two.
    EitherShell(&'static OptionSpec, &'static OptionSpec),
    Source,
}

fn program_kind(program: &str) -> Option<ProgramKind> {
    let program = program.rsplit('/').next().unwrap_or(program);
    let program = program.to_ascii_lowercase();
    let program = program.as_str();
    // Issue #60: a versioned binary of a known shell (`bash5`, `bash-5.2`,
    // `zsh-5.9`, `ksh2020`, `ksh93u+m`) gets the family of its base name.
    // Only the exact name and shell base names are looked up, so this can
    // only classify more calls, never fewer.
    exact_program_kind(program)
        .or_else(|| versioned_shell_base(program).and_then(exact_program_kind))
}

/// Shell names whose versioned binaries (`<name><version>`) are recognised
/// by [`versioned_shell_base`].
fn known_shell_names() -> impl Iterator<Item = &'static str> {
    BASH_LIKE_SHELLS
        .iter()
        .chain(KSH_LIKE_SHELLS)
        .chain(UNKNOWN_POSIX_SHELLS)
        .chain(&["fish", "csh", "tcsh"])
        .copied()
}

/// The known shell `program` (lower-cased basename) is a versioned binary
/// of: the shell name, optionally `-` or `_`, then a version that starts
/// with a digit and contains only ASCII letters, digits, `.`, `+`, `-` and
/// `_` (`bash5`, `bash-5.2`, `zsh-5.9`, `ksh2020`, `ksh93u+m`). Requiring a
/// digit right after the name keeps unrelated names (`sha1sum`, `shred`,
/// `bashbug`) out.
fn versioned_shell_base(program: &str) -> Option<&'static str> {
    known_shell_names().find(|&shell| {
        program.strip_prefix(shell).is_some_and(|rest| {
            let version = rest
                .strip_prefix('-')
                .or_else(|| rest.strip_prefix('_'))
                .unwrap_or(rest);
            version.starts_with(|c: char| c.is_ascii_digit())
                && version
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-' | '_'))
        })
    })
}

/// [`program_kind`] for an exact lower-cased basename.
fn exact_program_kind(program: &str) -> Option<ProgramKind> {
    if BASH_LIKE_SHELLS.contains(&program) {
        return Some(ProgramKind::WithOptions(&BASH_LIKE_SHELL_OPTIONS, true));
    }
    if KSH_LIKE_SHELLS.contains(&program) {
        return Some(ProgramKind::WithOptions(&KSH_LIKE_SHELL_OPTIONS, true));
    }
    if UNKNOWN_POSIX_SHELLS.contains(&program) {
        return Some(ProgramKind::EitherShell(
            &BASH_LIKE_SHELL_OPTIONS,
            &KSH_LIKE_SHELL_OPTIONS,
        ));
    }
    let interpreter = |spec| Some(ProgramKind::WithOptions(spec, false));
    match program {
        "fish" => Some(ProgramKind::WithOptions(&FISH_OPTIONS, true)),
        "csh" | "tcsh" => Some(ProgramKind::WithOptions(&CSH_OPTIONS, true)),
        "perl" => interpreter(&PERL_OPTIONS),
        "ruby" => interpreter(&RUBY_OPTIONS),
        "node" | "nodejs" => interpreter(&NODE_OPTIONS),
        "php" => interpreter(&PHP_OPTIONS),
        "source" | "." => Some(ProgramKind::Source),
        // `python`, `python3`, `python3.12`, ...
        _ if program
            .strip_prefix("python")
            .is_some_and(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.')) =>
        {
            interpreter(&PYTHON_OPTIONS)
        }
        _ => None,
    }
}

/// An argument that names the process's own stdin.
fn is_stdin_path(word: &str) -> bool {
    matches!(word, "-" | "/dev/stdin")
        || word.starts_with("/dev/fd/")
        || (word.starts_with("/proc/") && word.contains("/fd/"))
}

/// Analyses one segment that starts (after wrappers, `sudo` and variable
/// assignments) with a shell, a script interpreter, or `source`/`.`, and
/// says where the program it runs comes from. Returns `None` for any other
/// command. `literal` as for [`opaque_command_word_reason`].
///
/// Options are parsed per program ([`OptionSpec`]), so option values
/// (`bash -o errexit`, `python3 -W ignore`), `+` options (`sh +x`) and an
/// option before `-c` (`bash -e -c CODE`) are not mistaken for the script
/// operand or missed.
pub(super) fn program_source(literal: &str) -> Option<ProgramSource> {
    let resolved = resolve_effective_command(literal);
    let program = resolved.split_whitespace().next()?;
    let Some(kind) = program_kind(program) else {
        return unclassified_shell_source(program, &resolved);
    };
    let Ok(words) = shell_words::split(&resolved) else {
        return Some(ProgramSource::Opaque);
    };
    let args = words.get(1..).unwrap_or_default();
    Some(match kind {
        ProgramKind::Source => {
            if args.iter().any(|arg| is_stdin_path(arg)) {
                ProgramSource::Stdin
            } else {
                ProgramSource::Operand
            }
        }
        ProgramKind::WithOptions(spec, is_shell) => analyse_options(spec, is_shell, args),
        ProgramKind::EitherShell(a, b) => merge_program_sources(
            analyse_options(a, true, args),
            analyse_options(b, true, args),
        ),
    })
}

/// Basenames that end in `sh` but are not shells, so the fail-closed
/// fallback in [`unclassified_shell_source`] leaves them alone. `ssh` and
/// its relatives take a cipher with `-c`; `chsh -s` sets a login shell; the
/// others are ordinary commands and shell builtins whose `-c`/`-s` (if any)
/// has nothing to do with running code.
const NON_SHELL_SH_NAMES: &[&str] = &[
    "ssh", "autossh", "lsh", "chsh", "lchsh", "ypchsh", "flush", "fdflush", "crash", "push",
    "publish", "refresh", "rehash", "hash", "finish",
];

/// Issue #60: fail-closed fallback for a command whose basename looks like a
/// shell [`program_kind`] does not know (`mysh`, `pwsh`, `xonsh`, ...): it
/// ends in `sh`, consists only of ASCII letters, digits, `-` and `_` (so
/// `deploy.sh` and other script files are not covered), and is not in
/// [`NON_SHELL_SH_NAMES`].
///
/// Such a call is only flagged when it carries a short option cluster with
/// `c` or `s` (`-c`, `-s`, `-xc`, `+s`) before `--`. It is then read like
/// `sh` (both option readings, merged fail closed): code found that way is
/// returned as [`ProgramSource::Code`], so it is evaluated recursively and a
/// `Deny` rule still applies; otherwise it is at least
/// [`ProgramSource::Stdin`] or [`ProgramSource::Opaque`], i.e. `Confirm`.
/// Any other call returns `None`, exactly as before.
fn unclassified_shell_source(program: &str, resolved: &str) -> Option<ProgramSource> {
    let name = program.rsplit('/').next().unwrap_or(program);
    let name = name.to_ascii_lowercase();
    let looks_like_shell = name.len() > 2
        && name.ends_with("sh")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        && !NON_SHELL_SH_NAMES.contains(&name.as_str());
    if !looks_like_shell {
        return None;
    }
    let words = shell_words::split(resolved)
        .unwrap_or_else(|_| resolved.split_whitespace().map(str::to_string).collect());
    let args = words.get(1..).unwrap_or_default();
    let has_code_or_stdin_option = args
        .iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| {
            let cluster = arg
                .strip_prefix('-')
                .filter(|rest| !rest.starts_with('-'))
                .or_else(|| arg.strip_prefix('+'));
            cluster.is_some_and(|c| c.contains(['c', 's']))
        });
    if !has_code_or_stdin_option {
        return None;
    }
    Some(
        match merge_program_sources(
            analyse_options(&BASH_LIKE_SHELL_OPTIONS, true, args),
            analyse_options(&KSH_LIKE_SHELL_OPTIONS, true, args),
        ) {
            source @ (ProgramSource::Code(_) | ProgramSource::Stdin) => source,
            ProgramSource::Operand | ProgramSource::Opaque => ProgramSource::Opaque,
        },
    )
}

/// Merges two readings of the same call into one that is at least as
/// strict as each (fail closed): the code of both readings if either finds
/// code (the engine adds a `Confirm` floor for code and evaluates each part,
/// so a `Deny` behind either reading still applies), the common result if
/// both agree, and [`ProgramSource::Opaque`] otherwise.
fn merge_program_sources(a: ProgramSource, b: ProgramSource) -> ProgramSource {
    match (a, b) {
        (ProgramSource::Code(mut a), ProgramSource::Code(b)) => {
            for code in b {
                if !a.contains(&code) {
                    a.push(code);
                }
            }
            ProgramSource::Code(a)
        }
        (ProgramSource::Code(code), _) | (_, ProgramSource::Code(code)) => {
            ProgramSource::Code(code)
        }
        (a, b) if a == b => a,
        _ => ProgramSource::Opaque,
    }
}

/// Option names (normalised: lower case, without `-`/`_`) that zsh and the
/// ksh family accept for `-o`/`+o` and that do not change where the shell
/// reads its program from. Any other multi-letter name makes the call
/// [`ProgramSource::Opaque`] (fail closed): the shell might read it as a
/// short option (ksh93 ignores `-`/`_` in names, so `-o c_` means `-c`),
/// as an abbreviation, or as a name this list does not know yet.
const KNOWN_SHELL_OPTION_NAMES: &[&str] = &[
    // ksh93, mksh and the POSIX `set -o` names.
    "allexport",
    "bgnice",
    "braceexpand",
    "emacs",
    "errexit",
    "errtrace",
    "functrace",
    "globstar",
    "gmacs",
    "hashall",
    "histexpand",
    "history",
    "ignoreeof",
    "inheritxtrace",
    "interactive",
    "keyword",
    "letoctal",
    "login",
    "markdirs",
    "monitor",
    "multiline",
    "noclobber",
    "noexec",
    "noglob",
    "nohup",
    "nolog",
    "notify",
    "nounset",
    "physical",
    "pipefail",
    "posix",
    "privileged",
    "restricted",
    "sh",
    "showme",
    "trackall",
    "utf8mode",
    "verbose",
    "vi",
    "viesccomplete",
    "viraw",
    "vitabcomplete",
    "xtrace",
    // zsh (names that are not already listed above).
    "aliases",
    "alwayslastprompt",
    "alwaystoend",
    "appendhistory",
    "autocd",
    "autolist",
    "automenu",
    "autonamedirs",
    "autoparamkeys",
    "autoparamslash",
    "autopushd",
    "autoremoveslash",
    "autoresume",
    "badpattern",
    "banghist",
    "bareglobqual",
    "bashautolist",
    "bashrematch",
    "beep",
    "braceccl",
    "bsdecho",
    "caseglob",
    "casematch",
    "cbases",
    "cdablevars",
    "chasedots",
    "chaselinks",
    "checkjobs",
    "checkrunningjobs",
    "clobber",
    "combiningchars",
    "completealiases",
    "completeinword",
    "correct",
    "correctall",
    "cprecedences",
    "cshjunkiehistory",
    "cshjunkieloops",
    "cshjunkiequotes",
    "cshnullcmd",
    "cshnullglob",
    "equals",
    "errreturn",
    "evallineno",
    "exec",
    "extendedglob",
    "extendedhistory",
    "flowcontrol",
    "functionargzero",
    "glob",
    "globalexport",
    "globalrcs",
    "globassign",
    "globcomplete",
    "globdots",
    "globsubst",
    "hashcmds",
    "hashdirs",
    "hashexecutablesonly",
    "hashlistall",
    "histallowclobber",
    "histbeep",
    "histexpiredupsfirst",
    "histfcntllock",
    "histfindnodups",
    "histignorealldups",
    "histignoredups",
    "histignorespace",
    "histlexwords",
    "histnofunctions",
    "histnostore",
    "histreduceblanks",
    "histsavebycopy",
    "histsavenodups",
    "histsubstpattern",
    "histverify",
    "hup",
    "ignorebraces",
    "ignoreclosebraces",
    "incappendhistory",
    "incappendhistorytime",
    "interactivecomments",
    "ksharrays",
    "kshautoload",
    "kshglob",
    "kshoptionprint",
    "kshtypeset",
    "kshzerosubscript",
    "listambiguous",
    "listbeep",
    "listpacked",
    "listrowsfirst",
    "listtypes",
    "localloops",
    "localoptions",
    "localpatterns",
    "localtraps",
    "longlistjobs",
    "magicequalsubst",
    "mailwarning",
    "menucomplete",
    "multibyte",
    "multifuncdef",
    "multios",
    "nomatch",
    "nullglob",
    "numericglobsort",
    "octalzeroes",
    "overstrike",
    "pathdirs",
    "pathscript",
    "posixaliases",
    "posixargzero",
    "posixbuiltins",
    "posixcd",
    "posixidentifiers",
    "posixjobs",
    "posixstrings",
    "posixtraps",
    "printeightbit",
    "printexitvalue",
    "promptbang",
    "promptcr",
    "promptpercent",
    "promptsp",
    "promptsubst",
    "pushdignoredups",
    "pushdminus",
    "pushdsilent",
    "pushdtohome",
    "rcexpandparam",
    "rcquotes",
    "rcs",
    "recexact",
    "rematchpcre",
    "rmstarsilent",
    "rmstarwait",
    "sharehistory",
    "shfileexpansion",
    "shglob",
    "shnullcmd",
    "shoptionletters",
    "shortloops",
    "shortrepeat",
    "shwordsplit",
    "singlecommand",
    "singlelinezle",
    "sourcetrace",
    "sunkeyboardhack",
    "transientrprompt",
    "trapsasync",
    "typesetsilent",
    "typesettounset",
    "unset",
    "warncreateglobal",
    "warnnestedvar",
    "zle",
];

/// Option names (normalised as above) that make the shell read its program
/// from stdin, like `-s`: zsh's `SHIN_STDIN`, mksh's `stdin`.
const STDIN_SHELL_OPTION_NAMES: &[&str] = &["shinstdin", "stdin"];

/// What an option name given to `-o` means (see
/// [`OptionSpec::letter_name_value`]).
enum LetterOption {
    CodeOperand,
    Stdin,
    Opaque,
}

/// Reads `name`, the value of an `-o`-style option (`minus` is false for
/// `+o`). The name is normalised first, the way ksh93 and zsh compare
/// option names: `-`/`_` are dropped (`no-c`, `c_` → `noc`, `c`) and a
/// leading `no` negates.
///
/// - A known option name ([`KNOWN_SHELL_OPTION_NAMES`]) or a harmless
///   single letter (`x`) gives `None`.
/// - A name meaning "read from stdin" ([`STDIN_SHELL_OPTION_NAMES`]) or the
///   letter of a stdin option gives `Stdin`, the letter of the code option
///   gives `CodeOperand`.
/// - Everything else fails closed (`Opaque`): an unknown multi-letter name,
///   an empty name, or a negated (`+o c`, `-o no-c`), case-folded (`-o C`)
///   or value-taking (`-o o`) dangerous letter.
fn letter_option(spec: &OptionSpec, name: &str, minus: bool) -> Option<LetterOption> {
    let normalised: String = name.chars().filter(|&c| c != '-' && c != '_').collect();
    let lower = normalised.to_ascii_lowercase();
    let (negated, stripped) = match lower.strip_prefix("no") {
        Some(rest) => (true, rest),
        None => (false, lower.as_str()),
    };
    if STDIN_SHELL_OPTION_NAMES.contains(&lower.as_str()) {
        return Some(if minus {
            LetterOption::Stdin
        } else {
            LetterOption::Opaque
        });
    }
    if negated && STDIN_SHELL_OPTION_NAMES.contains(&stripped) {
        return Some(LetterOption::Opaque);
    }
    if KNOWN_SHELL_OPTION_NAMES.contains(&lower.as_str())
        || (negated && KNOWN_SHELL_OPTION_NAMES.contains(&stripped))
    {
        return None;
    }
    // Single letter, case kept (ksh93 option letters are case-sensitive).
    let letter = if normalised.chars().count() == 1 {
        normalised.as_str()
    } else if negated && stripped.chars().count() == 1 {
        &normalised[2..]
    } else {
        return Some(LetterOption::Opaque);
    };
    let negated = letter.len() < normalised.len();
    let c = letter.chars().next()?;
    let dangerous = |d: char| {
        spec.code_operand.contains(d)
            || spec.stdin_flag.contains(d)
            || spec.code_value.contains(d)
            || spec.script_value.contains(d)
            || spec.value.contains(d)
            || spec.next_word_value.contains(d)
            || spec.attached_value.contains(d)
            || spec.digit_value.contains(d)
    };
    let folded = |d: char| dangerous(d.to_ascii_lowercase()) || dangerous(d.to_ascii_uppercase());
    if !dangerous(c) {
        return folded(c).then_some(LetterOption::Opaque);
    }
    if negated || !minus {
        return Some(LetterOption::Opaque);
    }
    if spec.code_operand.contains(c) {
        Some(LetterOption::CodeOperand)
    } else if spec.stdin_flag.contains(c) {
        Some(LetterOption::Stdin)
    } else {
        Some(LetterOption::Opaque)
    }
}

fn analyse_options(spec: &OptionSpec, is_shell: bool, args: &[String]) -> ProgramSource {
    let mut codes: Vec<String> = Vec::new();
    let mut code_operand = false;
    let mut stdin = false;
    let mut script = false;
    let mut i = 0usize;

    // Takes an option's value: the rest of the cluster if non-empty, else
    // the next word. `None` if the value is missing.
    let take_value = |attached: &str, i: &mut usize| -> Option<String> {
        if !attached.is_empty() {
            return Some(attached.to_string());
        }
        let value = args.get(*i).cloned();
        if value.is_some() {
            *i += 1;
        }
        value
    };

    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if arg == "--" {
            break;
        }
        if arg == "-" {
            // A shell treats a lone `-` like `--`; an interpreter reads its
            // program from stdin.
            if !is_shell {
                stdin = true;
            }
            break;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (long, None),
            };
            let in_list = |list: &[&str]| list.contains(&name);
            if in_list(spec.long_code) {
                match inline.or_else(|| take_value("", &mut i)) {
                    Some(code) => codes.push(code),
                    None => return ProgramSource::Opaque,
                }
            } else if in_list(spec.long_script) || in_list(spec.long_value) {
                script |= in_list(spec.long_script);
                let value = inline.or_else(|| take_value("", &mut i));
                stdin |= value.is_some_and(|v| is_stdin_path(&v));
            } else if !in_list(spec.long_flag) {
                return ProgramSource::Opaque;
            }
            continue;
        }
        let cluster = match arg.strip_prefix('-') {
            Some(cluster) => cluster,
            None => match arg.strip_prefix('+') {
                Some(cluster) if spec.plus_options && !cluster.is_empty() => cluster,
                // First operand: options end here.
                _ => {
                    i -= 1;
                    break;
                }
            },
        };
        let minus = arg.starts_with('-');
        // Bytes of the cluster already consumed as a digit value.
        let mut skip = 0usize;
        for (pos, c) in cluster.char_indices() {
            if skip > 0 {
                skip -= c.len_utf8();
                continue;
            }
            let attached = &cluster[pos + c.len_utf8()..];
            if spec.code_operand.contains(c) {
                code_operand = true;
            } else if spec.stdin_flag.contains(c) && minus {
                stdin = true;
            } else if spec.code_value.contains(c) {
                match take_value(attached, &mut i) {
                    Some(code) => codes.push(code),
                    None => return ProgramSource::Opaque,
                }
                break;
            } else if spec.script_value.contains(c) || spec.value.contains(c) {
                script |= spec.script_value.contains(c);
                let value = take_value(attached, &mut i);
                stdin |= value.as_deref().is_some_and(is_stdin_path);
                if spec.letter_name_value.contains(c) {
                    match value.as_deref().and_then(|v| letter_option(spec, v, minus)) {
                        Some(LetterOption::CodeOperand) => code_operand = true,
                        Some(LetterOption::Stdin) => stdin = true,
                        Some(LetterOption::Opaque) => return ProgramSource::Opaque,
                        None => {}
                    }
                }
                break;
            } else if spec.next_word_value.contains(c) {
                stdin |= take_value("", &mut i).is_some_and(|v| is_stdin_path(&v));
            } else if spec.digit_value.contains(c) {
                if c == '0' && attached.starts_with(['x', 'X']) {
                    return ProgramSource::Opaque;
                }
                let digits = attached.len()
                    - attached
                        .trim_start_matches(|d: char| d.is_ascii_digit())
                        .len();
                skip = digits;
            } else if spec.attached_value.contains(c) {
                let dangerous = |d: char| {
                    spec.code_operand.contains(d)
                        || spec.code_value.contains(d)
                        || spec.stdin_flag.contains(d)
                        || spec.script_value.contains(d)
                };
                if attached.contains(dangerous) {
                    return ProgramSource::Opaque;
                }
                stdin |= is_stdin_path(attached);
                break;
            }
        }
    }

    let rest = &args[i.min(args.len())..];
    if code_operand {
        match rest.first() {
            Some(code) => codes.push(code.clone()),
            None if codes.is_empty() => return ProgramSource::Opaque,
            None => {}
        }
    }
    if !codes.is_empty() {
        return ProgramSource::Code(codes);
    }
    // Conservative: any argument naming stdin counts, even one meant for
    // the script (`bash x.sh /dev/stdin`) — that only costs a confirmation.
    if stdin || rest.iter().any(|arg| is_stdin_path(arg)) {
        return ProgramSource::Stdin;
    }
    if script || !rest.is_empty() {
        ProgramSource::Operand
    } else {
        ProgramSource::Stdin
    }
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

/// Which shell construct stores code for later execution (issue #55).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DeferredCodeKind {
    /// `trap ACTION SIGNAL...`: the action runs when a signal arrives or the
    /// shell exits.
    Trap,
    /// `alias NAME=VALUE`: the value runs whenever a later command starts
    /// with `NAME`.
    Alias,
}

/// A segment that stores shell code to run later (issue #55).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DeferredCode {
    pub(super) kind: DeferredCodeKind,
    /// The stored code, one entry per handler/alias value, quotes removed.
    /// Empty when the code could not be extracted reliably — the segment is
    /// then only floored at `Confirm` (fail closed).
    pub(super) codes: Vec<String>,
}

/// Whether `word` can be a signal specification (`EXIT`, `INT`, `SIGTERM`,
/// `15`, `RTMIN+1`). A lone `trap WORD` with such a word resets that signal;
/// any other lone operand is treated as code, fail closed.
fn is_signal_spec(word: &str) -> bool {
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
}

/// If the segment is a `trap` call that sets a handler, or an `alias` call
/// that defines at least one alias, returns the code it stores. `literal` as
/// for [`opaque_command_word_reason`]; wrappers such as `command`/`builtin`
/// are resolved first, like for [`eval_code`].
///
/// Exempt (returns `None`), because they store no code:
/// - `trap`, `trap -p [SIG...]`, `trap -l` (listing);
/// - `trap - SIG...`, `trap '' SIG...` (reset to default / ignore the
///   signal);
/// - `trap SIG` with a single operand that looks like a signal (reset);
/// - `alias`, `alias NAME...`, `alias -p` (listing — no argument with `=`).
///
/// Everything else that is not clearly one of those forms counts as a
/// definition (fail closed): an unknown `trap` option, a lone `trap`
/// operand that is not a signal name, or input that cannot be tokenised
/// but contains a `=` (alias) or an operand (trap).
pub(super) fn deferred_code(literal: &str) -> Option<DeferredCode> {
    let resolved = resolve_effective_command(literal);
    let trimmed = resolved.trim_start();
    let rest_start = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let (program, rest) = trimmed.split_at(rest_start);
    let kind = if program.eq_ignore_ascii_case("trap") {
        DeferredCodeKind::Trap
    } else if program.eq_ignore_ascii_case("alias") {
        DeferredCodeKind::Alias
    } else {
        return None;
    };
    let Ok(args) = shell_words::split(rest) else {
        // Not tokenisable: no reliable extraction. Read-only forms never
        // need quoting, so anything that might define code is floored.
        let might_define = match kind {
            DeferredCodeKind::Trap => !rest.trim().is_empty(),
            DeferredCodeKind::Alias => rest.contains('='),
        };
        return might_define.then_some(DeferredCode {
            kind,
            codes: Vec::new(),
        });
    };
    let codes = match kind {
        DeferredCodeKind::Trap => trap_handler(&args)?,
        DeferredCodeKind::Alias => alias_values(&args)?,
    };
    Some(DeferredCode { kind, codes })
}

/// `trap` arguments → the handler it sets (as a one-element list), or
/// `None` for the read-only and reset forms listed at [`deferred_code`].
/// An unknown option yields an empty list (`Confirm`, nothing to evaluate).
fn trap_handler(args: &[String]) -> Option<Vec<String>> {
    let mut i = 0usize;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "--" => {
                i += 1;
                break;
            }
            // `-p`/`-l` only list handlers/signals; with them, the
            // remaining operands are signal names, not an action.
            "-p" | "-l" | "-lp" | "-pl" => return None,
            // `-` alone is the reset action, handled below as an operand.
            "-" => break,
            _ if arg.starts_with('-') && arg.len() > 1 => return Some(Vec::new()),
            _ => break,
        }
    }
    let operands = &args[i.min(args.len())..];
    match operands {
        [] => None,
        [single] if is_signal_spec(single) => None,
        [action, ..] if action == "-" || action.is_empty() => None,
        [action, ..] => Some(vec![action.clone()]),
    }
}

/// `alias` arguments → the value of every `NAME=VALUE` definition, or
/// `None` when there is none (listing forms). Any argument with a `=`
/// counts, even one that starts with `-` (fail closed: shells differ in
/// which options they accept before a definition).
fn alias_values(args: &[String]) -> Option<Vec<String>> {
    let values: Vec<String> = args
        .iter()
        .filter_map(|arg| arg.split_once('=').map(|(_, value)| value.to_string()))
        .collect();
    (!values.is_empty()).then_some(values)
}
