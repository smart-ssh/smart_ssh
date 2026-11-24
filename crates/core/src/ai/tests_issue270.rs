//! Redactor tests for issue #270 and the keyword lists (split off `tests.rs`).

use super::*;

// --- Issue #270: weitere Kommandozeilen-Formen und Hashes ---------------

/// Spec 0095 A1.1: weitere Programmnamen mit angehängtem `-p<wert>`;
/// Gegenprobe: `-p <wert>` (Datenbankname) und fremde Programme bleiben.
#[test]
fn test_redactor_redacts_password_argument_of_further_mysql_family_names() {
    let redactor = DefaultOutputRedactor::new();

    for cmd in [
        "mysqlsh -u r -pHunter2xyz",
        "mysqlpump -pHunter2xyz db",
        "mysql5 -pHunter2xyz",
        "mariadb-dump -pHunter2xyz db",
        "mariadb-admin -u r -pHunter2xyz status",
    ] {
        let out = redactor.redact_text(cmd);
        assert!(!out.contains("Hunter2xyz"), "{cmd} -> {out}");
        assert!(out.contains("[REDACTED]"), "{cmd} -> {out}");
    }
    // Gegenproben: Datenbankname mit Leerzeichen, Port (-P), fremdes Programm.
    for cmd in [
        "mysqlsh -p mydb",
        "mysqlpump -P3306 -h db",
        "mariadb-dump -p mydb",
        "mysqlshx -pHunter2xyz",
    ] {
        assert_eq!(redactor.redact_text(cmd), cmd);
    }
}

/// Issue #270: Drupal-7-Hash `$S$…`; ein `$S` ohne Hash-Körper bleibt.
#[test]
fn test_redactor_redacts_drupal7_hash() {
    let redactor = DefaultOutputRedactor::new();
    let hash = "$S$DabcdefghXYZ0123456789abcdefghijklmnopqrstuvwxyzABCDEF";

    let out = redactor.redact_text(&format!("pass = '{hash}' ok"));
    assert!(!out.contains("abcdefgh"), "{out}");
    assert!(out.contains("[REDACTED]"), "{out}");

    for harmless in ["echo $S and $SHELL", "price is $S$5 today", "$S$short"] {
        assert_eq!(redactor.redact_text(harmless), harmless);
    }
}

/// Issue #270: `openssl enc -K <hex>` wird redigiert, `-iv` bleibt lesbar.
#[test]
fn test_redactor_redacts_openssl_raw_key_but_not_iv() {
    let redactor = DefaultOutputRedactor::new();
    let key = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    let iv = "ffeeddccbbaa99887766554433221100";

    let out = redactor.redact_text(&format!("openssl enc -aes-256-cbc -K {key} -iv {iv}"));
    assert!(!out.contains(key), "{out}");
    assert!(out.contains(iv), "{out}");

    // Gegenprobe: ohne -K und mit fremdem Programm bleibt alles stehen.
    let plain = format!("openssl enc -aes-256-cbc -iv {iv}");
    assert_eq!(redactor.redact_text(&plain), plain);
    let other = format!("mytool -K {key}");
    assert_eq!(redactor.redact_text(&other), other);
}

/// Issue #270: ein Quote-Wert, dessen schließender Quote nach einem
/// Zeilenumbruch steht, wird redigiert; ein fehlender schließender Quote
/// verschluckt den Folgetext nicht.
#[test]
fn test_redactor_redacts_quoted_password_across_one_line_break() {
    let redactor = DefaultOutputRedactor::new();

    let out = redactor.redact_text("mysql -p'Hunter2\nxyz' mydb");
    assert!(!out.contains("Hunter2") && !out.contains("xyz"), "{out}");
    assert!(out.ends_with(" mydb"), "{out}");

    // Gegenprobe: kein schließender Quote → Folgezeilen bleiben unberührt.
    let unclosed = "mysql -p'Hunter2\nSELECT 1 FROM t;\necho done";
    let out = redactor.redact_text(unclosed);
    assert!(out.contains("SELECT 1 FROM t;\necho done"), "{out}");
}

/// Issue #270, Spec 0078 §5: BEKANNTE EINSCHRÄNKUNG — nur der letzte
/// Parameter wird geschwärzt, `Secret1`/`Secret2` bleiben stehen. Pinnt den
/// aktuellen Ausgang, damit er nicht unbemerkt schlechter wird.
#[test]
fn test_redactor_known_remaining_case_url_password_with_query_and_at_sign() {
    let redactor = DefaultOutputRedactor::new();

    assert_eq!(
        redactor.redact_text("postgres://u:Secret1?x=Secret2&token=b@h/db"),
        "postgres://u:Secret1?x=Secret2&[REDACTED]"
    );
}

/// Issue #332: per added UI language, the local word for "password"
/// announces a credential value (case-insensitive); counter-case: the
/// word without a value stays untouched.
#[test]
fn test_redactor_masks_value_after_password_word_per_language() {
    let redactor = DefaultOutputRedactor::new();
    for (lang, word) in [("pt-BR", "senha"), ("nl", "wachtwoord")] {
        assert!(
            keywords::keyword_languages().contains(&lang),
            "no keyword list for {lang}"
        );
        for w in [word.to_string(), word.to_uppercase()] {
            let out = redactor.redact_text(&format!("{w}: hunter2"));
            assert!(!out.contains("hunter2"), "{lang}: {out}");
            let out = redactor.redact_text(&format!("{w}=hunter2"));
            assert!(!out.contains("hunter2"), "{lang}: {out}");
        }
        assert_eq!(
            redactor.redact_text(&format!("Enter {word}")),
            format!("Enter {word}")
        );
    }
}

