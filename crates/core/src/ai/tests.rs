//! Unit-Tests für das KI-Provider-Modul: Redactor-Muster (Spec 0006,
//! Abschnitt 7, erster Block) und `MockAiProvider`-Grundverhalten. Echte
//! Provider-HTTP-Tests sind Sache von `crates/ai-providers`.

use std::pin::Pin;

use futures::{Stream, StreamExt};
use regex::Regex;

use super::*;
use crate::profiles::AiAction;
use crate::ssh::CommandOutput;

// --- MockAiProvider (Aufgabenstellung Teil 1, Punkt 3) --------------------

/// Konfigurierbar mit einer festen Sequenz von [`AiEvent`]s, die bei
/// `send()` als Stream zurückgegeben wird — unabhängig vom übergebenen
/// [`SessionContext`] (dient dem Testen von Aufrufer-Logik, nicht des
/// Providers selbst).
struct MockAiProvider {
    events: Vec<AiEvent>,
}

impl MockAiProvider {
    fn new(events: Vec<AiEvent>) -> Self {
        Self { events }
    }
}

impl AiProvider for MockAiProvider {
    fn send(&self, _context: SessionContext) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
        Box::pin(futures::stream::iter(self.events.clone()))
    }
}

fn empty_context() -> SessionContext {
    SessionContext {
        system_context: String::new(),
        history: Vec::new(),
        available_actions: Vec::new(),
    }
}

#[tokio::test]
async fn test_mock_ai_provider_replays_configured_event_sequence() {
    let provider = MockAiProvider::new(vec![
        AiEvent::TextDelta("Hallo".to_string()),
        AiEvent::TextDelta(", Welt".to_string()),
        AiEvent::ActionProposed(AiAction::SuggestCommand {
            command: "ls -la".to_string(),
        }),
        AiEvent::Done,
    ]);

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![
            AiEvent::TextDelta("Hallo".to_string()),
            AiEvent::TextDelta(", Welt".to_string()),
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string()
            }),
            AiEvent::Done,
        ]
    );
}

#[tokio::test]
async fn test_mock_ai_provider_can_replay_error_event() {
    let provider = MockAiProvider::new(vec![AiEvent::Error(AiError::RateLimited)]);

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::RateLimited)]);
}

// --- DefaultOutputRedactor (Spec 0006, Abschnitt 7, erster Block) --------

fn output(stdout: &str) -> CommandOutput {
    CommandOutput {
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
        exit_code: Some(0),
        truncated: false,
    }
}

fn stdout_text(redacted: &CommandOutput) -> String {
    String::from_utf8(redacted.stdout.clone()).expect("Redactor liefert gültiges UTF-8")
}

#[test]
fn test_redactor_detects_private_key_block() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(
        "vor dem Key\n\
         -----BEGIN RSA PRIVATE KEY-----\n\
         MIIEpAIBAAKCAQEA1234567890abcdef\n\
         -----END RSA PRIVATE KEY-----\n\
         nach dem Key",
    );

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(redacted.contains("[REDACTED]"));
    assert!(!redacted.contains("MIIEpAIBAAKCAQEA1234567890abcdef"));
    assert!(redacted.starts_with("vor dem Key"));
    assert!(redacted.ends_with("nach dem Key"));
}

#[test]
fn test_redactor_detects_password_token_api_key_lines_case_insensitive() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(
        "PASSWORD=hunter2\n\
         token=abc123XYZ\n\
         Api_Key=sk-superduper\n\
         harmlose Zeile",
    );

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("hunter2"));
    assert!(!redacted.contains("abc123XYZ"));
    assert!(!redacted.contains("sk-superduper"));
    assert!(redacted.contains("harmlose Zeile"));
    assert_eq!(redacted.matches("[REDACTED]").count(), 3);
}

#[test]
fn test_redactor_detects_aws_access_key() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("AWS_ACCESS_KEY_ID=AKIAABCDEFGHIJKLMNOP wurde gesetzt");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("AKIAABCDEFGHIJKLMNOP"));
    assert!(redacted.contains("[REDACTED]"));
}

#[test]
fn test_redactor_leaves_unsuspicious_text_unchanged() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("Build erfolgreich.\n3 Tests bestanden, 0 fehlgeschlagen.\n");

    let redacted = redactor.redact(&input);

    assert_eq!(redacted.stdout, input.stdout);
    assert_eq!(redacted.stderr, input.stderr);
    assert_eq!(redacted.exit_code, input.exit_code);
}

