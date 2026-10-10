//! Spec 0101, T15 (adversarial) und die Zusicherungen aus A14/A19.
//!
//! **Zur Laufzeit dieser Tests:** Jedes Verpacken und Entpacken rechnet
//! einmal Argon2id mit 64 MiB (A14). Deshalb kommen hier so wenige Aufrufe
//! wie möglich vor — manipuliert wird eine **einmal** erzeugte Verpackung,
//! und der teure Weg wird nur dort gegangen, wo die Aussage ihn braucht.
//! Die Parameter sind nicht künstlich abgesenkt: Ein Test mit m = 8 KiB
//! prüfte nicht das Verfahren, das die App benutzt.

use super::*;

const ROOT_KEY: [u8; 32] = [
    0x5a, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0xa5,
];

fn password() -> SecretString {
    SecretString::from("korrekt-pferd-batterie".to_string())
}

/// Die Verpackung einmal bauen und für alle Manipulationsfälle
/// weiterverwenden — s. Modulkommentar.
fn wrapped_once() -> Vec<u8> {
    wrap_root_key(&ROOT_KEY, &password()).expect("Verpacken muss gelingen")
}

/// A14/A16: Was verpackt wurde, kommt mit demselben Passwort unverändert
/// zurück — und nur mit ihm.
#[test]
fn test_a14_round_trip_and_wrong_password() {
    let wrapped = wrapped_once();

    let unwrapped = unwrap_root_key(&wrapped, &password()).expect("Entpacken muss gelingen");
    assert_eq!(
        unwrapped.expose(),
        &ROOT_KEY,
        "K muss unverändert zurückkommen"
    );

    // A17: falsches Passwort und beschädigte Datei sind derselbe Fehler.
    let wrong = SecretString::from("korrekt-pferd-batteriE".to_string());
    assert_eq!(
        unwrap_root_key(&wrapped, &wrong).err(),
        Some(KeyWrapError::AuthenticationFailed)
    );
}

/// A19/§6: K lässt sich nicht ausgeben. Der Weg, auf dem ein Schlüssel am
/// leichtesten ins Log gerät, ist ein `?key`-Feld in einem
/// `tracing`-Makro — bei `Zeroizing<[u8; 32]>` hätte das die Bytes
/// gezeigt.
#[test]
fn test_a19_the_root_key_cannot_be_printed() {
    let wrapped = wrapped_once();
    let key = unwrap_root_key(&wrapped, &password()).expect("Entpacken");

    let rendered = format!("{key:?}");
    assert_eq!(rendered, "RootKey(<nicht anzeigbar>)");
    for byte in ROOT_KEY {
        assert!(
            !rendered.contains(&format!("{byte}")),
            "Debug zeigt ein Schlüsselbyte: {rendered}"
        );
    }
}

/// A14: K steht nicht im Klartext in der Verpackung, und der Kopf trägt
/// die vorgeschriebenen Parameter.
#[test]
fn test_a14_file_layout_and_no_plaintext_key() {
    let wrapped = wrapped_once();

    assert_eq!(&wrapped[..8], WRAPPING_MAGIC);
    assert_eq!(wrapped[8], WRAPPING_FORMAT_VERSION);
    assert_eq!(
        u32::from_le_bytes([wrapped[10], wrapped[11], wrapped[12], wrapped[13]]),
        WRITE_M_COST_KIB,
        "A14 schreibt m = 64 MiB"
    );
    assert_eq!(
        u32::from_le_bytes([wrapped[14], wrapped[15], wrapped[16], wrapped[17]]),
        WRITE_T_COST
    );
    assert_eq!(
        u32::from_le_bytes([wrapped[18], wrapped[19], wrapped[20], wrapped[21]]),
        WRITE_P_COST
    );
    assert!(
        wrapped[22] as usize >= MIN_SALT_LEN,
        "Salt ≥ 16 Byte (A14), war {}",
        wrapped[22]
    );

    // Der entscheidende Teil: K selbst darf nirgends in der Datei stehen.
    assert!(
        !wrapped.windows(ROOT_KEY.len()).any(|w| w == ROOT_KEY),
        "der Wurzelschlüssel steht im Klartext in der Verpackung"
    );
}

