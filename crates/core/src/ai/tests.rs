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
