//! Spec 0101, A14/A19 (E9): Der Wurzelschlüssel K, verpackt unter einem
//! Master-Passwort.
//!
//! **Verpacken, nicht ersetzen** (E9): K bleibt die Wurzel. Im
//! Passwort-Modus liegt er nicht im Schlüsselbund, sondern als Chiffrat in
//! einer Datei neben der Datenbank; das Passwort öffnet nur dieses
//! Chiffrat. Dadurch ändert ein Moduswechsel oder eine Passwortänderung
//! **nur die Verpackung** — die Datenbank und der feldweise verschlüsselte
//! Verlauf bleiben mit demselben K lesbar (A15).
//!
//! **Hier und nicht in `app-logic`:** Format, Ableitung und AEAD sind reine
//! Logik ohne I/O und vollständig testbar (T15). Das Schreiben und Lesen
//! der Datei selbst (atomar, Unix-Rechte 0600) liegt dort, wo auch die
//! übrigen Dateien des Datenverzeichnisses liegen — der Bytestrom, den
//! diese Datei trägt, entsteht und zerfällt hier.
//!
//! **Warum ein eigenes Format und nicht PHC-String + beliebige Parameter:**
//! Ein Format, das die Parameter aus der Datei übernimmt, nimmt auch die
//! Parameter eines Angreifers (T15: „Gültige Verpackung mit m = 8 KiB").
//! Deshalb stehen die Parameter zwar in der Datei — sie müssen es, sonst
//! wäre eine Erhöhung später nicht möglich —, werden beim Entpacken aber
//! gegen eine Untergrenze geprüft und gehen zusätzlich als AAD in die
//! Authentifizierung ein. Ein Herunterschrauben ist damit nicht nur
//! wirkungslos, sondern sichtbar.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::ChaCha20Poly1305;
use secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroizing;

use super::db_key::DATABASE_KEY_LEN;

/// Erkennungsmarke am Anfang jeder Verpackungsdatei. Eine fremde Datei am
/// Ort der Verpackung wird daran erkannt, statt als „beschädigte
/// Verpackung" durchzugehen.
pub const WRAPPING_MAGIC: &[u8; 8] = b"SSHKWRP\x00";

/// Version des Dateiformats (A14: „versioniert").
///
/// Eine **unbekannte** Version ist kein Fehler im Chiffrat, sondern
/// „Format oder Version nicht lesbar" — in der Tabelle A3 der Zustand
/// *ungültig*, der zu D3 führt und niemals still einen neuen K erzeugt.
pub const WRAPPING_FORMAT_VERSION: u8 = 1;

/// Kennung des Schlüsselableitungsverfahrens. Nur Argon2id (A14).
const KDF_ARGON2ID: u8 = 1;

/// Die Parameter, mit denen **geschrieben** wird (A14, wörtlich):
/// m = 64 MiB, t = 3, p = 1.
pub const WRITE_M_COST_KIB: u32 = 64 * 1024;
pub const WRITE_T_COST: u32 = 3;
pub const WRITE_P_COST: u32 = 1;

/// Untergrenze beim **Entpacken** (A14: „Parameter darunter werden
/// abgelehnt"). Gleich den Schreibparametern: Eine Datei mit schwächeren
/// Parametern kann nicht von dieser App stammen.
const MIN_M_COST_KIB: u32 = WRITE_M_COST_KIB;
const MIN_T_COST: u32 = WRITE_T_COST;
const MIN_P_COST: u32 = WRITE_P_COST;

/// Obergrenzen — **nicht** in A14 gefordert und trotzdem hier.
///
/// Die Parameter stehen in einer Datei, die ein Angreifer schreiben kann.
/// Ohne Obergrenze wäre `m_cost = 4 GiB` eine Speicherbombe, die vor dem
/// ersten Fenster zuschlägt: Argon2 würde den Speicher anfordern, bevor
/// irgendetwas das Chiffrat authentifizieren kann (die Authentifizierung
/// **braucht** den abgeleiteten Schlüssel, die Reihenfolge ist nicht
/// umkehrbar). Eine Grenze nach oben ist deshalb die einzige Stelle, an der
/// dieser Fall abzufangen ist. Sie liegt weit über dem, was A14 schreibt,
/// also behindert sie keine spätere Erhöhung.
const MAX_M_COST_KIB: u32 = 1024 * 1024; // 1 GiB
const MAX_T_COST: u32 = 16;
const MAX_P_COST: u32 = 16;

