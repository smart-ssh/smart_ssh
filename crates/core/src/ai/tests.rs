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
        max_tokens_hint: None,
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
/// `~ ! ' ( ) *`, die von der ursprünglichen Nutzername-Zeichenklasse
/// (`[A-Za-z0-9_.%+-]`) nicht abgedeckt waren — ein Nutzername wie
/// `user~name` ließ das Muster komplett ins Leere laufen (kein Treffer,
/// Passwort blieb im Klartext).
///
/// Deckt NICHT `,`/`;`/`"` ab — diese drei sind weiterhin bewusst
/// ausgeschlossen (s. Doc-Kommentar beim Muster in `built_in_patterns()`,
/// zweite Review-Runde): sie werden als Feldtrenner gebraucht, um zwei
/// andere, real aufgetretene Regressionen zu schließen. Ein Nutzername
/// mit rohem Komma/Semikolon/Anführungszeichen matcht deshalb nicht —
/// bekannter, offengelegter Restfall, kein Testversehen.
#[test]
fn test_redactor_detects_db_connection_string_with_rfc3986_special_username_chars() {
    let redactor = DefaultOutputRedactor::new();
    let input = output(
        "postgres://user~name:hunter2@host/db\n\
         postgres://user!name:pw@host/db\n\
         postgres://user'name:pw2@host/db",
    );

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("hunter2"));
    assert!(!redacted.contains(":pw@"));
    assert!(!redacted.contains(":pw2@"));
    assert!(redacted.contains("postgres://user~name:[REDACTED]@host/db"));
    assert!(redacted.contains("postgres://user!name:[REDACTED]@host/db"));
    assert!(redacted.contains("postgres://user'name:[REDACTED]@host/db"));
}

/// Regressionstest, spec-reviewer-Fund (ZWEITE Review-Runde): die erste
/// Fassung des Feldtrenner-Ausschlusses (`, ; " ' =`) schloss `=` mit ein
/// — trug aber nichts zur Behebung der gemeldeten Funde bei und verpasste
/// dadurch neu ein extrem alltägliches Passwort-Format: Base64-generierte
/// Secrets enden wegen des Padding-Zeichens sehr häufig auf `=` (z. B.
/// `openssl rand -base64 24`, von Kubernetes/Terraform generierte
/// DB-Passwörter). `=` ist jetzt wieder im Passwort-Zeichensatz erlaubt.
#[test]
fn test_redactor_detects_db_connection_string_with_base64_padded_password() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("postgres://user:c2VjcmV0Cg==@host/db");

    let redacted = stdout_text(&redactor.redact(&input));

    assert!(!redacted.contains("c2VjcmV0Cg=="));
    assert_eq!(redacted, "postgres://user:[REDACTED]@host/db");
}

/// Dokumentiert den bewusst akzeptierten Restfall (spec-reviewer
/// bestätigt, zweite Review-Runde): ein Passwort mit einem ROHEN
/// (nicht Prozent-kodierten) Komma/Semikolon/Anführungszeichen matcht
/// nicht — dieselben drei Zeichen müssen als Feldtrenner ausgeschlossen
/// bleiben, um `test_redactor_does_not_let_db_pattern_swallow_a_later_
/// password_keyword_field` und `test_redactor_does_not_over_redact_
/// across_an_unrelated_at_sign_later_in_the_line` nicht wieder zu öffnen.
/// Kein Falsch-Positiv-Test — bewusst dokumentiertes Verhalten, kein
/// Fehler.
#[test]
fn test_redactor_does_not_detect_db_password_containing_a_raw_comma_or_semicolon() {
    let redactor = DefaultOutputRedactor::new();
    let input = output("mongodb://admin:abc,def@mongo:27017/admin");

    let redacted = redactor.redact(&input);

    assert_eq!(redacted.stdout, input.stdout);
}

// --- Spec 0068, Teil 1: nackte Provider-Keys und Auth-Header ------------
//
// Formate geprüft gegen die gitleaks-Regeln (gitleaks/config/gitleaks.toml:
// anthropic-api-key, anthropic-admin-api-key, openai-api-key, gitlab-pat,
// gitlab-pat-routable, huggingface-access-token), gitleaks-Issue #2158
// (Anthropic-OAuth `sk-ant-oat01-`/`sk-ant-ort01-`) und die OpenRouter-Doku
// (`sk-or-v1-`). Die Beispiel-Keys unten sind formatgerecht, aber erfunden.

const ANTHROPIC_KEY: &str = concat!("sk-ant-api03-Abcd", "efghijklmnopqrstuvwxyz0123456789_-ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz012345AA");
const OPENAI_PROJECT_KEY: &str = concat!("sk-proj-Abcd", "efghijklmnopqrstuvwxyz0123456789_-ABCDEFGHIJKLMNOPQRSTUVT3BlbkFJabcdefghijklmnopqrstuvwxyz0123456789_-ABCDEFGHIJKLMNOPQRSTU");
const OPENAI_LEGACY_KEY: &str = concat!("sk-Abcd", "efghij0123456789T3BlbkFJabcdefghij0123456789");
const OPENROUTER_KEY: &str = concat!(
    "sk-or-v1-0123",
    "456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
);
const GITLAB_PAT: &str = concat!("glpat-Abcd", "efghij0123456789");
const GITLAB_PAT_ROUTABLE: &str =
    concat!("glpat-Abcd", "efghij0123456789_-abcdefghijk.01.0abcdefg");
const HF_TOKEN: &str = concat!("hf_Abcd", "efghijklmnopqrstuvwxyzABCDEFGH");

