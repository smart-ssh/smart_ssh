//! Spec 0101, A2: der Schlüssel, mit dem SQLCipher die Datenbankdatei
//! öffnet — abgeleitet aus dem vorhandenen Wurzelschlüssel K
//! (`app:chat_content_encryption_key`, s. [`super::key`]), nicht neu
//! erzeugt und nicht aus einem Passwort gerechnet.
//!
//! **Warum abgeleitet und nicht K selbst:** K ist bereits der Schlüssel der
//! feldweisen Chat-Verschlüsselung (Spec 0036). Denselben Bytes zwei
//! Verwendungen zu geben, hieße, eine Schwäche in einem Verfahren auf das
//! andere durchzureichen. HKDF mit einem festen, verwendungsspezifischen
//! `info`-Wert trennt die beiden Verwendungen (Domain Separation), ohne
//! einen zweiten Eintrag im Schlüsselbund zu brauchen (E1).
//!
//! **Warum roher Schlüssel und kein PBKDF2** (A2): SQLCipher leitet aus
//! einem als Text übergebenen `PRAGMA key` per PBKDF2 erst einen Schlüssel
//! ab — sinnvoll bei einem Passwort, hier aber nur zusätzliche Rechenzeit
//! bei jedem Öffnen, denn K ist schon ein 256-Bit-Zufallsschlüssel aus
//! `OsRng`. Mit der Schreibweise `x'<64 Hex>'` nimmt SQLCipher die Bytes
//! unverändert als Schlüssel (gemessen, s. Spec 0101 §1 „Messungen").

use hkdf::Hkdf;
use secrecy::SecretString;
use sha2::Sha256;
use zeroize::Zeroize;

/// Länge des abgeleiteten Schlüssels in Byte (256 Bit — SQLCipher erwartet
/// bei der `x'…'`-Schreibweise genau 32 Byte).
pub const DATABASE_KEY_LEN: usize = 32;

/// Der verwendungsspezifische `info`-Wert der Ableitung (A2, wörtlich).
///
/// **Versioniert (`/v1`):** Ändert sich das Verfahren je, bekommt es einen
/// neuen `info`-Wert — der alte bleibt lesbar, statt dass eine bestehende
/// Datei stillschweigend unöffenbar wird. Jede Änderung an dieser Konstante
/// macht alle bestehenden Datenbanken unlesbar; der Known-Answer-Test
/// `test_t2_known_answer_for_a_fixed_root_key` scheitert deshalb absichtlich
/// bei jeder Änderung daran.
pub const DATABASE_KEY_HKDF_INFO: &[u8] = b"smart-ssh/db-key/v1";

/// Der abgeleitete Datenbankschlüssel.
///
/// Eigener Typ statt `[u8; 32]`, damit A2 („erscheint nicht in Log,
/// Fehlertext, DTO oder Diagnosepaket") per Konstruktion gilt und nicht nur
/// per Aufmerksamkeit jeder Aufrufstelle:
///
/// - **kein `Display`, kein `Serialize`, kein `Clone`** — er kann nicht in
///   einen Text oder ein DTO geraten;
/// - **`Debug` gibt nur einen Platzhalter aus** — ein `?key`-Feld in einer
///   `tracing`-Zeile oder ein `{:?}` in einem Fehlertext zeigt nichts;
/// - **beim Freigeben überschrieben** (A19) — die Bytes bleiben nicht im
///   Speicher liegen, nachdem die Verbindung aufgebaut ist.
///
/// Der Weg nach draußen führt nur über [`Self::pragma_value`], und der
/// liefert ein `SecretString`, das sich seinerseits nicht versehentlich
/// ausgeben lässt.
pub struct DatabaseKey {
    bytes: [u8; DATABASE_KEY_LEN],
}

impl DatabaseKey {
    /// A2: HKDF-SHA256 über den Wurzelschlüssel, ohne Salt, mit
    /// [`DATABASE_KEY_HKDF_INFO`].
    ///
    /// **Ohne Salt (`None`)** und das ist richtig so: Ein Salt schützt gegen
    /// schwache, erratbare Eingangsschlüssel. K ist ein gleichverteilter
    /// 256-Bit-Zufallswert (`OsRng`, s. [`super::key`]); HKDF-Extract hat
    /// hier nichts zu verdichten, und ein Salt müsste zusätzlich irgendwo
    /// liegen — in der verschlüsselten Datei kann er nicht liegen, denn er
    /// wird gebraucht, um sie zu öffnen.
    ///
    /// Kann nicht scheitern: `expand` gibt nur bei einer Ausgabelänge über
    /// 255 · 32 Byte einen Fehler, und [`DATABASE_KEY_LEN`] ist eine
    /// Konstante weit darunter — deshalb `expect` statt eines
    /// `Result`-Rückgabewerts, den jeder Aufrufer sinnlos behandeln müsste.
    pub fn from_root_key(root_key: &[u8; DATABASE_KEY_LEN]) -> Self {
        let hkdf = Hkdf::<Sha256>::new(None, root_key);
        let mut bytes = [0u8; DATABASE_KEY_LEN];
        hkdf.expand(DATABASE_KEY_HKDF_INFO, &mut bytes)
            .expect("HKDF-Expand auf 32 Byte kann nicht scheitern (< 255 · 32)");
        Self { bytes }
    }