/// Salt-Länge beim Schreiben. A14 verlangt „≥ 16 Byte zufällig je
/// Verpacken"; 32 Byte kosten nichts und lassen keinen Zweifel.
const WRITE_SALT_LEN: usize = 32;

/// Grenzen beim Lesen: unter 16 Byte wäre das Salt zu kurz (A14), über 64
/// Byte nimmt Argon2 es ohnehin nicht an.
const MIN_SALT_LEN: usize = 16;
const MAX_SALT_LEN: usize = 64;

const NONCE_LEN: usize = 12;
/// Poly1305-Tag.
const TAG_LEN: usize = 16;

/// Mindestlänge des Mindestpassworts (A13: „mindestens 12 Zeichen").
///
/// Hier und nicht erst in der Oberfläche: Eine Prüfung, die nur im
/// Frontend steht, prüft nichts — ein Kommandoaufruf umgeht sie. Die
/// Oberfläche darf dieselbe Regel zusätzlich anzeigen.
pub const MIN_PASSWORD_CHARS: usize = 12;

/// Warum das Verpacken oder Entpacken nicht geklappt hat.
///
/// **Kein Fehler trägt Schlüsselmaterial, Passwort oder einen
/// Bibliothekstext mit einer Nutzlast darin** (A2, §6): Jede Variante ist
/// ein fester Text. Das ist auch der Grund, warum `KeyDerivationFailed`
/// den Argon2-Fehler nicht weiterreicht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyWrapError {
    /// Marke, Formatversion oder KDF-Kennung unbekannt — die Datei ist
    /// keine Verpackung dieser App (oder aus einer neueren).
    UnsupportedFormat,
    /// Die Datei ist zu kurz oder ihre Längenangaben passen nicht zum
    /// Inhalt.
    Malformed,
    /// Die Parameter in der Datei liegen unter der Untergrenze (A14) oder
    /// über der Obergrenze (s. [`MAX_M_COST_KIB`]).
    UnacceptableParameters,
    /// A17, wörtlich: „Passwort falsch oder Datei beschädigt" — beides ist
    /// bei einem AEAD nicht unterscheidbar, und das ist Absicht (kein
    /// Orakel).
    AuthenticationFailed,
    /// Argon2 selbst hat abgelehnt. Nach der Parameterprüfung praktisch
    /// unerreichbar; bleibt ein eigener Fall, statt in `Malformed`
    /// unterzugehen.
    KeyDerivationFailed,
    /// Das Passwort ist kürzer als [`MIN_PASSWORD_CHARS`] (A13).
    PasswordTooShort,
}

impl std::fmt::Display for KeyWrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyWrapError::UnsupportedFormat => {
                write!(f, "unbekanntes Format der Schlüsseldatei")
            }
            KeyWrapError::Malformed => write!(f, "Schlüsseldatei unvollständig"),
            KeyWrapError::UnacceptableParameters => {
                write!(f, "unzulässige Parameter in der Schlüsseldatei")
            }
            KeyWrapError::AuthenticationFailed => {
                write!(f, "Passwort falsch oder Datei beschädigt")
            }
            KeyWrapError::KeyDerivationFailed => {
                write!(f, "Schlüsselableitung fehlgeschlagen")
            }
            KeyWrapError::PasswordTooShort => write!(
                f,
                "das Master-Passwort braucht mindestens {MIN_PASSWORD_CHARS} Zeichen"
            ),
        }
    }
}

impl std::error::Error for KeyWrapError {}

/// Der entpackte Wurzelschlüssel K (A19).
///
/// **Nicht `Zeroizing<[u8; 32]>`, und das ist der Punkt:** `Zeroizing`
/// überschreibt beim Freigeben, erbt aber das `Debug` seines Inhalts — ein
/// `tracing::warn!(?key)` oder ein `{:?}` in einem Fehlertext hätte K als
/// Byte-Liste ausgegeben. Genau diese Lücke ist bei `RootKeyState` und
/// [`super::DatabaseKey`] schon einmal geschlossen worden; derselbe Schutz
/// gehört an den Weg, der K aus einer Datei holt.
///
/// Kein `Display`, kein `Serialize`, kein `Clone` — K kann nicht in ein DTO
/// oder einen Text geraten. Der Weg nach draußen ist [`Self::expose`], und
/// der heißt so, dass er in einem Review auffällt.
pub struct RootKey {
    bytes: Zeroizing<[u8; DATABASE_KEY_LEN]>,
}

impl RootKey {
    fn new(bytes: Zeroizing<[u8; DATABASE_KEY_LEN]>) -> Self {
        Self { bytes }
    }

