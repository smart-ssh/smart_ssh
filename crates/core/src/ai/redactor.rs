use regex::Regex;

use crate::ssh::CommandOutput;

/// Läuft über einen [`CommandOutput`], bevor er als
/// `MessageContent::CommandResult` in den Kontext für die nächste
/// KI-Anfrage aufgenommen wird (Spec 0006, Abschnitt 5). Redaction passiert
/// **immer**, unabhängig vom gewählten Provider, auch bei lokalen Modellen.
pub trait OutputRedactor: Send + Sync {
    fn redact(&self, output: &CommandOutput) -> CommandOutput;

    /// Wendet dieselben Muster wie [`Self::redact`] auf eine reine
    /// Textzeichenkette an — für Stellen, die kein `CommandOutput` haben
    /// (z. B. das ausgeführte Kommando selbst, das bisher unredigiert
    /// geloggt wurde, unabhängiger Review-Pass Spec 0016).
    fn redact_text(&self, text: &str) -> String;
}

/// Platzhalter, der einen erkannten Treffer ersetzt.
///
/// `pub` seit Spec 0096, A1: Aufrufer, die einen redigierten Text darauf
/// prüfen müssen, ob davon **nur noch Platzhalter und Leerraum** übrig sind
/// (der generierte Sitzungstitel, der KI-Notizvorschlag), brauchen genau
/// diese Zeichenfolge — eine zweite, hartkodierte Kopie an der Aufrufstelle
/// würde stillschweigend auseinanderlaufen, sobald der Platzhalter sich
/// hier ändert.
pub const REDACTED_PLACEHOLDER: &str = "[REDACTED]";

/// Default-Implementierung (Spec 0006, Abschnitt 5): erkennt
/// Private-Key-Blöcke, `password=`/`token=`/`api_key=`-artige Zeilen
/// (Groß-/Kleinschreibung ignoriert), AWS-/GitHub-/Slack-/Stripe-/Google-/
/// npm-Zugangsdaten-Muster, DB-Connection-Strings und allgemeine
/// URL-Zugangsdaten (`schema://user:pass@host`, auch mit unkodiertem `@`
/// in Benutzer oder Passwort und als Passwort-Parameter im Query-String,
/// Spec 0078), nackte Provider-Keys
/// (Anthropic, OpenAI, OpenRouter, GitLab, Hugging Face), `x-api-key`-/
/// `Authorization: Basic`-Header (Spec 0068), Unix-Crypt-/Shadow-Passwort-Hashes
/// (`$1$`/`$5$`/`$6$`/`$y$`/`$2b$`/`$7$`/`$apr1$` & verwandte Varianten, dazu
/// Argon2 und phpass seit Spec 0095), Twilio-/SendGrid-Keys,
/// Azure-`AccountKey=`/`SharedAccessKey=` und JWTs ohne Präfix (Spec 0095)
/// sowie Passwörter, die als **Argument eines Kommandozeilenprogramms**
/// übergeben werden (Spec 0095: `mysql`/`mariadb`/`mysqldump`/`mysqladmin`
/// `-p<wert>`, `sshpass -p`, `curl -u user:wert`, `htpasswd -b`,
/// `redis-cli -a`, `smbclient -U user%wert`, `openssl -pass pass:<wert>`
/// und `-k <wert>`) — s. `built_in_patterns()`.
///
/// Um "leicht um nutzerdefinierte Muster erweiterbar" zu sein (Aufgabe
/// Teil 1, Punkt 2 — die Speicherung dieser Muster ist explizit noch nicht
/// Teil dieses Schritts, s. Spec Abschnitt 5), akzeptiert
/// [`DefaultOutputRedactor::with_extra_patterns`] zusätzliche `Regex`-Werte,
/// die nach den eingebauten Mustern angewendet werden.
pub struct DefaultOutputRedactor {
    patterns: Vec<PatternRule>,
}

/// Ein Muster plus die Ersetzung, mit der ein Treffer überschrieben wird.
/// Für die meisten eingebauten Muster ist das schlicht der globale
/// [`REDACTED_PLACEHOLDER`] (der komplette Treffer verschwindet). Für
/// DB-Connection-Strings (Diagnose-Folge-Fix, 2026-09) reicht das nicht:
/// nur das Passwort-Segment zwischen `:` und `@` soll weg, Schema/
/// Nutzername/Host/Datenbank sollen lesbar bleiben — dafür nutzt die
/// `regex`-Crate `$name`-Platzhalter in der Ersetzungszeichenkette, die
/// auf benannte Capture-Groups im Muster zurückgreifen (s. das
/// DB-Connection-String-Muster in `built_in_patterns()`). Das erfordert
/// ein individuelles
/// `replacement` pro Muster statt des bisherigen einzigen globalen
/// `REDACTED_PLACEHOLDER`-Aufrufs in `redact_bytes`.
struct PatternRule {
    regex: Regex,
    replacement: &'static str,
}

impl DefaultOutputRedactor {
    pub fn new() -> Self {
        Self {
            patterns: built_in_patterns(),
        }
    }

    pub fn with_extra_patterns(extra: Vec<Regex>) -> Self {
        let mut patterns = built_in_patterns();
        patterns.extend(extra.into_iter().map(|regex| PatternRule {
            regex,
            replacement: REDACTED_PLACEHOLDER,
        }));
        Self { patterns }
    }
}

impl Default for DefaultOutputRedactor {
    fn default() -> Self {
        Self::new()
    }
}

/// Spec 0094, A2: der Redactor für die `debug`-Zeilen, die den Inhalt
/// tragen, den A1 aus `info`/`warn`/`error` entfernt — an den Stellen, an
/// denen kein Session-Redactor erreichbar ist (`filter::engine`,
/// `ai_providers::request_logging`, `mcp_server::tool_server`).
///
/// **Schwächer als der Redactor einer Sitzung, und das ist zu wissen
/// wichtig** (spec-reviewer-Fund, Runde 1 zu Spec 0094): Hier laufen nur
/// die eingebauten Muster ([`DefaultOutputRedactor::new`]). Der
/// Session-Redactor wird dagegen mit
/// [`DefaultOutputRedactor::with_extra_patterns`] gebaut und kennt
/// zusätzlich das **Sudo-Passwort dieses Servers**
/// (`app_shell::commands::connect`, `app_shell::commands::notes`). Ein
/// Kommando, das genau dieses Passwort enthält, bleibt auf einer
/// `debug`-Zeile, die über diesen Weg redigiert wird, im Klartext stehen.
/// Gegenüber dem Stand vor Spec 0094 ist das trotzdem eine Verbesserung —
/// dort stand derselbe Text roh auf `info` —, aber wer eine neue
/// Inhaltszeile ergänzt, nimmt den Session-Redactor, wo er erreichbar ist.
///
/// **Genau eine Instanz pro Prozess**, weil A2 es verlangt und weil der
/// Grund dafür handfest ist: `new()` übersetzt bei jedem Aufruf sämtliche
/// eingebauten Muster (`built_in_patterns`, mehrere Dutzend `Regex`).
/// Einmal je Filter-Entscheidung wäre das ein spürbarer Aufwand auf dem
/// heißesten Pfad der Anwendung.
pub fn default_log_redactor() -> &'static DefaultOutputRedactor {
    static REDACTOR: std::sync::OnceLock<DefaultOutputRedactor> = std::sync::OnceLock::new();
    REDACTOR.get_or_init(DefaultOutputRedactor::new)
}

/// Baut ein [`PatternRule`], dessen kompletter Treffer durch
/// [`REDACTED_PLACEHOLDER`] ersetzt wird — der weit überwiegende Fall
/// (alle eingebauten Muster außer dem DB-Connection-String-Muster unten,
/// das gezielt nur einen Teil des Treffers ersetzen muss). Reduziert die
/// sonst nötige `PatternRule { regex: ..., replacement: REDACTED_PLACEHOLDER
/// }`-Wiederholung an jeder Stelle unten auf einen Funktionsaufruf.
fn simple(pattern: &str, expect_msg: &str) -> PatternRule {
    PatternRule {
        regex: Regex::new(pattern).expect(expect_msg),
        replacement: REDACTED_PLACEHOLDER,
    }
}

/// Wie [`simple`], ersetzt aber nur den Teil des Treffers **hinter** der
/// benannten Gruppe `head`; `head` selbst wird wörtlich zurückgeschrieben
/// (Spec 0095, A1: „nur der Wert, der Rest bleibt stehen" — Programmname,
/// Schalter und Benutzername bleiben lesbar).
fn keep_head(pattern: &str, expect_msg: &str) -> PatternRule {
    PatternRule {
        regex: Regex::new(pattern).expect(expect_msg),
        replacement: "${head}[REDACTED]",
    }
}

/// Wie [`keep_head`], schreibt zusätzlich ein `replacement` mit dem
/// schließenden Anführungszeichen zurück — für Werte, die in Quotes stehen
/// und deshalb Leerraum enthalten dürfen (`-p"a b"`, `-u 'user:a b'`). Ohne
/// das zurückgeschriebene Quote bliebe eine unpaarige Anführung stehen.
fn keep_head_quoted(pattern: &str, replacement: &'static str, expect_msg: &str) -> PatternRule {
    PatternRule {
        regex: Regex::new(pattern).expect(expect_msg),
        replacement,
    }
}

/// Argumente zwischen Programmname und Passwort-Schalter (Spec 0095, A1.8:
/// `mysqldump -u r -pX db`, `sudo -u x mysql -pX`, `env LANG=C curl -u …`).
///
/// Bewusst **lazy und beschränkt** (`{0,12}?`): Ein Kommando hat vor dem
/// Passwort-Schalter keine zwölf Argumente, und eine unbeschränkte Folge
/// würde jede noch so weit entfernte Stelle mit dem Programmnamen verbinden.
/// Die ausgeschlossenen Zeichen `;`, `|`, `&`, `<`, `>` sind die Grenzen
/// zwischen zwei Kommandos: über sie hinweg verbindet **diese** Folge keinen
/// Programmnamen mit dem Schalter eines anderen Kommandos
/// (`mysql -e 'select 1' && grep -p x` bleibt unangetastet). Der Satz gilt
/// für `CMD_ARGS`, **nicht** automatisch für jede Regel: Die `sshpass`-Regel
/// unten hat eine eigene, engere Schalterkette, und deren erste Fassung
/// benutzte `\S*` — das enthält `;`/`|`/`&` und reichte deshalb über eine
/// Kommandogrenze hinweg (`sshpass -e;mysql -p mydb` schwärzte `mydb`,
/// spec-reviewer-Fund Runde 1, gemessen). Sie benutzt jetzt dieselbe
/// Zeichenklasse wie hier.
///
/// Der Preis ist Über-Redaktion, wenn ein Programmname irgendwo in einer
/// Zeile steht — auch als Pfadbestandteil — und auf derselben Kommandostufe
/// später ein `-p<wort>` folgt: `find /var/lib/mysql -type f -print` verliert
/// sein `-print` (spec-reviewer-Fund Runde 1, gemessen; Test
/// `test_redactor_known_over_redaction_when_a_program_name_appears_in_a_path`).
///
/// Bewusst nicht behoben, weil die naheliegende Verengung („der Programmname
/// muss in Kommandoposition stehen") die Eingaben verfehlen würde, für die
/// dieser Redactor überhaupt existiert: Er läuft auf **freiem Text** mit
/// eingebetteten Kommandos — „Kommando 'mysql -pX' konnte nicht ausgeführt
/// werden", „bitte mysql -pX ausführen" sind reale Log- und Kontextzeilen
/// dieses Projekts, und dort steht ein echtes Passwort. Ein regulärer
/// Ausdruck kann `/usr/bin/mysql` (Programm) nicht von `/var/lib/mysql`
/// (Argument) unterscheiden. Von beiden Fehlern ist Über-Redaktion der
/// harmlose: sie kostet Lesbarkeit, kein Geheimnis (dieselbe Abwägung wie
/// beim Stripe-`pk_`-Muster oben). ADR 0087.
///
/// Mit der Zeilenfortsetzung als Trenner (s. [`CMD_SEP`]) reicht dieselbe
/// Über-Redaktion bis in die Folgezeile, wenn eine Zeile aus anderem Grund
/// auf `\` endet (Windows-Pfad, umgebrochener String) und dort ein
/// `-p<wort>` steht — wieder nur die harmlose Richtung
/// (spec-reviewer-Fund, Runde 2).
const CMD_ARGS: &str = r"(?:(?:[ \t]|\\\r?\n)+[^\s;|&<>]+){0,12}?";

