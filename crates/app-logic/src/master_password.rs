//! Spec 0101, Etappe 3 (A13, A15–A17): die Verpackungsdatei neben der
//! Datenbank und die vier Vorgänge auf ihr — einrichten, entsperren,
//! Passwort ändern, zurück auf den Schlüsselbund.
//!
//! **Hier und nicht in `core`:** Format und Kryptografie liegen in
//! `ssh_manager_core::crypto::key_wrapping` (reine Logik). Dieses Modul
//! macht das I/O: Datei anlegen, atomar ersetzen, umbenennen, entfernen —
//! und es kennt den Schlüsselbund, weil jeder Moduswechsel beide Seiten
//! berührt.
//!
//! **Der Modus steht in keiner Einstellung** (§5): Er ergibt sich allein
//! daraus, ob die Verpackungsdatei existiert. Eine zweite Quelle (ein Flag
//! in der Datenbank, ein Eintrag im Store) könnte mit der Datei
//! auseinanderlaufen — und im Passwort-Modus ist die Datenbank beim
//! Bestimmen des Modus noch nicht offen.
//!
//! **Die Reihenfolge jeder Schreiboperation ist Teil der Anforderung** und
//! immer dieselbe Form: neuen Zustand herstellen, zurücklesen und
//! vergleichen, **erst dann** den alten entfernen. Dadurch gibt es in
//! keinem Abbruchfenster einen Zeitpunkt, an dem K nirgends liegt — die
//! Angriffsrichtung „Abgebrochener Moduswechsel löscht die einzige Kopie
//! von K" (T13, T16).

use std::path::{Path, PathBuf};

use secrecy::{ExposeSecret, SecretString};

use ssh_manager_core::crypto::{
    self, CipherError, KeyWrapError, RootKey, CHAT_CONTENT_ENCRYPTION_KEY_REF,
};
use ssh_manager_core::profiles::{CredentialError, CredentialRef, CredentialStore};

/// Der Modus, in dem K verwahrt wird (A18 zeigt ihn an).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMode {
    /// Standard (E8): K liegt im Schlüsselbund des Betriebssystems.
    Keychain,
    /// K liegt mit Argon2id(Master-Passwort) verpackt in einer Datei neben
    /// der Datenbank.
    Password,
}

/// Was an einem Vorgang dieses Moduls scheitern kann.
///
/// **Kein Fehler trägt Passwort, K oder einen Schlüsselbund-Text mit
/// Nutzlast** (§6). Die Bibliotheks-Nutzlast landet ausschließlich im
/// `detail`, und der geht nur ins Log.
#[derive(Debug)]
pub enum MasterPasswordError {
    /// A17, wörtlich: „Passwort falsch oder Datei beschädigt".
    WrongPasswordOrDamagedFile,
    /// Format, Version oder Parameter der Verpackungsdatei sind nicht
    /// brauchbar — in der Tabelle A3 der Zustand *ungültig* (D3), **nicht**
    /// „Passwort falsch".
    UnusableWrappingFile,
    /// Das Passwort ist zu kurz (A13) oder die Wiederholung weicht ab.
    PasswordRejected(&'static str),
    /// Die Datei ließ sich nicht lesen, schreiben oder ersetzen.
    FileFailed { detail: String },
    /// Der Schlüsselbund hat nicht geantwortet (A15: dann wird die
    /// Verpackungsdatei **nicht** entfernt).
    KeychainFailed { detail: String },
    /// Es gibt keine Verpackungsdatei — der Vorgang passt nicht zum Modus.
    NotInPasswordMode,
    /// Es gibt schon eine Verpackungsdatei.
    AlreadyInPasswordMode,
}

impl std::fmt::Display for MasterPasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongPasswordOrDamagedFile => {
                write!(f, "Passwort falsch oder Datei beschädigt")
            }
            Self::UnusableWrappingFile => {
                write!(f, "die Schlüsseldatei ist nicht brauchbar")
            }
            Self::PasswordRejected(why) => write!(f, "{why}"),
            Self::FileFailed { .. } => write!(f, "die Schlüsseldatei ließ sich nicht schreiben"),
            Self::KeychainFailed { .. } => {
                write!(f, "der Schlüsselbund hat nicht geantwortet")
            }
            Self::NotInPasswordMode => write!(f, "es ist kein Master-Passwort eingerichtet"),
            Self::AlreadyInPasswordMode => {
                write!(f, "es ist schon ein Master-Passwort eingerichtet")
            }
        }
    }
}