    /// Die rohen Bytes — für den Chat-Cipher und die Ableitung des
    /// Datenbankschlüssels, die beide `[u8; 32]` brauchen.
    pub fn expose(&self) -> &[u8; DATABASE_KEY_LEN] {
        &self.bytes
    }
}

impl std::fmt::Debug for RootKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RootKey(<nicht anzeigbar>)")
    }
}

/// Der Kopf der Verpackung — und gleichzeitig das AAD der
/// Authentifizierung (A14: „Kopf als AAD").
///
/// **Warum der ganze Kopf und nicht nur die Parameter:** Salt und Nonce
/// gehören mit hinein. Sonst ließe sich ein gültiges Chiffrat mit einem
/// fremden Salt neu zusammensetzen, und der Fehlschlag sähe wie ein
/// falsches Passwort aus, statt wie die Manipulation, die er ist (T15,
/// Angriffsrichtung „vertauschter Kopf").
struct Header {
    m_cost_kib: u32,
    t_cost: u32,
    p_cost: u32,
    salt: Vec<u8>,
    nonce: [u8; NONCE_LEN],
}

/// Länge des Kopfes ohne Salt: Marke (8) + Version (1) + KDF (1) +
/// drei u32 (12) + Salt-Länge (1) + Nonce (12).
const HEADER_FIXED_LEN: usize = 8 + 1 + 1 + 12 + 1 + NONCE_LEN;

impl Header {
    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_FIXED_LEN + self.salt.len());
        out.extend_from_slice(WRAPPING_MAGIC);
        out.push(WRAPPING_FORMAT_VERSION);
        out.push(KDF_ARGON2ID);
        out.extend_from_slice(&self.m_cost_kib.to_le_bytes());
        out.extend_from_slice(&self.t_cost.to_le_bytes());
        out.extend_from_slice(&self.p_cost.to_le_bytes());
        // Die Länge passt in ein Byte, weil `MAX_SALT_LEN` 64 ist; der
        // Wert kommt beim Schreiben aus `WRITE_SALT_LEN`, beim Lesen aus
        // der geprüften Spanne.
        out.push(self.salt.len() as u8);
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.nonce);
        out
    }

    /// Liest den Kopf und gibt zusätzlich seine Byte-Länge zurück — das
    /// AAD ist **genau** dieser Abschnitt, nicht eine nachgebaute Fassung
    /// davon.
    ///
    /// Jede Längenangabe wird vor dem Zugriff geprüft; es gibt in dieser
    /// Funktion keinen Index, der über das Ende laufen könnte.
    fn parse(bytes: &[u8]) -> Result<(Self, usize), KeyWrapError> {
        if bytes.len() < HEADER_FIXED_LEN {
            return Err(KeyWrapError::Malformed);
        }
        if &bytes[..8] != WRAPPING_MAGIC {
            return Err(KeyWrapError::UnsupportedFormat);
        }
        if bytes[8] != WRAPPING_FORMAT_VERSION || bytes[9] != KDF_ARGON2ID {
            return Err(KeyWrapError::UnsupportedFormat);
        }
        let m_cost_kib = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
        let t_cost = u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]);
        let p_cost = u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
        let salt_len = bytes[22] as usize;
        if !(MIN_SALT_LEN..=MAX_SALT_LEN).contains(&salt_len) {
            return Err(KeyWrapError::Malformed);
        }
        let header_len = HEADER_FIXED_LEN + salt_len;
        if bytes.len() < header_len {
            return Err(KeyWrapError::Malformed);
        }
        let salt = bytes[23..23 + salt_len].to_vec();
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[23 + salt_len..header_len]);
        Ok((
            Self {
                m_cost_kib,
                t_cost,
                p_cost,
                salt,
                nonce,
            },
            header_len,
        ))
    }

    /// A14: „beim Entpacken werden Parameter darunter abgelehnt" — und
    /// darüber ebenfalls, s. [`MAX_M_COST_KIB`].
    ///
    /// **Vor der Ableitung** aufgerufen, nicht danach: Der Sinn der
    /// Obergrenze ist, dass die teure Rechnung mit fremden Parametern gar
    /// nicht erst beginnt.
    fn check_parameters(&self) -> Result<(), KeyWrapError> {
        let within = self.m_cost_kib >= MIN_M_COST_KIB
            && self.m_cost_kib <= MAX_M_COST_KIB
            && self.t_cost >= MIN_T_COST
            && self.t_cost <= MAX_T_COST
            && self.p_cost >= MIN_P_COST
            && self.p_cost <= MAX_P_COST;
        if within {
            Ok(())
        } else {
            Err(KeyWrapError::UnacceptableParameters)
        }
    }
}