fn assert_fully_redacted(input: &str, secret: &str) {
    let redacted = DefaultOutputRedactor::new().redact_text(input);
    assert!(
        redacted.contains("[REDACTED]"),
        "nichts redigiert: {redacted}"
    );
    // Kein Teilstück des Geheimnisses (> 8 Zeichen) darf übrig bleiben —
    // fängt auch ein von einem anderen Muster zerteiltes Geheimnis.
    let chars: Vec<char> = secret.chars().collect();
    for window in chars.windows(9) {
        let piece: String = window.iter().collect();
        assert!(
            !redacted.contains(&piece),
            "Rest „{piece}“ von {secret} steht noch im Klartext: {redacted}"
        );
    }
}

#[test]
fn test_bare_provider_keys_without_keyword_are_redacted() {
    for key in [
        ANTHROPIC_KEY,
        concat!("sk-ant-admin01-Abcd", "efghijklmnopqrstuvwxyz0123456789_-ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz012345AA"),
        concat!("sk-ant-oat01-Abcd", "efghijklmnopqrstuvwxyz0123456789_-ABCDEFGH"),
        OPENAI_PROJECT_KEY,
        concat!("sk-svcacct-Abcd", "efghijklmnopqrstuvwxyz0123456789_-ABCDEFGHT3BlbkFJabcdefghij"),
        concat!("sk-admin-Abcd", "efghijklmnopqrstuvwxyz0123456789_-ABCDEFGHT3BlbkFJabcdefghij"),
        OPENAI_LEGACY_KEY,
        OPENROUTER_KEY,
        GITLAB_PAT,
        GITLAB_PAT_ROUTABLE,
        HF_TOKEN,
    ] {
        // Kein Schlüsselwort (password=/token=/api_key=) davor — genau der
        // bisher unerkannte Fall.
        assert_fully_redacted(&format!("using {key} for requests"), key);
        assert_fully_redacted(&format!("{{\"key\": \"{key}\"}}"), key);
    }
}

#[test]
fn test_provider_key_containing_accidental_aws_or_google_substring_is_not_split() {
    // Reihenfolge-Lehre (Shadow-Fix): `AKIA…`/`AIza…` laufen später; ein
    // zufälliger Treffer mitten im Key darf keinen Rest übrig lassen.
    let key = concat!(
        "sk-ant-api03-xxxx",
        "AKIAABCDEFGHIJKLMNOPyyyyAIzaSyA0123456789abcdefghijklmnopqrstuTAILSECRET0123456789AA"
    );
    assert_fully_redacted(&format!("key {key} end"), key);
}

#[test]
fn test_auth_headers_are_redacted() {
    assert_fully_redacted(
        "x-api-key: someopaquekeyvalue123456",
        "someopaquekeyvalue123456",
    );
    assert_fully_redacted(
        "X-Api-Key: \"someopaquekeyvalue123456\"",
        "someopaquekeyvalue123456",
    );
    assert_fully_redacted(
        "curl -H 'x-api-key: someopaquekeyvalue123456' https://api.example.com",
        "someopaquekeyvalue123456",
    );
    assert_fully_redacted(
        "Authorization: Basic ZGVwbG95OnMzY3JldC1wYXNz",
        "ZGVwbG95OnMzY3JldC1wYXNz",
    );
    assert_fully_redacted(
        "Proxy-Authorization: basic ZGVwbG95OnMzY3JldC1wYXNz",
        "ZGVwbG95OnMzY3JldC1wYXNz",
    );
}

#[test]
fn test_generic_url_credentials_redact_only_the_password() {
    let redactor = DefaultOutputRedactor::new();
    assert_eq!(
        redactor.redact_text("remote: https://deploy:s3cretPass@git.example.com/repo.git"),
        "remote: https://deploy:[REDACTED]@git.example.com/repo.git"
    );
    assert_eq!(
        redactor.redact_text("ftp://backup:hunter2hunter2@files.example.com/"),
        "ftp://backup:[REDACTED]@files.example.com/"
    );
    // Bestehendes DB-Muster unverändert (kein doppeltes/zerstörtes Ergebnis).
    assert_eq!(
        redactor.redact_text("postgres://app:pw123456@db:5432/app"),
        "postgres://app:[REDACTED]@db:5432/app"
    );
}

#[test]
fn test_new_patterns_do_not_flag_harmless_text() {
    let redactor = DefaultOutputRedactor::new();
    for harmless in [
        "Keys start with sk- and are secret.",
        "use the sk-ant- prefix check",
        "commit 3aef6ce0f1b2c3d4e5f60718293a4b5c6d7e8f90",
        "id 550e8400-e29b-41d4-a716-446655440000",
        "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
        "Basic authentication is configured for the admin area",
        concat!("from huggingface_hub import hf_hub_", "download"),
        "see https://example.com:8443/path and http://[::1]:8080/",
        "ssh://git@github.com:22/org/repo.git",
        "The password is required for login.",
        "Enter the password below and press enter",
        "api-key rotation is documented in the wiki",
        "10:30:00:12:45 elapsed",
        "use gsk_ prefix and xai- prefix checks",
    ] {
        assert_eq!(redactor.redact_text(harmless), harmless, "fälschlich redigiert");
    }
}

/// spec-reviewer-Fund (Spec 0068, ERHÖHT): die generische URL-Regel darf
/// über einen Port nicht in den Query-String greifen und so den Anker des
/// `password=`-/`token=`-Musters zerstören (vorher blieb `ssw0rd123` im
/// Klartext).
#[test]
fn test_url_rule_does_not_split_query_string_password() {
    assert_fully_redacted("https://host:8443?password=p@ssw0rd123", "ssw0rd123");
    assert_fully_redacted("http://localhost:8080?token=abc@defsecret", "defsecret");
    assert_fully_redacted("https://h:8443#password=p@ssw0rd123", "ssw0rd123");
}

