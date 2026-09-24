//! Zeilenweiser `ssh_config`-Parser (Spec 0075, §4.2 und Klarstellung M-1).
//!
//! **Liest keine Dateien.** Er bekommt Bytes und gibt Blöcke, `Include`-
//! Rohwerte und nicht übernommene Direktivennamen zurück; wer die Bytes
//! beschafft und `Include` auflöst, ist `app-shell` (§4.1). Damit bleibt
//! die Auflösung dort, wo Tiefe, Schleifenerkennung und Gesamtgrenzen
//! durchgesetzt werden, und dieses Modul ist ohne Dateisystem prüfbar.
//!
//! **Was hier nicht passiert, ist genauso wichtig wie was passiert:** Von
//! einer Zeile, die wir nicht übernehmen, verlässt nur ihr *Name* dieses
//! Modul, nie ihr Wert (§3.1.5, §5.4 Regel 2). Das ist der Grund, warum
//! §5.4 eine Eigenschaft des Parsers ist und keine Liste von Verboten
//! weiter außen (E-7).

use super::pattern::HostClause;
use crate::profiles::INVISIBLE_CREDENTIAL_EDGE_CHARS;

/// §3.3: Länge eines einzelnen Werts. Wird sie überschritten, bricht der
/// **ganze** Import ab (§3.1.13) — anders als eine unbekannte Direktive,
/// die nur gemeldet wird.
pub const MAX_VALUE_CHARS: usize = 4_096;

/// Die Direktiven aus §3.1.2, die wir tatsächlich übernehmen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Known {
    Host,
    HostName,
    Port,
    User,
    ProxyJump,
    IdentityFile,
    Include,
}

impl Known {
    fn from_keyword(kw_lower: &str) -> Option<Self> {
        Some(match kw_lower {
            "host" => Known::Host,
            "hostname" => Known::HostName,
            "port" => Known::Port,
            "user" => Known::User,
            "proxyjump" => Known::ProxyJump,
            "identityfile" => Known::IdentityFile,
            "include" => Known::Include,
            _ => return None,
        })
    }
}

/// Echte `ssh_config`-Schlüsselwörter, die wir **nicht** übernehmen.
///
/// Zweck ist allein §3.1.4a (§9/Q-1, Punkt 3): zu entscheiden, ob eine
/// eingebundene Datei überhaupt eine `ssh_config` ist. Über die Datei
/// entscheidet die **Liste**, über „Name oder `unlesbare Zeile`"
/// anschließend nur noch die **Form** (§3.1.5) — zwei Ebenen, weil die
/// Form allein §6.4.9 (b) bräche: In einer Prosadatei hat fast jede Zeile
/// ein formgültiges erstes Wort.
///
/// **Bewusst kurz** (§9/Q-1, Punkt 3: „Im Zweifel die kürzere Liste").
/// Jedes Wort hier ist ein Wort, mit dem eine Prosazeile beginnen und die
/// Datei damit als `ssh_config` durchgehen lassen könnte. Eine Lücke
/// kostet dagegen nur, dass eine dünn besetzte echte `ssh_config`
/// übersprungen und mit ihrem Pfad gemeldet wird — die Richtung, in der
/// nichts nach außen gelangt. Eine echte `ssh_config`, aus der ein Profil
/// entstehen soll, enthält ohnehin `Host` (aus [`Known`]).
///
/// `Compression` steht hier, weil §9/Q-1 Punkt 7 es für den Prüfling von
/// §6.4.9a verlangt.
const OTHER_KEYWORDS: &[&str] = &[
    "match",
    "compression",
    "addkeystoagent",
    "casignaturealgorithms",
    "certificatefile",
    "checkhostip",
    "controlmaster",
    "controlpath",
    "controlpersist",
    "dynamicforward",
    "globalknownhostsfile",
    "hostbasedauthentication",
    "hostkeyalgorithms",
    "hostkeyalias",
    "identitiesonly",
    "identityagent",
    "ignoreunknown",
    "kbdinteractiveauthentication",
    "kexalgorithms",
    "localforward",
    "loglevel",
    "passwordauthentication",
    "preferredauthentications",
    "proxycommand",
    "pubkeyacceptedalgorithms",
    "pubkeyauthentication",
    "remoteforward",
    "serveralivecountmax",
    "serveraliveinterval",
    "stricthostkeychecking",
    "tcpkeepalive",
    "userknownhostsfile",
    "visualhostkey",
];