/// Baut den AEAD aus dem abgeleiteten Schlüssel.
///
/// `.into()` statt `Key::from_slice`: Dieselbe Schreibweise wie in
/// [`super::chacha`], und `from_slice` ist in `generic-array` 0.14
/// deprecated — `clippy -D warnings` nimmt es nicht an.
fn cipher_for(key: &Zeroizing<[u8; DATABASE_KEY_LEN]>) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(&(**key).into())
}

fn nonce_of(bytes: &[u8; NONCE_LEN]) -> chacha20poly1305::Nonce {
    (*bytes).into()
}

/// Argon2id(Passwort, Salt) → der Schlüssel, der das Chiffrat öffnet.
///
/// `Zeroizing`, nicht `[u8; 32]` (A19): Der abgeleitete Schlüssel ist genau
/// so viel wert wie K selbst und wird beim Freigeben überschrieben.
fn derive_wrapping_key(
    password: &SecretString,
    header: &Header,
) -> Result<Zeroizing<[u8; DATABASE_KEY_LEN]>, KeyWrapError> {
    let params = argon2::Params::new(
        header.m_cost_kib,
        header.t_cost,
        header.p_cost,
        Some(DATABASE_KEY_LEN),
    )
    .map_err(|_| KeyWrapError::UnacceptableParameters)?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; DATABASE_KEY_LEN]);
    argon
        .hash_password_into(
            password.expose_secret().as_bytes(),
            &header.salt,
            out.as_mut(),
        )
        .map_err(|_| KeyWrapError::KeyDerivationFailed)?;
    Ok(out)
}