#[test]
fn test_redactor_detects_user_defined_extra_patterns() {
    let custom = Regex::new(r"internal-secret-\d+").unwrap();
    let redactor = DefaultOutputRedactor::with_extra_patterns(vec![custom]);
    let input = output("Wert: internal-secret-42 wurde geladen");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("internal-secret-42"));
    assert!(redacted.contains("[REDACTED]"));
}

#[test]
fn test_redactor_extra_patterns_do_not_disable_built_in_patterns() {
    let custom = Regex::new(r"internal-secret-\d+").unwrap();
    let redactor = DefaultOutputRedactor::with_extra_patterns(vec![custom]);
    let input = output("password=hunter2 und internal-secret-42");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("hunter2"));
    assert!(!redacted.contains("internal-secret-42"));
}

// --- Spec 0013: Redactor Hardening Tests (T7) -------------------------

#[test]
fn test_t7_redactor_handles_quoted_password_with_spaces() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("config: password=\"top secret 123\" and secret: 'my passphrase'");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("top secret 123"));
    assert!(!redacted.contains("my passphrase"));
    assert!(redacted.contains("[REDACTED]"));
}

#[test]
fn test_redactor_detects_bearer_token() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.e30.t-IDcZMW64A1Rh6mOF9Aq5bE099MV8");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted
        .contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.e30.t-IDcZMW64A1Rh6mOF9Aq5bE099MV8"));
    assert!(redacted.contains("[REDACTED]"));
}

#[test]
fn test_redactor_detects_github_and_aws_session_tokens() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("GITHUB_TOKEN=ghp_1234567890abcdefghijklmnopqrstuvwxyzAB\nAWS_SESSION_TOKEN=ASIABBBBBBBBBBBBBBBB");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("ghp_1234567890abcdefghijklmnopqrstuvwxyzAB"));
    assert!(!redacted.contains("ASIABBBBBBBBBBBBBBBB"));
    assert!(redacted.contains("[REDACTED]"));
}

#[test]
fn test_redactor_detects_pgp_and_pkcs8_private_keys() {
    let redactor = DefaultOutputRedactor::new();
    let input_pgp = output("-----BEGIN PGP PRIVATE KEY BLOCK-----\nVersion: BCPG C# v1.6.1.0\nlQPGBF...\n-----END PGP PRIVATE KEY BLOCK-----");
    let redacted_pgp = stdout_text(&redactor.redact(&input_pgp));
    assert!(!redacted_pgp.contains("BCPG"));
    assert!(redacted_pgp.contains("[REDACTED]"));

    let input_pkcs8 = output("-----BEGIN ENCRYPTED PRIVATE KEY-----\nMIIFDjBABgkqhkiG9w0BBQ0wMzAbBgkqhkiG9w0BBQwwDgQI...\n-----END ENCRYPTED PRIVATE KEY-----");
    let redacted_pkcs8 = stdout_text(&redactor.redact(&input_pkcs8));
    assert!(!redacted_pkcs8.contains("MIIFDjBABgkqhkiG9w0BBQ0wMzAbBgkqhkiG9w0BBQwwDgQI"));
    assert!(redacted_pkcs8.contains("[REDACTED]"));
}

/// Regressionstest für den unabhängigen Review-Pass (Spec 0006): das
/// unverschlüsselte, moderne PKCS8-Format ("-----BEGIN PRIVATE KEY-----",
/// ohne jeden Typ-Präfix — z. B. `openssl genpkey`, Let's-Encrypt-
/// `privkey.pem`, Kubernetes-`tls.key`, GCP-Service-Account-JSON) wurde
/// bislang gar nicht erkannt, weil das Muster fälschlich mindestens ein
/// Zeichen zwischen "BEGIN " und "PRIVATE KEY" verlangte. Genau dieser Fall
/// ist der von Spec 0006 Abschnitt 5 zuerst genannte ("Private-Key-Blöcke").
#[test]
fn test_redactor_detects_plain_unencrypted_pkcs8_private_key() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(
        "vor dem Key\n\
         -----BEGIN PRIVATE KEY-----\n\
         MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEA\n\
         -----END PRIVATE KEY-----\n\
         nach dem Key",
    );

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(redacted.contains("[REDACTED]"));
    assert!(!redacted.contains("MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEA"));
    assert!(redacted.starts_with("vor dem Key"));
    assert!(redacted.ends_with("nach dem Key"));
}