impl std::error::Error for MasterPasswordError {}

impl MasterPasswordError {
    /// Die Bibliotheks-Nutzlast für das Log — nie für einen Dialog oder ein
    /// DTO (§6, Spec 0098 A5).
    pub fn detail_for_log(&self) -> Option<&str> {
        match self {
            Self::FileFailed { detail } | Self::KeychainFailed { detail } => Some(detail),
            _ => None,
        }
    }
}

/// A14/§5: die Verpackungsdatei liegt **neben** der Datenbank, im
/// Datenverzeichnis.
///
/// Derselbe Ort wie die Zwischendatei aus A6 und aus demselben Grund: Das
/// atomare Ersetzen per `rename` wirkt nur innerhalb eines Dateisystems.
/// Der Name hängt am Datenbanknamen, damit beide als Satz erkennbar sind —
/// A5 benennt ihn mit um.
pub fn wrapping_file_path(db_path: &Path) -> PathBuf {
    let mut name = db_path.as_os_str().to_os_string();
    name.push(".master-key");
    PathBuf::from(name)
}

/// §5: Der Modus ergibt sich aus der Existenz der Verpackungsdatei.
///
/// `symlink_metadata`, nicht `exists`: Eine Verknüpfung ins Leere ist eine
/// vorhandene Datei an diesem Ort. Sie als „kein Passwort-Modus" zu lesen
/// hieße, in den Schlüsselbund-Modus zu fallen und dort einen neuen K zu
/// erzeugen — genau das, was A3 verbietet.
pub fn key_mode(db_path: &Path) -> KeyMode {
    if std::fs::symlink_metadata(wrapping_file_path(db_path)).is_ok() {
        KeyMode::Password
    } else {
        KeyMode::Keychain
    }
}

/// A16/A17: die Verpackungsdatei mit `password` öffnen.
///
/// Rührt **nichts** an: kein Schlüsselbund, keine Datenbank, keine Datei.
/// Ein falsches Passwort lässt die App im gesperrten Zustand mit einer
/// Meldung zurück (A16: „erneute Eingabe, kein Datenbankzugriff, nie ein
/// neuer Schlüssel").
pub fn unlock(db_path: &Path, password: &SecretString) -> Result<RootKey, MasterPasswordError> {
    let bytes = read_wrapping_file(db_path)?;
    crypto::unwrap_root_key(&bytes, password).map_err(classify_unwrap_error)
}

/// A17: Verpackungsdatei **und** Schlüsselbund-Eintrag — ein abgebrochener
/// Wechsel.
///
/// „Die Verpackungsdatei gilt. Nach Entsperrung wird der Eintrag gelöscht,
/// wenn er gleich K ist; sonst nichts gelöscht, Warnung ins Log. Ist der
/// Schlüsselbund nicht erreichbar: nichts tun, Warnung ins Log."
///
/// Scheitert nie: Jeder Ausgang außer dem Löschen ist eine Log-Zeile. Der
/// Rückgabewert sagt nur, ob gelöscht wurde — für T16.
pub fn tidy_up_keychain_after_unlock(
    keyring: &dyn CredentialStore,
    root_key: &RootKey,
) -> KeychainTidyResult {
    match crypto::read_root_key(keyring) {
        crypto::RootKeyState::Present(entry) => {
            // **Byte-Vergleich, kein „ist da"** (A17): Ein *anderer*
            // Schlüssel im Schlüsselbund gehört zu irgendetwas anderem —
            // vielleicht zu einer zweiten Installation, vielleicht zu einem
            // Verlauf, der sonst verloren wäre. Er wird nicht angefasst.
            if &entry == root_key.expose() {
                match keyring.delete(&CredentialRef::new(CHAT_CONTENT_ENCRYPTION_KEY_REF)) {
                    Ok(()) | Err(CredentialError::NotFound(_)) => {
                        tracing::info!(
                            "removed the root key from the OS keychain after unlocking; the \
                             wrapping file is authoritative (Spec 0101, A17)"
                        );
                        KeychainTidyResult::Deleted
                    }
                    Err(CredentialError::Backend(_)) => {
                        tracing::warn!(
                            "the root key in the OS keychain matches but could not be removed; \
                             nothing was deleted (Spec 0101, A17)"
                        );
                        KeychainTidyResult::LeftAlone
                    }
                }
            } else {
                tracing::warn!(
                    "the OS keychain holds a root key that differs from the unwrapped one; \
                     nothing was deleted (Spec 0101, A17)"
                );
                KeychainTidyResult::LeftAlone
            }
        }
        crypto::RootKeyState::NotFound => KeychainTidyResult::NothingThere,
        // „Ist der Schlüsselbund nicht erreichbar: nichts tun, Warnung ins
        // Log." Ein unbrauchbarer Eintrag (`Invalid`) ist genauso wenig
        // „gleich K" — auch er bleibt liegen.
        crypto::RootKeyState::Unreachable(_) | crypto::RootKeyState::Invalid => {
            tracing::warn!(
                "the OS keychain could not be checked after unlocking; nothing was deleted \
                 (Spec 0101, A17)"
            );
            KeychainTidyResult::LeftAlone
        }
    }
}

