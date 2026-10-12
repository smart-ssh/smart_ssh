//! Schlüsselwörter, die einen Zugangsdaten-Wert in `schlüsselwort: wert` /
//! `schlüsselwort=wert` ankündigen (Spec 0013, Issue #261).
//!
//! **Eine Quelle**: Die Listen stehen je UI-Sprache hier, und der
//! Redactor wendet **immer alle** an, unabhängig von der gewählten
//! Oberflächensprache — die KI antwortet nicht verlässlich in der
//! UI-Sprache, und ein Kommando kann aus einer anderen Quelle stammen.
//! Eine neue UI-Sprache braucht hier einen Eintrag; ein Test gegen
//! `SUPPORTED_LANGUAGES` in `i18n.ts` erzwingt das.
//!
//! Abgelehnt (False Positives, `env`-/Shell-Ausgabe): `pwd` (`PWD=/home/…`
//! steht in jeder Umgebungsausgabe), `pass` und `pw` (zu kurz, matchen als
//! Teilstring in Wörtern wie `bypass=`, `pw=` in Prompt-Formaten).
//!
//! Ein Leerzeichen in einem Eintrag steht für „Leerzeichen, `_` oder `-`“.
//! Die Muster sind reine Alternationen ohne verschachtelte Quantoren.

use std::sync::LazyLock;

