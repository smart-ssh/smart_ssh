//! Redactor tests for `.netrc` entries (split off `tests.rs`).
//!
//! Layouts as curl, ftp (`ruserpass`) and Python's `netrc` accept them:
//! fields in any order, `account`, bare `default`, quoted values, several
//! entries per file, `macdef` blocks.

use super::*;

fn redact(text: &str) -> String {
    DefaultOutputRedactor::new().redact_text(text)
}

/// Kein Teilstück (> 8 Zeichen) des Geheimnisses bleibt stehen.
fn assert_secret_gone(input: &str, secret: &str) {
    let out = redact(input);
    assert!(
        out.contains("[REDACTED]"),
        "nichts redigiert: {input:?} -> {out:?}"
    );
    let chars: Vec<char> = secret.chars().collect();
    for window in chars.windows(9) {
        let part: String = window.iter().collect();
        assert!(!out.contains(&part), "{part:?} übrig: {input:?} -> {out:?}");
    }
}

#[test]
fn test_netrc_password_is_redacted_in_any_field_order() {
    for entry in [
        "machine h.example.com account acct1 login deploy password s3cr3tNetrcPw",
        "machine h.example.com login deploy account acct1 password s3cr3tNetrcPw",
        "machine h.example.com password s3cr3tNetrcPw login deploy",
        "machine h.example.com password s3cr3tNetrcPw account acct1 login deploy",
        "machine h.example.com user deploy password s3cr3tNetrcPw",
        "machine h.example.com login deploy port 443 protocol https password s3cr3tNetrcPw",
        "machine h.example.com login deploy passwd s3cr3tNetrcPw",
        "MACHINE h.example.com LOGIN deploy ACCOUNT acct1 PASSWORD s3cr3tNetrcPw",
    ] {
        assert_secret_gone(entry, "s3cr3tNetrcPw");
    }
}

/// `account` ist laut netrc(5) ein zusätzliches Passwort.
#[test]
fn test_netrc_account_value_is_redacted() {
    for entry in [
        "machine h.example.com login deploy password s3cr3tNetrcPw account s3cr3tAccountPw",
        "machine h.example.com account s3cr3tAccountPw login deploy password s3cr3tNetrcPw",
        "machine h.example.com login deploy account s3cr3tAccountPw",
    ] {
        assert_secret_gone(entry, "s3cr3tAccountPw");
    }
}

/// Felder über mehrere Zeilen, mit Tabs, mehreren Leerzeichen, CRLF, dem
/// Wert erst auf der Folgezeile und einem Kommentar mitten im Eintrag.
#[test]
fn test_netrc_multi_line_entries_are_redacted() {
    for entry in [
        "machine h.example.com\n  login deploy\n  password s3cr3tNetrcPw\n",
        "machine h.example.com\n\taccount  acct1\n\tlogin\tdeploy\n\tpassword   s3cr3tNetrcPw\n",
        "machine h.example.com\r\n  account acct1\r\n  login deploy\r\n  password s3cr3tNetrcPw\r\n",
        "machine h.example.com login deploy password\n    s3cr3tNetrcPw\n",
        "machine h.example.com\n  # deploy key\n  account acct1 password s3cr3tNetrcPw\n",
        "machine h.example.com,login deploy,password s3cr3tNetrcPw\n",
        "machine h.example.com\n  login deploy\n  account\n  password s3cr3tNetrcPw\n",
        "machine h.example.com\n  login deploy\n  passwd\n  password s3cr3tNetrcPw\n",
        "machine h.example.com login deploy account password s3cr3tNetrcPw",
        "default\n  login anonymous\n  password\n  account s3cr3tNetrcPw\n",
    ] {
        assert_secret_gone(entry, "s3cr3tNetrcPw");
    }
}