/// Ergebnis von [`tidy_up_keychain_after_unlock`] — nur für Log und T16.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeychainTidyResult {
    Deleted,
    LeftAlone,
    NothingThere,
}

/// A13: Master-Passwort einrichten.
///
/// Die Reihenfolge steht wörtlich in A13 und ist der ganze Punkt dieser
/// Funktion: „K (vorhanden oder neu nach A3) verpacken, Verpackungsdatei
/// atomar schreiben, entpacken und mit K vergleichen, dann erst K aus dem
/// Schlüsselbund löschen bzw. umwandeln."
///
/// `root_key` ist das K, das weiter gelten soll — der Aufrufer hat es
/// entweder aus dem Schlüsselbund (Einrichten aus den Einstellungen) oder
/// gerade erzeugt (Einrichten aus D1, wo es keinen gibt).
///
/// `delete_from_keychain` sagt, ob am Ende der Schlüsselbund-Eintrag weg
/// soll. Aus D1 gibt es keinen; dort ist es `false`, und die Funktion
/// fasst den Schlüsselbund überhaupt nicht an.
///
/// Scheitert das Schreiben oder der Vergleich, bleibt der
/// Schlüsselbund-Eintrag unberührt (T13: „Fehlerinjektion nach dem
/// Schreiben der Verpackung → K bleibt im Schlüsselbund").
pub fn set_up_master_password(
    db_path: &Path,
    root_key: &[u8; 32],
    password: &SecretString,
    repeated: &SecretString,
    delete_from_keychain: Option<&dyn CredentialStore>,
) -> Result<(), MasterPasswordError> {
    check_new_password(password, repeated)?;
    if key_mode(db_path) == KeyMode::Password {
        return Err(MasterPasswordError::AlreadyInPasswordMode);
    }

    write_and_verify_wrapping(db_path, root_key, password)?;

    // **Erst jetzt** der Schlüsselbund. Bis hierher ist K an zwei Stellen;
    // ab hier nur noch in der Verpackung.
    if let Some(keyring) = delete_from_keychain {
        match keyring.delete(&CredentialRef::new(CHAT_CONTENT_ENCRYPTION_KEY_REF)) {
            Ok(()) | Err(CredentialError::NotFound(_)) => {}
            Err(CredentialError::Backend(detail)) => {
                // **Kein Rückbau der Verpackungsdatei** (A17): Beides liegt
                // jetzt vor, und für diesen Fall ist A17 geschrieben — die
                // Verpackung gilt, der Eintrag wird beim nächsten Entsperren
                // aufgeräumt. Die Datei wieder zu entfernen wäre der
                // schlechtere Ausgang: Der Nutzer hat ein Passwort gesetzt
                // und bekäme beim nächsten Start den Schlüsselbund-Modus.
                tracing::warn!(
                    "the master password is set up, but the root key could not be removed from \
                     the OS keychain; it is removed at the next unlock (Spec 0101, A17)"
                );
                let _ = detail;
            }
        }
    }
    tracing::info!("master password set up (Spec 0101, A13)");
    Ok(())
}

/// A15, zweite Hälfte: Passwort ändern — „altes Passwort, neu verpacken
/// (neues Salt), atomar ersetzen".
///
/// Die Datenbank wird dabei nicht angefasst (T13: „Datenbank byte-gleich"):
/// K bleibt dasselbe, nur seine Verpackung ist neu.
pub fn change_master_password(
    db_path: &Path,
    current: &SecretString,
    new_password: &SecretString,
    repeated: &SecretString,
) -> Result<(), MasterPasswordError> {
    check_new_password(new_password, repeated)?;
    // Das alte Passwort **zuerst** prüfen: Ohne es gibt es kein K, das neu
    // verpackt werden könnte.
    let root_key = unlock(db_path, current)?;
    write_and_verify_wrapping(db_path, root_key.expose(), new_password)?;
    tracing::info!("master password changed (Spec 0101, A15)");
    Ok(())
}