/// `(Sprachcode, Schlüsselwörter)` je unterstützter UI-Sprache.
pub const CREDENTIAL_KEYWORDS: &[(&str, &[&str])] = &[
    (
        "en",
        &[
            "password",
            "passwd",
            "token",
            "api_key",
            "secret",
            "passphrase",
            "credentials",
            "private key",
            "access key",
        ],
    ),
    (
        "de",
        &[
            "passwort",
            "kennwort",
            "zugangsdaten",
            "geheimnis",
            "schlüssel",
            "schluessel",
            "zugangsschlüssel",
            "zugangsschluessel",
            "token",
        ],
    ),
    (
        "fr",
        &[
            "mot de passe",
            "mots de passe",
            "phrase secrète",
            "phrase secrete",
            "phrase de passe",
            "jeton",
            "clé api",
            "cle api",
            "clé privée",
            "cle privee",
            "clé d'accès",
            "cle d'acces",
            "clé secrète",
            "cle secrete",
            "identifiants",
            "code pin",
        ],
    ),
    (
        "es",
        &[
            "contraseña",
            "contrasena",
            "clave",
            "frase de contraseña",
            "frase de contrasena",
            "secreto",
            "clave api",
            "clave privada",
            "clave de acceso",
            "clave secreta",
            "credenciales",
            "código pin",
            "codigo pin",
        ],
    ),
    (
        "pt-BR",
        &[
            "senha",
            "palavra-passe",
            "frase secreta",
            "segredo",
            "chave de api",
            "chave privada",
            "chave de acesso",
            "credenciais",
            "credencial",
            "chave secreta",
            "código pin",
            "codigo pin",
        ],
    ),
    (
        "it",
        &[
            "parola d'ordine",
            "parola chiave",
            "frase di accesso",
            "frase segreta",
            "segreto",
            "chiave api",
            "chiave privata",
            "chiave di accesso",
            "chiave segreta",
            "credenziali",
            "codice pin",
        ],
    ),
    (
        "pl",
        &[
            "hasło",
            "haslo",
            "hasła",
            "hasla",
            "fraza hasła",
            "fraza szyfrująca",
            "sekret",
            "klucz api",
            "klucz prywatny",
            "klucz dostępu",
            "klucz dostepu",
            "klucz tajny",
            "dane logowania",
            "dane uwierzytelniające",
            "poświadczenia",
            "kod pin",
        ],
    ),
    (
        "cs",
        &[
            "heslo",
            "hesla",
            "přístupová fráze",
            "pristupova fraze",
            "tajemství",
            "tajemstvi",
            "tajný klíč",
            "tajny klic",
            "klíč api",
            "klic api",
            "soukromý klíč",
            "soukromy klic",
            "přístupový klíč",
            "pristupovy klic",
            "přihlašovací údaje",
            "prihlasovaci udaje",
            "kód pin",
            "pin kód",
        ],
    ),
    (
        "uk",
        &[
            "пароль",
            "парольна фраза",
            "секрет",
            "секретний ключ",
            "токен",
            "ключ api",
            "api ключ",
            "приватний ключ",
            "закритий ключ",
            "ключ доступу",
            "облікові дані",
            "пін-код",
        ],
    ),
    (
        "ru",
        &[
            "пароль",
            "парольная фраза",
            "кодовая фраза",
            "секрет",
            "секретный ключ",
            "токен",
            "ключ api",
            "api ключ",
            "закрытый ключ",
            "приватный ключ",
            "ключ доступа",
            "учетные данные",
            "учётные данные",
            "пин-код",
        ],
    ),
    (
        "nl",
        &[
            "wachtwoord",
            "wachtzin",
            "geheim",
            "geheime sleutel",
            "api-sleutel",
            "privésleutel",
            "privesleutel",
            "private sleutel",
            "toegangssleutel",
            "toegangscode",
            "inloggegevens",
            "referenties",
            "pincode",
            "pin code",
        ],
    ),
    (
        "ja",
        &[
            "パスワード",
            "パスフレーズ",
            "パスコード",
            "暗証番号",
            "合言葉",
            "シークレット",
            "秘密鍵",
            "秘密キー",
            "アクセスキー",
            "apiキー",
            "認証情報",
            "資格情報",
            "トークン",
            "ピンコード",
        ],
    ),
    (
        "zh-Hans",
        &[
            "密码",
            "口令",
            "密码短语",
            "密钥",
            "私钥",
            "访问密钥",
            "api密钥",
            "凭据",
            "凭证",
            "令牌",
            "机密",
            "秘密",
            "认证信息",
            "pin码",
            "个人识别码",
        ],
    ),
    (
        "ko",
        &[
            "비밀번호",
            "패스워드",
            "암호",
            "패스프레이즈",
            "비밀 키",
            "비밀키",
            "개인 키",
            "개인키",
            "접근 키",
            "액세스 키",
            "api 키",
            "시크릿",
            "자격 증명",
            "인증 정보",
            "토큰",
            "핀 번호",
        ],
    ),
    (
        "sv",
        &[
            "lösenord",
            "lösenordsfras",
            "nyckelfras",
            "hemlighet",
            "hemlig nyckel",
            "privat nyckel",
            "åtkomstnyckel",
            "api nyckel",
            "inloggningsuppgifter",
            "autentiseringsuppgifter",
            "pin kod",
        ],
    ),
    (
        "da",
        &[
            "adgangskode",
            "kodeord",
            "adgangsfrase",
            "hemmelighed",
            "hemmelig nøgle",
            "privat nøgle",
            "privatnøgle",
            "adgangsnøgle",
            "api nøgle",
            "legitimationsoplysninger",
            "loginoplysninger",
            "pinkode",
        ],
    ),
    (
        "nb",
        &[
            "passord",
            "passfrase",
            "hemmelighet",
            "hemmelig nøkkel",
            "privat nøkkel",
            "privatnøkkel",
            "tilgangsnøkkel",
            "api nøkkel",
            "påloggingsinformasjon",
            "legitimasjon",
            "pinkode",
        ],
    ),
    (
        "fi",
        &[
            "salasana",
            "salasanalause",
            "salaisuus",
            "salainen avain",
            "yksityinen avain",
            "käyttöavain",
            "pääsyavain",
            "api avain",
            "kirjautumistiedot",
            "tunnistetiedot",
            "pin koodi",
            "tunnusluku",
        ],
    ),
    (
        "id",
        &[
            "kata sandi",
            "kata kunci",
            "sandi",
            "frasa sandi",
            "rahasia",
            "kunci api",
            "kunci pribadi",
            "kunci privat",
            "kunci rahasia",
            "kunci akses",
            "kredensial",
            "kode pin",
        ],
    ),
    (
        "vi",
        &[
            "mật khẩu",
            "mat khau",
            "cụm mật khẩu",
            "cum mat khau",
            "bí mật",
            "bi mat",
            "khóa api",
            "khoa api",
            "khóa riêng",
            "khoa rieng",
            "khóa bí mật",
            "khóa truy cập",
            "khoa truy cap",
            "thông tin xác thực",
            "thong tin xac thuc",
            "mã thông báo",
            "mã pin",
            "ma pin",
        ],
    ),
    (
        "hi",
        &[
            "पासवर्ड",
            "पासफ़्रेज़",
            "पासफ्रेज",
            "कूटशब्द",
            "गुप्त",
            "सीक्रेट",
            "टोकन",
            "एपीआई कुंजी",
            "एपीआई की",
            "निजी कुंजी",
            "प्राइवेट की",
            "गुप्त कुंजी",
            "एक्सेस कुंजी",
            "एक्सेस की",
            "क्रेडेंशियल",
            "क्रेडेंशियल्स",
            "पिन",
            "पिन कोड",
        ],
    ),
    (
        "tr",
        &[
            "şifre",
            "ŞİFRE",
            "sifre",
            "parola",
            "parola ifadesi",
            "PAROLA İFADESİ",
            "gizli anahtar",
            "GİZLİ ANAHTAR",
            "gizli",
            "GİZLİ",
            "api anahtarı",
            "api anahtari",
            "özel anahtar",
            "erişim anahtarı",
            "erisim anahtari",
            "ERİŞİM ANAHTARI",
            "kimlik bilgileri",
            "KİMLİK BİLGİLERİ",
            "pin kodu",
            "PİN KODU",
        ],
    ),
    (
        "el",
        &[
            "κωδικός πρόσβασης",
            "κωδικος προσβασης",
            "κωδικός",
            "κωδικος",
            "συνθηματικό",
            "συνθηματικο",
            "φράση πρόσβασης",
            "φραση προσβασης",
            "μυστικό",
            "μυστικο",
            "κλειδί api",
            "κλειδι api",
            "ιδιωτικό κλειδί",
            "ιδιωτικο κλειδι",
            "κλειδί πρόσβασης",
            "κλειδι προσβασης",
            "διαπιστευτήρια",
            "διαπιστευτηρια",
            "κωδικός pin",
            "κωδικος pin",
        ],
    ),
    (
        "hu",
        &[
            "jelszó",
            "jelszo",
            "jelmondat",
            "titkos kulcs",
            "titok",
            "api kulcs",
            "privát kulcs",
            "privat kulcs",
            "magánkulcs",
            "hozzáférési kulcs",
            "hozzaferesi kulcs",
            "hitelesítő adatok",
            "hitelesito adatok",
            "pin kód",
            "pin kod",
        ],
    ),
    (
        "ro",
        &[
            "parolă",
            "parola",
            "frază secretă",
            "fraza secreta",
            "cheie api",
            "cheie privată",
            "cheie privata",
            "cheie de acces",
            "cheie secretă",
            "cheie secreta",
            "date de autentificare",
            "credențiale",
            "credentiale",
            "cod pin",
        ],
    ),
];