#[test]
fn test_more_provider_keys_and_headers_are_redacted() {
    for key in [
        concat!("glrt-Abcd", "efghijklmnopqrst0123"),
        concat!("gldt-Abcd", "efghijklmnopqrst0123"),
        concat!("gsk_Abcd", "efghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMN"),
        concat!(
            "xai-Abcd",
            "efghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqr"
        ),
    ] {
        assert_fully_redacted(&format!("using {key} for requests"), key);
    }
    assert_fully_redacted(
        "api-key: someopaquekeyvalue123456",
        "someopaquekeyvalue123456",
    );
    assert_fully_redacted(
        "x-goog-api-key: someopaquekeyvalue123456",
        "someopaquekeyvalue123456",
    );
    assert_fully_redacted(
        "Authorization: Token someopaquekeyvalue123456",
        "someopaquekeyvalue123456",
    );
}

/// Die Dateiformate aus Spec 0068 Teil 2 — nach einem bestätigten Lesen
/// soll ihr Geheimnis trotzdem nicht im Klartext an die KI gehen.
#[test]
fn test_secret_file_formats_are_redacted() {
    assert_fully_redacted(
        r#"{"auths": {"registry.example.com": {"auth": "ZGVwbG95OnMzY3JldC1wYXNzd29yZA=="}}}"#,
        "ZGVwbG95OnMzY3JldC1wYXNzd29yZA",
    );
    assert_fully_redacted(
        "    client-key-data: LS0tLS1CRUdJTiBSU0EgUFJJVkFURSBLRVktLS0tLQo=",
        "LS0tLS1CRUdJTiBSU0EgUFJJVkFURSBLRVktLS0tLQo",
    );
    assert_fully_redacted(
        "machine api.example.com login deploy password s3cr3tNetrcPw",
        "s3cr3tNetrcPw",
    );
    assert_fully_redacted(
        "machine api.example.com\n  login deploy\n  password s3cr3tNetrcPw\n",
        "s3cr3tNetrcPw",
    );
    assert_fully_redacted(
        "db.example.com:5432:app:appuser:s3cr3tPgpassPw",
        "s3cr3tPgpassPw",
    );
    assert_fully_redacted("*:*:*:postgres:s3cr3tPgpassPw", "s3cr3tPgpassPw");
}

/// Zweite Review-Runde (Spec 0068, ERHÖHT): die neuen Muster dürfen
/// älteren Mustern keinen Anker wegnehmen, und die URL-Regel darf nicht
/// enger sein als in der ersten Fassung.
#[test]
fn test_new_patterns_never_leave_plaintext_that_older_patterns_redacted() {
    assert_fully_redacted(
        r#"Authorization: Token token="abc123secretvalue""#,
        "abc123secretvalue",
    );
    assert_fully_redacted(
        "api-key: Bearer abcdefghijklmnopqrstuvwxyz0123",
        "abcdefghijklmnopqrstuvwxyz0123",
    );
    assert_fully_redacted("login admin password = hunter2hunter2", "hunter2hunter2");
    assert_fully_redacted(
        "machine h login u password : hunter2hunter2",
        "hunter2hunter2",
    );
    assert_fully_redacted(
        "smtp://mailer:S3cr#tPassw0rd@mail.example.com",
        "S3cr#tPassw0rd",
    );
    assert_fully_redacted(
        "https://bob:pa?ssw0rdSecret@example.com/",
        "pa?ssw0rdSecret",
    );
    assert_fully_redacted("ftp://user:secret#1234567@host", "secret#1234567");
}

#[test]
fn test_pgpass_and_netrc_patterns_leave_addresses_and_prose_alone() {
    let redactor = DefaultOutputRedactor::new();
    for harmless in [
        "00:15:5d:01:02:03",
        "aa:bb:cc:dd:ee:ff",
        "2001:4860:4860:0:0:0:0:8888",
        "Login failed password incorrect for user",
    ] {
        assert_eq!(
            redactor.redact_text(harmless),
            harmless,
            "fälschlich redigiert"
        );
    }
}