/// Ein Passwortwert als Argument: ein Shell-Escape (`\;`, `\'`, `\ `), ein
/// Abschnitt in einfachen oder doppelten Anführungszeichen (dann darf er
/// Leerraum enthalten) oder freier Text bis zum nächsten Leerraum bzw.
/// Kommandotrenner. **Eine Folge** davon (`+`), weil die Shell
/// aneinandergehängte Abschnitte zu einem Wort verbindet: `-p'a'"Geheim"` ist
/// ein Passwort, nicht zwei (Spec 0095, T1). Ohne das `+` bliebe der zweite
/// Abschnitt im Klartext stehen.
///
/// Der Escape-Zweig steht **zuerst** und der freie Zweig schließt `\` aus
/// (spec-reviewer-Fund 1, Runde 1, gemessen): Vorher endete der Wert am
/// Backslash, und aus `mysql -pa\;b` wurde `mysql [REDACTED];b` — das
/// Geheimnis war halb geschwärzt, aber die Zeile behauptete das Gegenteil.
/// Bei `mysql -p\'SuperSecret` (Passwort `'SuperSecret`) ging der **ganze**
/// Wert durch, mit einem `[REDACTED]` daneben. Gegenüber dem Stand vor
/// Spec 0095 war das keine Verschlechterung, aber schlechter als nichts zu
/// tun: ein irreführender Platzhalter ist gefährlicher als ein sichtbares
/// Geheimnis.
/// `\s` steht bewusst **nicht** in der freien Klasse: in der `regex`-Crate ist
/// das Unicode-`White_Space` und enthält U+00A0 & Co. Für die Shell ist ein
/// geschütztes Leerzeichen kein Trenner, es gehört zum Wort — der Wert endete
/// dort trotzdem und `mysql -pSEC<U+00A0>RET` wurde zu
/// `mysql [REDACTED]<U+00A0>RET` (spec-reviewer-Fund, Runde 2, gemessen).
/// Ausgeschlossen sind deshalb genau die vier Zeichen, die für die Shell
/// wirklich trennen.
///
/// **Mindestens ein echter Abschnitt** ist Pflicht (die Mitte des Ausdrucks):
/// Ein Wert, der nur aus einer Zeilenfortsetzung besteht, erzeugte sonst einen
/// Platzhalter, wo nichts redigiert wurde — `mysql -p\` + Umbruch + `   mydb`
/// wurde zu `mysql [REDACTED]   mydb`, obwohl `mydb` laut §1.4 der
/// Datenbankname ist (Fund des `regression-guard`, gemessen).
const CMD_VALUE: &str = concat!(
    r"(?:\\\r?\n)*",
    r#"(?:\\[^\r\n]|'[^'\r\n]*'|"[^"\r\n]*"|[^ \t\r\n;|&<>'"\\]+)"#,
    r#"(?:\\\r?\n|\\[^\r\n]|'[^'\r\n]*'|"[^"\r\n]*"|[^ \t\r\n;|&<>'"\\]+)*"#,
);

/// Trenner zwischen Programmname, Schalter und Wert: Leerraum **oder** die
/// Shell-Zeilenfortsetzung `\` + Zeilenumbruch. `CMD_SEP` verlangt mindestens
/// einen, `CMD_GAP` erlaubt auch keinen (angehängte Werte wie `-uadmin:pw`).
///
/// Die Zeilenfortsetzung gehört dazu, weil sie für die Shell **kein**
/// Kommandoende ist, sondern verschwindet: `sshpass -p \` + Zeilenumbruch +
/// `  S3cret!` ist ein Aufruf mit dem Passwort `S3cret!`. Vorher erfasste die
/// Regel nur den Backslash als Wert, setzte dort ein `[REDACTED]` und ließ
/// das Passwort in der Folgezeile im Klartext stehen (spec-reviewer-Fund 2,
/// Runde 1, gemessen). Ein gewöhnlicher Zeilenumbruch bleibt weiterhin eine
/// Grenze.
/// Derselbe Wert wie [`CMD_VALUE`], aber der **erste** Abschnitt darf nicht mit
/// `[` beginnen — er darf also nicht auf einem schon geschriebenen
/// `[REDACTED]` aufsetzen.
///
/// Nur für die ZWEITE Anwendung der Kommandozeilen-Regeln (s.
/// [`built_in_patterns`]). Ohne diese Einschränkung reproduzierte der zweite
/// Durchlauf denselben Treffer wie der erste — `replace_all` nimmt den
/// linkesten, und `[REDACTED]` ist selbst ein gültiger Wert —, setzte danach
/// an derselben Stelle wieder auf und kam nie beim zweiten Geheimnis an
/// (gemessen). Die erste Anwendung benutzt weiter [`CMD_VALUE`] ohne diese
/// Einschränkung, es geht also keine Abdeckung verloren.
const CMD_VALUE_AFTER_PLACEHOLDER: &str = concat!(
    r"(?:\\\r?\n)*",
    r#"(?:\\[^\r\n]|'[^'\r\n]*'|"[^"\r\n]*"|[^ \t\r\n;|&<>'"\\\[]+)"#,
    r#"(?:\\\r?\n|\\[^\r\n]|'[^'\r\n]*'|"[^"\r\n]*"|[^ \t\r\n;|&<>'"\\]+)*"#,
);

const CMD_SEP: &str = r"(?:[ \t]|\\\r?\n)+";
const CMD_GAP: &str = r"(?:[ \t]|\\\r?\n)*";

/// Nur die Zeilenfortsetzung, ohne Leerraum — für `mysql -p`, wo ein
/// Leerzeichen zwischen Schalter und Wert bedeutet, dass der Wert **kein**
/// Passwort ist (§1.4: der Client fragt dann interaktiv, der folgende Wert
/// ist der Datenbankname). `mysql -p\` + Zeilenumbruch + `Geheim` ist für die
/// Shell dagegen `-pGeheim`, also ein angehängter Wert.
const CMD_CONT: &str = r"(?:\\\r?\n)*";

/// Ein Schalter von `htpasswd`, samt seinem Zahlenwert, falls er einen hat
/// (`-C 12`, `-r 4096`) — damit dieser Wert nicht als Positionsargument
/// zählt.
const HT_OPT: &str = r"(?:(?:[ \t]|\\\r?\n)+-[A-Za-z]+(?:(?:[ \t]|\\\r?\n)+\d+)?)";