/// Sprachcodes, für die eine Liste existiert.
pub fn keyword_languages() -> Vec<&'static str> {
    CREDENTIAL_KEYWORDS.iter().map(|(l, _)| *l).collect()
}

/// Alternation (ohne Gruppe) über die Listen aller Sprachen, einmal gebaut.
pub fn keyword_alternation() -> &'static str {
    static ALT: LazyLock<String> = LazyLock::new(|| {
        let mut all: Vec<&str> = CREDENTIAL_KEYWORDS
            .iter()
            .flat_map(|(_, kws)| kws.iter().copied())
            .collect();
        all.sort_unstable();
        all.dedup();
        // Zusätzlich je Eintrag die NFD-Form (Grundbuchstabe + kombinierendes
        // Zeichen, macOS, Zwischenablage) und, bei ASCII-Apostroph, die Form mit
        // typografischem Apostroph (U+2019), jeweils auch kombiniert. Der
        // Eingabetext wird nicht normalisiert, Offsets bleiben gleich.
        let mut variants: Vec<String> = Vec::new();
        for k in &all {
            let curly = k.replace('\'', "\u{2019}");
            let nfd = decompose_accents(k);
            let curly_nfd = decompose_accents(&curly);
            for v in [k.to_string(), curly, nfd, curly_nfd] {
                variants.push(v);
            }
        }
        variants.sort_unstable();
        variants.dedup();
        variants
            .iter()
            .map(|k| regex::escape(k).replace(' ', "[ _-]"))
            .collect::<Vec<_>>()
            .join("|")
    });
    &ALT
}