/// A14/A15: Jedes Verpacken zieht ein neues Salt und einen neuen Nonce —
/// zweimal dasselbe K mit demselben Passwort ergibt verschiedene Dateien.
///
/// Ohne diese Zusicherung wäre „Passwort ändern" an der Datei nicht
/// erkennbar, und zwei Verpackungen desselben K wären vergleichbar.
#[test]
fn test_a14_each_wrap_uses_fresh_salt_and_nonce() {
    let first = wrapped_once();
    let second = wrapped_once();

    assert_ne!(first, second, "zwei Verpackungen dürfen nicht gleich sein");
    let salt_len = first[22] as usize;
    assert_ne!(
        &first[23..23 + salt_len],
        &second[23..23 + salt_len],
        "das Salt muss je Verpacken neu sein"
    );
    let nonce_start = 23 + salt_len;
    assert_ne!(
        &first[nonce_start..nonce_start + 12],
        &second[nonce_start..nonce_start + 12],
        "der Nonce muss je Verpacken neu sein"
    );
}

/// T15, Kern: **ein** geändertes Byte — in Salt, Nonce, Chiffrat oder Tag —
/// und das Entsperren scheitert. Kein Zugriff auf K.
///
/// Vier Stellen, nicht die ganze Datei: Jeder dieser Aufrufe rechnet Argon2
/// mit 64 MiB. Dass **jedes** Byte gedeckt ist, zeigt der Test darunter —
/// dieser hier zeigt, dass der vollständige Weg
/// [`unwrap_root_key`] denselben Fehler liefert wie die rohe
/// Authentifizierung, also keine der Prüfungen auf dem Weg dorthin eine
/// Manipulation schluckt.
#[test]
fn test_t15_a_flipped_byte_in_salt_nonce_ciphertext_or_tag_is_rejected() {
    let wrapped = wrapped_once();
    let salt_len = wrapped[22] as usize;
    let header_len = HEADER_FIXED_LEN + salt_len;

    let places = [
        ("Salt", 23),
        ("Nonce", 23 + salt_len),
        ("Chiffrat", header_len),
        ("Tag", wrapped.len() - 1),
    ];
    for (what, index) in places {
        let mut tampered = wrapped.clone();
        tampered[index] ^= 0x01;
        assert_eq!(
            unwrap_root_key(&tampered, &password()).err(),
            Some(KeyWrapError::AuthenticationFailed),
            "{what} (Byte {index}) wurde nicht bemerkt"
        );
        assert_ne!(tampered, wrapped, "der Test hat nichts verändert");
    }
}

