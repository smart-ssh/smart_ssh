//! Reusable `proptest` generators for redactor tests (issue #247).
//!
//! Every generator yields a `Snippet`: the text to feed a redactor plus the
//! secret value(s) embedded in it. Later redactor work can reuse these for
//! further properties.

use proptest::prelude::*;

/// A piece of text with the secret values it contains.
#[derive(Debug, Clone)]
pub struct Snippet {
    pub text: String,
    pub secrets: Vec<String>,
}

fn chars(alphabet: &'static str, min: usize, max: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(
        proptest::sample::select(alphabet.chars().collect::<Vec<_>>()),
        min..=max,
    )
    .prop_map(|v| v.into_iter().collect())
}

const ALNUM: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const UPPER_DIGIT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const URLSAFE: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";
const NOISE: &str = "abcdefghijklmnopqrstuvwxyz ABCDEFGHIJ0123456789.,:;/_-=\n\n";

/// Secrets with a recognisable provider format (no context needed).
pub fn prefixed_secret() -> impl Strategy<Value = String> {
    prop_oneof![
        chars(UPPER_DIGIT, 16, 16).prop_map(|s| format!("AKIA{s}")),
        chars(ALNUM, 36, 36).prop_map(|s| format!("ghp_{s}")),
        chars(URLSAFE, 40, 90).prop_map(|s| format!("sk-ant-api03-{s}")),
        chars(ALNUM, 40, 48).prop_map(|s| format!("sk-{s}")),
        chars(ALNUM, 24, 32).prop_map(|s| format!("sk_live_{s}")),
        chars(ALNUM, 10, 24).prop_map(|s| format!("xoxb-123456789012-{s}")),
        (
            chars(URLSAFE, 20, 40),
            chars(URLSAFE, 20, 60),
            chars(URLSAFE, 30, 43)
        )
            .prop_map(|(h, p, s)| format!("eyJ{h}.eyJ{p}.{s}")),
    ]
}

/// Free-form secret values as they appear after `password=`, a header name
/// or a command-line flag.
pub fn plain_secret() -> impl Strategy<Value = String> {
    prop_oneof![
        chars(ALNUM, 8, 32),
        chars(URLSAFE, 12, 40),
        prefixed_secret(),
    ]
}

/// A secret embedded in one of the context shapes the redactor knows.
pub fn secret_snippet() -> impl Strategy<Value = Snippet> {
    (
        plain_secret(),
        0usize..26,
        prefixed_secret(),
        chars("abcdefghij", 3, 8),
    )
        .prop_map(|(secret, shape, strong, user)| {
            let text = match shape {
                0 => format!("Authorization: Bearer {secret}"),
                1 => format!("authorization: bearer {secret}"),
                2 => format!("x-api-key: {secret}"),
                3 => format!("X-API-Key: Bearer {secret}"),
                4 => format!("x-api-key: Bearer {secret}"),
                5 => format!("-H 'x-api-key: {secret}'"),
                6 => format!("curl -H \"Authorization: Bearer {secret}\" https://example.com/api"),
                7 => format!("password={secret}"),
                8 => format!("PASSWORD: {secret}"),
                9 => format!("token: {secret}"),
                10 => format!("API_KEY={secret}"),
                11 => format!("export GITHUB_TOKEN={secret}"),
                12 => format!("client_secret = \"{secret}\""),
                13 => format!("https://{user}:{secret}@example.com/path"),
                14 => format!("postgres://{user}:{secret}@db.example.com:5432/app"),
                15 => format!("https://example.com/cb?password={secret}&x=1"),
                16 => format!("mysql -u root -p{secret} mydb"),
                17 => format!("sshpass -p {secret} ssh {user}@host"),
                18 => format!("curl -u {user}:{secret} https://example.com"),
                19 => format!("redis-cli -a {secret} ping"),
                20 => format!("htpasswd -b /etc/htpasswd {user} {secret}"),
                21 => format!("openssl enc -aes-256-cbc -pass pass:{secret}"),
                22 => format!("AccountKey={secret}"),
                23 => strong.clone(),
                24 => format!("key is {strong} ok"),
                _ => format!("{{\"token\": \"{secret}\"}}"),
            };
            let secrets = if shape >= 23 {
                vec![strong]
            } else {
                vec![secret]
            };
            Snippet { text, secrets }
        })
}

/// Mixed document: noise and secret snippets joined by random separators.
/// Returns the full text and every embedded secret value.
pub fn document() -> impl Strategy<Value = (String, Vec<String>)> {
    proptest::collection::vec(
        (
            chars(NOISE, 0, 40),
            secret_snippet(),
            proptest::sample::select(vec!["\n", " ", "\r\n", "; "]),
        ),
        1..5,
    )
    .prop_map(|parts| {
        let mut text = String::new();
        let mut secrets = Vec::new();
        for (noise, snip, sep) in parts {
            text.push_str(&noise);
            text.push_str(sep);
            text.push_str(&snip.text);
            text.push_str(sep);
            secrets.extend(snip.secrets);
        }
        (text, secrets)
    })
}