fn built_in_patterns() -> Vec<PatternRule> {
    let mut patterns: Vec<PatternRule> = vec![
        // Spec 0078 §9 (Q-BL-0248-02, Fund des regression-guard, gemessen):
        // KOPIEN der vier Schlüsselmuster ganz am Anfang der Liste. Die
        // Originale bleiben wörtlich an ihrer Stelle weiter unten; hier
        // steht nur eine zusätzliche, frühere Anwendung.
        //
        // Grund: Diese vier sind die einzigen Muster mit einem
        // MEHRTEILIGEN, durch Leerraum getrennten Anker
        // (`-----BEGIN … PRIVATE KEY-----`). Jede Regel, die einen Wert
        // verbraucht und an Leerraum stoppt, kann ihn genau in der Mitte
        // durchschneiden — danach greift weder das Muster selbst noch sein
        // Fail-safe-Rückfall, und der komplette Schlüsselkörper steht im
        // Klartext. Das ist im Redactor dreimal passiert: bei der frühen
        // `api-key`-Kopie (`api-key: -----BEGIN …`, schon vor Spec 0078
        // offen) und zweimal bei der Query-String-Regel von Spec 0078.
        //
        // Die Query-String-Regel verlangt inzwischen ein `@` im Wert. Das
        // reicht NICHT: Das `@` darf vor dem Anker stehen
        // (`?secret=a@-----BEGIN PRIVATE KEY-----`), der Anker liegt dann
        // innerhalb des Werts. Nicht der Anker braucht das `@`, sondern
        // der Wert — ein Unterschied, den die frühere Begründung an jener
        // Regel übersah.
        //
        // Ganz vorn angewendet ist ein Schlüsselblock geschwärzt, bevor
        // irgendeine wertverbrauchende Regel seinen Anker überhaupt sieht.
        // Damit ist die Ursachenklasse geschlossen statt einzelner
        // Ausprägungen. Dieselbe Begründung führt der Kommentar an den
        // Shadow-Hash-Mustern direkt darunter; die beiden stören einander
        // nicht, weil ein Crypt-Hash keinen PEM-Anker enthält und
        // umgekehrt.
        simple(
            r"(?s)-----BEGIN (?:[A-Z0-9_\-]+ )?PRIVATE KEY-----.*?-----END (?:[A-Z0-9_\-]+ )?PRIVATE KEY-----",
            "eingebautes Private-Key-Muster (frühe Kopie) ist gültig",
        ),
        simple(
            r"(?s)-----BEGIN (?:[A-Z0-9_\-]+ )?PRIVATE KEY-----.*",
            "eingebautes Private-Key-Rückfallmuster (frühe Kopie) ist gültig",
        ),
        simple(
            r"(?s)-----BEGIN PGP PRIVATE KEY BLOCK-----.*?-----END PGP PRIVATE KEY BLOCK-----",
            "eingebautes PGP-Private-Key-Muster (frühe Kopie) ist gültig",
        ),
        simple(
            r"(?s)-----BEGIN PGP PRIVATE KEY BLOCK-----.*",
            "eingebautes PGP-Private-Key-Rückfallmuster (frühe Kopie) ist gültig",
        ),
        // Unix-Crypt-/Shadow-Passwort-Hashes (Diagnose-Bericht "unredigierte
        // /etc/shadow-Hashes über den MCP-Pfad", 2026-09): ein `/etc/shadow`
        // -bestätigter MCP-Testlauf ("cat /etc/shadow" nach korrekt
        // ausgelöster Confirm-Bestätigung durch die Filter-Engine) zeigte,
        // dass Passwort-Hashes wie `root:$6$salt$hash:19000:...` unredigiert
        // an den KI-Anbieter/MCP-Client gingen — keines der übrigen Muster
        // greift, da eine Shadow-Zeile keines der Schlüsselwörter
        // `password`/`token`/`secret`/... enthält. Deckt ab: `$1$` (MD5),
        // `$5$` (SHA-256), `$6$` (SHA-512), `$y$`/`$gy$` (yescrypt), `$7$`
        // (scrypt), `$apr1$` (Apache-MD5, `.htpasswd` — vom `spec-reviewer`
        // ergänzt nachgefordert: genauso alltäglich wie `/etc/shadow`,
        // identisches Format wie `$1$`) sowie `$2a$`/`$2b$`/`$2x$`/`$2y$`
        // (bcrypt, eigenes Muster unten — andere Struktur, s. dort).
        //
        // **Bewusst als ERSTE Muster in dieser Liste** (spec-reviewer-Fund):
        // `redact_bytes` wendet alle Muster sequenziell an. Stünde dieses
        // Muster hinter einem der `password=`/AWS-/GitHub-Muster, könnte
        // ein Zufallstreffer eines früheren Musters MITTEN in einem
        // Hash liegen (z. B. `AKIA[A-Z0-9]{16}` trifft rein zufällig 20
        // Zeichen aus der Mitte eines langen SHA-512-Hashes) und das
        // `[REDACTED]` dort einfügen — das zerstört die `$`-Struktur, auf
        // die dieses Muster angewiesen ist, und der Rest des Hashes bliebe
        // im Klartext stehen. Ganz vorn angewendet, ist der komplette Hash
        // bereits ersetzt, bevor ein anderes Muster ihn überhaupt sieht.
        //
        // Struktur (glibc-/BSD-/Apache-Familie): nach `$<id>$` folgt
        // optional GENAU EIN Parameter-Segment (`rounds=N` bei
        // sha256/sha512), dann Salt, dann der eigentliche Hash — mit einer
        // MINDESTLÄNGE von 10 Zeichen fürs letzte Segment (spec-reviewer-
        // Fund: ohne diese Grenze hätte z. B. `$1$2$3` — eine typische
        // `awk`/`sed`-Positionsparameter-Referenz, kein Hash — ebenfalls
        // gematcht; jeder unterstützte Algorithmus liefert real mindestens
        // 13 Hash-Zeichen, 10 ist also eine sichere, konservative
        // Untergrenze ohne echte Hashes zu verpassen). Die crypt-
        // Base64-Zeichenklasse (`./0-9A-Za-z` plus `=` für `rounds=N`)
        // enthält bewusst KEIN `:` — dadurch matcht dieses Muster in einer
        // `/etc/shadow`-Zeile (`user:$6$salt$hash:lastchg:min:max:...`) von
        // selbst nur den Hash-Teil, nie das Feld-Trennzeichen oder die
        // Aging-/Username-Felder drumherum (kein Über-Redigieren, s.
        // Diagnose-Bericht Teil 1 Punkt 2/"Falsch-Positiv-Vorsicht").
        //
        // Bewusst KEIN eigenes `/etc/passwd`-spezifisches Muster nötig
        // (Diagnose-Bericht Teil 1 Punkt 3): eine normale Zeile wie
        // `user:x:1000:1000:...` enthält gar kein `$id$`-Muster und bleibt
        // unangetastet; taucht dort ausnahmsweise doch ein echter,
        // moderner Crypt-Hash auf (uralte Systeme ohne Shadow-Datei),
        // greift exakt dasselbe Muster, weil die Zeichenkette unabhängig
        // von der Datei identisch aussieht — dieselbe Formverankerung deckt
        // aus demselben Grund auch `/etc/gshadow` (Gruppen-Passwort-Hashes,
        // identisches `$id$`-Format) ab, ohne dass das extra genannt werden
        // müsste.
        //
        // Bewusst NICHT abgedeckt (Kosten/Nutzen-Abwägung, spec-reviewer
        // bestätigt): das noch ältere, nicht-`$`-präfixierte DES-Crypt-
        // Format (13 Zeichen, kein erkennbares Trennzeichen) — ein Muster
        // dafür hätte praktisch keinen Anker außer "13 beliebige Zeichen
        // aus einem 64er-Alphabet" und würde reihenweise harmlose kurze
        // Tokens/IDs/Git-Kurz-Hashes fälschlich redigieren; DES-Crypt ist
        // zudem seit Jahrzehnten kein Standard-Ausgabeformat mehr. Ebenso
        // nicht abgedeckt: BSD/Solaris-Exoten (`$sha1$`, Solaris-`$md5$`) —
        // real vorkommend, aber seltener als `/etc/shadow`/`.htpasswd`; als
        // bewusste Scope-Grenze offengelegt statt stillschweigend
        // fallengelassen, nicht sicherheitskritisch verschwiegen.
        //
        // Argon2 (`$argon2id$…`) und phpass (`$P$`/`$H$`) standen bis
        // Spec 0095 ebenfalls hier — sie sind jetzt abgedeckt, s. die zwei
        // Muster direkt unter dem bcrypt-Muster.
        simple(
            r"\$(?:1|5|6|7|y|gy|apr1)\$(?:[A-Za-z0-9./=]{1,40}\$)?[A-Za-z0-9./=]{1,64}\$[A-Za-z0-9./=]{10,150}",
            "eingebautes Unix-Crypt-Hash-Muster ist gültig",
        ),
        // bcrypt (`$2a$`/`$2b$`/`$2x$`/`$2y$`, inkl. der "2x"-Buggy-
        // Variante): eigene, strengere Struktur als die glibc-Familie oben
        // — auf eine feste zweistellige Kostenstufe folgt genau EIN Segment
        // aus 53 Zeichen (22 Salt + 31 Hash, bcrypt-eigenes Base64-Alphabet
        // `./A-Za-z0-9`, kein `=`). Die exakte Länge (statt eines Bereichs)
        // hält die Falsch-Positiv-Rate praktisch bei null.
        simple(
            r"\$2[abxy]\$\d{2}\$[A-Za-z0-9./]{53}",
            "eingebautes bcrypt-Hash-Muster ist gültig",
        ),
        // Spec 0095, A2.4: Argon2 in der PHC-Kodierung
        // (`$argon2id$v=19$m=65536,t=3,p=4$<salt>$<hash>`, dazu `argon2i`
        // und `argon2d`; die `v=`-Angabe ist optional, weil ältere
        // Bibliotheken sie weglassen). Das Parameter-Segment ist über
        // `m=`/`t=`/`p=` verankert statt über eine freie Zeichenklasse —
        // sonst hätte schon `$argon2id$x$y$z` gematcht. Salt und Hash in
        // Standard-Base64 ohne Padding (`+` und `/` möglich, `:` bewusst
        // nicht: so bleibt in einer `/etc/shadow`-Zeile das Feldtrennzeichen
        // unberührt, genau wie beim glibc-Muster oben).
        //
        // **Position: mit den übrigen Hash-Mustern ganz vorn**, aus deren
        // Grund: Ein Zufallstreffer eines späteren Musters (`AKIA[0-9A-Z]{16}`
        // kann rein zufällig in einem Base64-Salt liegen) würde die
        // `$`-Struktur zerschneiden, auf die dieses Muster angewiesen ist —
        // danach bliebe der Rest des Hashes im Klartext. Umgekehrt kann
        // dieses Muster keinem späteren etwas nehmen: es matcht eine
        // zusammenhängende Zeichenkette ohne Leerraum und ohne `:`, kann
        // also keinen mehrteiligen Anker (PEM, `Bearer …`, netrc) zerteilen.
        simple(
            r"\$argon2(?:id|i|d)\$(?:v=\d+\$)?m=\d+,t=\d+,p=\d+(?:,[a-z]+=[A-Za-z0-9+/=]*)*\$[A-Za-z0-9+/=]{8,}\$[A-Za-z0-9+/=]{10,}",
            "eingebautes Argon2-Hash-Muster ist gültig",
        ),
        // Spec 0095, A2.4: phpass (`$P$`/`$H$` + 31 Zeichen aus dem
        // crypt-Base64-Alphabet) — das Format von WordPress- und
        // Drupal-6-Datenbankauszügen. Ab Drupal 7 ist es `$S$` (SHA-512,
        // 52 Zeichen); das deckt dieses Muster NICHT ab und Spec 0095
        // verlangt es nicht — offen wie die `$sha1$`/`$md5$`-Exoten aus
        // §3 (spec-reviewer-Fund, Runde 2).
        //
        // `{31,}` statt `{31}`: Ein echter phpass-Hash ist exakt 34 Zeichen
        // lang, aber `{31}` hätte bei einer längeren Zeichenkette nur die
        // ersten 31 ersetzt und den Rest im Klartext stehen lassen. Lieber
        // ein Zeichen zu viel schwärzen als einen Hash-Schwanz freilegen.
        simple(
            r"\$[PH]\$[A-Za-z0-9./]{31,}",
            "eingebautes phpass-Hash-Muster ist gültig",
        ),
        // --- Spec 0068, Teil 1: nackte Provider-Keys und Auth-Header ---
        //
        // Formate geprüft gegen die gitleaks-Regeln (config/gitleaks.toml:
        // anthropic-api-key `sk-ant-api03-…AA`, anthropic-admin-api-key
        // `sk-ant-admin01-…`, openai-api-key `sk-(proj|svcacct|admin)-…
        // T3BlbkFJ…` sowie Legacy `sk-<20>T3BlbkFJ<20>`, gitlab-pat
        // `glpat-[\w-]{20}`, gitlab-pat-routable `glpat-…{27,}.<2><7>`,
        // huggingface-access-token `hf_<34 Buchstaben>`), gitleaks-Issue
        // #2158 (Anthropic-OAuth `sk-ant-oat01-`/`sk-ant-ort01-`) und die
        // OpenRouter-Doku (`sk-or-v1-`). Bewusst über den eindeutigen
        // Präfix verankert, bei der Länge aber mit Mindest- statt exakter
        // Länge: ein leicht abweichend langer echter Key soll nicht
        // durchrutschen (Fail-safe), der Präfix hält Falsch-Positive fern.
        //
        // **Reihenfolge**: direkt nach den `$`-Hash-Mustern (Keys enthalten
        // kein `$`, können von dort also nicht zerteilt werden) und VOR
        // allen übrigen Mustern — `AKIA…`/`AIza…`/Slack/Stripe könnten
        // sonst zufällig mitten in einem langen Key treffen, ihn zerteilen
        // und den Rest im Klartext lassen (Shadow-Fix-Lehre). Umgekehrt
        // kann keins dieser Muster ein späteres zerstören: sie verlangen
        // `-`/`_`-Präfixe, die in Base64-Key-Blöcken nicht vorkommen.
        //
        // Kein generisches Hoch-Entropie-Muster (Spec 0068: verworfen).
        simple(
            r"sk-ant-[a-z]+\d{2}-[A-Za-z0-9_-]{20,}",
            "eingebautes Anthropic-Key-Muster ist gültig",
        ),
        simple(
            r"sk-(?:proj|svcacct|admin|None)-[A-Za-z0-9_-]{20,}",
            "eingebautes OpenAI-Projekt-/Service-/Admin-Key-Muster ist gültig",
        ),
        // Legacy-OpenAI-Key mit dem Wasserzeichen `T3BlbkFJ` (Base64 für
        // "OpenAI") — das Wasserzeichen verhindert Treffer auf ein bloßes
        // `sk-` in normalem Text.
        simple(
            r"sk-[A-Za-z0-9]{20}T3BlbkFJ[A-Za-z0-9]{20}",
            "eingebautes Legacy-OpenAI-Key-Muster ist gültig",
        ),
        simple(
            r"sk-or-v\d+-[A-Za-z0-9]{32,}",
            "eingebautes OpenRouter-Key-Muster ist gültig",
        ),
        // GitLab PAT inkl. des routbaren Formats mit `.<2><7>`-Suffix — der
        // Suffix gehört zum Token und wird mit redigiert.
        simple(
            r"glpat-[A-Za-z0-9_-]{20,}(?:\.[0-9a-z]{2}\.?[0-9a-z]{7,})?",
            "eingebautes GitLab-PAT-Muster ist gültig",
        ),
        // Hugging Face: `hf_` + 34 Zeichen; Wortgrenzen, damit Bezeichner
        // wie `hf_hub_download` unberührt bleiben. Organisations-Token
        // `api_org_` gleich mit.
        simple(
            r"\b(?:hf|api_org)_[A-Za-z0-9]{30,}\b",
            "eingebautes Hugging-Face-Token-Muster ist gültig",
        ),
        // spec-reviewer-Fund (Spec 0068, ERHÖHT): weitere Präfixe.
        // GitLab Runner-/Deploy-Token (gitleaks: gitlab-runner-
        // authentication-token `glrt-[\w-]{20}` inkl. routbarer Form,
        // gitlab-deploy-token `gldt-[\w-]{20}`); Groq `gsk_` (≈ 48–52
        // Zeichen, Groq-Beispiele/gitleaks-Issue #2180 — derselbe Präfix
        // wird auch von Snyk genutzt, ebenfalls ein Geheimnis); xAI `xai-`
        // (≈ 80 Zeichen, docs.x.ai). Mindestlängen bewusst niedriger als
        // beobachtet.
        simple(
            r"\bgl(?:rt|dt)-[A-Za-z0-9_-]{20,}(?:\.[0-9a-z]{2}\.?[0-9a-z]{7,})?",
            "eingebautes GitLab-Runner-/Deploy-Token-Muster ist gültig",
        ),
        simple(
            r"\bgsk_[A-Za-z0-9]{32,}\b",
            "eingebautes Groq-Key-Muster ist gültig",
        ),
        simple(
            r"\bxai-[A-Za-z0-9]{40,}\b",
            "eingebautes xAI-Key-Muster ist gültig",
        ),
        // --- Spec 0095, A2.1/A2.2/A2.5: weitere nackte Token-Formen -------
        //
        // **Position: in diesem Block, also VOR `AKIA…`/`AIza…`/Slack/
        // Stripe** — aus genau dem Grund, den der Kommentar zu den Keys
        // oben nennt: Diese drei Muster verlangen eine zusammenhängende
        // Zeichenkette aus einem Base64-artigen Alphabet. Ein Zufallstreffer
        // eines kürzeren Musters mitten darin (`AIza` + 35 Zeichen kann in
        // einer langen Base64-Nutzlast rein zufällig vorkommen) würde sie
        // zerschneiden und den Rest im Klartext lassen.
        //
        // Umgekehrt können sie keinem späteren Muster einen mehrteiligen
        // Anker nehmen: ihre Zeichenklassen enthalten keinen Leerraum, und
        // jeder mehrteilige Anker im Redactor (`-----BEGIN … PRIVATE KEY`,
        // `Bearer <wert>`, `Authorization: Basic <wert>`,
        // `machine … password <wert>`) ist durch Leerraum getrennt. Ein
        // JWT, der einem `Bearer`-Anker vorausgeht, wird deshalb vollständig
        // ersetzt, bevor die Bearer-Regel ihn sieht — geschwärzt ist er
        // danach so oder so (Test
        // `test_t11_0095_token_rules_do_not_steal_existing_anchors`).
        //
        // A2.1 Twilio API-Key-SID: `SK` + 32 Hex als ganzes Wort. Die
        // Wortgrenzen halten `SKU-12345` und einen längeren Hex-Block
        // heraus.
        simple(
            r"\bSK[0-9a-fA-F]{32}\b",
            "eingebautes Twilio-API-Key-Muster ist gültig",
        ),
        // A2.2 SendGrid: `SG.` + 22 + `.` + 43 Zeichen. Exakte Längen (wie
        // beim Google- und bcrypt-Muster) statt Mindestlängen — das Format
        // ist fest, und `SG.` allein ist als Anker zu schwach.
        simple(
            r"\bSG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}\b",
            "eingebautes SendGrid-Key-Muster ist gültig",
        ),
        // A2.5 JWT ohne Präfix: drei base64url-Abschnitte, die ersten beiden
        // beginnen mit `eyJ` — das ist Base64 für `{"`, also der Anfang
        // eines JSON-Objekts, und damit der einzige Anker, der ein JWT
        // zuverlässig von beliebigem Base64 unterscheidet. Ohne diese
        // Forderung wäre das Muster ein Hoch-Entropie-Fallback, und genau
        // das ist für BL-0118 verworfen.
        //
        // Mindestlängen konservativ niedrig (10 statt der real ~24/~43
        // Zeichen), damit ein kurzer, aber echter Token nicht durchrutscht;
        // `eyJ` hält die Falsch-Positiv-Rate trotzdem praktisch bei null
        // (Spec 0095, T12: `eyJ…` allein und `abc.def.ghi` bleiben
        // unberührt).
        simple(
            r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            "eingebautes JWT-Muster ist gültig",
        ),
        // Die Header-Muster zusätzlich an ihrer ursprünglichen Stelle (dritte
        // Review-Runde) — in der ERWEITERTEN Form (vierte Runde): die
        // wörtliche erste Fassung verbrauchte `x-api-key: Bearer` bzw.
        // `Basic cred=` ohne Wert und nahm dem Bearer-/Credential-Muster den
        // Anker. Hier deckt jede Kopie ihren Wert selbst vollständig ab.
        simple(
            r#"(?i)\b(?:x-(?:goog-)?)?api-key['"]?\s*[:=]\s*(?:bearer\s+)?(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s'"]+)"#,
            "eingebautes x-api-key-Header-Muster ist gültig",
        ),
        simple(
            r#"(?i)\b(?:proxy-)?authorization['"]?\s*[:=]\s*['"]?(?:basic|token)\s+(?:bearer\s+)?(?:[a-z_]+=)?(?:"[^"\r\n]*"|[A-Za-z0-9+/._~-]+=*)"#,
            "eingebautes Authorization-Basic-Muster ist gültig",
        ),
        // Spec 0078: Passwort-Parameter im Query-String einer URL. Steht
        // BEWUSST VOR dem DB-Muster — das schließt `?`/`#` nicht aus und
        // liest bei einer URL ohne Pfad (`redis://cache:6379?password=p@ss…`)
        // `6379?password=p` als Passwort. Damit nimmt es dem
        // Schlüsselwort-Muster weiter unten den Anker, und der Rest des
        // Werts bleibt im Klartext stehen. Diese Regel greift vorher und
        // ersetzt den Wert, sodass hinter dem `=` kein `@` mehr steht, über
        // das das DB-Muster laufen könnte. Mit Pfad (`…/0?password=…`)
        // stoppt das DB-Muster ohnehin am `/`.
        //
        // Das DB-Muster bleibt dafür WÖRTLICH unangetastet (Spec 0078 §4):
        // es ist über vier Review-Runden gegen echte Regressionen verengt
        // worden, und `?`/`#` dort nachzutragen hieße, ein geprüftes Muster
        // zu ändern statt zu ergänzen.
        //
        // Die Schlüsselwörter sind genau die des Schlüsselwort-Musters
        // weiter unten; ein hier nicht erfasstes Schlüsselwort verliert
        // also nichts, es läuft weiter über jenes.
        //
        // Wert: erst die beiden Anführungszeichen-Formen, dann die freie.
        // Die Quote-Alternativen sind nötig, weil die freie Form `'` und
        // `"` ausschließt: ohne sie griffe diese Regel bei
        // `redis://cache:6379?password='p@ss w0rd'` gar nicht, das
        // DB-Muster läse `6379?password='p` als Passwort, und `ss w0rd'`
        // bliebe im Klartext stehen (Test `…_redacts_a_password_query_
        // parameter_containing_an_at_sign`, Fälle mit Quotes). Die freie
        // Form endet an `&` `#`, Leerraum und an den Feldtrennern `,` `;`
        // `"` `'`, aus demselben Grund wie beim DB-Muster unten.
        // (Spec 0078 §3.1 begründet die Quote-Alternativen mit
        // `?password='top secret 123'`; das trifft nicht zu — dort matcht
        // diese Regel ohnehin nicht, weil die freie Form am `'` scheitert,
        // und das Schlüsselwort-Muster redigiert wie bisher. Der Nutzen
        // ist derselbe, der Grund ein anderer. spec-reviewer-Fund, erste
        // Review-Runde.)
        //
        // JEDER der drei Zweige verlangt MINDESTENS EIN `@` im Wert
        // (spec-reviewer-Fund, erste und zweite Review-Runde, beide Male
        // eine echte Regression): ohne diese Forderung schnitt die Regel
        // den mehrteiligen Anker der Schlüsselmuster durch —
        // `?secret=-----BEGIN PRIVATE KEY-----` wurde bis zum Leerzeichen
        // ersetzt (freier Zweig), `?secret="-----BEGIN PRIVATE KEY-----"`
        // bis zum schließenden Quote (quotierter Zweig). Der komplette
        // Schlüsselkörper stand danach im Klartext.
        //
        // Die `@`-Forderung allein schützt den Anker aber NICHT, und die
        // frühere Fassung dieses Kommentars behauptete genau das („ein
        // Anker ohne `@` ist für diese Regel unerreichbar"). Das ist
        // falsch: Nicht der Anker muss das `@` enthalten, sondern der
        // WERT — und der beginnt vor dem Anker. Bei
        // `?secret=a@-----BEGIN PRIVATE KEY-----` liefert das `a@` die
        // Bedingung, und der Anker liegt mitten im Treffer (Fund des
        // regression-guard über `ee017af..387a91e`, gemessen, Spec 0078
        // §9 / Q-BL-0248-02). Geschützt wird der Anker deshalb an einer
        // anderen Stelle: durch die KOPIEN der vier Schlüsselmuster ganz
        // am Anfang der Liste (s. dort). Die `@`-Forderung bleibt, weil
        // sie die Regel auf ihren Zweck begrenzt.
        //
        // Sie kostet dadurch keine Abdeckung. Der Grund ist enger, als er
        // zunächst aussieht, deshalb genau: Das DB-Muster LÄUFT sehr wohl
        // über einen Wert ohne `@` hinweg (seine Passwortklasse endet erst
        // am ersten `@`, das irgendwo dahinter stehen darf) — aber alles,
        // worüber es läuft, liegt INNERHALB seiner Ersetzung und ist damit
        // redigiert. Ein Teil des Parameterwerts kann nur dann hinter dem
        // `@` stehenbleiben, wenn der Wert das `@` selbst enthält, und
        // dann greift diese Regel. Ein Wert ohne `@` wird unverändert vom
        // Schlüsselwort-Muster redigiert. Tests
        // `…_does_not_cut_a_private_key_armor_anchor`,
        // `…_redacts_a_key_block_behind_an_at_sign_in_a_query_parameter`
        // und `…_still_redacts_a_query_parameter_without_an_at_sign`.
        //
        // Der Trenner: ein `?`, danach beliebig viele weitere Parameter
        // mit `&`. Ein `&` OHNE vorangehendes `?` im selben Token zählt
        // NICHT (Spec 0078 §9 / Q-BL-0248-02, gemessene Lockerung): sonst
        // griff diese Regel mitten in ein Passwort, das ein
        // `&<schlüsselwort>=` enthält, und nahm dem strengen URL-Muster
        // den Anker — `https://u:Geheim&token=b@h/x` wurde zu
        // `https://u:Geheim&[REDACTED]`, wo vor Spec 0078
        // `https://u:[REDACTED]@h/x` stand. Test
        // `…_does_not_treat_an_ampersand_without_a_question_mark_as_a_query_string`.
        //
        // Die Zeichenklasse zwischen `?` und `&` schließt `@` aus. Das
        // schützt NICHTS — der Trenner wird über `${sep}` wörtlich
        // zurückgeschrieben und kann gar nichts zerstören, worüber er
        // läuft (eine frühere Fassung dieses Kommentars behauptete das
        // Gegenteil, spec-reviewer-Fund der dritten Runde). Sein einziger
        // realer Effekt ist, dass der Trenner nicht über einen schon
        // `@`-haltigen Parameter hinwegkommt — genau deshalb braucht es
        // die zweite Anwendung unten. Der Ausschluss bleibt, weil er Teil
        // der gemessenen Fassung aus Spec 0078 §9 (Q-BL-0248-02) ist; ihn
        // zu entfernen bräuchte eine eigene Messrunde.
        //
        // BEKANNTER RESTFALL, bewusst entschieden (2026-09-24,
        // Q-BL-0248-01; Spec 0078 §5, Test
        // `…_known_remaining_case_password_with_a_query_parameter_prefix`):
        // Enthält das PASSWORT einer Verbindungs-URL wörtlich
        // `?<schlüsselwort>=` mit einem `@` im Wert, greift diese Regel,
        // und der Passwort-Präfix VOR dem `?` bleibt sichtbar —
        // `postgres://u:a?password=b@h/x` wird zu
        // `postgres://u:a?[REDACTED]`, vorher `postgres://u:[REDACTED]@h/x`.
        // Der Präfix kann beliebig lang sein. Das ist die EINZIGE Stelle,
        // an der Spec 0078 weniger redigiert als der Stand davor.
        //
        // Nicht auflösbar, ohne die Position dieser Regel aufzugeben und
        // damit Fall C wieder zu öffnen: Die Zeichenkette hat zwei
        // Lesarten (Passwort `a?password=b` mit Host `h`, oder Passwort
        // `a` mit Query-String), die ohne echtes URL-Parsing niemand
        // trennen kann. Vorher war die erste Lesart zu und die zweite
        // offen, jetzt umgekehrt. Betroffen sind nur DB-Schemata.
        //
        // Davon zu unterscheiden, weil es NICHT neu ist: Ohne `@` im
        // Parameterwert bleibt der Präfix ebenfalls sichtbar
        // (`postgres://u:Geheim?password=pl/ain&x=y@h/db` — das DB-Muster
        // bleibt am `/` im Wert hängen), aber das war vor Spec 0078
        // genauso. Gemessen.
        //
        // Restfall, bewusst (Spec 0078 §5): ein leerer Wert
        // (`?token=` am Zeilenende) wird nicht erfasst — es gibt nichts zu
        // redigieren. Ein Wert, der selbst ein rohes `,`/`;`/`"`/`'`
        // enthält, wird nur bis dorthin erfasst; den Rest übernimmt wie
        // bisher das Schlüsselwort-Muster.
        PatternRule {
            regex: Regex::new(
                r#"(?i)(?P<sep>\?(?:[^\s,;"'#?@&]*&)*)(?P<key>password|token|api_key|secret|passphrase)=(?:'[^'\r\n]*@[^'\r\n]*'|"[^"\r\n]*@[^"\r\n]*"|[^&#\s,;"']*@[^&#\s,;"']*)"#,
            )
            .expect("eingebautes Query-Parameter-Muster ist gültig"),
            replacement: "${sep}${key}=[REDACTED]",
        },
        // ZWEITE Anwendung derselben Regel, wörtlich gleich
        // (spec-reviewer-Fund, dritte Runde, gemessene Lockerung).
        //
        // `replace_all` sucht nur vorwärts, und die Trennergruppe kann
        // wegen des ausgeschlossenen `@` nicht über einen schon
        // `@`-haltigen Parameter hinweglaufen. Bei ZWEI `@`-haltigen
        // Schlüsselwort-Parametern im selben Token erreichte die Regel
        // deshalb nur den ersten:
        // `redis://cache:6379?password=p@ss&token=abc@SECRETTAIL` wurde zu
        // `redis://cache:[REDACTED]@SECRETTAIL` — das DB-Muster fraß
        // danach den Anker des zweiten Parameters, und dessen Schwanz
        // stand im Klartext, wo er VOR Spec 0078 geschwärzt war.
        //
        // Nach dem ersten Durchlauf steht dort `[REDACTED]` statt des
        // ersten Werts, und `[REDACTED]` enthält kein `@` — die
        // Trennergruppe kommt jetzt daran vorbei und erreicht den zweiten
        // Parameter.
        //
        // NICHT „per Konstruktion nur mehr redigieren": Der Satz gilt für
        // die Ersetzung selbst (der Trenner wird über `${sep}` wörtlich
        // zurückgeschrieben, ersetzt wird allein `<schlüsselwort>=<wert>`,
        // nichts wird freigelegt), aber NICHT für die Kette. Wie die erste
        // Anwendung kann die zweite einem späteren Muster einen
        // mehrteiligen Anker zerschneiden, der im Wert beginnt und hinter
        // dem ersten Leerraum weiterläuft. Über `<schlüsselwort>=`
        // erreichbar sind heute nur die PEM-/PGP-Anker, und die deckt die
        // Kopie am Listenanfang ab; netrc und `client-key-data` haben
        // keine `=`-Trennung und sind unerreichbar. Die Unbedenklichkeit
        // stützt sich also auf die Kopien und auf die Messung, nicht auf
        // die Konstruktion. (Eine frühere Fassung dieses Kommentars
        // behauptete das Gegenteil — Korrektur des Architekten zu
        // Q-BL-0248-03, s. Spec 0078 §9.)
        //
        // RESTFALL: Drei und mehr `@`-haltige Schlüsselwort-Parameter im
        // selben Token brauchten je eine weitere Anwendung; ab dem dritten
        // bleibt es beim Verhalten von vor Spec 0078. Eine Schleife wäre
        // die saubere Form, sie hieße aber `redact_bytes` umzubauen — von
        // Spec 0078 §2 ausdrücklich ausgeschlossen. Test
        // `…_redacts_two_at_bearing_query_parameters_in_one_token`.
        PatternRule {
            regex: Regex::new(
                r#"(?i)(?P<sep>\?(?:[^\s,;"'#?@&]*&)*)(?P<key>password|token|api_key|secret|passphrase)=(?:'[^'\r\n]*@[^'\r\n]*'|"[^"\r\n]*@[^"\r\n]*"|[^&#\s,;"']*@[^&#\s,;"']*)"#,
            )
            .expect("eingebautes Query-Parameter-Muster (zweite Anwendung) ist gültig"),
            replacement: "${sep}${key}=[REDACTED]",
        },
        // DB-Connection-Strings (Diagnose-Folge-Fix, 2026-09): das Passwort
        // steht zwischen `:` und `@`, keines der Schlüsselwort-Muster
        // (`password=`/`token=`/...) greift auf diese Syntax. Deckt die in
        // der DevOps-/Homelab-Zielgruppe (Docker-/CI-Logs, Config-Dumps)
        // häufigsten Schemata ab: Postgres, MySQL/MariaDB, MongoDB (inkl.
        // `+srv`), Redis (inkl. TLS-`rediss`) und AMQP/RabbitMQ (inkl.
        // TLS-`amqps`). `(?i)`, weil das Schema in freier Ausgabe auch
        // großgeschrieben vorkommt (z. B. `POSTGRES://` in manchen
        // Log-Formatierern) — spec-reviewer-Fund, ursprünglich fehlte das.
        //
        // Ersetzt bewusst NUR das Passwort-Segment, nicht den ganzen
        // Treffer (anders als jedes andere Muster in dieser Liste) —
        // Schema/Nutzername/Host/Datenbank bleiben für Fehlersuche lesbar,
        // exakt dieselbe "nur der sensible Teil weg"-Leitlinie wie beim
        // Shadow-Hash-Fix. Technisch über benannte Capture-Groups
        // (`scheme`, `user`) und eine `${name}`-Ersetzungszeichenkette
        // statt des globalen `REDACTED_PLACEHOLDER` — deshalb das einzige
        // Muster hier, das nicht über `simple()` läuft.
        //
        // Nutzername optional (`*` statt `+`, spec-reviewer-Fund): die
        // KANONISCHE Redis-URL-Form vor ACLs ist `redis://:passwort@host`
        // (kein Nutzername) — in unzähligen docker-compose-/Heroku-/
        // Sidekiq-Configs so verwendet. Mit `+` (verlangt mindestens ein
        // Zeichen) matchte dieser extrem häufige Fall gar nicht.
        //
        // Nutzername-/Passwort-Zeichenklasse schließt bewusst `, ; "` aus
        // (spec-reviewer-Fund, ERSTE Review-Runde dieses Musters — echte,
        // nicht nur theoretische Regression): die ursprüngliche
        // Zeichenklasse `[^@/\s]+` lief bei einer Zeile wie
        // `redis://cache:6379,password=p@ssw0rd` über das komma-getrennte
        // `password=`-Feld bis zum NÄCHSTEN `@` (dem in `p@ssw0rd`)
        // hinweg — das zerstörte den Anker, auf den das generische
        // `password=`-Muster weiter unten angewiesen ist, und `ssw0rd`
        // blieb im Klartext stehen (`redis://cache:[REDACTED]@ssw0rd`)
        // statt vollständig redigiert zu werden. Dieselbe Einschränkung
        // mindert Über-Redaktion, wenn irgendwo später in derselben Zeile
        // ein UNABHÄNGIGES `@` auftaucht (z. B. eine E-Mail-Adresse in
        // JSON: `{"redis":"redis://cache:6379","admin":"ops@example.com"}`
        // — das schließende `"` bricht das Matching vor dem `admin`-Feld
        // ab).
        //
        // ZWEITE Review-Runde deckte auf, dass die erste Fassung dieses
        // Ausschlusses (`, ; " ' =`) zu weit ging: `=` und `'`
        // auszuschließen trug NICHTS zur Behebung der beiden obigen Funde
        // bei (die entstehen durch `,`/`;`/`"` als Feldtrenner in
        // Shell-/JSON-Kontexten, nicht durch `=`/`'`), kostete aber echte
        // Redaction-Abdeckung: ein Base64-generiertes Passwort mit
        // `=`-Padding (`openssl rand -base64 24`, Kubernetes-Secrets,
        // Terraform-generierte DB-Passwörter — alltäglich, nicht exotisch)
        // wie `postgres://user:c2VjcmV0Cg==@host/db` wurde dadurch
        // versehentlich WIEDER unredigiert durchgelassen — eine neue
        // Regression, die die erste Fassung dieses Kommentars nicht
        // erwähnte. `=` und `'` sind jetzt wieder erlaubt.
        //
        // Bekannter, bewusst akzeptierter Restfall (spec-reviewer
        // bestätigt, kein vollständiger Schutz möglich ohne echtes
        // URL-Parsing statt Regex): ein Passwort, das selbst ein
        // UNENCODIERTES `,`/`;`/`"` enthält (z. B. `pw,with,commas`),
        // matcht nicht — dieselben drei Zeichen sind ja genau die, die
        // diese Klasse als Feldtrenner ausschließen MUSS, um die beiden
        // oben genannten Funde zu schließen. Diese drei Zeichen roh
        // (nicht Prozent-kodiert) in einem echten Connection-String-
        // Passwort sind selten (die meisten Connection-String-Ersteller
        // kodieren URI-Sonderzeichen ohnehin); die Alternative (sie
        // wieder erlauben) würde die beiden oben genannten, real
        // aufgetretenen Falsch-Negative erneut öffnen. Ebenso bleibt ein
        // Restrisiko für Über-Redaktion bei anderen, selteneren
        // Feldtrennern (`|`/`#`/`&`) — kein vollständiger Schutz, nur eine
        // Verengung auf die in beiden Review-Runden tatsächlich
        // beobachteten Trennzeichen.
        PatternRule {
            regex: Regex::new(
                r#"(?i)(?P<scheme>postgres(?:ql)?|mysql|mariadb|mongodb(?:\+srv)?|rediss?|amqps?)://(?P<user>[^:@/\s,;"]*):[^@/\s,;"]+@"#,
            )
            .expect("eingebautes DB-Connection-String-Muster ist gültig"),
            replacement: "${scheme}://${user}:[REDACTED]@",
        },
        // Spec 0068, Teil 1: generische URL-Zugangsdaten für alle übrigen
        // Schemata (http/https/ftp/sftp/…). Bewusst NACH dem DB-Muster und
        // mit EXAKT dessen Zeichenklassen für Nutzer und Passwort
        // (`[^:@/\s,;"]`) — die DB-String-Runde hatte zwei Regressionen
        // durch zu gierige Klassen, die hier nicht wiederkommen dürfen. Auf
        // einem vom DB-Muster schon redigierten String ist es idempotent
        // (`[REDACTED]` wird zu `[REDACTED]`), das DB-Muster bleibt also
        // unverändert wirksam. Ersetzt wie dort nur das Passwort.
        //
        // spec-reviewer-Fund (Spec 0068, ERHÖHT): zusätzlich ohne `?`/`#` in
        // Nutzer und Passwort — beide sind im Userinfo-Teil einer URL nie
        // unkodiert erlaubt. Sonst griff die Regel bei
        // `https://host:8443?password=p@ss…` über den Port in den Query-
        // String und zerstörte den Anker des `password=`-Musters, sodass
        // der Rest des Passworts im Klartext blieb.
        PatternRule {
            regex: Regex::new(
                r#"(?i)(?P<scheme>\b[a-z][a-z0-9+.-]*)://(?P<user>[^:@/\s,;"?#]*):[^@/\s,;"?#]+@"#,
            )
            .expect("eingebautes URL-Zugangsdaten-Muster ist gültig"),
            replacement: "${scheme}://${user}:[REDACTED]@",
        },
        // Slack-Tokens: `xoxb-`/`xoxp-`/`xoxo-`/`xoxa-`/`xoxs-` (Bot/User/
        // Legacy-Workspace/App-/Legacy-Workspace-Signing-Token) sowie das
        // strukturell andere `xapp-`-Präfix (Socket-Mode-App-Level-Token —
        // spec-reviewer-Fund: hätte mit erfasst werden sollen, gleiches
        // Risikoprofil wie die `xox*`-Familie, kein Zusatzaufwand),
        // gefolgt von mehreren `-`-getrennten alphanumerischen Segmenten.
        // Reale Tokens sind weit über 24 Zeichen lang (klassische
        // Bot-Tokens z. B. ~50+ Zeichen) — die Mindestlänge ist
        // konservativ niedrig genug, um keine echte Variante zu
        // verpassen, aber hoch genug, um einen zufälligen Präfix-Treffer
        // auf kurzem, unzusammenhängendem Text unwahrscheinlich zu
        // machen.
        simple(
            r"(?:xox[bpoacs]|xapp)-[A-Za-z0-9-]{24,}",
            "eingebautes Slack-Token-Muster ist gültig",
        ),
        // Stripe-Keys: `sk_`/`pk_` (secret/publishable) je `live`/`test`.
        // `pk_`-Keys sind laut Stripes eigener Doku explizit zur
        // öffentlichen Client-Einbettung gedacht (nicht wirklich
        // sensibel) — hier trotzdem mit erfasst, für Konsistenz mit den
        // `sk_`-Varianten und weil ein zu viel redigierter, aber
        // harmloser Wert kein Schaden ist. `test`-Keys ebenfalls erfasst:
        // ein Secret-Test-Key ist immer noch ein echtes Credential
        // (Zugriff auf die Stripe-Testumgebung), nur mit geringerem
        // Schadenspotential als ein Live-Key — die Unterscheidung
        // "weniger kritisch" rechtfertigt aus Fail-safe-Sicht keine
        // Lücke, wenn beide Formate gleich eindeutig und ohne
        // Mehraufwand erfassbar sind.
        simple(
            r"(?:sk|pk)_(?:live|test)_[A-Za-z0-9]{20,}",
            "eingebautes Stripe-Key-Muster ist gültig",
        ),
        // Google-API-Keys: `AIza` + exakt 35 Zeichen (Google-Format ist
        // fest 39 Zeichen insgesamt) — exakte statt Mindestlänge, analog
        // zur bcrypt-Begründung oben, hält die Falsch-Positiv-Rate
        // praktisch bei null.
        simple(
            r"AIza[A-Za-z0-9_-]{35}",
            "eingebautes Google-API-Key-Muster ist gültig",
        ),
        // npm Access Tokens (`npm_...`, seit npm 7 das Standardformat für
        // Publish-/CI-Tokens, z. B. in `.npmrc` oder CI-Umgebungsvariablen).
        simple(
            r"npm_[A-Za-z0-9]{20,}",
            "eingebautes npm-Token-Muster ist gültig",
        ),
        // Private-Key-Blöcke (RSA/EC/OPENSSH/PKCS8 ...), über mehrere
        // Zeilen hinweg — `(?s)`, damit `.` auch Zeilenumbrüche matcht. Der
        // Typ-Teil (`RSA `/`OPENSSH `/`ENCRYPTED ` ...) ist absichtlich
        // OPTIONAL (`(?:...)?`, nicht `+`): das unverschlüsselte, moderne
        // PKCS8-Format hat schlicht "-----BEGIN PRIVATE KEY-----" ohne
        // jeden Typ-Präfix (z. B. `openssl genpkey`, Let's-Encrypt-
        // `privkey.pem`, Kubernetes-`tls.key`, GCP-Service-Account-JSON) —
        // die vorherige Fassung verlangte hier zwingend mindestens ein
        // Zeichen und ließ dadurch genau diesen häufigsten Fall
        // unerkannt durch (unabhängiger Review-Pass, Spec 0006).
        simple(
            r"(?s)-----BEGIN (?:[A-Z0-9_\-]+ )?PRIVATE KEY-----.*?-----END (?:[A-Z0-9_\-]+ )?PRIVATE KEY-----",
            "eingebautes Private-Key-Muster ist gültig",
        ),
        // Fail-safe-Rückfallebene zum vorigen Muster: fehlt das passende
        // `-----END ... PRIVATE KEY-----` (z. B. weil die Ausgabe bei der
        // 2-MB-Obergrenze oder einem manuellen Abbruch mitten im
        // Key-Block gekappt wurde), matcht das obige Muster gar nicht —
        // dann lieber ab dem erkannten `BEGIN`-Header bis zum Ende
        // redigieren als den kompletten (unvollständigen) Key
        // unredigiert durchzulassen (Spec 0002 Abschnitt 1: Fail-safe).
        simple(
            r"(?s)-----BEGIN (?:[A-Z0-9_\-]+ )?PRIVATE KEY-----.*",
            "eingebautes Private-Key-Rückfallmuster ist gültig",
        ),
        // PGP Private Keys, mit derselben Fail-safe-Rückfallebene.
        simple(
            r"(?s)-----BEGIN PGP PRIVATE KEY BLOCK-----.*?-----END PGP PRIVATE KEY BLOCK-----",
            "eingebautes PGP-Private-Key-Muster ist gültig",
        ),
        simple(
            r"(?s)-----BEGIN PGP PRIVATE KEY BLOCK-----.*",
            "eingebautes PGP-Private-Key-Rückfallmuster ist gültig",
        ),
        // password=/token=/api_key=/secret=/passphrase=-artige Zeilen, auch
        // mit : und Quotes. `['"]?` direkt nach dem Schlüsselwort deckt
        // JSON-artige Ausgaben ab (`"password": "hunter2"` — der Schlüssel
        // selbst steht dort in Anführungszeichen, direkt gefolgt vom
        // schließenden Quote, nicht vom Trenner; ohne dieses `['"]?` bricht
        // das Matching genau an dieser Stelle ab, unabhängiger Review-Pass
        // Spec 0006).
        simple(
            r#"(?i)(password|token|api_key|secret|passphrase)['"]?\s*[:=]\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|\S+)"#,
            "eingebautes Credential-Zeilen-Muster ist gültig",
        ),
        // `aws_secret_access_key = ...` — vom Muster oben NICHT erfasst:
        // "secret" ist zwar als Teilstring in "aws_secret_access_key"
        // enthalten, aber unmittelbar danach folgt "_access_key", kein
        // Trenner, wodurch das obige Muster dort nicht matcht. Eigenes,
        // spezifisches Muster für genau diesen (extrem gebräuchlichen,
        // z. B. `~/.aws/credentials`) Schlüsselnamen.
        simple(
            r"(?i)aws_secret_access_key\s*[:=]\s*\S+",
            "eingebautes AWS-Secret-Key-Muster ist gültig",
        ),
        // Bearer Tokens
        simple(
            r"(?i)Bearer\s+[A-Za-z0-9_\-\.]{16,}",
            "eingebautes Bearer-Token-Muster ist gültig",
        ),
        // AWS-Access-Key- & Session-Token-Muster (AKIA, ASIA)
        simple(r"(AKIA|ASIA)[0-9A-Z]{16}", "eingebautes AWS-Key-Muster ist gültig"),
        // GitHub Personal Access Tokens: klassisches Format (ghp_, gho_,
        // ghu_, ghs_, ghr_, 36+ Zeichen) sowie das neuere,
        // fein-granulare Format (`github_pat_...`, deutlich länger).
        simple(
            r"(ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36,}",
            "eingebautes GitHub-Token-Muster ist gültig",
        ),
        simple(
            r"github_pat_[A-Za-z0-9_]{20,}",
            "eingebautes GitHub-Fine-Grained-Token-Muster ist gültig",
        ),
        // --- Spec 0068: Muster, die einen Header-/Schlüsselnamen mit
        // verbrauchen, stehen bewusst AM ENDE der Liste (spec-reviewer-Fund,
        // zweite Runde): weiter vorn nahmen sie älteren Mustern (Bearer,
        // Credential-Zeile) den Anker weg und ließen Klartext stehen, z. B.
        // `Authorization: Token token="…"` oder `api-key: Bearer …`. Hier
        // sehen alle älteren Muster den Originaltext zuerst; diese greifen
        // nur noch, was übrig ist.
        // `x-api-key`-Header (Anthropic-Header, viele Gateways), dazu Azure
        // `api-key` und Google `x-goog-api-key` (Review-Fund) — auch in
        // JSON-/Quote-Form, analog zum Credential-Zeilen-Muster unten, das
        // `x-api-key` (Bindestrich) nicht erfasst.
        simple(
            r#"(?i)\b(?:x-(?:goog-)?)?api-key['"]?\s*[:=]\s*(?:bearer\s+)?(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s'"]+)"#,
            "eingebautes x-api-key-Header-Muster ist gültig",
        ),
        // `Authorization: Basic <base64>` (auch `Proxy-Authorization`). Nur
        // mit Header-Namen — ein bloßes "Basic authentication" in Text
        // bleibt unberührt.
        simple(
            r#"(?i)\b(?:proxy-)?authorization['"]?\s*[:=]\s*['"]?(?:basic|token)\s+(?:bearer\s+)?(?:[a-z_]+=)?(?:"[^"\r\n]*"|[A-Za-z0-9+/._~-]+=*)"#,
            "eingebautes Authorization-Basic-Muster ist gültig",
        ),
        // Spec 0095, A2.3: Azure-Connection-Strings. `AccountKey=` (Storage)
        // und `SharedAccessKey=` (Service Bus / Event Hubs) tragen den
        // eigentlichen Schlüssel; keines der Schlüsselwort-Muster oben kennt
        // diese Namen.
        //
        // Der Wert endet am nächsten `;`, an Leerraum oder an einem
        // Anführungszeichen — die Zeichenklasse ist deshalb exakt das
        // Base64-Alphabet mit Padding, nichts darüber hinaus.
        //
        // **Position: bei den übrigen Mustern mit Schlüsselnamen am Ende der
        // Liste.** Weiter vorn könnte diese Regel einem älteren Muster den
        // Anker nehmen (`AccountKey=Bearer abc…` würde `Bearer` verbrauchen
        // und den Token freilegen) — derselbe Fehler, den die Header-Muster
        // hier schon einmal gemacht haben. Der Schlüsselname bleibt über
        // `${key}` stehen: `AccountName=` daneben soll lesbar bleiben, sonst
        // ist nicht mehr erkennbar, welche Ressource gemeint war.
        PatternRule {
            regex: Regex::new(r"(?i)(?P<key>\b(?:Account|SharedAccess)Key=)[A-Za-z0-9+/=]+")
                .expect("eingebautes Azure-Connection-String-Muster ist gültig"),
            replacement: "${key}[REDACTED]",
        },
        // spec-reviewer-Fund (Spec 0068, ERHÖHT): die Formate genau der
        // Dateien, deren Lesen Teil 2 bestätigungspflichtig macht — nach
        // einem bestätigten Lesen sollen sie trotzdem nicht im Klartext an
        // die KI gehen.
        // Docker `~/.docker/config.json`: `"auth": "<base64 user:pass>"`.
        simple(
            r#"(?i)"auth"\s*:\s*"[A-Za-z0-9+/=]{8,}""#,
            "eingebautes Docker-auth-Muster ist gültig",
        ),
        // kubeconfig: privater Client-Schlüssel (Base64).
        simple(
            r"(?i)\bclient-key-data\s*:\s*\S+",
            "eingebautes kubeconfig-client-key-data-Muster ist gültig",
        ),
        // `.netrc`: `machine h login u password p` bzw. `password p` als
        // eigene Zeile. Nur in dieser Struktur, damit Fließtext wie
        // "password is required" unberührt bleibt.
        simple(
            r"(?i)\b(?:machine\s+\S+(?:\s+login\s+\S+)?|default\s+login\s+\S+)\s+password\s+[^\s:=]\S*",
            "eingebautes netrc-Muster ist gültig",
        ),
        // `default password x` (ohne `login`, laut netrc-Syntax gültig) —
        // am Zeilenanfang verankert, damit Fließtext "The default password
        // is …" unberührt bleibt (vierte Review-Runde).
        simple(
            r"(?im)^[ \t]*default\s+(?:login\s+\S+\s+)?password\s+[^\s:=]\S*",
            "eingebautes netrc-default-Muster ist gültig",
        ),
        simple(
            r"(?im)^[ \t]*password[ \t]+[^\s:=]\S*[ \t]*$",
            "eingebautes netrc-Zeilen-Muster ist gültig",
        ),
        // `.pgpass`: `host:port:datenbank:nutzer:passwort` (Port Zahl oder
        // `*`) — ganze Zeile. Der Host muss einen Buchstaben/Punkt enthalten
        // oder `*` sein, damit MAC- und IPv6-Adressen nicht treffen.
        simple(
            r"(?im)^(?:\*|[^:\s#]*[a-z.][^:\s#]*):(?:\d{1,5}|\*):[^:\s]+:[^:\s]+:\S+$",
            "eingebautes pgpass-Muster ist gültig",
        ),
        // Die breite URL-Regel der ersten Fassung (Passwort darf `?`/`#`
        // enthalten, unkodiert in `.env`-Dateien real) zusätzlich am Ende:
        // der Query-String-Fall (`?password=`) ist hier schon vom
        // Credential-Zeilen-Muster redigiert, die strenge Regel weiter oben
        // bleibt davor unverändert (zweite Review-Runde).
        PatternRule {
            regex: Regex::new(
                r#"(?i)(?P<scheme>\b[a-z][a-z0-9+.-]*)://(?P<user>[^:@/\s,;"]*):[^@/\s,;"]+@"#,
            )
            .expect("eingebautes breites URL-Zugangsdaten-Muster ist gültig"),
            replacement: "${scheme}://${user}:[REDACTED]@",
        },
        // Spec 0078: URL-Zugangsdaten, bei denen Benutzername ODER Passwort
        // ein unkodiertes `@` enthalten. Formal müsste dort `%40` stehen
        // (die prozentkodierte Form fangen die Muster oben schon), in der
        // Praxis schreiben Menschen und Werkzeuge das `@` roh, und viele
        // Clients werten das LETZTE `@` als Trenner. Alle Muster oben
        // schließen `@` aus beiden Klassen aus: das Passwort endete am
        // ersten `@` und der Rest blieb im Klartext; enthielt der BENUTZER
        // ein `@` (`postgres://svc@tenant:pw@db/x` — bei mehreren
        // gehosteten Datenbanken die vorgeschriebene Schreibweise), griff
        // gar keines von ihnen.
        //
        // Das ist das strenge URL-Muster (s. oben) mit genau einem
        // Unterschied: `@` fehlt in beiden ausgeschlossenen Klassen. Der
        // Benutzer endet am ersten `:`, das Passwort reicht gierig bis zum
        // LETZTEN `@` vor einem Stoppzeichen. Alle Muster oben bleiben
        // wörtlich und an ihrer Stelle; diese Regel ergänzt nur.
        //
        // Steht BEWUSST AM ENDE der Liste, nach dem Schlüsselwort-Muster:
        // weiter vorn schluckt sie ein späteres `password=` samt dessen
        // Anker (`https://u:x@h:1|password=ab@cdSecret` → `…@cdSecret`,
        // Rest im Klartext) — derselbe Fehler, den die Muster mit
        // Header-/Schlüsselnamen weiter oben schon einmal gemacht haben.
        // Hier sehen alle älteren Muster den Text zuerst.
        //
        // `?` und `#` bleiben Stoppzeichen (wie beim strengen Muster): ohne
        // sie liefe die Regel über einen Query-String mit `@` hinweg und
        // schluckte den Host
        // (`postgres://app:pw@db?sslmode=require&user=x@y` → `…:[REDACTED]@y`).
        //
        // Bewusst akzeptiert (Spec 0078 §5, Tests `…_known_remaining_case_…`
        // und `…_known_over_redaction_…`):
        // - Ein Passwort, das `@` UND eines der Zeichen `/ ? #` enthält,
        //   wird weiter nur bis zum ersten dieser Zeichen redigiert —
        //   dieselben Zeichen müssen Stoppzeichen bleiben. Ebenso `,` `;`
        //   `"` wie beim DB-Muster.
        // - Überredaktion: folgt einer URL ohne Stoppzeichen ein weiteres
        //   `@` (etwa hinter `|`), wird der Teil dazwischen mit geschwärzt.
        //   Dabei leakt nichts; dieselbe Art Nebenwirkung wie die schon
        //   dokumentierte bei `|`/`&` am DB-Muster.
        PatternRule {
            regex: Regex::new(
                r#"(?i)(?P<scheme>\b[a-z][a-z0-9+.-]*)://(?P<user>[^:/\s,;"?#]*):[^/\s,;"?#]+@"#,
            )
            .expect("eingebautes URL-Muster mit @ in den Zugangsdaten ist gültig"),
            replacement: "${scheme}://${user}:[REDACTED]@",
        },
    ];
    // Spec 0095, A1: die Kommandozeilen-Regeln **zweimal** anhängen, wörtlich
    // gleich. Dasselbe Mittel, mit dem schon die Query-Parameter-Regel zweimal
    // läuft (s. dort) und aus demselben Grund.
    //
    // `replace_all` sucht nicht überlappend und setzt hinter einem Treffer
    // wieder auf. Verschluckt ein Wert den **Anker des nächsten Treffers**,
    // entfällt dieser — das zweite Geheimnis bleibt im Klartext neben dem
    // Platzhalter stehen. Gemessen (Fund des `regression-guard`):
    // `mysql -pA<U+00A0>mysql -pGeheim` wurde zu
    // `mysql [REDACTED] -pGeheim`, weil das zweite `mysql` Teil des ersten
    // Werts war (U+00A0 ist für die Shell kein Trenner, der Wert reicht also
    // zu Recht darüber hinweg).
    //
    // Dieselbe Lücke gab es unabhängig davon bei **zwei Passwort-Schaltern in
    // einem Aufruf** (`mysql -pA -pGeheim`): der zweite hat keinen
    // Programmnamen mehr vor sich. Auch das schließt der zweite Durchlauf, er
    // ist also nicht bloß eine Reparatur, sondern zusätzliche Abdeckung.
    //
    // Nach dem ersten Durchlauf steht an der Stelle des ersten Werts
    // `[REDACTED]`, und der zweite Durchlauf findet den freigelegten Anker.
    // Drei und mehr Vorkommen in einer Kette brauchten je einen weiteren
    // Durchlauf — dieselbe bewusste Grenze wie bei der Query-Parameter-Regel.
    patterns.extend(command_line_password_patterns(CMD_VALUE));
    patterns.extend(htpasswd_patterns(CMD_VALUE));
    patterns.extend(command_line_password_patterns(CMD_VALUE_AFTER_PLACEHOLDER));
    patterns
}