/// Regressionstest: fehlt das schließende `-----END ... PRIVATE
/// KEY-----` (z. B. weil die Ausgabe an der 2-MB-Obergrenze oder durch
/// einen manuellen Abbruch mitten im Key-Block gekappt wurde), muss die
/// Redaction trotzdem ab dem erkannten `BEGIN`-Header greifen (Fail-safe,
/// Spec 0002 Abschnitt 1) statt den unvollständigen Key komplett
/// durchzulassen.
#[test]
fn test_redactor_redacts_truncated_private_key_without_matching_end() {
    let redactor = DefaultOutputRedactor::new();
    let input =
        output("vor dem Key\n-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKc");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(redacted.contains("[REDACTED]"));
    assert!(!redacted.contains("MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKc"));
    assert!(redacted.starts_with("vor dem Key"));
}

/// Regressionstest: JSON-artige Ausgaben (`docker inspect`, `kubectl get
/// -o json`, Service-Account-Dateien) haben den Schlüssel selbst in
/// Anführungszeichen — direkt gefolgt vom schließenden Quote, nicht vom
/// Trenner (`"password": "hunter2"` statt `password: "hunter2"`). Ohne ein
/// optionales Quote-Zeichen direkt nach dem Schlüsselwort brach das
/// Matching genau an dieser Stelle ab.
#[test]
fn test_redactor_detects_json_shaped_credential_fields() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(r#"{"password": "hunter2", "api_key":"sk-superduper"}"#);

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("hunter2"));
    assert!(!redacted.contains("sk-superduper"));
    assert!(redacted.contains("[REDACTED]"));
}

/// Regressionstest: `aws_secret_access_key = ...` (z. B. aus
/// `~/.aws/credentials`) wurde vom generischen `secret`-Muster nicht
/// erfasst — "secret" steckt zwar als Teilstring in
/// "aws_secret_access_key", aber direkt danach folgt "_access_key", kein
/// Trenner, wodurch das generische Muster dort nicht matcht.
#[test]
fn test_redactor_detects_aws_secret_access_key_line() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(
        "aws_access_key_id = AKIAABCDEFGHIJKLMNOP\naws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
    );

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"));
    assert!(redacted.contains("[REDACTED]"));
}

/// Regressionstest: das neuere, fein-granulare GitHub-Token-Format
/// (`github_pat_...`) ist deutlich länger als die klassischen Präfixe
/// (`ghp_`/`gho_`/...) und wurde vom bisherigen Muster nicht erfasst.
#[test]
fn test_redactor_detects_github_fine_grained_token() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(
        "GITHUB_TOKEN=github_pat_11ABCDEFG0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOP",
    );

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("11ABCDEFG0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOP"));
    assert!(redacted.contains("[REDACTED]"));
}

// --- Unix-Crypt-/Shadow-Passwort-Hashes (Diagnose-Bericht "unredigierte
// /etc/shadow-Hashes über den MCP-Pfad", 2026-09) ------------------------

/// Regressionstest für den Kernbefund: ein `/etc/shadow`-Zeile mit einem
/// SHA-512-Crypt-Hash (`$6$...`) wurde bislang komplett unredigiert
/// durchgelassen, weil keines der bisherigen Muster auf `keyword=wert`-
/// oder PEM-Block-Syntax verzichtbare Shadow-Zeilen passt. Prüft
/// zusätzlich, dass NUR der Hash-Teil verschwindet — Username und die
/// Aging-Felder (lastchg/min/max/warn/inactive/expire) bleiben lesbar,
/// wie von der Diagnose ausdrücklich gefordert ("kein Über-Redigieren").
#[test]
fn test_redactor_detects_shadow_line_sha512_crypt_hash_and_preserves_surrounding_fields() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(
        "root:$6$abcSaltSalt$AzWmGvzQTBbTLYo3lQfDMXf7v2fJDqVvOTBbTLYo3lQfDMXf7v2fJDqVvOTBbTLYo3lQfDMXf7v2:19000:0:99999:7:::",
    );

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted
        .contains("AzWmGvzQTBbTLYo3lQfDMXf7v2fJDqVvOTBbTLYo3lQfDMXf7v2fJDqVvOTBbTLYo3lQfDMXf7v2"));
    assert_eq!(redacted, "root:[REDACTED]:19000:0:99999:7:::");
}