/// A13/A14: K unter `password` verpacken. Jeder Aufruf zieht ein **neues**
/// Salt und einen neuen Nonce (A15: „neu verpacken (neues Salt)").
///
/// Der Rückgabewert ist der vollständige Inhalt der Verpackungsdatei. Er
/// enthält K nur als Chiffrat — ein Blick in die Datei zeigt Parameter,
/// Salt und Nonce, nichts weiter.
pub fn wrap_root_key(
    root_key: &[u8; DATABASE_KEY_LEN],
    password: &SecretString,
) -> Result<Vec<u8>, KeyWrapError> {
    check_password_length(password)?;

    let header = Header {
        m_cost_kib: WRITE_M_COST_KIB,
        t_cost: WRITE_T_COST,
        p_cost: WRITE_P_COST,
        salt: random_bytes(WRITE_SALT_LEN),
        nonce: {
            let mut nonce = [0u8; NONCE_LEN];
            nonce.copy_from_slice(&random_bytes(NONCE_LEN));
            nonce
        },
    };
    // Auch beim Schreiben geprüft: Die Konstanten oben und die Grenzen
    // unten dürfen nicht auseinanderlaufen, ohne dass es auffällt — sonst
    // schriebe die App Dateien, die sie selbst nicht mehr öffnet.
    header.check_parameters()?;

    let wrapping_key = derive_wrapping_key(password, &header)?;
    let cipher = cipher_for(&wrapping_key);
    let aad = header.to_bytes();
    let ciphertext = cipher
        .encrypt(
            &nonce_of(&header.nonce),
            Payload {
                msg: root_key,
                aad: &aad,
            },
        )
        .map_err(|_| KeyWrapError::KeyDerivationFailed)?;

    let mut out = aad;
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Spec 0101, Klarstellung 9: Kann diese Verpackung **überhaupt** K
/// hergeben — unabhängig vom Passwort?
///
/// Prüft Marke, Formatversion, KDF-Kennung, die Argon2-Parameter und die
/// Länge des Chiffrats. Das ist genau der Teil von [`unwrap_root_key`], der
/// **vor** der Ableitung liegt und deshalb ohne Passwort entschieden werden
/// kann. Was hier scheitert, scheitert dort mit jedem Passwort — und ist
/// damit in der Tabelle A3 der Zustand *ungültig* (D3), nicht „Passwort
/// falsch" (A17).
///
/// **Kein Orakel:** Die Funktion sagt nichts über das Passwort oder über
/// das Chiffrat aus; sie liest nur die Felder, die in der Datei offen
/// stehen und ohnehin als AAD authentifiziert werden.
///
pub fn inspect_wrapped_key(wrapped: &[u8]) -> Result<(), KeyWrapError> {
    parse_and_check(wrapped).map(|_| ())
}

/// Der geteilte Rumpf von [`inspect_wrapped_key`] und [`unwrap_root_key`].
///
/// Privat, weil [`Header`] privat bleibt — nach außen sagt
/// `inspect_wrapped_key` nur „brauchbar oder nicht", und genau das ist die
/// Frage, die Klarstellung 9 stellt.
fn parse_and_check(wrapped: &[u8]) -> Result<(Header, usize), KeyWrapError> {
    let (header, header_len) = Header::parse(wrapped)?;
    // **Reihenfolge:** Parameter prüfen, bevor gerechnet wird (T15:
    // „Gültige Verpackung mit m = 8 KiB → abgelehnt"; und die Obergrenze
    // wäre nach der Rechnung wirkungslos).
    header.check_parameters()?;
    let ciphertext = wrapped.get(header_len..).ok_or(KeyWrapError::Malformed)?;
    if ciphertext.len() != DATABASE_KEY_LEN + TAG_LEN {
        return Err(KeyWrapError::Malformed);
    }
    Ok((header, header_len))
}

/// A16/A17: Die Verpackung mit `password` öffnen.
///
/// Scheitert die Authentifizierung, ist **nicht unterscheidbar**, ob das
/// Passwort falsch war oder die Datei verändert wurde ([`KeyWrapError::
/// AuthenticationFailed`], A17). Der Aufrufer darf daraus nie schließen,
/// die Datei sei kaputt, und sie ersetzen.
///
/// [`RootKey`] (A19): K verlässt diese Funktion in einem Typ, der beim
/// Freigeben überschrieben wird und sich nicht ausgeben lässt.
pub fn unwrap_root_key(wrapped: &[u8], password: &SecretString) -> Result<RootKey, KeyWrapError> {
    // **Dieselbe Prüfung wie [`inspect_wrapped_key`], und zwar wörtlich
    // dieselbe Funktion** (Spec 0101, Klarstellung 9): Die Entscheidung
    // „unbrauchbar" führt in der Tabelle A3 nach *ungültig* und damit zu
    // D3 — also zu einem neuen K. Liefe die Vorprüfung für diesen Weg
    // getrennt von der hier, könnte eine Datei irgendwann für
    // `inspect_wrapped_key` unbrauchbar und für `unwrap_root_key` brauchbar
    // sein: Der Nutzer bekäme „Neu anfangen" angeboten, obwohl sein K noch
    // zu holen ist. Ein geteilter Aufruf schließt das per Konstruktion aus.
    let (header, header_len) = parse_and_check(wrapped)?;
    let ciphertext = &wrapped[header_len..];

    // **Keine Längenprüfung am Passwort beim Entpacken.** Eine Verpackung,
    // die mit einem (nach heutiger Regel zu kurzen) Passwort entstanden
    // ist, muss sich weiter öffnen lassen — sonst wäre eine künftige
    // Erhöhung der Mindestlänge ein Datenverlust. Die Regel gilt beim
    // Einrichten und Ändern (A13), nicht beim Entsperren.
    let wrapping_key = derive_wrapping_key(password, &header)?;
    let cipher = cipher_for(&wrapping_key);
    let aad = &wrapped[..header_len];
    let plaintext = cipher
        .decrypt(
            &nonce_of(&header.nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| KeyWrapError::AuthenticationFailed)?;

    let mut key = Zeroizing::new([0u8; DATABASE_KEY_LEN]);
    if plaintext.len() != DATABASE_KEY_LEN {
        // Authentifiziert, aber falsche Länge: kann nur aus einem anderen
        // Format stammen, das dieselbe Marke trägt.
        return Err(KeyWrapError::Malformed);
    }
    key.copy_from_slice(&plaintext);
    Ok(RootKey::new(key))
}

/// A13: „mindestens 12 Zeichen“ — **Zeichen**, nicht Byte.
///
/// `chars().count()`, nicht `len()`: Ein Passwort aus zwölf Umlauten hat
/// 24 Byte und zwölf Zeichen; eines aus zwölf Emoji hat 48 Byte. Die
/// Oberfläche zählt Zeichen, also zählt die Prüfung dasselbe — sonst
/// lehnt sie ab, was die Maske als lang genug anzeigt.
pub fn check_password_length(password: &SecretString) -> Result<(), KeyWrapError> {
    if password.expose_secret().chars().count() < MIN_PASSWORD_CHARS {
        return Err(KeyWrapError::PasswordTooShort);
    }
    Ok(())
}

fn random_bytes(len: usize) -> Vec<u8> {
    use chacha20poly1305::aead::{rand_core::RngCore, OsRng};
    let mut bytes = vec![0u8; len];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests;