/// Zerlegt vorkomponierte Buchstaben mit Akzent in Grundbuchstabe +
/// kombinierendes Zeichen, also die NFD-Form. Tabelle statt Normalisierungs-
/// Crate: sie deckt die Akzentbuchstaben der Schlüsselwortlisten ab (Buchstaben
/// ohne Zerlegung wie `ø`, `ł`, `ı` bleiben unverändert).
fn decompose_accents(k: &str) -> String {
    let mut out = String::with_capacity(k.len() + 4);
    for c in k.chars() {
        decompose_char(c, &mut out);
    }
    out
}

/// Volle kanonische Zerlegung eines Zeichens (rekursiv, damit Buchstaben mit
/// mehreren Zeichen wie `ậ` = `a` + U+0323 + U+0302 stimmen): Akzenttabelle,
/// Kana mit (Halb-)Dakuten und Hangul-Silben (algorithmisch zu Jamo).
fn decompose_char(c: char, out: &mut String) {
    let code = c as u32;
    if (0xAC00..=0xD7A3).contains(&code) {
        let s = code - 0xAC00;
        let (l, v, t) = (s / 588, (s % 588) / 28, s % 28);
        out.extend(char::from_u32(0x1100 + l));
        out.extend(char::from_u32(0x1161 + v));
        if t > 0 {
            out.extend(char::from_u32(0x11A7 + t));
        }
        return;
    }
    match kana_base(c).or_else(|| accent_base(c)) {
        Some((base, mark)) => {
            decompose_char(base, out);
            out.push(mark);
        }
        None => out.push(c),
    }
}

/// Hiragana/Katakana mit Dakuten (U+3099) oder Handakuten (U+309A).
fn kana_base(c: char) -> Option<(char, char)> {
    const VOICED: char = '\u{3099}';
    const SEMI: char = '\u{309A}';
    let code = c as u32;
    match code {
        0x309E => return Some(('\u{309D}', VOICED)),
        0x30FE => return Some(('\u{30FD}', VOICED)),
        0x30F7..=0x30FA => return Some((char::from_u32(code - 0x30F7 + 0x30EF)?, VOICED)),
        _ => {}
    }
    // Katakana liegt 0x60 hinter Hiragana.
    let (hira, shift) = match code {
        0x30A1..=0x30F6 => (code - 0x60, 0x60),
        _ => (code, 0),
    };
    let back = |x: u32| char::from_u32(x + shift);
    // Reihen mit Dakuten direkt hinter dem Grundzeichen (か-ち, て, と, は-ほ).
    let voiced_after = |h: u32| matches!(h, 0x304B..=0x3062 | 0x3064..=0x3069 | 0x306F..=0x307D);
    match hira {
        0x3094 => Some((back(0x3046)?, VOICED)), // ゔ
        h if voiced_after(h) => {
            // Grundzeichen stehen an ungeraden/geraden Stellen je nach Reihe.
            let (base, mark) = if (0x304B..=0x3062).contains(&h) {
                // か(304B)=か, が(304C): Grundzeichen sind ungerade.
                if h % 2 == 0 {
                    (h - 1, VOICED)
                } else {
                    return None;
                }
            } else if (0x3064..=0x3069).contains(&h) {
                // つ(3064) づ(3065) て(3066) で(3067) と(3068) ど(3069)
                if h % 2 == 1 {
                    (h - 1, VOICED)
                } else {
                    return None;
                }
            } else {
                // は(306F) ば(3070) ぱ(3071) ひ(3072) …: Dreiergruppen.
                match (h - 0x306F) % 3 {
                    1 => (h - 1, VOICED),
                    2 => (h - 2, SEMI),
                    _ => return None,
                }
            };
            Some((back(base)?, mark))
        }
        _ => None,
    }
}