/// Ein `Host`-Block mit den Werten, die in ihm **selbst** stehen.
///
/// Bewusst **ohne** Vererbung aus Platzhalterblöcken: Das Zusammenlegen
/// nach „der erste gewinnt" macht [`super::plan`], weil nur dort die
/// Herkunft jedes Feldes mitgeschrieben werden kann, die §3.1.3 für die
/// Vorschau verlangt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostBlock {
    pub clauses: Vec<HostClause>,
    /// Zeile der `Host`-Direktive selbst (1-basiert).
    pub line: u32,
    pub host_name: Option<Value>,
    pub port: Option<Value>,
    pub user: Option<Value>,
    pub proxy_jump: Option<Value>,
    /// Nur der **erste** `IdentityFile`-Wert; weitere stehen als nicht
    /// übernommen in [`ParsedFile::skipped`] (§3.1.9, letzter Absatz).
    pub identity_file: Option<Value>,
}

impl HostBlock {
    /// §3.1.3: Ein Block, dessen Angaben einen Platzhalter tragen,
    /// beschreibt keinen einzelnen Server und wird **nie** ein Profil.
    ///
    /// Es genügt, dass **eine** Angabe ein Platzhalter ist: `Host web1 *.de`
    /// ist damit ein Platzhalterblock. Die Alternative — `web1` als Profil
    /// und `*.de` als Vorgabe zu behandeln — wäre eine Sonderregel, die in
    /// der Spec nicht steht; und ein Block, der beides mischt, ist
    /// erfahrungsgemäß eher eine Vorgabe als eine Serverliste.
    pub fn is_wildcard(&self) -> bool {
        self.clauses.iter().any(|c| c.is_wildcard())
    }
}

/// Ein übernommener Wert samt Zeile, aus der er stammt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub value: String,
    pub line: u32,
}

/// Eine `Include`-Direktive — als **Rohwert**. Auflösen ist Sache von
/// `app-shell` (§4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeDirective {
    pub raw: String,
    pub line: u32,
}

/// Eine Zeile, die nicht übernommen wurde (§3.1.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedDirective {
    pub line: u32,
    pub kind: SkippedKind,
    /// Zeile der `Host`-Direktive des Blocks, in dem die Zeile stand —
    /// `None` für den Bereich vor dem ersten `Host` (§3.1.5 „je Block").
    pub block_line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkippedKind {
    /// Der **Name** der Direktive, nie ihr Wert (§3.1.5, §5.4 Regel 2).
    Directive(String),
    /// Das erste Wort sieht nicht wie ein Direktivenname aus. Dann wird
    /// **nur** die Zeilennummer gemeldet und sonst nichts (§3.1.5) — hier
    /// steckt deshalb bewusst kein Feld für Inhalt.
    UnreadableLine,
}

/// Ergebnis für **eine** gelesene Datei.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileParse {
    /// §3.1.4a: NUL-Byte, kein gültiges UTF-8, oder keine einzige erkannte
    /// Direktive. Trägt **keinen** Inhalt — das ist der Kern von §5.4.
    NotSshConfig,
    Parsed(ParsedFile),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedFile {
    pub blocks: Vec<HostBlock>,
    pub includes: Vec<IncludeDirective>,
    pub skipped: Vec<SkippedDirective>,
    /// Für die Gesamtgrenze aus §3.3, die `app-shell` über alle Dateien
    /// zieht.
    pub line_count: u32,
}

/// §3.3: Ein einzelner Wert ist länger als [`MAX_VALUE_CHARS`]. Trägt die
/// Zeilennummer, **nicht** den Wert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueTooLong {
    pub line: u32,
    pub chars: usize,
}

/// §3.1.2, Absatz „Randzeichen": dieselbe Zeichenliste wie Spec 0073 für
/// Zugangsdaten — die **Liste** wird geteilt, nicht der Geltungsbereich.
/// Deshalb die Konstante und nicht `trim_credential_value`: ein `HostName`
/// ist kein Zugangsdatum.
fn trim_edges(raw: &str) -> String {
    raw.trim_matches(|c: char| c.is_whitespace() || INVISIBLE_CREDENTIAL_EDGE_CHARS.contains(&c))
        .to_string()
}