/// Die Regeln aus Spec 0095, A1 — Passwörter als Argument eines
/// Kommandozeilenprogramms. Eigene Funktion, weil
/// [`built_in_patterns`] sie **zweimal** anhängt (s. dort).
fn command_line_password_patterns(value: &str) -> Vec<PatternRule> {
    vec![
        // --- Spec 0095, A1: Passwörter als Argument eines
        // Kommandozeilenprogramms -------------------------------------------
        //
        // **Position: ganz am Ende der Liste, und zwar aus einem Grund, der
        // sich beweisen lässt.** Jede dieser Regeln verbraucht einen
        // Programmnamen, einen Schalter und einen Wert, der am nächsten
        // Leerraum endet — genau die Bauart, die im Redactor schon dreimal
        // einem späteren Muster den mehrteiligen Anker zerschnitten hat
        // (`api-key: -----BEGIN …`, zweimal die Query-String-Regel von
        // Spec 0078, s. den Kommentar am Listenanfang). Hinter allen
        // bestehenden Mustern angewendet, sehen diese den Originaltext
        // zuerst: **keine bestehende Regel kann durch die neuen etwas
        // verlieren.** Das ist dieselbe Begründung, mit der schon die
        // Muster mit Header-/Schlüsselnamen weiter oben ans Ende gewandert
        // sind — nur hier per Konstruktion vollständig, weil nichts mehr
        // folgt.
        //
        // Die umgekehrte Richtung (eine bestehende Regel nimmt einer neuen
        // den Anker) ist möglich und bewusst hingenommen: Steht z. B.
        // `password=mysql -pGeheim` im Text, ersetzt das Schlüsselwort-Muster
        // `password=mysql`, und die mysql-Regel findet ihren Programmnamen
        // nicht mehr. Der Wert bliebe im Klartext — aber genau wie vor
        // dieser Spec, also ohne Verschlechterung. Es ist keine Lockerung,
        // nur eine Grenze der Abdeckung.
        //
        // Der bestehende Absorb-Schritt schluckt bei angehängten Werten den
        // Schalter mit (`-p[REDACTED]` → `[REDACTED]`, s. dort). Spec 0095
        // §5 nimmt das ausdrücklich hin; der Absorb-Schritt bleibt
        // unverändert.
        //
        // A1.1 mysql-Familie: NUR der angehängte Wert (`-pX`, `-p'X'`).
        // `mysql -p <wert>` mit Leerzeichen ist kein Passwort, sondern der
        // Datenbankname — ohne angehängten Wert fragt der Client interaktiv
        // (MySQL-Client-Dokumentation zu `--password[=password], -p[password]`,
        // Spec 0095 §1.4). `--password=<wert>` deckt das
        // Schlüsselwort-Muster oben schon ab.
        //
        // **Kein `(?i)` für das ganze Muster**, nur für den Programmnamen:
        // `-P` ist bei mysql der Port. Global case-insensitive hätte
        // `mysql -P3306 -h db` den Port geschwärzt (Spec 0095, T12).
        keep_head(
            &format!(
                r"(?P<head>\b(?i:mysqldump|mysqladmin|mysql|mariadb)\b{CMD_ARGS}{CMD_SEP}-p{CMD_CONT})(?:{value})"
            ),
            "eingebautes mysql-Passwortargument-Muster ist gültig",
        ),
        // A1.2 `sshpass -p`, mit und ohne Leerzeichen.
        //
        // Die Argumente zwischen `sshpass` und `-p` sind hier auf eigene
        // Schalter (`-`-Präfix) beschränkt statt auf [`CMD_ARGS`]: sonst
        // reichte die Regel über `sshpass -f pwfile ssh` hinweg bis zum
        // `-p 2222` des `ssh`-Aufrufs und schwärzte den Port (Spec 0095,
        // T12 — mit `CMD_ARGS` gemessen fehlgeschlagen). Die Optionen von
        // `sshpass` stehen ohnehin alle vor dem auszuführenden Kommando.
        keep_head(
            &format!(
                r"(?P<head>\b(?i:sshpass)\b(?:{CMD_SEP}-[A-Za-z][^\s;|&<>]*){{0,6}}?{CMD_SEP}-p{CMD_GAP})(?:{value})"
            ),
            "eingebautes sshpass-Passwortargument-Muster ist gültig",
        ),
        // A1.3 `curl -u user:passwort` / `--user`. Redigiert wird nur der
        // Teil nach dem ERSTEN `:` — der Benutzername bleibt lesbar, und
        // `curl -u admin` ohne `:` bleibt unangetastet (das `:` ist in
        // dieser Regel Pflicht).
        //
        // `-[A-Za-z]*u` deckt den Kurzschalterblock ab, in dem `u` der
        // letzte Buchstabe ist (`-sSu`, `-sSuadmin:pw`) — gemessen senden
        // `curl -uadmin:pw`, `curl -sSuadmin:pw` und `curl -u admin:pw`
        // alle denselben `Authorization: Basic`-Header (Spec 0095 §1.4).
        //
        // Drei Varianten, weil die Quote-Form nicht in einer Alternative
        // mitgehen kann: ohne Backreferenz kann kein Muster verlangen, dass
        // der schließende Quote zum öffnenden passt. Ein gemeinsames Muster
        // mit optionalem Quote hätte bei `curl -u a:pw -H 'X: y'` den Text
        // bis zum ERSTEN Quote irgendwo dahinter geschluckt.
        keep_head_quoted(
            &format!(
                r"(?P<head>\b(?i:curl)\b{CMD_ARGS}{CMD_SEP}(?:-[A-Za-z]*u|--user)=?{CMD_GAP}'[^':\r\n]*:)[^'\r\n]*'"
            ),
            "${head}[REDACTED]'",
            "eingebautes curl-Basic-Auth-Muster (einfache Quotes) ist gültig",
        ),
        keep_head_quoted(
            &format!(
                r#"(?P<head>\b(?i:curl)\b{CMD_ARGS}{CMD_SEP}(?:-[A-Za-z]*u|--user)=?{CMD_GAP}"[^":\r\n]*:)[^"\r\n]*""#
            ),
            "${head}[REDACTED]\"",
            "eingebautes curl-Basic-Auth-Muster (doppelte Quotes) ist gültig",
        ),
        keep_head(
            &format!(
                r#"(?P<head>\b(?i:curl)\b{CMD_ARGS}{CMD_SEP}(?:-[A-Za-z]*u|--user)=?{CMD_GAP}[^\s:;|&<>'"]*:)(?:{value})"#
            ),
            "eingebautes curl-Basic-Auth-Muster ist gültig",
        ),
        // A1.5 `redis-cli -a <wert>` / `--pass <wert>` (auch angehängt).
        keep_head(
            &format!(
                r"(?P<head>\b(?i:redis-cli)\b{CMD_ARGS}{CMD_SEP}(?:-a|--pass(?:=|{CMD_SEP}))=?{CMD_GAP})(?:{value})"
            ),
            "eingebautes redis-cli-Passwortargument-Muster ist gültig",
        ),
        // A1.6 `smbclient`/`rpcclient`: `-U user%passwort`. Das `%` ist der
        // Trenner, es bleibt (wie das `:` bei curl) im `head` stehen.
        keep_head_quoted(
            &format!(
                r"(?P<head>\b(?i:smbclient|rpcclient)\b{CMD_ARGS}{CMD_SEP}(?:-U|--user)=?{CMD_GAP}'[^'%\r\n]*%)[^'\r\n]*'"
            ),
            "${head}[REDACTED]'",
            "eingebautes smbclient-Muster (einfache Quotes) ist gültig",
        ),
        keep_head_quoted(
            &format!(
                r#"(?P<head>\b(?i:smbclient|rpcclient)\b{CMD_ARGS}{CMD_SEP}(?:-U|--user)=?{CMD_GAP}"[^"%\r\n]*%)[^"\r\n]*""#
            ),
            "${head}[REDACTED]\"",
            "eingebautes smbclient-Muster (doppelte Quotes) ist gültig",
        ),
        keep_head(
            &format!(
                r#"(?P<head>\b(?i:smbclient|rpcclient)\b{CMD_ARGS}{CMD_SEP}(?:-U|--user)=?{CMD_GAP}[^\s%;|&<>'"]*%)(?:{value})"#
            ),
            "eingebautes smbclient-Muster ist gültig",
        ),
        // A1.7 `openssl`: bei `-pass`/`-passin`/`-passout` ist nur die Form
        // `pass:<wert>` das Passwort selbst — `env:`, `file:`, `fd:` und
        // `stdin` benennen eine QUELLE und bleiben lesbar (Spec 0095, T12).
        // Deshalb ist `pass:` hier Pflicht.
        keep_head_quoted(
            &format!(
                r"(?P<head>\b(?i:openssl)\b{CMD_ARGS}{CMD_SEP}-pass(?:in|out)?=?{CMD_GAP}'pass:)[^'\r\n]*'"
            ),
            "${head}[REDACTED]'",
            "eingebautes openssl-pass-Muster (einfache Quotes) ist gültig",
        ),
        keep_head_quoted(
            &format!(
                r#"(?P<head>\b(?i:openssl)\b{CMD_ARGS}{CMD_SEP}-pass(?:in|out)?=?{CMD_GAP}"pass:)[^"\r\n]*""#
            ),
            "${head}[REDACTED]\"",
            "eingebautes openssl-pass-Muster (doppelte Quotes) ist gültig",
        ),
        keep_head(
            &format!(
                r#"(?P<head>\b(?i:openssl)\b{CMD_ARGS}{CMD_SEP}-pass(?:in|out)?=?{CMD_GAP}pass:)(?:{value})"#
            ),
            "eingebautes openssl-pass-Muster ist gültig",
        ),
        // `openssl … -k <wert>`: hier ist der Wert die Passphrase selbst
        // (`openssl enc -help`: „-k val  Passphrase", gemessen mit
        // OpenSSL 3.6.3). Der hinter `-k` verlangte [`CMD_SEP`] (Leerraum
        // oder Zeilenfortsetzung, mindestens einer) hält `-keyform`/`-key`
        // heraus — beides benennt eine Datei.
        keep_head(
            &format!(r"(?P<head>\b(?i:openssl)\b{CMD_ARGS}{CMD_SEP}-k{CMD_SEP})(?:{value})"),
            "eingebautes openssl-k-Muster ist gültig",
        ),
    ]
}

