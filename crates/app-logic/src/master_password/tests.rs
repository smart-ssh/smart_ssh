//! Spec 0101, T13 (A13–A15) und T16 (A17).
//!
//! Jeder Vorgang hier rechnet Argon2id mit 64 MiB (A14) — die Tests
//! verpacken deshalb so selten wie möglich und prüfen danach Dateien und
//! Schlüsselbund, statt den teuren Weg zu wiederholen.

use std::collections::HashMap;
use std::sync::Mutex;

use super::*;

const ROOT_KEY: [u8; 32] = [0x11; 32];
const OTHER_KEY: [u8; 32] = [0x22; 32];

fn pw(text: &str) -> SecretString {
    SecretString::from(text.to_string())
}

fn good() -> SecretString {
    pw("mein-master-passwort")
}

/// Zählt, was am Schlüsselbund passiert — T7/T13 verlangen Aussagen über
/// `set` und `delete`, nicht nur über den Inhalt.
#[derive(Default)]
struct CountingKeychain {
    entries: Mutex<HashMap<String, SecretString>>,
    sets: Mutex<usize>,
    deletes: Mutex<usize>,
    fail_delete: bool,
    fail_read: bool,
}

impl CountingKeychain {
    fn with_root_key(key: &[u8; 32]) -> Self {
        use base64::engine::general_purpose::STANDARD as BASE64;
        use base64::Engine;
        let store = Self::default();
        store.entries.lock().unwrap().insert(
            CHAT_CONTENT_ENCRYPTION_KEY_REF.to_string(),
            SecretString::from(BASE64.encode(key)),
        );
        store
    }

    fn has_root_key(&self) -> bool {
        self.entries
            .lock()
            .unwrap()
            .contains_key(CHAT_CONTENT_ENCRYPTION_KEY_REF)
    }
}

impl CredentialStore for CountingKeychain {
    fn get(&self, r: &CredentialRef) -> Result<SecretString, CredentialError> {
        if self.fail_read {
            return Err(CredentialError::Backend("Schlüsselbund gesperrt".into()));
        }
        self.entries
            .lock()
            .unwrap()
            .get(r.as_str())
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(r.clone()))
    }
    fn set(&self, r: &CredentialRef, value: SecretString) -> Result<(), CredentialError> {
        *self.sets.lock().unwrap() += 1;
        self.entries
            .lock()
            .unwrap()
            .insert(r.as_str().to_string(), value);
        Ok(())
    }
    fn delete(&self, r: &CredentialRef) -> Result<(), CredentialError> {
        *self.deletes.lock().unwrap() += 1;
        if self.fail_delete {
            return Err(CredentialError::Backend("Schlüsselbund gesperrt".into()));
        }
        self.entries.lock().unwrap().remove(r.as_str());
        Ok(())
    }
}

/// Ein Datenverzeichnis, das am Ende des Tests verschwindet.
struct Dir {
    path: std::path::PathBuf,
}