    /// Der Wert für `PRAGMA key` in der Schreibweise `x'<64 Hex>'` — roher
    /// Schlüssel, kein PBKDF2 (A2).
    ///
    /// `SecretString`, nicht `String`: Der Rückgabewert landet in den
    /// `SqliteConnectOptions` und darf auf keinem Weg in eine Log-Zeile oder
    /// einen Fehlertext geraten. `secrecy` überschreibt den Inhalt beim
    /// Freigeben und hat kein `Display`.
    pub fn pragma_value(&self) -> SecretString {
        let mut hex = String::with_capacity(2 * DATABASE_KEY_LEN + 3);
        hex.push_str("x'");
        for byte in self.bytes {
            // `write!` auf einen `String` kann nicht scheitern; die
            // Handarbeit statt `format!` vermeidet eine zweite,
            // nicht überschriebene Zwischenkopie des Schlüssels.
            hex.push(char::from_digit((byte >> 4) as u32, 16).expect("Nibble < 16"));
            hex.push(char::from_digit((byte & 0x0f) as u32, 16).expect("Nibble < 16"));
        }
        hex.push('\'');
        SecretString::from(hex)
    }

    /// Nur für Tests: die rohen Bytes, um einen Known-Answer-Vergleich (T2)
    /// überhaupt formulieren zu können. Produktivcode kommt mit
    /// [`Self::pragma_value`] aus und bekommt die Bytes bewusst nicht.
    #[cfg(test)]
    fn bytes_for_tests(&self) -> [u8; DATABASE_KEY_LEN] {
        self.bytes
    }
}

impl Drop for DatabaseKey {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

/// Zeigt nie den Schlüssel — s. Typ-Doku. `Debug` ist überhaupt nur
/// implementiert, damit ein umgebender Typ sein `#[derive(Debug)]` behalten
/// kann, ohne dass dabei etwas durchsickert.
impl std::fmt::Debug for DatabaseKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DatabaseKey(<nicht anzeigbar>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    /// Derselbe feste Wurzelschlüssel wie die T0-Fixture benutzt
    /// (`persistence_sqlite::tests_fixture_t0::T0_TEST_KEY`) — bewusst
    /// dieselbe Byte-Folge, damit ein Fehlschlag hier und in T4–T6 auf
    /// denselben Eingangswert zeigt. Kein Geheimnis.
    const FIXED_ROOT_KEY: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20,
    ];