/// Regressionstest: ein Crypt-Hash AUSSERHALB einer `/etc/shadow`-Zeile
/// (z. B. in einer Config oder einem Skript, das einen vorgegebenen
/// Passwort-Hash setzt) muss ebenso erkannt werden — das Muster ist nicht
/// an die Shadow-Zeilenstruktur gebunden, sondern an die Hash-Syntax
/// selbst.
#[test]
fn test_redactor_detects_crypt_hash_outside_shadow_line() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("useradd -p '$1$O3JMY.Tw$AdLnLjQx5jXF9MzYUlWO0' deploy");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("AdLnLjQx5jXF9MzYUlWO0"));
    assert!(redacted.contains("[REDACTED]"));
}

/// Deckt die übrigen in der Diagnose genannten Crypt-Tag-Familien ab:
/// SHA-256 mit explizitem `rounds=N`-Parameter, bcrypt (`$2b$`, fixe
/// Kostenstufe + kombinierter Salt/Hash-Block) und yescrypt (`$y$`,
/// variabler Parameter-Block).
#[test]
fn test_redactor_detects_sha256_bcrypt_and_yescrypt_hashes() {
    let redactor = DefaultOutputRedactor::new();

    let sha256 = output("$5$rounds=5000$saltsaltsalt$1234567890abcdefghijklmnopqrstuvwxyzABCDEFGH");
    let redacted_sha256 = stdout_text(&redactor.redact(&sha256));
    assert!(!redacted_sha256.contains("1234567890abcdefghijklmnopqrstuvwxyzABCDEFGH"));
    assert!(redacted_sha256.contains("[REDACTED]"));

    let bcrypt = output("$2b$12$eImiTXuWVxfM37uY4JANjQZ4Grv2mHewkQwmzCJk5tHqDNMV6ANyG");
    let redacted_bcrypt = stdout_text(&redactor.redact(&bcrypt));
    assert!(!redacted_bcrypt.contains("eImiTXuWVxfM37uY4JANjQZ4Grv2mHewkQwmzCJk5tHqDNMV6ANyG"));
    assert!(redacted_bcrypt.contains("[REDACTED]"));

    let yescrypt = output("$y$j9T$saltsaltsaltsalt$hashhashhashhashhashhashhashhash");
    let redacted_yescrypt = stdout_text(&redactor.redact(&yescrypt));
    assert!(!redacted_yescrypt.contains("hashhashhashhashhashhashhashhash"));
    assert!(redacted_yescrypt.contains("[REDACTED]"));
}

/// Falsch-Positiv-Check (Diagnose-Bericht, Verifikationsabschnitt): Git-
/// Commit-Hashes, UUIDs und kurze Hex-Strings enthalten kein
/// `$<id>$`-Muster und dürfen nicht redigiert werden. Eine normale
/// `/etc/passwd`-Zeile (`user:x:1000:...`, Hash liegt in `/etc/shadow`,
/// hier nur der Platzhalter `x`) darf ebenfalls unangetastet bleiben —
/// genau das von der Diagnose verlangte "kein Über-Redigieren".
#[test]
fn test_redactor_does_not_flag_commit_hashes_uuids_or_passwd_placeholder_lines() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(
        "a1b2c3d4e5f67890abcd1234ef567890abcd1234\n\
         550e8400-e29b-41d4-a716-446655440000\n\
         deadbeef\n\
         user:x:1000:1000:User:/home/user:/bin/bash",
    );

    let redacted = redactor.redact(&input);

    assert_eq!(redacted.stdout, input.stdout);
}

/// Regressionstest, `spec-reviewer`-Fund (Review dieses Schritts): Apache
/// `.htpasswd`-Dateien (`$apr1$...`, dasselbe Format wie `$1$`) sind
/// mindestens so ein alltägliches Ziel für "cat die Datei" wie
/// `/etc/shadow` — ursprünglich nicht abgedeckt.
#[test]
fn test_redactor_detects_apache_apr1_htpasswd_hash() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("admin:$apr1$salt1234$AbCdEfGhIjKlMnOpQrStUv");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("AbCdEfGhIjKlMnOpQrStUv"));
    assert_eq!(redacted, "admin:[REDACTED]");
}