/// Die beiden `htpasswd`-Regeln aus Spec 0095, A1.4 — getrennt von
/// [`command_line_password_patterns`], weil sie **nur einmal** laufen dürfen.
///
/// Sie zählen Positionsargumente, und ein zweiter Durchlauf verschiebt diese
/// Zählung: Nach der ersten Redaktion fand die Dreipositionsregel bei
/// `htpasswd -bB -C 12 f user [REDACTED]` eine andere, ebenfalls gültige
/// Lesart (`-C` ohne Zahlenwert, `12` als erstes Positionsargument) und
/// schwärzte `user` mit (gemessen). Der Grund für den zweiten Durchlauf —
/// zwei Passwort-Schalter in einem Aufruf — gibt es bei `htpasswd` ohnehin
/// nicht: es nimmt genau ein Passwort.
fn htpasswd_patterns(value: &str) -> Vec<PatternRule> {
    vec![
        // A1.4 `htpasswd` mit einem Schalterblock, der `b` enthält (nur dann
        // steht das Passwort überhaupt auf der Kommandozeile; ohne `-b` fragt
        // `htpasswd` interaktiv). Gemessen an der Usage-Ausgabe (Spec 0095
        // §1.4): `-b[…] [-C cost] [-r rounds] file user password` und
        // `-nb[…] [-C cost] [-r rounds] user password` — mit `n` im Block
        // entfällt die Datei, das Passwort ist dann das ZWEITE statt dritte
        // Positionsargument. Deshalb zwei Regeln.
        //
        // `n` und `b` dürfen in **einem** Block stehen (`-nbB`) oder auf
        // getrennte Schalter verteilt sein (`htpasswd -n -b user pw`) — die
        // erste Fassung verlangte sie im selben Block, und die getrennte,
        // völlig gültige Form fiel dadurch durch **beide** Regeln: das
        // Passwort blieb im Klartext (spec-reviewer-Fund, Runde 1, gemessen).
        // Daher die drei Alternativen: gemeinsam, `n` vor `b`, `b` vor `n`.
        //
        // Die `n`-Regel steht zuerst, die Dreipositionsregel danach. Deren
        // Schalterkette schließt `n` NICHT aus: bei einer ungültigen
        // Mischform (`htpasswd -nb f user pw` — mit `-n` gibt es keine
        // Datei) redigiert die erste Regel `user` und die zweite `pw`, sodass
        // kein Geheimnis übrig bleibt. Der Preis ist Über-Redaktion, wenn
        // hinter dem Passwort noch ein viertes Wort steht
        // (`htpasswd -nbB user pw extra` verliert auch `extra`) — harmlos,
        // und die Gegenrichtung wäre ein Leak.
        //
        // [`HT_OPT`] fängt die Schalter mit Zahlenwert (`-C 12`, `-r 4096`)
        // ein, damit deren Wert nicht als Positionsargument zählt. Was nach
        // dem Passwort kommt (`> out`, `| tee`), bleibt unberührt — die
        // Trennzeichen sind aus allen Zeichenklassen ausgeschlossen.
        keep_head(
            &format!(
                r"(?P<head>\b(?i:htpasswd)\b{HT_OPT}*?(?:{CMD_SEP}-(?:[A-Za-z]*b[A-Za-z]*n[A-Za-z]*|[A-Za-z]*n[A-Za-z]*b[A-Za-z]*)|{CMD_SEP}-[A-Za-z]*n[A-Za-z]*{HT_OPT}*?{CMD_SEP}-[A-Za-z]*b[A-Za-z]*|{CMD_SEP}-[A-Za-z]*b[A-Za-z]*{HT_OPT}*?{CMD_SEP}-[A-Za-z]*n[A-Za-z]*){HT_OPT}*{CMD_SEP}[^\s;|&<>]+{CMD_SEP})(?:{value})"
            ),
            "eingebautes htpasswd-Muster (mit -n) ist gültig",
        ),
        keep_head(
            &format!(
                r"(?P<head>\b(?i:htpasswd)\b{HT_OPT}*?{CMD_SEP}-[A-Za-z]*b[A-Za-z]*{HT_OPT}*{CMD_SEP}[^\s;|&<>]+{CMD_SEP}[^\s;|&<>]+{CMD_SEP})(?:{value})"
            ),
            "eingebautes htpasswd-Muster ist gültig",
        ),
    ]
}