#[test]
fn test_netrc_default_entries_are_redacted() {
    for entry in [
        "default\n  login anonymous\n  password s3cr3tNetrcPw\n",
        "default account acct1 password s3cr3tNetrcPw",
        "default password s3cr3tNetrcPw login anonymous",
        "machine a.example.com login u password s3cr3tFirstPw default password s3cr3tNetrcPw",
        "default login anonymous password pw1 account s3cr3tNetrcPw",
        "default\n  login anonymous\n  password pw1\n  account s3cr3tNetrcPw\n",
        "default\n  login anonymous\n  account s3cr3tNetrcPw\n",
        "default login anonymous account s3cr3tNetrcPw",
        "; default\n\tlogin anonymous\n\tpassword \"s3cr3tNetrcPw\"; a\r\n",
    ] {
        assert_secret_gone(entry, "s3cr3tNetrcPw");
    }
}

/// Eine einzelne Zeile eines mehrzeiligen Eintrags ohne `machine` davor,
/// wie sie `grep` oder `tail` zeigen.
#[test]
fn test_netrc_bare_login_password_line_is_redacted() {
    for line in [
        "login deploy password s3cr3tNetrcPw",
        "  login deploy password s3cr3tNetrcPw\n",
        "login deploy password s3cr3tNetrcPw\r\n",
        "\taccount acct1 login deploy password s3cr3tNetrcPw",
        "password s3cr3tNetrcPw login deploy",
        "login deploy passwd s3cr3tNetrcPw",
        "password s3cr3tNetrcPw\r\n",
        "~$ grep deploy ~/.netrc\nlogin deploy password s3cr3tNetrcPw\n~$ ",
        "  account s3cr3tNetrcPw\n",
        "  login deploy account s3cr3tNetrcPw\n",
        "machine h\n  login u\n  macdef init\ncd /\n\n  account s3cr3tNetrcPw\n",
        "  password \"s3cr3t\nNetrcPw on the next line\"\n",
    ] {
        assert_secret_gone(line, "s3cr3tNetrcPw");
    }
}

/// Dieselben Zeilen mit dem Präfix von `cat -n`, `grep -n`, `grep -H` und
/// einem Diff; das Präfix bleibt lesbar, außer das Ausweiten der Platzhalter
/// schluckt es (`5-`).
#[test]
fn test_netrc_lines_with_a_tool_prefix_are_redacted() {
    for (line, prefix) in [
        ("     3\t  password s3cr3tNetrcPw\n", "     3\t"),
        ("3:  password s3cr3tNetrcPw\n", "3:"),
        ("5-  login deploy password s3cr3tNetrcPw\n", ""),
        (
            "/home/u/.netrc:  login deploy password s3cr3tNetrcPw\n",
            "/home/u/.netrc:",
        ),
        (".netrc:3:  password s3cr3tNetrcPw\r\n", ".netrc:3:"),
        ("+  login deploy password s3cr3tNetrcPw\n", "+"),
        (">   password s3cr3tNetrcPw\n", "> "),
        ("3:default password s3cr3tNetrcPw\n", "3:"),
    ] {
        assert_secret_gone(line, "s3cr3tNetrcPw");
        assert!(
            redact(line).starts_with(prefix),
            "{line:?} -> {:?}",
            redact(line)
        );
    }
}

/// curl, ftp und Python erlauben Werte in Anführungszeichen, mit
/// Leerzeichen und `\"` darin; ftp und Python auch `\ ` ohne Quotes.
#[test]
fn test_netrc_quoted_values_are_redacted_completely() {
    for (entry, secret) in [
        (
            r#"machine h.example.com login "deploy user" password "s3cr3t with spaces""#,
            "s3cr3t with spaces",
        ),
        (
            r#"machine h.example.com login deploy password "s3cr3t\"QuotePart""#,
            r#"s3cr3t\"QuotePart"#,
        ),
        (
            "machine h.example.com login deploy password \"s3cr3t\nNextLinePart\"",
            "NextLinePart",
        ),
        (
            r"machine h.example.com login deploy password s3cr3t\ EscapedPart",
            r"s3cr3t\ EscapedPart",
        ),
        (
            r#"login "deploy user" password "s3cr3t with spaces""#,
            "s3cr3t with spaces",
        ),
        (
            r#"machine h.example.com login deploy password "s3cr3t never closed"#,
            "s3cr3t never closed",
        ),
    ] {
        assert_secret_gone(entry, secret);
    }
}