/// §3.1.5: Sieht das erste Wort wie ein Direktivenname aus?
/// `[A-Za-z][A-Za-z0-9-]{0,31}` — wörtlich die Regel aus der Spec.
fn looks_like_directive_name(word: &str) -> bool {
    let mut chars = word.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    let rest: Vec<char> = chars.collect();
    rest.len() <= 31 && rest.iter().all(|c| c.is_ascii_alphanumeric() || *c == '-')
}

/// Zerlegt eine Zeile in Schlüsselwort und Rest.
///
/// OpenSSH trennt mit Leerraum und/oder **einem** `=`; `Port 22`,
/// `Port=22` und `Port = 22` sind gleichwertig. Ein Kommentar ist nur eine
/// Zeile, deren erstes nicht-leeres Zeichen `#` ist — ein `#` mitten in
/// der Zeile gehört zum Wert (so verhält sich `readconf.c`), und genau
/// darum darf der Export ihn nicht ungeschützt schreiben (§4.3).
fn split_keyword(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    let kw_end = t
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(t.len());
    let (kw, rest) = t.split_at(kw_end);
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
    Some((kw, rest))
}

/// Zerlegt den Rest einer Zeile in Wörter, wobei `"…"` ein Wort
/// zusammenhält (§3.1.2 / §6.1.5). Ein nicht geschlossenes
/// Anführungszeichen reicht bis zum Zeilenende — dieselbe Nachsicht, die
/// OpenSSH hier zeigt.
fn tokenize(rest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut started = false;

    for c in rest.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                started = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

/// Parst eine gelesene Datei (§3.1.4a, §3.1.2, §3.1.5).
///
/// `Ok(FileParse::NotSshConfig)` geht **vor** `Err(ValueTooLong)`: Eine
/// Datei, die gar keine `ssh_config` ist, soll übersprungen werden
/// (§3.1.4a) und nicht den ganzen Import an einer Grenze scheitern
/// lassen, die für ihren Inhalt keine Bedeutung hat.
pub fn parse_source(bytes: &[u8]) -> Result<FileParse, ValueTooLong> {
    // §3.1.4a, Merkmal 1 und 2: NUL-Byte oder kein gültiges UTF-8.
    if bytes.contains(&0) {
        return Ok(FileParse::NotSshConfig);
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => return Ok(FileParse::NotSshConfig),
    };

    let mut file = ParsedFile::default();
    let mut saw_known_keyword = false;
    let mut too_long: Option<ValueTooLong> = None;
    let mut current: Option<HostBlock> = None;
    // Stehen wir in einem `Match`-Block? Dann wirkt keine Zeile auf ein
    // Profil, bis das nächste `Host` kommt (§9/Q-1, Punkt 4).
    let mut in_match = false;

    for (idx, raw_line) in text.lines().enumerate() {
        let line = (idx + 1) as u32;
        file.line_count = line;

        let Some((kw, rest)) = split_keyword(raw_line) else {
            continue;
        };
        let kw_lower = kw.to_ascii_lowercase();

        if Known::from_keyword(&kw_lower).is_some() || OTHER_KEYWORDS.contains(&kw_lower.as_str()) {
            saw_known_keyword = true;
        }

        // §4.2 und §9/Q-1 Punkt 4: **`Match` ist eine Blockgrenze.** Es
        // beendet den laufenden `Host`-Block, damit keine Zeile aus dem
        // `Match`-Block auf ein Profil wirkt. Genau das war der schwerste
        // Messbefund gegen `ssh2-config` (§9/M-1): Dort landete `User y`
        // aus `Match host b` im vorhergehenden `Host a`. Ohne diese Zeile
        // hätten wir den Fehler nachgebaut.
        if kw_lower == "match" {
            if let Some(done) = current.take() {
                file.blocks.push(done);
            }
            in_match = true;
            file.skipped.push(SkippedDirective {
                line,
                kind: SkippedKind::Directive(kw.to_string()),
                block_line: None,
            });
            continue;
        }

        let Some(known) = Known::from_keyword(&kw_lower) else {
            // §3.1.5: Name melden, nie den Wert — und wenn das erste Wort
            // nicht wie ein Name aussieht, nur die Zeilennummer.
            let kind = if looks_like_directive_name(kw) {
                SkippedKind::Directive(kw.to_string())
            } else {
                SkippedKind::UnreadableLine
            };
            file.skipped.push(SkippedDirective {
                line,
                kind,
                block_line: current.as_ref().map(|b| b.line),
            });
            continue;
        };

        let words = tokenize(rest);

        // §3.3: Grenze je Wert. Gemerkt statt sofort abgebrochen, damit
        // §3.1.4a den Vortritt behält.
        for w in &words {
            let n = w.chars().count();
            if n > MAX_VALUE_CHARS && too_long.is_none() {
                too_long = Some(ValueTooLong { line, chars: n });
            }
        }

        match known {
            Known::Host => {
                if let Some(done) = current.take() {
                    file.blocks.push(done);
                }
                in_match = false;
                let clauses: Vec<HostClause> = words
                    .iter()
                    .map(|w| HostClause::parse(&trim_edges(w)))
                    .collect();
                current = Some(HostBlock {
                    clauses,
                    line,
                    host_name: None,
                    port: None,
                    user: None,
                    proxy_jump: None,
                    identity_file: None,
                });
            }
            Known::Include if in_match => {
                // ANNAHME A-1 (Q-BL-0216-01): Ein `Include` **innerhalb**
                // eines `Match`-Blocks wird gemeldet, nicht gefolgt. Weil
                // wir `Match` nicht auswerten (§4.2), wissen wir nicht, ob
                // `ssh` die Bedingung erfüllt sähe — die Datei zu öffnen
                // hieße, eine Datei zu lesen, die der Nutzer selbst
                // vielleicht nie liest. §5.1 („von sich aus öffnet der
                // Import ausschließlich `ssh_config`-Dateien") zeigt in die
                // Richtung, in der weniger geöffnet wird; die nehmen wir.
                file.skipped.push(SkippedDirective {
                    line,
                    kind: SkippedKind::Directive(kw.to_string()),
                    block_line: None,
                });
            }
            Known::Include => {
                // Rohwert; auflösen ist Sache von `app-shell` (§4.1).
                // Mehrere Pfade in einer Zeile sind erlaubt.
                for w in &words {
                    file.includes.push(IncludeDirective {
                        raw: trim_edges(w),
                        line,
                    });
                }
                if words.is_empty() {
                    file.skipped.push(SkippedDirective {
                        line,
                        kind: SkippedKind::Directive(kw.to_string()),
                        block_line: current.as_ref().map(|b| b.line),
                    });
                }
            }
            other => {
                // Werte gelten nur innerhalb eines `Host`-Blocks. Was davor
                // steht, ist OpenSSHs impliziter globaler Bereich; den
                // werten wir nicht aus und melden ihn als nicht übernommen,
                // statt ihn stillschweigend fallen zu lassen (§3.1.5).
                let Some(block) = current.as_mut() else {
                    file.skipped.push(SkippedDirective {
                        line,
                        kind: SkippedKind::Directive(kw.to_string()),
                        block_line: None,
                    });
                    continue;
                };
                let first = words.first().map(|w| trim_edges(w));
                let slot = match other {
                    Known::HostName => &mut block.host_name,
                    Known::Port => &mut block.port,
                    Known::User => &mut block.user,
                    Known::ProxyJump => &mut block.proxy_jump,
                    Known::IdentityFile => &mut block.identity_file,
                    Known::Host | Known::Include => unreachable!("oben behandelt"),
                };
                // §3.1.2 / §6.4.7 (c): Ein Wert, der nach dem Trim leer ist,
                // gilt als nicht angegeben. „Der erste gewinnt" gilt auch
                // innerhalb eines Blocks (§3.1.9 für `IdentityFile`, und
                // OpenSSH hält es bei jeder Direktive so). Jeder andere Fall
                // wird gemeldet statt verschluckt.
                match first {
                    Some(v) if !v.is_empty() && slot.is_none() => {
                        *slot = Some(Value { value: v, line });
                    }
                    _ => {
                        file.skipped.push(SkippedDirective {
                            line,
                            kind: SkippedKind::Directive(kw.to_string()),
                            block_line: Some(block.line),
                        });
                    }
                }
            }
        }
    }

    if let Some(done) = current.take() {
        file.blocks.push(done);
    }

    // §3.1.4a, Merkmal 3.
    if !saw_known_keyword {
        return Ok(FileParse::NotSshConfig);
    }
    if let Some(e) = too_long {
        return Err(e);
    }
    Ok(FileParse::Parsed(file))
}