/// Regressionstest, `spec-reviewer`-Fund (Review dieses Schritts): die
/// ursprüngliche Fassung des Musters verlangte fürs letzte Segment keine
/// Mindestlänge — `$1$2$3` (eine gewöhnliche `awk`/`sed`-
/// Positionsparameter-Referenz, `$1`/`$2`/`$3`, KEIN Hash) wäre
/// fälschlich als Crypt-Hash erkannt und ersetzt worden. Jeder real
/// unterstützte Algorithmus liefert mindestens 13 Hash-Zeichen; die neue
/// 10-Zeichen-Untergrenze fürs letzte Segment schließt diese
/// Falsch-Positiv-Klasse, ohne einen einzigen echten Hash zu verpassen
/// (s. die vorherigen Tests in diesem Abschnitt, die alle weiterhin
/// grün sind).
#[test]
fn test_redactor_does_not_flag_short_dollar_delimited_shell_syntax() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("awk '{print $1$2$3}'\nsed -e 's/(a)(b)/$1$2$3/'\necho $1$2");

    let redacted = redactor.redact(&input);

    assert_eq!(redacted.stdout, input.stdout);
}

/// Regressionstest, `spec-reviewer`-Fund (Review dieses Schritts): steht
/// das Crypt-Hash-Muster NACH einem anderen Muster in der Liste (z. B.
/// dem AWS-Access-Key-Muster `(AKIA|ASIA)[0-9A-Z]{16}`), kann dieses
/// andere Muster durch puren Zufall genau 20 Zeichen MITTEN in einem
/// langen Hash treffen (wenn dort zufällig `AKIA`/`ASIA` gefolgt von 16
/// Großbuchstaben/Ziffern steht) und dort `[REDACTED]` einfügen — das
/// zerstört die `$`-Struktur, auf die das Crypt-Muster angewiesen ist,
/// und der Rest des Hashes bliebe unredigiert stehen. Da das Crypt-Muster
/// jetzt bewusst als ERSTES angewendet wird (s. `built_in_patterns()`),
/// ist der komplette Hash schon ersetzt, bevor das AWS-Muster ihn
/// überhaupt sehen könnte.
#[test]
fn test_redactor_fully_redacts_hash_containing_an_accidental_aws_key_substring() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(
        "root:$6$saltsalt$AKIAABCDEFGHIJKLMNOP0123456789abcdefghijklmnopqrstuvwxyzSECRETTAIL:19000:...",
    );

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("AKIAABCDEFGHIJKLMNOP"));
    assert!(!redacted.contains("SECRETTAIL"));
    assert_eq!(redacted, "root:[REDACTED]:19000:...");
}

// --- DB-Connection-Strings & Provider-Tokens (Diagnose-Folge-Fix,
// 2026-09) --------------------------------------------------------------

/// Regressionstest: die häufigsten DB-Schemata aus der DevOps-/
/// Homelab-Zielgruppe (Postgres, MySQL/MariaDB, MongoDB inkl. `+srv`,
/// Redis inkl. TLS-`rediss`, AMQP inkl. TLS-`amqps`). Prüft für jedes
/// Schema, dass NUR das Passwort verschwindet — Schema, Nutzername, Host
/// und Datenbank bleiben exakt erhalten (kein Über-Redigieren, analog zum
/// Shadow-Hash-Fix).
#[test]
fn test_redactor_detects_db_connection_string_passwords_and_preserves_the_rest() {
    let redactor = DefaultOutputRedactor::new();
    let cases = [
        (
            "postgres://user:GEHEIM@host:5432/db",
            "postgres://user:[REDACTED]@host:5432/db",
        ),
        (
            "postgresql://admin:s3cr3t!@db.internal:5432/app",
            "postgresql://admin:[REDACTED]@db.internal:5432/app",
        ),
        (
            "mysql://root:hunter2@127.0.0.1:3306/mydb",
            "mysql://root:[REDACTED]@127.0.0.1:3306/mydb",
        ),
        (
            "mariadb://user:pw@host/db",
            "mariadb://user:[REDACTED]@host/db",
        ),
        (
            "mongodb://user:pass@cluster0.mongodb.net/db",
            "mongodb://user:[REDACTED]@cluster0.mongodb.net/db",
        ),
        (
            "mongodb+srv://user:pass@cluster0.mongodb.net/db",
            "mongodb+srv://user:[REDACTED]@cluster0.mongodb.net/db",
        ),
        (
            "redis://user:pass@host:6379",
            "redis://user:[REDACTED]@host:6379",
        ),
        (
            "rediss://user:pass@host:6380",
            "rediss://user:[REDACTED]@host:6380",
        ),
        (
            "amqp://guest:guest@localhost:5672/",
            "amqp://guest:[REDACTED]@localhost:5672/",
        ),
        (
            "amqps://user:pass@host:5671",
            "amqps://user:[REDACTED]@host:5671",
        ),
    ];

    for (input_str, expected) in cases {
        let redacted = stdout_text(&redactor.redact(&output(input_str)));
        assert_eq!(redacted, expected, "input was: {input_str}");
    }
}