/// Issue #332: further credential words of the added languages.
#[test]
fn test_redactor_masks_other_credential_words_pt_br_nl() {
    let redactor = DefaultOutputRedactor::new();
    for word in [
        "chave de api",
        "chave privada",
        "credenciais",
        "frase secreta",
        "api-sleutel",
        "toegangssleutel",
        "inloggegevens",
        "pincode",
    ] {
        let out = redactor.redact_text(&format!("{word}: hunter2"));
        assert!(!out.contains("hunter2"), "{word}: {out}");
    }
}

/// Issue #337: per added UI language (id, vi, hi), the local word for
/// "password" announces a credential value; counter-case: the word without
/// a value stays untouched.
#[test]
fn test_redactor_masks_value_after_password_word_id_vi_hi() {
    let redactor = DefaultOutputRedactor::new();
    for (lang, word) in [("id", "kata sandi"), ("vi", "mật khẩu"), ("hi", "पासवर्ड")]
    {
        assert!(
            keywords::keyword_languages().contains(&lang),
            "no keyword list for {lang}"
        );
        for w in [word.to_string(), word.to_uppercase()] {
            for sep in [": ", "="] {
                let out = redactor.redact_text(&format!("{w}{sep}hunter2"));
                assert!(!out.contains("hunter2"), "{lang}: {out}");
            }
        }
        assert_eq!(
            redactor.redact_text(&format!("Enter {word}")),
            format!("Enter {word}")
        );
    }
}

/// Issue #337: further credential words of id, vi and hi.
#[test]
fn test_redactor_masks_other_credential_words_id_vi_hi() {
    let redactor = DefaultOutputRedactor::new();
    for word in [
        "frasa sandi",
        "kunci pribadi",
        "kredensial",
        "kode pin",
        "cụm mật khẩu",
        "khóa riêng",
        "thông tin xác thực",
        "mã pin",
        "पासफ़्रेज़",
        "निजी कुंजी",
        "क्रेडेंशियल",
        "टोकन",
    ] {
        let out = redactor.redact_text(&format!("{word}: hunter2"));
        assert!(!out.contains("hunter2"), "{word}: {out}");
    }
}

/// Issue #336: per added UI language (tr, el, hu, ro), the local word for
/// "password" masks the value after it (case-insensitive); the word without a
/// value stays untouched.
#[test]
fn test_redactor_masks_value_after_password_word_in_tr_el_hu_ro() {
    let redactor = DefaultOutputRedactor::new();
    for (lang, words) in [
        (
            "tr",
            vec![
                "şifre", "Şifre", "ŞİFRE", "ŞIFRE", "sifre", "SIFRE", "parola",
            ],
        ),
        (
            "el",
            vec![
                "κωδικός πρόσβασης",
                "κωδικός",
                "ΚΩΔΙΚΟΣ",
                "κωδικος",
                "συνθηματικό",
            ],
        ),
        ("hu", vec!["jelszó", "Jelszó", "JELSZÓ", "jelszo"]),
        ("ro", vec!["parolă", "Parolă", "PAROLĂ", "parola"]),
    ] {
        assert!(
            keywords::keyword_languages().contains(&lang),
            "no keyword list for {lang}"
        );
        for word in words {
            for sep in [": ", "="] {
                let out = redactor.redact_text(&format!("{word}{sep}hunter2"));
                assert!(
                    !out.contains("hunter2"),
                    "{lang} {word:?}{sep:?} leaked: {out}"
                );
            }
            assert_eq!(
                redactor.redact_text(&format!("Enter {word}")),
                format!("Enter {word}")
            );
        }
    }
}

/// Issue #336: Turkish dotted/dotless i. `(?i)` alone does not map `İ`
/// (U+0130) to `i` or `ı` (U+0131) to `I`, so the Turkish list carries the
/// upper-case dotted and ASCII forms explicitly.
#[test]
fn test_redactor_turkish_dotted_and_dotless_i() {
    let redactor = DefaultOutputRedactor::new();
    for text in [
        "şifre: hunter2",
        "ŞİFRE: hunter2",
        "Şifre=hunter2",
        "GİZLİ ANAHTAR: hunter2",
        "gizli anahtar: hunter2",
        "API ANAHTARI: hunter2",
        "api anahtarı: hunter2",
        "ERİŞİM ANAHTARI=hunter2",
        "KİMLİK BİLGİLERİ: hunter2",
        "PAROLA İFADESİ: hunter2",
        "PİN KODU: 1234secret",
    ] {
        let out = redactor.redact_text(text);
        assert!(
            !out.contains("hunter2") && !out.contains("1234secret"),
            "{text:?} leaked: {out}"
        );
    }
}

/// Issue #336: further credential words of the added languages.
#[test]
fn test_redactor_masks_other_credential_words_tr_el_hu_ro() {
    let redactor = DefaultOutputRedactor::new();
    for word in [
        "parola ifadesi",
        "özel anahtar",
        "kimlik bilgileri",
        "ιδιωτικό κλειδί",
        "κλειδί api",
        "διαπιστευτήρια",
        "jelmondat",
        "titkos kulcs",
        "privát kulcs",
        "hitelesítő adatok",
        "frază secretă",
        "cheie api",
        "cheie privată",
        "date de autentificare",
        "cod pin",
    ] {
        let out = redactor.redact_text(&format!("{word}: hunter2"));
        assert!(!out.contains("hunter2"), "{word}: {out}");
    }
}