impl Dir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("smart-ssh-0101-mp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }
    fn db(&self) -> std::path::PathBuf {
        self.path.join("smart-ssh.db")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// T13: Einrichten → Schlüsselbund ohne K, Verpackungsdatei da; Neustart
/// mit richtigem Passwort öffnet.
#[test]
fn test_t13_setting_up_removes_the_key_from_the_keychain_and_unlocks_again() {
    let dir = Dir::new("setup");
    let keyring = CountingKeychain::with_root_key(&ROOT_KEY);
    assert_eq!(key_mode(&dir.db()), KeyMode::Keychain);

    set_up_master_password(&dir.db(), &ROOT_KEY, &good(), &good(), Some(&keyring)).unwrap();

    assert_eq!(
        key_mode(&dir.db()),
        KeyMode::Password,
        "A18: Modus gewechselt"
    );
    assert!(
        !keyring.has_root_key(),
        "A13: K muss nach dem Einrichten aus dem Schlüsselbund verschwunden sein"
    );
    assert_eq!(*keyring.sets.lock().unwrap(), 0, "A13 schreibt nichts hin");

    // „Neustart mit richtigem Passwort öffnet."
    let unlocked = unlock(&dir.db(), &good()).unwrap();
    assert_eq!(unlocked.expose(), &ROOT_KEY);

    // A14: Unix-Rechte 0600.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(wrapping_file_path(&dir.db()))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "A14: Verpackungsdatei mit 0600");
    }
}

/// T13: „Passwort mit 11 Zeichen und ohne Bestätigung abgelehnt" — und
/// zwar **ohne** dass eine Datei entsteht oder K verschwindet.
#[test]
fn test_t13_short_or_mismatched_password_changes_nothing() {
    let dir = Dir::new("reject");
    let keyring = CountingKeychain::with_root_key(&ROOT_KEY);

    let eleven = pw("abcdefghijk");
    assert!(matches!(
        set_up_master_password(&dir.db(), &ROOT_KEY, &eleven, &eleven, Some(&keyring)),
        Err(MasterPasswordError::PasswordRejected(_))
    ));

    assert!(matches!(
        set_up_master_password(
            &dir.db(),
            &ROOT_KEY,
            &good(),
            &pw("ein-anderes-passwort"),
            Some(&keyring)
        ),
        Err(MasterPasswordError::PasswordRejected(_))
    ));

    assert_eq!(key_mode(&dir.db()), KeyMode::Keychain);
    assert!(keyring.has_root_key(), "K darf nicht angefasst worden sein");
    assert_eq!(*keyring.deletes.lock().unwrap(), 0);
}

/// T13: „Fehlerinjektion nach dem Schreiben der Verpackung → K bleibt im
/// Schlüsselbund."
///
/// Die Einspeisung ist hier kein Hook, sondern der echte Fall aus A17: Das
/// `delete` scheitert. Danach liegt K an **beiden** Stellen — und genau
/// dafür ist A17 geschrieben.
#[test]
fn test_t13_a_failing_delete_leaves_the_key_in_the_keychain() {
    let dir = Dir::new("delete-fails");
    let keyring = CountingKeychain {
        fail_delete: true,
        ..CountingKeychain::with_root_key(&ROOT_KEY)
    };

    set_up_master_password(&dir.db(), &ROOT_KEY, &good(), &good(), Some(&keyring)).unwrap();

    assert_eq!(key_mode(&dir.db()), KeyMode::Password);
    assert!(
        keyring.has_root_key(),
        "das Löschen ist gescheitert — K liegt noch im Schlüsselbund (A17)"
    );

    // T16, Gleich-Fall: Beim nächsten Entsperren wird er aufgeräumt.
    let keyring = CountingKeychain::with_root_key(&ROOT_KEY);
    let key = unlock(&dir.db(), &good()).unwrap();
    assert_eq!(
        tidy_up_keychain_after_unlock(&keyring, &key),
        KeychainTidyResult::Deleted
    );
    assert!(!keyring.has_root_key());
}

/// T16 (A17): „Verpackung und Eintrag, einmal gleich, einmal verschieden →
/// Verpackung gilt; gelöscht nur im Gleich-Fall."
#[test]
fn test_t16_a_differing_keychain_entry_is_never_deleted() {
    let dir = Dir::new("tidy");
    let keyring = CountingKeychain::default();
    set_up_master_password(&dir.db(), &ROOT_KEY, &good(), &good(), None).unwrap();
    let key = unlock(&dir.db(), &good()).unwrap();

    // Verschieden → nichts gelöscht.
    let differing = CountingKeychain::with_root_key(&OTHER_KEY);
    assert_eq!(
        tidy_up_keychain_after_unlock(&differing, &key),
        KeychainTidyResult::LeftAlone
    );
    assert!(differing.has_root_key(), "ein fremdes K bleibt liegen");
    assert_eq!(*differing.deletes.lock().unwrap(), 0, "kein delete");

    // Nicht erreichbar → nichts tun.
    let unreachable = CountingKeychain {
        fail_read: true,
        ..CountingKeychain::with_root_key(&ROOT_KEY)
    };
    assert_eq!(
        tidy_up_keychain_after_unlock(&unreachable, &key),
        KeychainTidyResult::LeftAlone
    );
    assert_eq!(*unreachable.deletes.lock().unwrap(), 0);

    // Kein Eintrag → nichts zu tun.
    assert_eq!(
        tidy_up_keychain_after_unlock(&keyring, &key),
        KeychainTidyResult::NothingThere
    );
}

/// T13/A15: „Passwort ändern → altes scheitert, neues gelingt, Datenbank
/// byte-gleich"; „zurück auf Schlüsselbund → Verpackungsdatei weg".
#[test]
fn test_t13_change_password_and_switch_back() {
    let dir = Dir::new("change");
    let keyring = CountingKeychain::default();
    set_up_master_password(&dir.db(), &ROOT_KEY, &good(), &good(), None).unwrap();

    // Eine „Datenbank" daneben, die byte-gleich bleiben muss.
    std::fs::write(dir.db(), b"nicht wirklich eine Datenbank").unwrap();
    let before = std::fs::read(dir.db()).unwrap();
    let wrapping_before = std::fs::read(wrapping_file_path(&dir.db())).unwrap();

    let new = pw("ein-neues-master-passwort");
    // Falsches aktuelles Passwort → nichts geändert.
    assert!(matches!(
        change_master_password(&dir.db(), &pw("falsch-aber-lang-genug"), &new, &new),
        Err(MasterPasswordError::WrongPasswordOrDamagedFile)
    ));
    assert_eq!(
        std::fs::read(wrapping_file_path(&dir.db())).unwrap(),
        wrapping_before,
        "ein falsches aktuelles Passwort darf die Verpackung nicht ersetzen"
    );

    change_master_password(&dir.db(), &good(), &new, &new).unwrap();
    assert_eq!(
        std::fs::read(dir.db()).unwrap(),
        before,
        "Datenbank byte-gleich"
    );
    assert_ne!(
        std::fs::read(wrapping_file_path(&dir.db())).unwrap(),
        wrapping_before,
        "A15: neu verpackt, neues Salt"
    );
    // Das alte Passwort öffnet nicht mehr, das neue schon — und K ist dasselbe.
    assert!(unlock(&dir.db(), &good()).is_err());
    assert_eq!(unlock(&dir.db(), &new).unwrap().expose(), &ROOT_KEY);

    // A15, zurück auf den Schlüsselbund.
    switch_to_keychain(&dir.db(), &new, &keyring).unwrap();
    assert_eq!(key_mode(&dir.db()), KeyMode::Keychain);
    assert!(
        std::fs::symlink_metadata(wrapping_file_path(&dir.db())).is_err(),
        "A15: Verpackungsdatei weg"
    );
    match ssh_manager_core::crypto::read_root_key(&keyring) {
        ssh_manager_core::crypto::RootKeyState::Present(key) => assert_eq!(key, ROOT_KEY),
        other => panic!("K muss im Schlüsselbund liegen, war {other:?}"),
    }
}

/// A15: Scheitert das Zurücklesen aus dem Schlüsselbund, bleibt die
/// Verpackungsdatei liegen — der Modus wechselt **nicht**.
///
/// Das ist die Angriffsrichtung „Abgebrochener Moduswechsel löscht die
/// einzige Kopie von K": Ohne diese Reihenfolge wäre die Verpackung weg und
/// der Schlüsselbund-Eintrag nicht verlässlich da.
#[test]
fn test_a15_switch_back_keeps_the_wrapping_when_the_keychain_cannot_be_read() {
    let dir = Dir::new("switch-fails");
    set_up_master_password(&dir.db(), &ROOT_KEY, &good(), &good(), None).unwrap();

    let keyring = CountingKeychain {
        fail_read: true,
        ..Default::default()
    };
    assert!(matches!(
        switch_to_keychain(&dir.db(), &good(), &keyring),
        Err(MasterPasswordError::KeychainFailed { .. })
    ));

    assert_eq!(
        key_mode(&dir.db()),
        KeyMode::Password,
        "der Modus darf nicht gewechselt sein"
    );
    assert_eq!(
        unlock(&dir.db(), &good()).unwrap().expose(),
        &ROOT_KEY,
        "K muss weiter aus der Verpackung kommen"
    );
}

/// A5 im Passwort-Modus: Die alte Verpackungsdatei wird **umbenannt**, nie
/// überschrieben — und ein belegter Zielname ist ein Fehler, kein
/// stillschweigendes Ersetzen.
#[test]
fn test_a5_renaming_the_wrapping_never_overwrites() {
    let dir = Dir::new("rename");
    set_up_master_password(&dir.db(), &ROOT_KEY, &good(), &good(), None).unwrap();
    let original = std::fs::read(wrapping_file_path(&dir.db())).unwrap();

    let renamed = rename_wrapping_file(&dir.db(), ".unreadable-TEST")
        .unwrap()
        .expect("es gab eine Verpackungsdatei");
    assert_eq!(
        std::fs::read(&renamed).unwrap(),
        original,
        "der Inhalt muss byte-gleich erhalten bleiben"
    );
    assert_eq!(key_mode(&dir.db()), KeyMode::Keychain, "der Platz ist frei");

    // Ohne Verpackungsdatei: `None`, kein Fehler.
    assert!(rename_wrapping_file(&dir.db(), ".unreadable-TEST")
        .unwrap()
        .is_none());

    // Belegter Zielname → Fehler, und die neue Datei bleibt stehen.
    set_up_master_password(&dir.db(), &OTHER_KEY, &good(), &good(), None).unwrap();
    let err = rename_wrapping_file(&dir.db(), ".unreadable-TEST").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(
        std::fs::read(&renamed).unwrap(),
        original,
        "die zuvor umbenannte Datei darf nicht überschrieben sein"
    );
    assert_eq!(unlock(&dir.db(), &good()).unwrap().expose(), &OTHER_KEY);
}

/// A3/§5: Der Modus hängt allein an der Datei — und eine Verknüpfung ins
/// Leere zählt als vorhanden.
///
/// Ohne diese Lesart fiele der Start in den Schlüsselbund-Modus und
/// erzeugte dort einen neuen K (A3 verbietet genau das).
#[test]
fn test_mode_follows_the_file_including_a_dangling_symlink() {
    let dir = Dir::new("mode");
    assert_eq!(key_mode(&dir.db()), KeyMode::Keychain);

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            dir.path.join("gibt-es-nicht"),
            wrapping_file_path(&dir.db()),
        )
        .unwrap();
        assert_eq!(
            key_mode(&dir.db()),
            KeyMode::Password,
            "eine Verknüpfung ins Leere ist eine vorhandene Datei an diesem Ort"
        );
        // Und sie ist nicht lesbar → *ungültig*, nicht „Passwort falsch".
        assert!(matches!(
            unlock(&dir.db(), &good()),
            Err(MasterPasswordError::FileFailed { .. })
                | Err(MasterPasswordError::NotInPasswordMode)
        ));
    }
}

/// §6: Kein Fehlertext trägt Passwort, K oder eine Bibliotheks-Nutzlast.
#[test]
fn test_no_error_text_carries_secret_material() {
    let secret = "mein-master-passwort";
    let errors = [
        MasterPasswordError::WrongPasswordOrDamagedFile,
        MasterPasswordError::UnusableWrappingFile,
        MasterPasswordError::PasswordRejected("zu kurz"),
        MasterPasswordError::FileFailed {
            detail: format!("Pfad mit {secret} darin"),
        },
        MasterPasswordError::KeychainFailed {
            detail: format!("Nutzlast mit {secret} darin"),
        },
        MasterPasswordError::NotInPasswordMode,
        MasterPasswordError::AlreadyInPasswordMode,
    ];
    for err in &errors {
        let shown = err.to_string();
        assert!(
            !shown.contains(secret),
            "der angezeigte Text trägt das Passwort: {shown}"
        );
    }
    // Der `detail` darf es tragen — er geht ausschließlich ins Log.
    assert!(errors[3].detail_for_log().unwrap().contains(secret));
    assert!(errors[0].detail_for_log().is_none());
}