fn accent_base(c: char) -> Option<(char, char)> {
    const GRAVE: char = '\u{300}';
    const ACUTE: char = '\u{301}';
    const CIRC: char = '\u{302}';
    const TILDE: char = '\u{303}';
    const BREVE: char = '\u{306}';
    const DOT: char = '\u{307}';
    const DIAER: char = '\u{308}';
    const RING: char = '\u{30A}';
    const DACUTE: char = '\u{30B}';
    const CARON: char = '\u{30C}';
    const CEDILLA: char = '\u{327}';
    const OGONEK: char = '\u{328}';
    const COMMA_BELOW: char = '\u{326}';
    Some(match c {
        'à' => ('a', GRAVE),
        'á' => ('a', ACUTE),
        'â' => ('a', CIRC),
        'ã' => ('a', TILDE),
        'ä' => ('a', DIAER),
        'å' => ('a', RING),
        'ă' => ('a', BREVE),
        'ą' => ('a', OGONEK),
        'ç' => ('c', CEDILLA),
        'ć' => ('c', ACUTE),
        'č' => ('c', CARON),
        'ď' => ('d', CARON),
        'è' => ('e', GRAVE),
        'é' => ('e', ACUTE),
        'ê' => ('e', CIRC),
        'ë' => ('e', DIAER),
        'ę' => ('e', OGONEK),
        'ě' => ('e', CARON),
        'ì' => ('i', GRAVE),
        'í' => ('i', ACUTE),
        'î' => ('i', CIRC),
        'ï' => ('i', DIAER),
        'İ' => ('I', DOT),
        'ń' => ('n', ACUTE),
        'ñ' => ('n', TILDE),
        'ň' => ('n', CARON),
        'ò' => ('o', GRAVE),
        'ó' => ('o', ACUTE),
        'ô' => ('o', CIRC),
        'õ' => ('o', TILDE),
        'ö' => ('o', DIAER),
        'ő' => ('o', DACUTE),
        'ř' => ('r', CARON),
        'ś' => ('s', ACUTE),
        'ş' => ('s', CEDILLA),
        'Ş' => ('S', CEDILLA),
        'š' => ('s', CARON),
        'ș' => ('s', COMMA_BELOW),
        'ť' => ('t', CARON),
        'ț' => ('t', COMMA_BELOW),
        'ù' => ('u', GRAVE),
        'ú' => ('u', ACUTE),
        'û' => ('u', CIRC),
        'ü' => ('u', DIAER),
        'ů' => ('u', RING),
        'ű' => ('u', DACUTE),
        'ý' => ('y', ACUTE),
        'ź' => ('z', ACUTE),
        'ż' => ('z', DOT),
        'ž' => ('z', CARON),
        'ụ' => ('u', '\u{323}'),
        'ạ' => ('a', '\u{323}'),
        'ậ' => ('ạ', CIRC),
        'ẩ' => ('â', '\u{309}'),
        'ư' => ('u', '\u{31B}'),
        'ơ' => ('o', '\u{31B}'),
        'ự' => ('ư', '\u{323}'),
        'й' => ('и', BREVE),
        'ё' => ('е', DIAER),
        'ά' => ('α', ACUTE),
        'ή' => ('η', ACUTE),
        'ί' => ('ι', ACUTE),
        'ό' => ('ο', ACUTE),
        _ => return None,
    })
}

#[cfg(test)]
mod nfd_tests {
    use super::decompose_char;
    use unicode_normalization::UnicodeNormalization;

    /// Issue #366: Kana und Hangul werden wie die Referenz-NFD zerlegt.
    #[test]
    fn kana_and_hangul_decompose_like_reference_nfd() {
        for code in (0x3041..=0x30FF).chain(0xAC00..=0xD7A3) {
            let c = char::from_u32(code).unwrap();
            let mut ours = String::new();
            decompose_char(c, &mut ours);
            let reference: String = c.to_string().nfd().collect();
            assert_eq!(ours, reference, "U+{code:04X}");
        }
    }
}