/// T15, Vollständigkeit: **jedes** Byte der Datei ist authentifiziert —
/// auch Marke, Version und Parameter („Kopf als AAD", A14).
///
/// Der Schlüssel wird hier **einmal** aus dem unveränderten Kopf
/// abgeleitet, danach prüft der Test jede einzelne Byte-Position mit der
/// rohen AEAD-Entschlüsselung. Dadurch ist die Aussage vollständig, ohne
/// 115-mal Argon2 zu rechnen — und sie ist die eigentlich interessante:
/// Fällt ein Feld aus dem AAD heraus, lässt sich genau dieses Feld
/// austauschen, ohne dass es auffällt (Angriffsrichtung „vertauschter
/// Kopf").
#[test]
fn test_t15_the_aead_covers_every_single_byte_of_the_file() {
    let wrapped = wrapped_once();
    let (header, header_len) = Header::parse(&wrapped).expect("eigene Datei ist lesbar");
    let derived = derive_wrapping_key(&password(), &header).expect("Ableitung");
    let cipher = cipher_for(&derived);

    // Gegenprobe: unverändert geht es auf. Ohne sie könnte der Test auch
    // dann grün sein, wenn er grundsätzlich nichts entschlüsseln kann.
    let opened = cipher
        .decrypt(
            &nonce_of(&header.nonce),
            Payload {
                msg: &wrapped[header_len..],
                aad: &wrapped[..header_len],
            },
        )
        .expect("die unveränderte Datei muss aufgehen");
    assert_eq!(opened, ROOT_KEY);

    // **Das AAD ist genau der Kopf dieser Datei** (A14), und das prüft
    // hier den Produktivpfad: Hätte `wrap_root_key` ein leeres oder ein
    // selbst zusammengebautes AAD benutzt, ginge einer der beiden
    // folgenden Versuche auf.
    for (what, aad) in [
        ("leeres AAD", &wrapped[..0]),
        ("um ein Byte verkürzter Kopf", &wrapped[..header_len - 1]),
        ("um ein Byte verlängerter Kopf", &wrapped[..header_len + 1]),
    ] {
        assert!(
            cipher
                .decrypt(
                    &nonce_of(&header.nonce),
                    Payload {
                        msg: &wrapped[header_len..],
                        aad,
                    },
                )
                .is_err(),
            "{what}: die Verpackung ging auf — der Kopf ist nicht das AAD"
        );
    }

    for index in 0..wrapped.len() {
        let mut tampered = wrapped.clone();
        tampered[index] ^= 0x01;
        // Der Nonce wird aus der **manipulierten** Datei gelesen, genau wie
        // `unwrap_root_key` es tut — sonst bliebe das Nonce-Feld
        // ungeprüft.
        let mut used_nonce = [0u8; NONCE_LEN];
        used_nonce.copy_from_slice(&tampered[23 + header.salt.len()..header_len]);
        let result = cipher.decrypt(
            &nonce_of(&used_nonce),
            Payload {
                msg: &tampered[header_len..],
                aad: &tampered[..header_len],
            },
        );
        assert!(
            result.is_err(),
            "Byte {index} ist nicht authentifiziert — es ließe sich ändern, \
             ohne dass die Verpackung es bemerkt"
        );
    }
}

/// T15, Fortsetzung: Auch die strukturellen Felder des Kopfes — Marke,
/// Version, KDF-Kennung, Parameter, Salt-Länge — führen über
/// [`unwrap_root_key`] nie zu einem Erfolg.
///
/// Die Fehlerart ist hier bewusst nicht festgeschrieben: Ein gekipptes Bit
/// in `m_cost` kann die Parameter *erhöhen* (65536 → 65537) und damit
/// zulässig bleiben; dann greift die Authentifizierung über das AAD. Beide
/// Wege sind richtig, der Erfolg ist es nie.
#[test]
fn test_t15_structural_header_fields_never_open_the_wrapping() {
    let wrapped = wrapped_once();
    // Marke (0–7), Version (8), KDF (9), m/t/p (10–21), Salt-Länge (22).
    for index in 0..23usize {
        let mut tampered = wrapped.clone();
        tampered[index] ^= 0x01;
        let result = unwrap_root_key(&tampered, &password());
        assert!(
            result.is_err(),
            "Byte {index} im Kopf wurde nicht bemerkt: die Verpackung ging auf"
        );
    }

    // Und die eine Richtung, die eigens benannt ist (A14): Parameter unter
    // der Untergrenze sind kein Authentifizierungsfehler, sondern eine
    // Ablehnung **vor** der Rechnung.
    let mut weakened = wrapped.clone();
    weakened[12] = 0x00; // m_cost 65536 → 0
    assert_eq!(
        unwrap_root_key(&weakened, &password()).err(),
        Some(KeyWrapError::UnacceptableParameters)
    );
}