impl OutputRedactor for DefaultOutputRedactor {
    fn redact(&self, output: &CommandOutput) -> CommandOutput {
        CommandOutput {
            stdout: redact_bytes(&output.stdout, &self.patterns),
            stderr: redact_bytes(&output.stderr, &self.patterns),
            exit_code: output.exit_code,
            truncated: output.truncated,
        }
    }

    fn redact_text(&self, text: &str) -> String {
        String::from_utf8_lossy(&redact_bytes(text.as_bytes(), &self.patterns)).into_owned()
    }
}

/// Wendet alle `patterns` nacheinander auf `data` an. Arbeitet auf einer
/// (ggf. verlustbehafteten) UTF-8-Interpretation der Bytes — Kommando-Output
/// ist praktisch immer Text, und selbst bei vereinzelten ungültigen
/// Byte-Sequenzen ist "durch Ersatzzeichen ersetzt, aber Secret trotzdem
/// erkannt" dem Fail-safe-Prinzip (Spec 0002 Abschnitt 1) angemessener als
/// die Redaction bei Nicht-UTF-8-Output ganz zu überspringen.
fn redact_bytes(data: &[u8], patterns: &[PatternRule]) -> Vec<u8> {
    let mut text = String::from_utf8_lossy(data).into_owned();
    for rule in patterns {
        if rule.regex.is_match(&text) {
            text = rule.regex.replace_all(&text, rule.replacement).into_owned();
        }
    }
    absorb_fragments_next_to_placeholders(&text).into_bytes()
}