/// Falsch-Positiv-Check (explizit gefordert): ein Schema+Host OHNE
/// Zugangsdaten (kein `user:pass@`) darf nicht redigiert werden — sonst
/// würden harmlose, credential-freie Verbindungsangaben (z. B. lokale
/// Entwicklungs-DBs ohne Auth) grundlos verstümmelt.
#[test]
fn test_redactor_does_not_flag_db_urls_without_credentials() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(
        "postgres://host:5432/db\n\
         mysql://host/db\n\
         redis://localhost:6379",
    );

    let redacted = redactor.redact(&input);

    assert_eq!(redacted.stdout, input.stdout);
}

/// Regressionstest, Reihenfolge-Lehre aus dem Shadow-Hash-Review: ein
/// DB-Passwort, das zufällig wie ein AWS-Access-Key aussieht (`AKIA` +
/// 16 Großbuchstaben/Ziffern), darf nicht nur teilweise redigiert werden
/// — das DB-Connection-String-Muster steht bewusst VOR dem AWS-Muster in
/// `built_in_patterns()`, ersetzt das komplette Passwort-Segment in einem
/// Zug, bevor das AWS-Muster überhaupt etwas davon sehen könnte.
#[test]
fn test_redactor_fully_redacts_db_password_containing_an_accidental_aws_key_substring() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("postgres://user:AKIAABCDEFGHIJKLMNOPsecretTail@host:5432/db");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("AKIAABCDEFGHIJKLMNOP"));
    assert!(!redacted.contains("secretTail"));
    assert_eq!(redacted, "postgres://user:[REDACTED]@host:5432/db");
}