/// A15, erste Hälfte: zurück auf den Schlüsselbund — „aktuelles Passwort, K
/// in den Schlüsselbund schreiben, zurücklesen, vergleichen, dann
/// Verpackungsdatei entfernen".
///
/// Scheitert eine der drei ersten Stufen, bleibt die Verpackungsdatei
/// liegen und der Modus damit unverändert.
pub fn switch_to_keychain(
    db_path: &Path,
    current: &SecretString,
    keyring: &dyn CredentialStore,
) -> Result<(), MasterPasswordError> {
    let root_key = unlock(db_path, current)?;

    store_root_key(keyring, root_key.expose())?;
    // Zurücklesen und vergleichen, bevor die einzige andere Kopie von K
    // verschwindet.
    match crypto::read_root_key(keyring) {
        crypto::RootKeyState::Present(read_back) if &read_back == root_key.expose() => {}
        crypto::RootKeyState::Present(_) => {
            return Err(MasterPasswordError::KeychainFailed {
                detail: "zurückgelesener Wurzelschlüssel weicht ab".to_string(),
            })
        }
        other => {
            return Err(MasterPasswordError::KeychainFailed {
                detail: format!("Wurzelschlüssel nicht zurücklesbar: {other:?}"),
            })
        }
    }

    let path = wrapping_file_path(db_path);
    std::fs::remove_file(&path).map_err(|err| MasterPasswordError::FileFailed {
        detail: format!("Verpackungsdatei nicht entfernbar: {err}"),
    })?;
    tracing::info!("switched back to the OS keychain (Spec 0101, A15)");
    Ok(())
}

/// A5 im Passwort-Modus: „die alte Verpackungsdatei umbenennen, nie
/// überschreiben".
///
/// Gibt den Zielpfad zurück, damit der Aufrufer ihn im Dialog nennen kann.
/// `None`, wenn es keine Verpackungsdatei gibt.
pub fn rename_wrapping_file(db_path: &Path, suffix: &str) -> std::io::Result<Option<PathBuf>> {
    let path = wrapping_file_path(db_path);
    if std::fs::symlink_metadata(&path).is_err() {
        return Ok(None);
    }
    let mut target = path.as_os_str().to_os_string();
    target.push(suffix);
    let target = PathBuf::from(target);
    // Dieselbe Regel wie in `StartOverPlan::execute`: Ein vorhandenes Ziel
    // wird **nicht** überschrieben — `fs::rename` täte es stillschweigend.
    if std::fs::symlink_metadata(&target).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} existiert bereits", target.display()),
        ));
    }
    std::fs::rename(&path, &target)?;
    Ok(Some(target))
}

/// A13/A14: verpacken, atomar schreiben, **zurücklesen und mit K
/// vergleichen**.
///
/// Der Vergleich ist nicht Zierde: Er ist der Beweis, dass die Datei auf
/// der Platte K wirklich hergibt, **bevor** die andere Kopie entfernt wird.
/// Ohne ihn wäre ein Schreibfehler, den das Dateisystem erst beim Lesen
/// zeigt, ein Totalverlust.
fn write_and_verify_wrapping(
    db_path: &Path,
    root_key: &[u8; 32],
    password: &SecretString,
) -> Result<(), MasterPasswordError> {
    let wrapped = crypto::wrap_root_key(root_key, password).map_err(classify_unwrap_error)?;
    write_file_atomically(&wrapping_file_path(db_path), &wrapped)?;

    let read_back = read_wrapping_file(db_path)?;
    let unwrapped = crypto::unwrap_root_key(&read_back, password).map_err(classify_unwrap_error)?;
    if unwrapped.expose() != root_key {
        return Err(MasterPasswordError::FileFailed {
            detail: "entpackter Schlüssel weicht von K ab".to_string(),
        });
    }
    Ok(())
}

fn read_wrapping_file(db_path: &Path) -> Result<Vec<u8>, MasterPasswordError> {
    let path = wrapping_file_path(db_path);
    std::fs::read(&path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            MasterPasswordError::NotInPasswordMode
        } else {
            MasterPasswordError::FileFailed {
                detail: format!("Verpackungsdatei nicht lesbar: {err}"),
            }
        }
    })
}