/// Dritte Review-Runde (Spec 0068, ERHÖHT): ein Teiltreffer mitten in einem
/// längeren Wert darf keinen Rest im Klartext lassen, und die Header-Muster
/// der ersten Fassung wirken weiter an ihrer ursprünglichen Stelle.
#[test]
fn test_partial_matches_inside_values_leave_no_plaintext_tail() {
    assert_fully_redacted(
        "Authorization: Basic abcAKIAABCDEFGHIJKLMNOPxyzSECRETTAIL+/==",
        "xyzSECRETTAIL",
    );
    assert_fully_redacted(
        "Authorization: Basic sk_live_ABCDEFGHIJKLMNOPQRSTUV_SECRETTAIL",
        "_SECRETTAIL",
    );
    assert_fully_redacted(
        concat!(
            "Authorization: Bearer xyzsk-proj-ABCD",
            "EFGHIJKLMNOPQRSTUVWX.SECRETTAIL"
        ),
        "SECRETTAIL",
    );
    assert_fully_redacted(r#"x-api-key: "SECRETHEAD token=abc""#, "SECRETHEAD");
}

#[test]
fn test_netrc_default_branch_needs_login() {
    let redactor = DefaultOutputRedactor::new();
    let prose = "The default password is admin123 until changed";
    assert_eq!(redactor.redact_text(prose), prose);
    assert_fully_redacted(
        "default login anonymous password s3cr3tNetrcPw",
        "s3cr3tNetrcPw",
    );
}

/// Vierte Review-Runde (Spec 0068, ERHÖHT).
#[test]
fn test_fourth_review_round_redaction_findings() {
    assert_fully_redacted(
        "x-api-key: Bearer SECRETVALUE1234567890",
        "SECRETVALUE1234567890",
    );
    assert_fully_redacted(
        "X-API-KEY: bearer abcdefghijklmnopqrstuvwxyz",
        "abcdefghijklmnopqrstuvwxyz",
    );
    assert_fully_redacted("x-api-key: Bearer SHORTSECRET", "SHORTSECRET");
    assert_fully_redacted(
        r#"Authorization: Basic cred="SEC RET VALUE""#,
        "SEC RET VALUE",
    );
    assert_fully_redacted(
        "Authorization: Basic Bearer abcdefghijklmnopqrstuvwx",
        "abcdefghijklmnopqrstuvwx",
    );
    assert_fully_redacted("default password s3cr3tNetrcPw", "s3cr3tNetrcPw");
    assert_fully_redacted("  default password s3cr3tNetrcPw", "s3cr3tNetrcPw");
}

/// Der Schlussdurchgang lässt Schlüsselnamen links vom Platzhalter lesbar.
#[test]
fn test_placeholder_absorption_keeps_variable_names() {
    let redactor = DefaultOutputRedactor::new();
    let out = redactor.redact_text("GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789");
    assert!(out.starts_with("GITHUB_"), "{out}");
    assert!(!out.contains("abcdefghijklmnop"), "{out}");
}

// --- Spec 0078: `@` in den Zugangsdaten einer URL -----------------------
//
// Alle Fälle prüfen die EXAKTE Ausgabe (`assert_eq`), nicht nur
// "irgendwo steht [REDACTED]": `assert_fully_redacted` oben betrachtet
// Fenster aus 9 Zeichen und prüft bei kürzeren Geheimnissen (`pw123`,
// `p@ss`) gar nichts. Die erwarteten Zeichenketten sind gegen den
// echten Redactor gemessen (Spec 0078, §6).

/// Spec 0078, §6.1 (T-1, T-2a bis T-2f): enthält das Passwort einer
/// Verbindungs-URL ein unkodiertes `@`, wird es bis zum LETZTEN `@` vor
/// dem Host redigiert, nicht nur bis zum ersten. Deckt die Schemata ab,
/// die auch das DB-Muster kennt, plus die generischen (`https`, `ftp`).
#[test]
fn test_redactor_redacts_a_url_password_containing_an_unencoded_at_sign() {
    let redactor = DefaultOutputRedactor::new();

    for (input, expected) in [
        (
            "postgres://app:Xy9@kLm2@db.internal/prod",
            "postgres://app:[REDACTED]@db.internal/prod",
        ),
        (
            "mysql://root:a@b@c@db:3306/x",
            "mysql://root:[REDACTED]@db:3306/x",
        ),
        (
            "mongodb+srv://u:p@ss@cluster0.example.net/db",
            "mongodb+srv://u:[REDACTED]@cluster0.example.net/db",
        ),
        (
            "amqps://guest:g@st@mq:5671",
            "amqps://guest:[REDACTED]@mq:5671",
        ),
        (
            "redis://:p@ss@cache:6379/0",
            "redis://:[REDACTED]@cache:6379/0",
        ),
        (
            "https://deploy:t0k@n@git.example.com/repo.git",
            "https://deploy:[REDACTED]@git.example.com/repo.git",
        ),
        (
            "ftp://anon:mail@example.com@ftp.example.org/pub",
            "ftp://anon:[REDACTED]@ftp.example.org/pub",
        ),
    ] {
        assert_eq!(redactor.redact_text(input), expected, "Eingabe: {input}");
    }
}

/// Spec 0078, §6.1 (T-3, T-4): derselbe Fall mit einem
/// Umgebungsvariablen-Präfix davor und mit zwei URLs in einer Zeile —
/// beide werden vollständig redigiert, und die zweite URL beginnt ein
/// eigenes Passwort-Segment.
#[test]
fn test_redactor_redacts_at_passwords_with_a_prefix_and_in_repeated_urls() {
    let redactor = DefaultOutputRedactor::new();

    assert_eq!(
        redactor.redact_text("DATABASE_URL=postgres://app:Xy9@kLm2@db.internal/prod"),
        "DATABASE_URL=postgres://app:[REDACTED]@db.internal/prod"
    );
    assert_eq!(
        redactor.redact_text("https://u:a@b@host:8443 and https://v:c@d@other"),
        "https://u:[REDACTED]@host:8443 and https://v:[REDACTED]@other"
    );
}

/// Spec 0078, §6.1 (T-5a bis T-5c): enthält der BENUTZERNAME ein `@`
/// (bei mehreren gehosteten Datenbanken die vorgeschriebene Schreibweise
/// `benutzer@mandant`), blieb das Passwort bisher vollständig sichtbar,
/// weil alle bestehenden URL-Muster `@` aus der Benutzerklasse
/// ausschließen. Der Benutzername bleibt lesbar, wie bei allen anderen
/// URL-Mustern auch.
#[test]
fn test_redactor_redacts_the_password_when_the_url_user_contains_an_at_sign() {
    let redactor = DefaultOutputRedactor::new();

    for (input, expected) in [
        (
            "postgres://svc@tenant:pw123@db/x",
            "postgres://svc@tenant:[REDACTED]@db/x",
        ),
        (
            "https://user@corp:pw123@host/x",
            "https://user@corp:[REDACTED]@host/x",
        ),
        (
            "https://user@corp:p@ss@host/x",
            "https://user@corp:[REDACTED]@host/x",
        ),
    ] {
        assert_eq!(redactor.redact_text(input), expected, "Eingabe: {input}");
    }
}

/// Spec 0078, §6.1 (T-6, T-7a, T-7b) und §6.2 (T-A12, Positionsbeleg für
/// die Query-String-Regel): ein Passwort-Parameter mit `@` im
/// Query-String einer URL ohne Pfad. Das DB-Muster schließt `?` nicht aus
/// und las bisher `6379?password=p` als Passwort — damit war der Anker
/// des Schlüsselwort-Musters zerstört und `ssw0rd` blieb im Klartext.
/// Steht die Query-String-Regel HINTER dem DB-Muster, scheitert dieser
/// Test.
#[test]
fn test_redactor_redacts_a_password_query_parameter_containing_an_at_sign() {
    let redactor = DefaultOutputRedactor::new();

    for (input, expected) in [
        (
            "redis://cache:6379?password=p@ssw0rd",
            "redis://cache:6379?[REDACTED]",
        ),
        (
            "redis://cache:6379?password='p@ss w0rd'",
            "redis://cache:6379?[REDACTED]",
        ),
        (
            "redis://cache:6379?password='p@ss&w0rd'&db=1",
            "redis://cache:6379?[REDACTED]",
        ),
    ] {
        assert_eq!(redactor.redact_text(input), expected, "Eingabe: {input}");
    }
}

/// Spec 0078, §6.2 (T-A2, T-A3): Gegenprobe zu den beiden neuen Regeln —
/// `?` und `/` bleiben Stoppzeichen. Ohne sie liefe die neue URL-Regel
/// über den Query-String hinweg und schluckte den Host samt allem, was
/// bis zum nächsten `@` folgt.
#[test]
fn test_redactor_does_not_run_across_a_query_string_when_redacting_an_at_password() {
    let redactor = DefaultOutputRedactor::new();

    for (input, expected) in [
        (
            "https://u:p@host?next=a@b",
            "https://u:[REDACTED]@host?next=a@b",
        ),
        (
            "https://u:p@host/path?x=a@b.com",
            "https://u:[REDACTED]@host/path?x=a@b.com",
        ),
        (
            "postgres://app:pw@db?sslmode=require&user=x@y",
            "postgres://app:[REDACTED]@db?sslmode=require&user=x@y",
        ),
    ] {
        assert_eq!(redactor.redact_text(input), expected, "Eingabe: {input}");
    }
}

/// Spec 0078, §6.2 (T-A4, T-A5, T-A6): unabhängige `@` in derselben
/// Zeile (E-Mail-Adressen, `git@host`-Kurzform, komma-getrennte
/// MongoDB-Hostliste) bleiben stehen. Ohne diese Gegenprobe könnte die
/// neue Regel unbemerkt halbe Zeilen schwärzen.
#[test]
fn test_redactor_leaves_unrelated_at_signs_and_credential_free_urls_alone() {
    let redactor = DefaultOutputRedactor::new();

    assert_eq!(
        redactor.redact_text("postgres://app:pw@db1/x admin@example.com"),
        "postgres://app:[REDACTED]@db1/x admin@example.com"
    );
    assert_eq!(
        redactor.redact_text("mongodb://u:p@h1:27017,h2@x:27017/db"),
        "mongodb://u:[REDACTED]@h1:27017,h2@x:27017/db"
    );

    for unchanged in [
        "see https://example.com/@user and mailto:a@b",
        "ssh://git@github.com:22/x",
        "git@github.com:org/repo.git and a@b",
        "https://example.com:8080/path a@b",
        r#"{"redis":"redis://cache:6379","admin":"ops@example.com"}"#,
        // Leerer Wert: weder die Query-String-Regel (sie verlangt ein
        // `@` im Wert) noch das Schlüsselwort-Muster greifen hier — es
        // gibt nichts zu redigieren.
        "https://x.com/?q=password&token=",
    ] {
        assert_eq!(redactor.redact_text(unchanged), unchanged);
    }
}

/// Spec 0078, §6.2 (T-A7, T-A8, T-A9): was heute schon vollständig
/// redigiert wird, wird es weiterhin — die prozentkodierte Form, das
/// komma-getrennte `password=`-Feld aus der DB-Muster-Runde und
/// Passwort-Parameter hinter einem Pfad.
#[test]
fn test_redactor_still_fully_redacts_the_cases_that_were_already_closed() {
    let redactor = DefaultOutputRedactor::new();

    for (input, expected) in [
        (
            "postgres://app:Xy9%40kLm2@db.internal/prod",
            "postgres://app:[REDACTED]@db.internal/prod",
        ),
        (
            "redis://cache:6379,password=p@ssw0rd",
            "redis://cache:6379,[REDACTED]",
        ),
        (
            "redis://cache:6379/0?password=p@ssw0rd&db=1",
            "redis://cache:6379/0?[REDACTED]",
        ),
        (
            "postgres://db:5432/x?sslmode=require&password=p@ss;w",
            "postgres://db:5432/x?sslmode=require&[REDACTED]",
        ),
        (
            "https://api.example.com/v1?token=abc@def&x=1",
            "https://api.example.com/v1?[REDACTED]",
        ),
    ] {
        assert_eq!(redactor.redact_text(input), expected, "Eingabe: {input}");
    }
}

/// Spec 0078, §6.2 (T-A10): eine URL mit 10 000 `@` im Passwort. Die
/// `regex`-Crate kennt kein katastrophales Rückverfolgen, ein Tausch der
/// Bibliothek könnte das ändern — deshalb der Test mit großzügiger
/// Zeitgrenze in einem eigenen Thread, damit er im Fehlerfall scheitert
/// statt zu hängen.
#[test]
fn test_redactor_handles_a_pathological_number_of_at_signs_in_time() {
    let input = format!("https://u:{}host/x", "a@".repeat(10_000));

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(DefaultOutputRedactor::new().redact_text(&input));
    });

    let redacted = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("die Redaction muss in unter 10 s fertig sein");
    assert_eq!(redacted, "https://u:[REDACTED]@host/x");
}