#[test]
fn test_netrc_every_entry_of_a_file_is_redacted() {
    let file = concat!(
        "# ~/.netrc\n",
        "machine a.example.com login alice password s3cr3tAlicePw\n",
        "machine b.example.com\n",
        "  account acct1\n",
        "  login bob\n",
        "  password s3cr3tBobPw12\n",
        "\n",
        "macdef init\n",
        "cd /pub\n",
        "binary\n",
        "\n",
        "machine c.example.com password s3cr3tCarolPw login carol\n",
        "default login anonymous password s3cr3tAnonPw1\n",
    );
    for secret in [
        "s3cr3tAlicePw",
        "s3cr3tBobPw12",
        "s3cr3tCarolPw",
        "s3cr3tAnonPw1",
    ] {
        assert_secret_gone(file, secret);
    }
    let out = redact(file);
    assert!(out.contains("macdef init\ncd /pub\nbinary\n"), "{out}");
}

/// Gegenproben: Fließtext mit `login`, `account`, `password`, `default`
/// und `machine` bleibt unverändert.
#[test]
fn test_netrc_patterns_leave_prose_alone() {
    for prose in [
        "Login failed password incorrect for user",
        "Login failed: password incorrect",
        "The default password is admin123 until changed",
        "Default account is locked",
        "The machine rebooted and the password prompt appeared twice",
        "Enter your login and password to continue",
        "account bob has expired",
        "passwd root",
        "Your account password expires in 5 days",
        "The default, login and password fields",
        "Error: password reset",
        "3: password reset requested by alice",
    ] {
        assert_eq!(redact(prose), prose, "fälschlich redigiert");
    }
}

/// Der Rest einer Zeile hinter dem Eintrag und die Zeilenenden bleiben.
#[test]
fn test_netrc_redaction_keeps_surrounding_text() {
    assert_eq!(
        redact("login deploy password s3cr3tNetrcPw\r\nnext line"),
        "[REDACTED]\r\nnext line"
    );
    assert_eq!(redact("  password s3cr3tNetrcPw\r\n"), "[REDACTED]\r\n");
}

/// Bewusst in Kauf genommen: eine Zeile, die nur aus netrc-Feldern besteht,
/// wird redigiert, auch wenn sie eine Logzeile ist.
#[test]
fn test_netrc_known_over_redaction_of_field_shaped_lines() {
    for line in ["user root password expired", "login failed password wrong"] {
        assert_eq!(redact(line), "[REDACTED]", "{line:?}");
    }
}

/// Die Muster laufen über jede Ausgabe: 1 MB aus netrc-Schlüsselwörtern
/// ohne abschließendes Geheimnis und 1 MB aus Einträgen bleiben schnell.
#[test]
fn test_netrc_patterns_handle_a_one_megabyte_input() {
    let redactor = DefaultOutputRedactor::new();
    let chunk = "machine h.example.com login u port 21 default login x \"quoted ";
    let input = chunk.repeat(1_000_000 / chunk.len() + 1);
    let started = std::time::Instant::now();
    assert_eq!(redactor.redact_text(&input), input);
    eprintln!(
        "netrc ohne Treffer, {} Bytes: {:?}",
        input.len(),
        started.elapsed()
    );

    let chunk = "machine h.example.com login u password s3cr3tNetrcPw\n";
    let input = chunk.repeat(1_000_000 / chunk.len() + 1);
    let started = std::time::Instant::now();
    assert!(!redactor.redact_text(&input).contains("s3cr3tNetrcPw"));
    eprintln!(
        "netrc-Einträge, {} Bytes: {:?}",
        input.len(),
        started.elapsed()
    );
}