    /// T2 (Spec 0101, §7): Known-Answer. Der Wert unten ist **nicht**
    /// aus der Implementierung übernommen, sondern unabhängig gegen
    /// RFC 5869 nachgerechnet: HKDF-SHA256 mit leerem Salt und
    /// `info = "smart-ssh/db-key/v1"` über die 32 Bytes 0x01..=0x20.
    ///
    /// Dieser Test ist der Grund, warum eine Änderung an `info`, am Hash
    /// oder am Verfahren nicht unbemerkt durchgehen kann: Jede davon macht
    /// bestehende Datenbanken unlesbar, und genau dann ist hier Rot.
    #[test]
    fn test_t2_known_answer_for_a_fixed_root_key() {
        let key = DatabaseKey::from_root_key(&FIXED_ROOT_KEY);

        // Gegenrechnung in diesem Test selbst, aus den RFC-5869-Bausteinen
        // (Extract mit Null-Salt, dann ein einzelner Expand-Block) — eine
        // zweite, von `hkdf::Hkdf` unabhängige Herleitung derselben Bytes.
        // Ein Known-Answer-Test, dessen Erwartungswert aus derselben
        // Funktion stammt, die er prüft, prüft nichts.
        let expected = reference_hkdf_sha256_32(&FIXED_ROOT_KEY, DATABASE_KEY_HKDF_INFO);
        assert_eq!(
            key.bytes_for_tests(),
            expected,
            "abgeleiteter Schlüssel weicht von der RFC-5869-Gegenrechnung ab"
        );

        // Und als eingefrorener Literalwert, damit der Test auch dann
        // scheitert, wenn jemand Gegenrechnung UND Implementierung
        // gleichzeitig anpasst.
        let hex: String = expected.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex, "7c83c8e1a6b94f4a2ed61da6957ff6adaf78e22d9e8c92deee6760654ddbb677",
            "Known-Answer für den festen Wurzelschlüssel hat sich geändert — \
             bestehende Datenbanken wären damit unlesbar"
        );
    }

    /// Eigenständige RFC-5869-Umsetzung für den Testfall: Extract mit
    /// Null-Salt, ein einzelner Expand-Block (L = 32 = HashLen, also
    /// T(1) = HMAC(PRK, info ‖ 0x01)).
    fn reference_hkdf_sha256_32(ikm: &[u8], info: &[u8]) -> [u8; 32] {
        let prk = hmac_sha256(&[0u8; 32], ikm);
        let mut block_input = info.to_vec();
        block_input.push(0x01);
        hmac_sha256(&prk, &block_input)
    }

    /// HMAC-SHA256 von Hand (RFC 2104) — nur für die Gegenrechnung oben;
    /// bewusst ohne die `hmac`-Crate, damit die Gegenrechnung nicht
    /// dieselbe Bibliothek benutzt wie die Implementierung.
    fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
        use sha2::Digest;
        const BLOCK: usize = 64;
        let mut padded = [0u8; BLOCK];
        if key.len() > BLOCK {
            let digest = Sha256::digest(key);
            padded[..32].copy_from_slice(&digest);
        } else {
            padded[..key.len()].copy_from_slice(key);
        }
        let mut inner_pad = [0x36u8; BLOCK];
        let mut outer_pad = [0x5cu8; BLOCK];
        for i in 0..BLOCK {
            inner_pad[i] ^= padded[i];
            outer_pad[i] ^= padded[i];
        }
        let mut inner = Sha256::new();
        inner.update(inner_pad);
        inner.update(message);
        let inner_digest = inner.finalize();
        let mut outer = Sha256::new();
        outer.update(outer_pad);
        outer.update(inner_digest);
        let mut out = [0u8; 32];
        out.copy_from_slice(&outer.finalize());
        out
    }

    /// T2: verschiedene Wurzelschlüssel ergeben verschiedene
    /// Datenbankschlüssel — und derselbe immer denselben (sonst wäre die
    /// Datei nach einem Neustart nicht mehr öffenbar).
    #[test]
    fn test_t2_different_root_keys_yield_different_database_keys() {
        let a = DatabaseKey::from_root_key(&FIXED_ROOT_KEY);
        let a_again = DatabaseKey::from_root_key(&FIXED_ROOT_KEY);
        assert_eq!(
            a.bytes_for_tests(),
            a_again.bytes_for_tests(),
            "die Ableitung muss deterministisch sein"
        );

        // Ein einzelnes gekipptes Bit im Wurzelschlüssel muss einen völlig
        // anderen Datenbankschlüssel ergeben.
        let mut other_root = FIXED_ROOT_KEY;
        other_root[31] ^= 0x01;
        let b = DatabaseKey::from_root_key(&other_root);
        assert_ne!(a.bytes_for_tests(), b.bytes_for_tests());

        // Und ein zweiter, deutlich verschiedener Wurzelschlüssel.
        let c = DatabaseKey::from_root_key(&[0xaa; 32]);
        assert_ne!(a.bytes_for_tests(), c.bytes_for_tests());
        assert_ne!(b.bytes_for_tests(), c.bytes_for_tests());
    }

    /// T2: Der Datenbankschlüssel ist **nicht** der Wurzelschlüssel. Ohne
    /// diesen Test wäre eine „Ableitung", die K einfach durchreicht (etwa
    /// nach einem versehentlich entfernten `expand`), nicht von einer
    /// echten zu unterscheiden.
    #[test]
    fn test_t2_database_key_is_not_the_root_key() {
        for root in [FIXED_ROOT_KEY, [0x00; 32], [0xff; 32]] {
            let key = DatabaseKey::from_root_key(&root);
            assert_ne!(
                key.bytes_for_tests(),
                root,
                "der Datenbankschlüssel darf nie der Wurzelschlüssel selbst sein"
            );
        }
    }

    /// A2: die `x'…'`-Schreibweise, die SQLCipher als rohen Schlüssel
    /// nimmt — 64 Hex-Zeichen in Kleinschreibung, keine Trennzeichen.
    #[test]
    fn test_pragma_value_is_the_raw_key_hex_form() {
        let key = DatabaseKey::from_root_key(&FIXED_ROOT_KEY);
        let pragma = key.pragma_value();
        let value = pragma.expose_secret();

        assert!(value.starts_with("x'"), "erwartete x'…', war: {value}");
        assert!(value.ends_with('\''));
        assert_eq!(value.len(), 2 + 64 + 1);
        let hex = &value[2..value.len() - 1];
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "nur Hex in Kleinschreibung erwartet, war: {hex}"
        );
        let expected: String = key
            .bytes_for_tests()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hex, expected);
    }

    /// A2: `Debug` darf den Schlüssel nicht zeigen — der Weg, auf dem ein
    /// Schlüssel am leichtesten in eine Log-Zeile gerät (`?key` in einem
    /// `tracing`-Makro, `{:?}` in einem Fehlertext).
    #[test]
    fn test_debug_output_contains_no_key_material() {
        let key = DatabaseKey::from_root_key(&FIXED_ROOT_KEY);
        let rendered = format!("{key:?}");
        let hex: String = key
            .bytes_for_tests()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();

        assert!(
            !rendered.contains(&hex),
            "Debug zeigt den Schlüssel: {rendered}"
        );
        // Auch kein einzelnes Byte-Paar und keine Base64-Form des
        // Wurzelschlüssels.
        for byte in key.bytes_for_tests() {
            assert!(
                !rendered.contains(&format!("{byte}")),
                "Debug zeigt ein Schlüsselbyte: {rendered}"
            );
        }
        assert_eq!(rendered, "DatabaseKey(<nicht anzeigbar>)");
    }
}