/// Spec 0078, §6.2 (T-A11, Positionsbeleg für die neue URL-Regel): sie
/// läuft als LETZTE, damit sie dem Schlüsselwort-Muster nicht den Anker
/// nimmt. Stünde sie davor, griffe sie bis zu dem `@` in `ab@cdSecret`
/// und ließe `cdSecret` im Klartext stehen. Die Varianten mit `|` und
/// `'` belegen die Position; die `&`-Variante fängt schon die
/// Query-String-Regel ab und bleibt als Gegenprobe.
#[test]
fn test_redactor_does_not_let_the_at_url_rule_swallow_a_later_password_keyword() {
    let redactor = DefaultOutputRedactor::new();

    for (input, expected) in [
        (
            "https://u:x@h:1|password=ab@cdSecret",
            "https://u:[REDACTED]@h:1|[REDACTED]",
        ),
        (
            "https://u:x@h:1'password=ab@cdSecret",
            "https://u:[REDACTED]@h:1'[REDACTED]",
        ),
        (
            "https://u:x@h:1&password=ab@cdSecret",
            "https://u:[REDACTED]@h:1&[REDACTED]",
        ),
    ] {
        let redacted = redactor.redact_text(input);
        assert!(!redacted.contains("cdSecret"), "Eingabe: {input}");
        assert_eq!(redacted, expected, "Eingabe: {input}");
    }
}

