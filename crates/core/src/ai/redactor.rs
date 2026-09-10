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
const REDACTED_PLACEHOLDER: &str = "[REDACTED]";

/// Default-Implementierung (Spec 0006, Abschnitt 5): erkennt
/// Private-Key-Blöcke, `password=`/`token=`/`api_key=`-artige Zeilen
/// (Groß-/Kleinschreibung ignoriert), AWS-/GitHub-/Slack-/Stripe-/Google-/
/// npm-Zugangsdaten-Muster, DB-Connection-Strings
/// (`schema://user:pass@host`) und Unix-Crypt-/Shadow-Passwort-Hashes
/// (`$1$`/`$5$`/`$6$`/`$y$`/`$2b$`/`$7$`/`$apr1$` & verwandte Varianten,
/// s. `built_in_patterns()`).
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
/// auf benannte Capture-Groups im Muster zurückgreifen (s.
/// `db_connection_string_pattern()`). Das erfordert ein individuelles
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

fn built_in_patterns() -> Vec<PatternRule> {
    vec![
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
        // nicht abgedeckt: Argon2 (`$argon2id$...`, deutlich komplexeres
        // Mehrsegment-Format mit `,`-getrennten Parametern), phpass
        // (`$P$`/`$H$`, WordPress/Drupal-DB-Dumps) und BSD/Solaris-Exoten
        // (`$sha1$`, Solaris-`$md5$`) — real vorkommend, aber seltener als
        // `/etc/shadow`/`.htpasswd`; als bewusste Scope-Grenze für diesen
        // Fix offengelegt statt stillschweigend fallengelassen, nicht
        // sicherheitskritisch verschwiegen.
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
        // DB-Connection-Strings (Diagnose-Folge-Fix, 2026-09): das Passwort
        // steht zwischen `:` und `@`, keines der Schlüsselwort-Muster
        // (`password=`/`token=`/...) greift auf diese Syntax. Deckt die in
        // der DevOps-/Homelab-Zielgruppe (Docker-/CI-Logs, Config-Dumps)
        // häufigsten Schemata ab: Postgres, MySQL/MariaDB, MongoDB (inkl.
        // `+srv`), Redis (inkl. TLS-`rediss`) und AMQP/RabbitMQ (inkl.
        // TLS-`amqps`).
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
        // Falsch-Positiv-Vorsicht (explizit gefordert): ein Schema+Host
        // OHNE Zugangsdaten (`postgres://host:5432/db`, kein `user:pass@`)
        // matcht nicht — das Muster verlangt zwingend die
        // `user:passwort@`-Struktur direkt nach dem Schema (das
        // Passwort-Zeichenklasse `[^@/\s]+` endet erst am `@`, es gibt also
        // gar keinen Match-Ansatz ohne ein tatsächliches `@`-getrenntes
        // Credential-Paar).
        PatternRule {
            regex: Regex::new(
                r"(?P<scheme>postgres(?:ql)?|mysql|mariadb|mongodb(?:\+srv)?|rediss?|amqps?)://(?P<user>[A-Za-z0-9_.%+-]+):[^@/\s]+@",
            )
            .expect("eingebautes DB-Connection-String-Muster ist gültig"),
            replacement: "${scheme}://${user}:[REDACTED]@",
        },
        // Slack-Tokens: `xoxb-`/`xoxp-`/`xoxo-`/`xoxa-` (Bot/User/
        // Legacy-Workspace/App-Token), gefolgt von mehreren
        // `-`-getrennten alphanumerischen Segmenten. Reale Tokens sind
        // weit über 24 Zeichen lang (klassische Bot-Tokens z. B. ~50+
        // Zeichen) — die Mindestlänge ist konservativ niedrig genug, um
        // keine echte Variante zu verpassen, aber hoch genug, um einen
        // zufälligen `xoxb-`-Präfix-Treffer auf kurzem, unzusammenhängendem
        // Text unwahrscheinlich zu machen.
        simple(
            r"xox[bpoa]-[A-Za-z0-9-]{24,}",
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
    text.into_bytes()
}