/// Regressionstest: Provider-Tokens mit eindeutigem Präfix (Slack —
/// inkl. `xoxs-`/`xapp-`, Stripe — live UND test, s. Doc-Kommentar bei
/// `built_in_patterns()` zur bewussten Einbeziehung von
/// Test-Keys/publishable Keys —, Google-API, npm), analog zu den
/// bestehenden AWS-/GitHub-Mustern.
///
/// Spec-reviewer-Fund (Review dieses Schritts): die ursprüngliche Fassung
/// bettete jedes Token in einen `SCHLÜSSELWORT=wert`-Kontext ein (z. B.
/// `GOOGLE_API_KEY=AIza...`) — das bereits bestehende generische
/// `password|token|api_key|secret`-Muster redigiert `API_KEY=...` schon
/// OHNE die neuen Muster, der Test bewies für Slack/Google/npm also gar
/// nichts über die neuen Muster selbst (nur Stripe fiel vorher wirklich
/// durch, weil "SECRET_KEY=" zwar "secret" enthält, aber der Wert dahinter
/// bereits vom generischen Muster erfasst worden wäre — auch das beweist
/// nichts Neues). Die Tokens stehen jetzt NACKT (ohne jeden
/// Schlüsselwort-Kontext) in der Ausgabe — nur die neuen, präfixbasierten
/// Muster können sie erkennen.
#[test]
fn test_redactor_detects_slack_stripe_google_and_npm_tokens() {
    let redactor = DefaultOutputRedactor::new();

    // Fake-Test-Fixtures unten enthalten bewusst NIE das echte Zielformat
    // als zusammenhängendes Quelltext-Literal: GitHubs Push-Protection-
    // Secret-Scanner erkennt rein formatbasiert (Präfix + Länge), unabhängig
    // davon, dass diese Werte offensichtlich erfunden sind — ein
    // zusammenhängendes Literal im exakten Zielformat blockiert sonst jeden
    // Push dieses Commits. Jedes Literal trägt deshalb ein `~` mitten im
    // Format-relevanten Teil, das per `.replace('~', "")` erst zur
    // Laufzeit entfernt wird — dadurch matcht im Quelltext selbst nirgends
    // ein zusammenhängender Treffer des jeweiligen Formats, während der
    // tatsächliche Testwert zur Laufzeit exakt dem echten Format entspricht.
    let slack_bot_fake_token =
        "xoxb-123456~7890123-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx".replace('~', "");
    let slack_bot = output(&format!("found in build log: {slack_bot_fake_token}"));
    let redacted_slack_bot = stdout_text(&redactor.redact(&slack_bot));
    assert!(!redacted_slack_bot.contains(&slack_bot_fake_token));
    assert!(redacted_slack_bot.contains("[REDACTED]"));

    // Spec-reviewer-Fund: `xoxs-`/`xapp-` fehlten ursprünglich.
    let slack_app_fake_token =
        "xapp-1-A01AB~CDEF-1234567890123-AbCdEfGhIjKlMnOpQrStUvWxYz012345".replace('~', "");
    let slack_app = output(&slack_app_fake_token);
    let redacted_slack_app = stdout_text(&redactor.redact(&slack_app));
    assert!(!redacted_slack_app.contains(&slack_app_fake_token));
    assert!(redacted_slack_app.contains("[REDACTED]"));

    let stripe_live_fake_key = "sk_live_4eC~39HqLyjWDarjtT1zdp7dc1234567890".replace('~', "");
    let stripe_live = output(&format!("stripe key in CI output: {stripe_live_fake_key}"));
    let redacted_stripe_live = stdout_text(&redactor.redact(&stripe_live));
    assert!(!redacted_stripe_live.contains(&stripe_live_fake_key));
    assert!(redacted_stripe_live.contains("[REDACTED]"));

    let stripe_test_fake_key = "sk_test_4eC~39HqLyjWDarjtT1zdp7dc1234567890".replace('~', "");
    let stripe_test = output(&format!("stripe key in CI output: {stripe_test_fake_key}"));
    let redacted_stripe_test = stdout_text(&redactor.redact(&stripe_test));
    assert!(!redacted_stripe_test.contains(&stripe_test_fake_key));
    assert!(redacted_stripe_test.contains("[REDACTED]"));

    let google_fake_key = "AIzaSyD1234~567890abcdefghijklmnopqrstuv".replace('~', "");
    let google = output(&format!("client config uses {google_fake_key} for maps"));
    let redacted_google = stdout_text(&redactor.redact(&google));
    assert!(!redacted_google.contains(&google_fake_key));
    assert!(redacted_google.contains("[REDACTED]"));

    let npm_fake_token = "npm_1234~567890abcdefghijklmnopqrstuvwxyz".replace('~', "");
    let npm = output(&format!("registry auth line: {npm_fake_token}"));
    let redacted_npm = stdout_text(&redactor.redact(&npm));
    assert!(!redacted_npm.contains(&npm_fake_token));
    assert!(redacted_npm.contains("[REDACTED]"));
}

/// Falsch-Positiv-Check (explizit gefordert): zu kurze, dem echten Format
/// nur ähnliche Strings dürfen nicht als Provider-Token erkannt werden —
/// die Mindest-/Exaktlängen sind kein Zufall, sondern die eigentliche
/// Falsch-Positiv-Bremse dieser Muster.
#[test]
fn test_redactor_does_not_flag_too_short_provider_token_lookalikes() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(
        "xoxb-short\n\
         sk_live_short\n\
         AIzaTooShort\n\
         npm_short",
    );

    let redacted = redactor.redact(&input);

    assert_eq!(redacted.stdout, input.stdout);
}

/// Regressionstest, spec-reviewer-Fund (Review dieses Schritts): die
/// KANONISCHE Redis-URL-Form vor ACLs hat gar keinen Nutzernamen
/// (`redis://:passwort@host`) — genau diese Form steht in unzähligen
/// docker-compose-/Heroku-/Sidekiq-Configs. Die ursprüngliche Fassung
/// verlangte mindestens ein Zeichen für den Nutzernamen und verpasste
/// diesen extrem häufigen Fall komplett.
#[test]
fn test_redactor_detects_db_connection_string_with_empty_username() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(
        "REDIS_URL=redis://:sup3rs3cr3t@redis:6379/0\n\
         DATABASE_URL=postgres://:hunter2@db:5432/app",
    );

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("sup3rs3cr3t"));
    assert!(!redacted.contains("hunter2"));
    assert!(redacted.contains("REDIS_URL=redis://:[REDACTED]@redis:6379/0"));
    assert!(redacted.contains("DATABASE_URL=postgres://:[REDACTED]@db:5432/app"));
}