/// Spec 0068 (dritte Review-Runde, ERHÖHT): hat ein Muster nur einen Teil
/// eines längeren Tokens ersetzt (z. B. ein zufälliges `AKIA…` mitten in
/// einem Base64-Wert, einen Provider-Key mitten in einem Bearer-Token),
/// nimmt jedes spätere Muster, dessen Wert-Klasse kein `[` kennt, den Rest
/// nicht mehr mit — er bliebe im Klartext. Deshalb zum Schluss: jeder
/// Platzhalter schluckt die direkt angrenzenden Token-Zeichen
/// (Base64/URL-sicher). Es entsteht dadurch nie eine neue Redaction ohne
/// vorhandenen Platzhalter; `:`/`@`/Leerzeichen/Quotes begrenzen, so
/// bleiben z. B. Nutzer und Host einer URL lesbar. Links davon zählen `_`
/// und `=` nicht (vierte Review-Runde): so bleiben Schlüsselnamen wie
/// `GITHUB_TOKEN=`/`DB_PASSWORD=` lesbar; ein Rest vor dem Treffer ist der
/// Anfang des Werts, nicht sein Geheimnis-Kern.
fn absorb_fragments_next_to_placeholders(text: &str) -> String {
    static ABSORB: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let absorb = ABSORB.get_or_init(|| {
        Regex::new(r"[A-Za-z0-9+/.~-]*\[REDACTED\][A-Za-z0-9+/=._~-]*")
            .expect("eingebautes Platzhalter-Muster ist gültig")
    });
    if !text.contains(REDACTED_PLACEHOLDER) {
        return text.to_string();
    }
    absorb.replace_all(text, REDACTED_PLACEHOLDER).into_owned()
}