/// Atomar und von Anfang an mit 0600.
///
/// **Die Rechte werden beim Anlegen gesetzt, nicht danach** — anders als in
/// `host_key_store`. Zwischen `write` und `set_permissions` läge die Datei
/// mit den Rechten aus der `umask` auf der Platte; bei einem Chiffrat, das
/// ein Offline-Rateangriff braucht, ist dieses Fenster eines zu viel.
fn write_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), MasterPasswordError> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".new");
    let tmp = PathBuf::from(tmp);

    let file_failed = |step: &str, err: std::io::Error| MasterPasswordError::FileFailed {
        detail: format!("{step}: {err}"),
    };

    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp)
            .map_err(|err| file_failed("Verpackungsdatei anlegen", err))?;
        use std::io::Write;
        file.write_all(bytes)
            .map_err(|err| file_failed("Verpackungsdatei schreiben", err))?;
        // `sync_all`, bevor umbenannt wird: Sonst könnte das `rename`
        // sichtbar sein, während der Inhalt noch im Puffer steht — ein
        // Absturz dazwischen ließe eine leere Verpackungsdatei zurück, und
        // K wäre weg.
        file.sync_all()
            .map_err(|err| file_failed("Verpackungsdatei sichern", err))?;
    }

    // Eine vorhandene Datei wird hier **absichtlich** ersetzt: Das ist der
    // Fall „Passwort ändern" (A15, „atomar ersetzen"). Das Umbenennen des
    // alten Stands gehört zu A5, nicht hierher.
    std::fs::rename(&tmp, path).map_err(|err| {
        let _ = std::fs::remove_file(&tmp);
        file_failed("Verpackungsdatei ersetzen", err)
    })?;

    #[cfg(unix)]
    {
        // Nach dem `rename` erneut, falls die Datei schon vorher existierte
        // und laxere Rechte trug (Passwortwechsel auf einer alten Datei).
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn store_root_key(
    keyring: &dyn CredentialStore,
    root_key: &[u8; 32],
) -> Result<(), MasterPasswordError> {
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine;
    keyring
        .set(
            &CredentialRef::new(CHAT_CONTENT_ENCRYPTION_KEY_REF),
            SecretString::from(BASE64.encode(root_key)),
        )
        .map_err(|err| MasterPasswordError::KeychainFailed {
            detail: format!("Wurzelschlüssel nicht schreibbar: {err}"),
        })
}

/// A13: „Passwort zweimal, mindestens 12 Zeichen".
///
/// Beides im Backend, nicht nur in der Maske: Ein Kommandoaufruf umgeht die
/// Maske.
fn check_new_password(
    password: &SecretString,
    repeated: &SecretString,
) -> Result<(), MasterPasswordError> {
    crypto::check_password_length(password).map_err(|_| {
        MasterPasswordError::PasswordRejected("das Master-Passwort braucht mindestens 12 Zeichen")
    })?;
    if password.expose_secret() != repeated.expose_secret() {
        return Err(MasterPasswordError::PasswordRejected(
            "die beiden Eingaben sind nicht gleich",
        ));
    }
    Ok(())
}

/// A17: „Passwort falsch" und „Datei beschädigt" sind **ein** Fehler, alles
/// andere ist ein Formatfehler und damit in A3 der Zustand *ungültig*.
fn classify_unwrap_error(err: KeyWrapError) -> MasterPasswordError {
    match err {
        KeyWrapError::AuthenticationFailed => MasterPasswordError::WrongPasswordOrDamagedFile,
        KeyWrapError::PasswordTooShort => MasterPasswordError::PasswordRejected(
            "das Master-Passwort braucht mindestens 12 Zeichen",
        ),
        KeyWrapError::UnsupportedFormat
        | KeyWrapError::Malformed
        | KeyWrapError::UnacceptableParameters
        | KeyWrapError::KeyDerivationFailed => MasterPasswordError::UnusableWrappingFile,
    }
}

/// Ein `CipherError` aus `generate_and_store_root_key` (A13 aus D1).
impl From<CipherError> for MasterPasswordError {
    fn from(err: CipherError) -> Self {
        MasterPasswordError::KeychainFailed {
            detail: format!("Wurzelschlüssel nicht anlegbar: {err}"),
        }
    }
}

#[cfg(test)]
mod tests;