/// Spec 0078, §6.2 (T-A14): Werte in Anführungszeichen bleiben
/// vollständig redigiert. Wächter, kein Gegenbeweis — diese drei Fälle
/// sind auch ohne die Quote-Alternativen der Query-String-Regel grün,
/// weil deren freie Form an `'`/`"` scheitert und dann wie bisher das
/// Schlüsselwort-Muster greift (spec-reviewer-Fund, erste Review-Runde:
/// die Begründung in Spec 0078 §3.1 trifft so nicht zu). Der echte
/// Gegenbeweis für die Quote-Alternativen steht in
/// `…_redacts_a_password_query_parameter_containing_an_at_sign`.
#[test]
fn test_redactor_keeps_quoted_query_parameter_values_fully_redacted() {
    let redactor = DefaultOutputRedactor::new();

    assert_eq!(
        redactor.redact_text("https://x/?password='top secret 123'"),
        "https://x/?[REDACTED]"
    );
    assert_eq!(
        redactor.redact_text(r#"https://x/?password="top secret 123""#),
        "https://x/?[REDACTED]"
    );
    // Unbeendetes Anführungszeichen: heutiges Verhalten des
    // Schlüsselwort-Musters, nicht Gegenstand von Spec 0078 — hier
    // festgehalten, damit die neue Regel es nicht unbemerkt verändert.
    assert_eq!(
        redactor.redact_text("https://x/?password='unterminated p@ss"),
        "https://x/?[REDACTED] p@ss"
    );
}

/// Spec 0078, §6.3 (T-R1): bekannter Restfall, Spec 0078 §5 — ein
/// Passwort, das `@` UND eines der Zeichen `/ ? #` enthält, wird weiter
/// nur teilweise redigiert. Kein Falsch-Positiv-Test, sondern bewusst
/// dokumentiertes Verhalten: `?` und `#` müssen Stoppzeichen bleiben
/// (s. `test_redactor_does_not_run_across_a_query_string_…`).
#[test]
fn test_redactor_known_remaining_case_password_with_at_sign_and_question_mark() {
    let redactor = DefaultOutputRedactor::new();

    assert_eq!(
        redactor.redact_text("postgres://app:a@b?c@db/x"),
        "postgres://app:[REDACTED]@b?c@db/x"
    );
}

/// Regressionstest, spec-reviewer-Fund (Spec 0078, erste Review-Runde,
/// ERHÖHT — echte Regression, keine nur theoretische): die
/// Query-String-Regel steht VOR den Private-Key-Mustern. Ihre freie
/// Wertklasse erlaubt `-`, stoppt aber an Leerraum — sie schnitt damit
/// den mehrteiligen Anker `-----BEGIN … PRIVATE KEY-----` genau in der
/// Mitte durch (`?secret=-----BEGIN` wurde ersetzt). Danach griff weder
/// das PEM-Muster noch sein Fail-safe-Rückfallmuster, und der komplette
/// Schlüsselkörper stand im Klartext — dort, wo er VOR Spec 0078
/// vollständig redigiert wurde. Das verletzt „nie weniger redigieren"
/// (CLAUDE.md, Spec 0078 §2).
///
/// Behoben, indem die freie Wertklasse der Regel mindestens ein `@`
/// verlangt: die Regel existiert allein, damit das DB-Muster den
/// Parameter nicht als Passwort lesen kann, und das DB-Muster kann nur
/// über einen Wert MIT `@` hinweglaufen. Ein Wert ohne `@` braucht sie
/// nicht — dort redigiert wie bisher das Schlüsselwort-Muster. Ein
/// PEM-/PGP-Anker enthält kein `@`, also kann die Regel ihn per
/// Konstruktion nicht mehr anschneiden.
#[test]
fn test_redactor_query_rule_does_not_cut_a_private_key_armor_anchor() {
    let redactor = DefaultOutputRedactor::new();

    let pem = redactor.redact_text(
        "https://vault.example/api?secret=-----BEGIN PRIVATE KEY-----\n\
         MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAsecretkeybody\n\
         -----END PRIVATE KEY-----",
    );
    assert!(!pem.contains("secretkeybody"), "{pem}");
    assert_eq!(pem, "https://vault.example/api?[REDACTED]");

    let pgp = redactor.redact_text(
        "https://v/api?passphrase=-----BEGIN PGP PRIVATE KEY BLOCK-----\n\
         lQPGBFsecretpgpbody\n\
         -----END PGP PRIVATE KEY BLOCK-----",
    );
    assert!(!pgp.contains("secretpgpbody"), "{pgp}");
    assert_eq!(pgp, "https://v/api?[REDACTED]");

    // Zweite Review-Runde: derselbe Anker in Anführungszeichen. Die
    // `@`-Forderung gilt deshalb in ALLEN drei Zweigen der Wertklasse,
    // nicht nur im freien — der quotierte Zweig schnitt den Anker sonst
    // genauso durch, und `MIIEvQbodyOfKey` stand im Klartext, wo es vor
    // Spec 0078 vollständig redigiert wurde.
    let quoted = redactor.redact_text(
        "https://x/?secret=\"-----BEGIN PRIVATE KEY-----\"\n\
         MIIEvQbodyOfKey\n\
         -----END PRIVATE KEY-----",
    );
    assert!(!quoted.contains("MIIEvQbodyOfKey"), "{quoted}");
    assert_eq!(quoted, "https://x/?[REDACTED]");
}

/// Regressionstest, Fund des `regression-guard` über `ee017af..387a91e`,
/// vom Architekten vorher/nachher gemessen (Spec 0078 §9, Q-BL-0248-02,
/// echte Lockerung): Die Query-String-Regel zerschnitt den Anker der
/// Schlüsselmuster auch dann, wenn das geforderte `@` **vor** dem Anker
/// im Wert steht — `?secret=a@-----BEGIN PRIVATE KEY-----`. Die frühere
/// Begründung „ein Anker ohne `@` ist für die Regel unerreichbar" war
/// deshalb falsch: nicht der Anker muss das `@` enthalten, sondern der
/// Wert, und der beginnt vor dem Anker.
///
/// Behoben durch Kopien der vier Schlüsselmuster ganz am Anfang der
/// Liste (die Originale bleiben wörtlich an ihrer Stelle): ein
/// Schlüsselblock ist geschwärzt, bevor irgendeine wertverbrauchende
/// Regel seinen Anker überhaupt sehen kann. Das ist die Begründung, die
/// der Kommentar an den Shadow-Hash-Mustern seit Langem führt.
#[test]
fn test_redactor_redacts_a_key_block_behind_an_at_sign_in_a_query_parameter() {
    let redactor = DefaultOutputRedactor::new();

    for (label, input) in [
        (
            "frei",
            "https://v/api?secret=a@-----BEGIN PRIVATE KEY-----\n\
             MIIEvQbodyOfKey\n\
             -----END PRIVATE KEY-----",
        ),
        (
            "quotiert",
            "https://v/api?secret=\"a@-----BEGIN PRIVATE KEY-----\"\n\
             MIIEvQbodyOfKey\n\
             -----END PRIVATE KEY-----",
        ),
        (
            "PGP",
            "https://v/api?secret=a@-----BEGIN PGP PRIVATE KEY BLOCK-----\n\
             MIIEvQbodyOfKey\n\
             -----END PGP PRIVATE KEY BLOCK-----",
        ),
        (
            "ohne END",
            "https://v/api?secret=a@-----BEGIN PRIVATE KEY-----\n\
             MIIEvQbodyOfKey",
        ),
    ] {
        let redacted = redactor.redact_text(input);
        assert!(
            !redacted.contains("MIIEvQbodyOfKey"),
            "{label}: Schlüsselkörper im Klartext: {redacted}"
        );
    }
}

/// Regressionstest zum selben Fund (Spec 0078 §9, Q-BL-0248-02): Die
/// Query-String-Regel zählte `&` auch ohne vorangehendes `?` und nahm
/// damit dem strengen URL-Muster den Anker, wenn das PASSWORT ein
/// `&<schlüsselwort>=` enthält. Gemessen: vor Spec 0078 vollständig
/// redigiert, danach stand der Passwort-Präfix im Klartext.
///
/// Behoben, indem die Regel `&` nur noch nach einem `?` im selben Token
/// akzeptiert. Der von Stefan entschiedene Restfall (§5) bleibt dadurch
/// auf die `?`-Form beschränkt, so wie §5 ihn beschreibt.
#[test]
fn test_redactor_does_not_treat_an_ampersand_without_a_question_mark_as_a_query_string() {
    let redactor = DefaultOutputRedactor::new();

    for (input, expected) in [
        ("https://u:Geheim&token=b@h/x", "https://u:[REDACTED]@h/x"),
        ("ssh://u:Geheim&password=b@h/x", "ssh://u:[REDACTED]@h/x"),
        ("postgres://u:a&token=b@h/x", "postgres://u:[REDACTED]@h/x"),
    ] {
        let redacted = redactor.redact_text(input);
        assert!(!redacted.contains("Geheim"), "Eingabe: {input}");
        assert_eq!(redacted, expected, "Eingabe: {input}");
    }

    // Gegenprobe: mit vorangehendem `?` bleibt `&` ein Query-Trenner,
    // Fall C also unverändert zu.
    assert_eq!(
        redactor.redact_text("redis://cache:6379?db=1&password=p@ssw0rd"),
        "redis://cache:6379?db=1&[REDACTED]"
    );
}

/// Nebenwirkung der Kopien am Listenanfang, und eine Verbesserung: Ein
/// Schlüsselblock hinter einem Header-Namen wurde **schon vor Spec 0078**
/// zerschnitten — die frühe `api-key`-Kopie verbrauchte `-----BEGIN` und
/// nahm den Schlüsselmustern den Anker. Mit den Kopien ganz vorn ist
/// dieses ältere Leck mit zu (Spec 0078 §9, Q-BL-0248-02).
#[test]
fn test_redactor_redacts_a_key_block_behind_a_header_name() {
    let redactor = DefaultOutputRedactor::new();

    let redacted = redactor.redact_text(
        "api-key: -----BEGIN PRIVATE KEY-----\n\
         MIIEvQbodyOfKey\n\
         -----END PRIVATE KEY-----",
    );

    assert!(
        !redacted.contains("MIIEvQbodyOfKey"),
        "Schlüsselkörper im Klartext: {redacted}"
    );
}

/// Wächter, kein Gegenbeweis (grün vor und nach der Verengung der
/// Wertklasse): ein Parameterwert OHNE `@` wird weiterhin vollständig
/// redigiert — das übernimmt das Schlüsselwort-Muster, genau wie vor
/// Spec 0078. Die Verengung kostet also keine Abdeckung.
#[test]
fn test_redactor_still_redacts_a_query_parameter_without_an_at_sign() {
    let redactor = DefaultOutputRedactor::new();

    assert_eq!(
        redactor.redact_text("redis://cache:6379?password=plainpw"),
        "redis://cache:6379?[REDACTED]"
    );
    assert_eq!(
        redactor.redact_text("https://api.example.com/v1?token=abcdef&x=1"),
        "https://api.example.com/v1?[REDACTED]"
    );
    // Auch quotiert und ohne `@` — hier greift die Query-String-Regel
    // nicht mehr, das Schlüsselwort-Muster aber unverändert.
    assert_eq!(
        redactor.redact_text("https://x/?password='top secret 123'"),
        "https://x/?[REDACTED]"
    );
}

/// Spec 0078, §6.3 (T-R2): bekannte Überredaktion, Spec 0078 §5 — folgt
/// einer URL ohne Stoppzeichen ein weiteres `@` (etwa hinter `|`), wird
/// der Teil dazwischen mit geschwärzt. Nichts leakt; dieselbe Art
/// Nebenwirkung wie die schon dokumentierte bei `|`/`&` (s. den
/// Kommentar am DB-Muster in `redactor.rs`).
#[test]
fn test_redactor_known_over_redaction_across_a_pipe_separated_second_at_sign() {
    let redactor = DefaultOutputRedactor::new();

    assert_eq!(
        redactor.redact_text("ssh://git@host:2222|deploy@server"),
        "ssh://git@host:[REDACTED]@server"
    );
    assert_eq!(
        redactor.redact_text("https://u:p@h:1|user=me@mail"),
        "https://u:[REDACTED]@mail"
    );
}

/// Spec 0078, §6.3 (T-R3): bekannter Restfall, Spec 0078 §5 —
/// Entscheidung Stefan vom 2026-09-24 (Q-BL-0248-01).
///
/// Dies ist die **einzige** Stelle, an der Spec 0078 weniger redigiert
/// als der Stand davor: Enthält das Passwort einer Verbindungs-URL
/// wörtlich `?<schlüsselwort>=` mit einem `@` im Wert, greift die
/// Query-String-Regel, und der Passwort-Präfix **vor** dem `?` bleibt
/// sichtbar. Vorher redigierte das DB-Muster `a?password=b` als Ganzes
/// (`postgres://u:[REDACTED]@h/x`). Der Präfix kann beliebig lang sein.
///
/// Ursache ist die Position der Query-String-Regel vor dem DB-Muster —
/// dieselbe Position, die den auslösenden Fall des Items löst. Die
/// Zeichenkette hat zwei Lesarten (Passwort `a?password=b` mit Host `h`,
/// oder Passwort `a` mit Query-String `?password=b@h/x`), und ohne
/// echtes URL-Parsing kann kein Muster sie unterscheiden: vorher war die
/// erste Lesart zu und die zweite offen, jetzt umgekehrt.
///
/// Kein Falsch-Positiv-Test und kein Gegenbeweis — bewusst entschiedenes
/// Verhalten, hier festgehalten, damit es nicht unbemerkt kippt.
#[test]
fn test_redactor_known_remaining_case_password_with_a_query_parameter_prefix() {
    let redactor = DefaultOutputRedactor::new();

    assert_eq!(
        redactor.redact_text("postgres://u:a?password=b@h/x"),
        "postgres://u:a?[REDACTED]"
    );

    // Ohne `@` im Parameterwert ist der Fall NICHT neu: der Präfix bleibt
    // dort sichtbar, aber genauso wie vor Spec 0078 (gemessen). Hier
    // festgehalten, damit die beiden Fälle nicht verwechselt werden.
    assert_eq!(
        redactor.redact_text("postgres://u:SuperSecret123?password=pl/ain&x=y@h/db"),
        "postgres://u:SuperSecret123?[REDACTED]"
    );
}