/// Regressionstest, spec-reviewer-Fund (Review dieses Schritts, echte
/// Regression, keine nur theoretische): die ursprüngliche
/// Passwort-Zeichenklasse (`[^@/\s]+`) war zu weit gefasst — bei
/// `redis://cache:6379,password=p@ssw0rd` (kein echtes DB-Passwort,
/// sondern ein komma-getrenntes `password=`-Feld DANACH) lief sie über
/// das `password=`-Feld hinweg bis zum NÄCHSTEN `@` (dem in `p@ssw0rd`)
/// und zerstörte damit den Anker, auf den das generische
/// `password=`-Muster weiter unten angewiesen ist — Ergebnis war
/// `redis://cache:[REDACTED]@ssw0rd`, mit `ssw0rd` im Klartext. Das
/// verletzt "never loosen an existing check" (CLAUDE.md): vor diesem
/// DB-URL-Fix wurde diese Zeile vom generischen Muster vollständig
/// redigiert. Die jetzt engere Zeichenklasse (schließt `, ; " ' =` aus)
/// lässt das DB-Muster hier gar nicht mehr greifen, sodass das
/// generische `password=`-Muster wieder ungestört zum Zug kommt.
#[test]
fn test_redactor_does_not_let_db_pattern_swallow_a_later_password_keyword_field() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("redis://cache:6379,password=p@ssw0rd");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("p@ssw0rd"));
    assert!(!redacted.contains("ssw0rd"));
    assert_eq!(redacted, "redis://cache:6379,[REDACTED]");
}

/// Regressionstest, spec-reviewer-Fund (Review dieses Schritts): eine
/// credential-freie DB-URL, gefolgt später in DERSELBEN Zeile von einem
/// unabhängigen `@` (z. B. einer E-Mail-Adresse in einem JSON-Einzeiler),
/// wurde von der ursprünglichen Zeichenklasse fälschlich bis zu diesem
/// `@` hin "redigiert" — obwohl gar kein `user:pass@`-Paar vorlag. Die im
/// Code dokumentierte Zusicherung "kein Match ohne echtes Credential-Paar"
/// stimmte nur für den einfachen Fall (URL allein auf eigener Zeile). Mit
/// der engeren Zeichenklasse bricht das Matching vor dem MongoDB in Text
/// eingebetteten Feldtrenner ab.
#[test]
fn test_redactor_does_not_over_redact_across_an_unrelated_at_sign_later_in_the_line() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(r#"{"redis":"redis://cache:6379","admin":"ops@example.com"}"#);

    let redacted = redactor.redact(&input);

    assert_eq!(redacted.stdout, input.stdout);
}

/// Regressionstest, spec-reviewer-Fund (Review dieses Schritts): das
/// DB-Connection-String-Schema kam ursprünglich nur kleingeschrieben vor
/// (`(?i)` fehlte) — `POSTGRES://`/`Mysql://` (z. B. aus manchen
/// Log-Formatierern oder groß geschriebenen Umgebungsvariablen-Werten)
/// blieben unredigiert.
#[test]
fn test_redactor_detects_db_connection_strings_with_uppercase_scheme() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("POSTGRES://user:hunter2@host/db\nMysql://user:pw@host/db");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("hunter2"));
    assert!(!redacted.contains(":pw@"));
    assert!(redacted.contains("POSTGRES://user:[REDACTED]@host/db"));
    assert!(redacted.contains("Mysql://user:[REDACTED]@host/db"));
}

/// Regressionstest, spec-reviewer-Fund (Review dieses Schritts): RFC
/// 3986 erlaubt in der Userinfo-Komponente einer URL Sonderzeichen wie
/// `~ ! ' ( ) * , ;`, die von der ursprünglichen Nutzername-Zeichenklasse
/// (`[A-Za-z0-9_.%+-]`) nicht abgedeckt waren — ein Nutzername wie
/// `user~name` ließ das Muster komplett ins Leere laufen (kein Treffer,
/// Passwort blieb im Klartext).
#[test]
fn test_redactor_detects_db_connection_string_with_rfc3986_special_username_chars() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("postgres://user~name:hunter2@host/db\npostgres://user!name:pw@host/db");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("hunter2"));
    assert!(!redacted.contains(":pw@"));
    assert!(redacted.contains("postgres://user~name:[REDACTED]@host/db"));
    assert!(redacted.contains("postgres://user!name:[REDACTED]@host/db"));
}