/// T15, zweite Hälfte, wörtlich: „Gültige Verpackung mit m = 8 KiB →
/// abgelehnt."
///
/// Die Verpackung ist hier **echt** gültig: sie wird mit m = 8 KiB
/// hergestellt und mit demselben AAD authentifiziert, das in ihr steht. Ein
/// Angreifer, der die Parameter herunterschraubt, hat also keinen
/// Formatfehler erzeugt, sondern eine korrekte Datei mit schwacher
/// Ableitung — genau das, was abgelehnt werden muss.
#[test]
fn test_t15_a_valid_wrapping_with_weak_parameters_is_rejected() {
    let weak = forge_wrapping_with_parameters(8, 1, 1);

    // Gegenprobe zuerst: Ohne die Parameterprüfung wäre diese Datei
    // benutzbar. Das zeigt, dass der Test nicht bloß einen Formatfehler
    // trifft.
    let (header, header_len) = Header::parse(&weak).expect("die Datei ist strukturell gültig");
    assert_eq!(header.m_cost_kib, 8);
    let derived = derive_wrapping_key(&password(), &header).expect("Ableitung mit m = 8 KiB");
    let cipher = cipher_for(&derived);
    let opened = cipher
        .decrypt(
            &nonce_of(&header.nonce),
            Payload {
                msg: &weak[header_len..],
                aad: &weak[..header_len],
            },
        )
        .expect("die geschmiedete Datei ist in sich korrekt");
    assert_eq!(opened, ROOT_KEY, "die Gegenprobe muss K liefern");

    // Und trotzdem lehnt `unwrap_root_key` ab.
    assert_eq!(
        unwrap_root_key(&weak, &password()).err(),
        Some(KeyWrapError::UnacceptableParameters)
    );
}

/// Die Obergrenze (s. `MAX_M_COST_KIB`): eine Datei, die 4 GiB Speicher
/// verlangt, wird abgelehnt — **ohne** den Speicher anzufordern.
///
/// Ohne diese Prüfung wäre der Test nicht bloß rot, sondern würde die
/// Maschine in die Knie zwingen; genau das ist der Grund für die Grenze.
#[test]
fn test_a_wrapping_demanding_absurd_memory_is_rejected_before_deriving() {
    let bomb = forge_header_only(4 * 1024 * 1024, 3, 1);
    assert_eq!(
        unwrap_root_key(&bomb, &password()).err(),
        Some(KeyWrapError::UnacceptableParameters)
    );
}

/// #266: Die Obergrenze liegt bei 256 MiB. Knapp darüber wird abgelehnt,
/// **ohne** abzuleiten; genau auf der Grenze und bei den Schreibparametern
/// gilt die Datei weiter als in Ordnung.
#[test]
fn test_memory_cost_cap_is_256_mib() {
    let just_above = forge_header_only(256 * 1024 + 1, 3, 1);
    assert_eq!(
        unwrap_root_key(&just_above, &password()).err(),
        Some(KeyWrapError::UnacceptableParameters)
    );
    // Gegenprobe: auf der Grenze wird nicht wegen der Parameter abgelehnt.
    let at_cap = forge_header_only(256 * 1024, 3, 1);
    assert_ne!(
        inspect_wrapped_key(&at_cap).err(),
        Some(KeyWrapError::UnacceptableParameters)
    );
}

/// A3: Eine fremde Datei am Ort der Verpackung und eine unbekannte Version
/// sind **nicht** „Passwort falsch" — sie führen in der Tabelle A3 zum
/// Zustand *ungültig* (D3) und dürfen nicht als beschädigtes Chiffrat
/// durchgehen.
#[test]
fn test_a3_foreign_file_and_unknown_version_are_format_errors() {
    assert_eq!(
        unwrap_root_key(
            b"irgendeine andere Datei mit genug Bytes drin......",
            &password()
        )
        .err(),
        Some(KeyWrapError::UnsupportedFormat)
    );

    let mut future = wrapped_once();
    future[8] = WRAPPING_FORMAT_VERSION + 1;
    assert_eq!(
        unwrap_root_key(&future, &password()).err(),
        Some(KeyWrapError::UnsupportedFormat)
    );

    // Zu kurz: kein Panic, ein Fehler.
    for len in 0..HEADER_FIXED_LEN {
        let short = vec![0u8; len];
        assert!(unwrap_root_key(&short, &password()).is_err());
    }
    // Und eine Datei mit richtiger Marke, aber abgeschnittenem Chiffrat.
    let wrapped = wrapped_once();
    for cut in 1..=16 {
        let short = &wrapped[..wrapped.len() - cut];
        assert_eq!(
            unwrap_root_key(short, &password()).err(),
            Some(KeyWrapError::Malformed),
            "abgeschnitten um {cut} Byte"
        );
    }
}

/// A13: mindestens 12 **Zeichen**, geprüft im Backend und nicht nur in der
/// Maske — und beim Entpacken bewusst nicht geprüft.
#[test]
fn test_a13_minimum_password_length_is_enforced_when_wrapping_only() {
    let eleven = SecretString::from("abcdefghijk".to_string());
    assert_eq!(eleven.expose_secret().chars().count(), 11);
    assert_eq!(
        wrap_root_key(&ROOT_KEY, &eleven),
        Err(KeyWrapError::PasswordTooShort)
    );

    let twelve = SecretString::from("abcdefghijkl".to_string());
    assert!(wrap_root_key(&ROOT_KEY, &twelve).is_ok());

    // Zeichen, nicht Byte: zwölf Umlaute sind 24 Byte und müssen reichen.
    let umlauts = SecretString::from("ääääääääääää".to_string());
    assert_eq!(umlauts.expose_secret().len(), 24);
    assert_eq!(umlauts.expose_secret().chars().count(), 12);
    assert!(check_password_length(&umlauts).is_ok());

    // Elf Emoji sind 44 Byte — und trotzdem zu kurz.
    let emoji = SecretString::from("🔐🔐🔐🔐🔐🔐🔐🔐🔐🔐🔐".to_string());
    assert!(emoji.expose_secret().len() > MIN_PASSWORD_CHARS);
    assert_eq!(
        check_password_length(&emoji),
        Err(KeyWrapError::PasswordTooShort)
    );
}

/// A2/§6: Kein Fehlertext trägt Passwort oder Schlüsselmaterial.
#[test]
fn test_no_error_text_carries_secret_material() {
    let secret_password = "geheimes-master-passwort";
    let errors = [
        KeyWrapError::UnsupportedFormat,
        KeyWrapError::Malformed,
        KeyWrapError::UnacceptableParameters,
        KeyWrapError::AuthenticationFailed,
        KeyWrapError::KeyDerivationFailed,
        KeyWrapError::PasswordTooShort,
    ];
    let key_hex: String = ROOT_KEY.iter().map(|b| format!("{b:02x}")).collect();
    for err in errors {
        let rendered = format!("{err} / {err:?}");
        assert!(!rendered.contains(secret_password), "{rendered}");
        assert!(!rendered.contains(&key_hex), "{rendered}");
    }
}

/// Baut eine in sich **gültige** Verpackung mit frei gewählten Parametern —
/// nur für die Tests oben. Produktivcode schreibt ausschließlich die
/// Parameter aus A14.
fn forge_wrapping_with_parameters(m_cost_kib: u32, t_cost: u32, p_cost: u32) -> Vec<u8> {
    let header = Header {
        m_cost_kib,
        t_cost,
        p_cost,
        salt: vec![7u8; 32],
        nonce: [3u8; 12],
    };
    let derived = derive_wrapping_key(&password(), &header).expect("Ableitung");
    let cipher = cipher_for(&derived);
    let aad = header.to_bytes();
    let ciphertext = cipher
        .encrypt(
            &nonce_of(&header.nonce),
            Payload {
                msg: &ROOT_KEY,
                aad: &aad,
            },
        )
        .expect("Verschlüsseln");
    let mut out = aad;
    out.extend_from_slice(&ciphertext);
    out
}

/// Wie [`forge_wrapping_with_parameters`], aber ohne zu rechnen: Der Kopf
/// allein genügt, wenn die Datei schon an der Parameterprüfung scheitern
/// soll. Das Chiffrat ist Füllmaterial in der richtigen Länge.
fn forge_header_only(m_cost_kib: u32, t_cost: u32, p_cost: u32) -> Vec<u8> {
    let header = Header {
        m_cost_kib,
        t_cost,
        p_cost,
        salt: vec![7u8; 32],
        nonce: [3u8; 12],
    };
    let mut out = header.to_bytes();
    out.extend_from_slice(&[0u8; 32 + 16]);
    out
}
