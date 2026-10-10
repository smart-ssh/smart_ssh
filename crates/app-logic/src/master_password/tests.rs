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

/// Klarstellung 12: die bestätigte Warnung aus A13/E10. Alle Tests, die
/// das Einrichten als *gelungen* annehmen, reichen sie mit — der Fall ohne
/// Bestätigung hat seinen eigenen Test
/// (`test_k12_setting_up_without_the_confirmed_warning_changes_nothing`).
const CONFIRMED: LossWarning = LossWarning::ConfirmedByTheUser;

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

    set_up_master_password(
        &dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        Some(&keyring),
    )
    .unwrap();

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
        set_up_master_password(
            &dir.db(),
            &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
            &eleven,
            &eleven,
            CONFIRMED,
            Some(&keyring)
        ),
        Err(MasterPasswordError::PasswordRejected(_))
    ));

    assert!(matches!(
        set_up_master_password(
            &dir.db(),
            &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
            &good(),
            &pw("ein-anderes-passwort"),
            CONFIRMED,
            Some(&keyring)
        ),
        Err(MasterPasswordError::PasswordRejected(_))
    ));

    assert_eq!(key_mode(&dir.db()), KeyMode::Keychain);
    assert!(keyring.has_root_key(), "K darf nicht angefasst worden sein");
    assert_eq!(*keyring.deletes.lock().unwrap(), 0);
}

/// Klarstellung 12 (A13/E10): **ohne die bestätigte Warnung wird nichts
/// eingerichtet** — „Jeder Weg, der ein Master-Passwort einrichtet, lehnt
/// ohne diese Bestätigung ab und verändert nichts."
///
/// Das Passwort ist hier **einwandfrei** (lang genug, beide Eingaben
/// gleich). Nur so prüft der Test die neue Bedingung und nicht eine der
/// beiden alten: Wäre er mit einem zu kurzen Passwort geschrieben, ginge er
/// auch dann durch, wenn der Riegel aus Klarstellung 12 fehlt.
///
/// **Gegenbeweis geführt:** Mit herausgenommenem Riegel (die Prüfung in
/// `set_up_master_password` entfernt, Signatur unverändert) richtet derselbe
/// Aufruf das Passwort ein — die Zusicherung auf
/// `LossWarningNotConfirmed` scheitert, und die drei Aussagen über Modus,
/// Datei und Schlüsselbund scheitern mit.
#[test]
fn test_k12_setting_up_without_the_confirmed_warning_changes_nothing() {
    let dir = Dir::new("unconfirmed");
    let keyring = CountingKeychain::with_root_key(&ROOT_KEY);

    let result = set_up_master_password(
        &dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        LossWarning::NotConfirmed,
        Some(&keyring),
    );

    assert!(
        matches!(result, Err(MasterPasswordError::LossWarningNotConfirmed)),
        "ohne Bestätigung muss genau dieser Fehler kommen, nicht PasswordRejected"
    );
    assert_eq!(
        key_mode(&dir.db()),
        KeyMode::Keychain,
        "der Modus darf nicht gewechselt haben"
    );
    assert!(
        !wrapping_file_path(&dir.db()).exists(),
        "es darf keine Verpackungsdatei entstanden sein"
    );
    assert!(
        keyring.has_root_key(),
        "K muss unverändert im Schlüsselbund liegen"
    );
    assert_eq!(*keyring.deletes.lock().unwrap(), 0, "nichts gelöscht");
    assert_eq!(*keyring.sets.lock().unwrap(), 0, "nichts geschrieben");

    // Und danach geht der richtige Weg weiter — der Riegel sperrt nicht
    // dauerhaft, er verlangt nur die Zusage.
    set_up_master_password(
        &dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        Some(&keyring),
    )
    .expect("mit bestätigter Warnung muss derselbe Aufruf durchlaufen");
    assert_eq!(key_mode(&dir.db()), KeyMode::Password);
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

    set_up_master_password(
        &dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        Some(&keyring),
    )
    .unwrap();

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
    set_up_master_password(
        &dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();
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
    set_up_master_password(
        &dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();

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

    // A15, zurück auf den Schlüsselbund. Der Eintrag ist hier weg (das
    // Einrichten hat ihn gelöscht), also fragt Klarstellung 10b nicht.
    switch_to_keychain(
        &dir.db(),
        &new,
        &keyring,
        KeychainOverwrite::OnlyAfterConfirmation,
    )
    .unwrap();
    assert_eq!(key_mode(&dir.db()), KeyMode::Keychain);
    assert!(
        std::fs::symlink_metadata(wrapping_file_path(&dir.db())).is_err(),
        "A15: Verpackungsdatei weg"
    );
    match ssh_manager_core::crypto::read_root_key(&keyring) {
        ssh_manager_core::crypto::RootKeyState::Present(key) => assert_eq!(key.expose(), &ROOT_KEY),
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
    set_up_master_password(
        &dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();

    let keyring = CountingKeychain {
        fail_read: true,
        ..Default::default()
    };
    assert!(matches!(
        switch_to_keychain(
            &dir.db(),
            &good(),
            &keyring,
            KeychainOverwrite::OnlyAfterConfirmation
        ),
        Err(MasterPasswordError::KeychainFailed { .. })
    ));

    assert_eq!(
        key_mode(&dir.db()),
        KeyMode::Password,
        "der Modus darf nicht gewechselt sein"
    );
    assert_eq!(
        *keyring.sets.lock().unwrap(),
        0,
        "Klarstellung 10b: Ist unbekannt, was im Schlüsselbund liegt, wird gar nicht erst \
         geschrieben — vorher scheiterte der Wechsel erst beim Zurücklesen, also nach dem `set`"
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
    set_up_master_password(
        &dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();
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
    set_up_master_password(
        &dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(OTHER_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();
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
        // Und sie ist nicht lesbar → A3 *nicht erreichbar*, nicht
        // „Passwort falsch" und **nicht** „kein Master-Passwort
        // eingerichtet" (Klarstellung 10a): Letzteres widersprach der Zeile
        // darüber, die für denselben Zustand `Password` sagt.
        assert!(matches!(
            unlock(&dir.db(), &good()),
            Err(MasterPasswordError::WrappingFileUnreachable { .. })
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

/// Klarstellung 9, dritter Punkt (spec-reviewer Lauf 4, Funde 3 und 4):
/// **Scheitert die Prüfung der neuen Verpackung, ist die alte noch da.**
///
/// Der Fehler wird an genau der Stelle eingespeist, auf die es ankommt:
/// **nach** einem erfolgreichen, synchronisierten Schreiben und **vor** dem
/// `rename` (`fail_the_next_wrapping_check`). Mit Dateien allein ist dieser
/// Punkt nicht zu treffen — eine Verknüpfung auf `/dev/null` scheitert schon
/// am `sync_all` und damit in beiden Reihenfolgen gleich (gemessen).
///
/// Mit der alten Reihenfolge (schreiben → `rename` → **Zieldatei**
/// zurücklesen → vergleichen) hätte das `rename` die einzige andere Kopie
/// von K an diesem Punkt längst ersetzt: Die Prüfung hätte den Verlust nur
/// noch gemeldet, nicht verhindert. Geprüft wird deshalb beides — dass der
/// Vorgang scheitert **und** dass K danach noch zu holen ist.
#[test]
fn test_a_failing_check_of_the_new_wrapping_leaves_the_old_one_able_to_yield_the_key() {
    let dir = Dir::new("verify-before-replace");
    let db = dir.db();
    set_up_master_password(
        &db,
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();

    let path = wrapping_file_path(&db);
    let before = std::fs::read(&path).unwrap();
    let tmp = {
        let mut name = path.as_os_str().to_os_string();
        name.push(".new");
        std::path::PathBuf::from(name)
    };

    let new_password = pw("ein-ganz-neues-master-passwort");
    fail_the_next_wrapping_check::arm();
    let result = change_master_password(&db, &good(), &new_password, &new_password);

    assert!(
        result.is_err(),
        "eine Verpackung, die K nicht zurückgibt, darf nie als Erfolg durchgehen"
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "die alte Verpackung muss unverändert an ihrem Platz liegen — sonst ist K weg"
    );
    assert!(
        !tmp.exists(),
        "die halbe Verpackung muss weg sein: sie trägt K unter dem neuen Passwort"
    );
    let recovered = unlock(&db, &good())
        .expect("das alte Passwort muss K weiter hergeben: der Wechsel ist nicht vollzogen");
    assert_eq!(recovered.expose(), &ROOT_KEY);
    // Und das neue Passwort gilt gerade **nicht** — halb vollzogen gibt es
    // nicht.
    assert!(unlock(&db, &new_password).is_err());
}

/// Fund 4: Nach einem **erfolgreichen** Schreiben bleibt keine `.new`-Datei
/// liegen.
///
/// Warum das zählt: Eine liegengebliebene `.new`-Datei ist eine
/// vollständige, gültige Verpackung von K — nach einem Passwortwechsel unter
/// dem jeweils anderen Passwort. Wer das Dateisystem lesen kann, greift dann
/// offline das schwächere der beiden an, statt des aktuellen.
#[test]
fn test_no_half_written_wrapping_is_left_behind() {
    let dir = Dir::new("no-leftovers");
    let db = dir.db();
    let tmp = {
        let mut name = wrapping_file_path(&db).as_os_str().to_os_string();
        name.push(".new");
        std::path::PathBuf::from(name)
    };

    set_up_master_password(
        &db,
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();
    assert!(!tmp.exists(), "nach dem Einrichten liegt kein Rest");

    let new_password = pw("ein-ganz-neues-master-passwort");
    change_master_password(&db, &good(), &new_password, &new_password).unwrap();
    assert!(
        !tmp.exists(),
        "nach dem Passwortwechsel liegt keine zweite, unter dem alten Passwort öffenbare \
         Verpackung von K daneben"
    );
    // Der Wechsel ist dabei wirklich vollzogen.
    assert_eq!(unlock(&db, &new_password).unwrap().expose(), &ROOT_KEY);
    assert!(unlock(&db, &good()).is_err());
}

/// Klarstellung 10b: Der Wechsel auf den Schlüsselbund **ersetzt keinen
/// fremden Schlüssel**, solange es niemand bestätigt hat.
///
/// Der Fall, der dahintersteht: Installation A liegt im
/// Schlüsselbund-Modus, ihr K_A liegt im einzigen Platz, den es je Benutzer
/// gibt. Datenverzeichnis B läuft im Passwort-Modus und wechselt zurück.
/// Ohne diese Prüfung ist K_A unwiederbringlich mit K_B überschrieben, und
/// As Datenbank ist beim nächsten Start nicht mehr zu öffnen (D2).
///
/// `tidy_up_keychain_after_unlock` schützt genau denselben Eintrag beim
/// Entsperren byteweise (A17) — die beiden Wege dürfen nicht
/// Verschiedenes tun.
#[test]
fn test_switching_back_does_not_replace_a_foreign_keychain_entry_without_confirmation() {
    let dir = Dir::new("switch-foreign");
    set_up_master_password(
        &dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();
    let wrapping_before = std::fs::read(wrapping_file_path(&dir.db())).unwrap();

    // Im Schlüsselbund liegt der Schlüssel einer **anderen** Installation.
    let keyring = CountingKeychain::with_root_key(&OTHER_KEY);

    assert!(
        matches!(
            switch_to_keychain(
                &dir.db(),
                &good(),
                &keyring,
                KeychainOverwrite::OnlyAfterConfirmation
            ),
            Err(MasterPasswordError::KeychainHoldsAnotherKey)
        ),
        "ohne Bestätigung muss der Wechsel mit einem eigenen Fehler abbrechen"
    );

    // „Ohne ausdrückliche Bestätigung bleibt alles, wie es war" — und zwar
    // auf allen vier Seiten.
    assert_eq!(
        *keyring.sets.lock().unwrap(),
        0,
        "kein `set`: der fremde Schlüssel darf nicht einmal kurz überschrieben werden"
    );
    assert_eq!(
        *keyring.deletes.lock().unwrap(),
        0,
        "und gelöscht wird auch nichts"
    );
    match ssh_manager_core::crypto::read_root_key(&keyring) {
        ssh_manager_core::crypto::RootKeyState::Present(key) => assert_eq!(
            key.expose(),
            &OTHER_KEY,
            "der fremde Schlüssel muss unverändert im Schlüsselbund liegen"
        ),
        other => panic!("der fremde Eintrag ist weg, war {other:?}"),
    }
    assert_eq!(key_mode(&dir.db()), KeyMode::Password);
    assert_eq!(
        std::fs::read(wrapping_file_path(&dir.db())).unwrap(),
        wrapping_before,
        "die Verpackung bleibt und gibt weiter K her"
    );

    // Mit Bestätigung geht derselbe Wechsel durch — sonst wäre die Prüfung
    // eine Sperre ohne Ausweg.
    switch_to_keychain(
        &dir.db(),
        &good(),
        &keyring,
        KeychainOverwrite::ConfirmedByTheUser,
    )
    .expect("mit Bestätigung muss der Wechsel gelingen");
    assert_eq!(key_mode(&dir.db()), KeyMode::Keychain);
    match ssh_manager_core::crypto::read_root_key(&keyring) {
        ssh_manager_core::crypto::RootKeyState::Present(key) => assert_eq!(key.expose(), &ROOT_KEY),
        other => panic!("nach der Bestätigung muss K dort liegen, war {other:?}"),
    }
}

/// Klarstellung 10b, nach dem Review (Runde 7): Wird ein fremder Schlüssel
/// **mit** Bestätigung ersetzt, sagt das eine eigene Zeile im Log.
///
/// Es ist der einzige unumkehrbare Schritt des Vorgangs. Sah er im Log aus
/// wie der Normalfall („switched back to the OS keychain"), ließe sich
/// hinterher nicht mehr feststellen, dass ein Schlüssel verloren ging — und
/// genau diese Frage stellt jemand, dessen zweite Installation plötzlich
/// nicht mehr aufgeht.
///
/// **Der Schlüssel selbst darf dabei nicht ins Log** (T17): deshalb hier
/// derselbe Abgleich wie dort.
#[test]
fn test_replacing_a_foreign_key_after_a_confirmation_is_visible_in_the_log() {
    use crate::test_support::{key_leak_needles, log_capture};

    log_capture::start_recording();

    let dir = Dir::new("switch-confirmed-log");
    set_up_master_password(
        &dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();
    let keyring = CountingKeychain::with_root_key(&OTHER_KEY);

    switch_to_keychain(
        &dir.db(),
        &good(),
        &keyring,
        KeychainOverwrite::ConfirmedByTheUser,
    )
    .unwrap();

    let log = log_capture::recorded_text();
    assert!(
        log.contains("replacing a different root key in the OS keychain after an explicit"),
        "das Ersetzen eines fremden Wurzelschlüssels muss im Log stehen, nicht nur der \
         gelungene Wechsel. Aufgezeichnet war: {log}"
    );
    key_leak_needles::assert_absent(
        &key_leak_needles::for_root_key(&ROOT_KEY, &["mein-master-passwort"], &[]),
        &[("das Log", &log)],
    );
    key_leak_needles::assert_absent(
        &key_leak_needles::for_root_key(&OTHER_KEY, &[], &[]),
        &[("das Log", &log)],
    );
}

/// Klarstellung 10b, die beiden Fälle **ohne** Frage: ein leerer
/// Schlüsselbund und einer, in dem schon genau K liegt („Ist der Eintrag
/// gleich K, entfällt die Frage").
///
/// Ohne diese Gegenprobe wäre „immer fragen" eine bestehende Umsetzung —
/// und der gewöhnliche Wechsel bräuchte einen Dialog, der nichts zu
/// entscheiden hat.
#[test]
fn test_switching_back_asks_nothing_when_the_keychain_is_empty_or_already_holds_k() {
    // Leer: der Normalfall nach `set_up_master_password`.
    let empty_dir = Dir::new("switch-empty");
    set_up_master_password(
        &empty_dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();
    let empty = CountingKeychain::default();
    switch_to_keychain(
        &empty_dir.db(),
        &good(),
        &empty,
        KeychainOverwrite::OnlyAfterConfirmation,
    )
    .expect("ein leerer Schlüsselbund hat nichts, wonach zu fragen wäre");
    assert_eq!(key_mode(&empty_dir.db()), KeyMode::Keychain);

    // Gleich K: der abgebrochene Wechsel aus A17, der beide Hälften liegen
    // ließ. Ihn zu „überschreiben" ändert nichts, also wird nicht gefragt.
    let same_dir = Dir::new("switch-same");
    set_up_master_password(
        &same_dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();
    let same = CountingKeychain::with_root_key(&ROOT_KEY);
    switch_to_keychain(
        &same_dir.db(),
        &good(),
        &same,
        KeychainOverwrite::OnlyAfterConfirmation,
    )
    .expect("derselbe Schlüssel ist kein fremder");
    assert_eq!(key_mode(&same_dir.db()), KeyMode::Keychain);
}

/// Klarstellung 10b, der Fall *ungültig*: Im Schlüsselbund liegt etwas, das
/// kein brauchbarer Schlüssel ist.
///
/// Auch das ist „nicht gleich K" — und wessen Eintrag dort liegt, ist von
/// hier aus nicht zu erkennen. Gefragt wird deshalb, statt zu raten; A17
/// behandelt denselben Zustand beim Entsperren genauso (nichts gelöscht).
#[test]
fn test_switching_back_asks_before_replacing_an_unusable_keychain_entry() {
    let dir = Dir::new("switch-invalid");
    set_up_master_password(
        &dir.db(),
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();

    let keyring = CountingKeychain::default();
    keyring.entries.lock().unwrap().insert(
        CHAT_CONTENT_ENCRYPTION_KEY_REF.to_string(),
        pw("kein-base64-und-kein-schluessel!!"),
    );

    assert!(matches!(
        switch_to_keychain(
            &dir.db(),
            &good(),
            &keyring,
            KeychainOverwrite::OnlyAfterConfirmation
        ),
        Err(MasterPasswordError::KeychainHoldsAnotherKey)
    ));
    assert_eq!(*keyring.sets.lock().unwrap(), 0);
    assert_eq!(key_mode(&dir.db()), KeyMode::Password);
}

/// Klarstellung 9: Die Urteilsfrage „kann diese Datei überhaupt K
/// hergeben?" — und zwar in **beide** Richtungen.
///
/// Die zweite Hälfte ist die wichtigere: Eine brauchbare Verpackung darf
/// **nie** „Neu anfangen" erlauben. Sonst wäre der Ausweg aus einer kaputten
/// Datei zugleich ein Knopf, der bei bloß vergessenem Passwort den Verlauf
/// wegwirft, obwohl K noch zu holen ist.
#[test]
fn test_wrapping_health_only_allows_starting_over_when_no_password_could_work() {
    let dir = Dir::new("health");
    let db = dir.db();

    // Ohne Datei: Schlüsselbund-Modus, kein Neuanfang.
    assert_eq!(wrapping_health(&db), WrappingHealth::Absent);
    assert!(!wrapping_health(&db).allows_starting_over());

    // Eine echte Verpackung: brauchbar — auch wenn das Passwort nicht passt.
    set_up_master_password(
        &db,
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();
    assert_eq!(wrapping_health(&db), WrappingHealth::Usable);
    assert!(
        !wrapping_health(&db).allows_starting_over(),
        "eine brauchbare Verpackung darf nie in „Neu anfangen“ führen — das wäre bei bloß \
         vergessenem Passwort der Verlust von K"
    );
    assert!(matches!(
        unlock(&db, &pw("ein-ganz-anderes-passwort")),
        Err(MasterPasswordError::WrongPasswordOrDamagedFile)
    ));
    // Und sie bleibt brauchbar, nachdem jemand falsch geraten hat.
    assert_eq!(wrapping_health(&db), WrappingHealth::Usable);

    // Eine fremde Datei am Ort der Verpackung: unbrauchbar → Ausweg.
    std::fs::write(wrapping_file_path(&db), [0xAB; 40]).unwrap();
    assert_eq!(wrapping_health(&db), WrappingHealth::Unusable);
    assert!(wrapping_health(&db).allows_starting_over());
    // Der Modus hängt weiter an der Datei — ohne den Ausweg wäre die
    // Installation damit dauerhaft ausgesperrt.
    assert_eq!(key_mode(&db), KeyMode::Password);
}

/// Klarstellung 9: Eine Verpackung, die zwar ein gültiges Chiffrat trägt,
/// deren **Parameter** aber unbrauchbar sind, ist *ungültig* — nicht
/// „Passwort falsch".
///
/// Der Fall ist der, den jemand mit Schreibzugriff herstellt: `m_cost`
/// senken, damit ein Offline-Angriff billig wird. Die App lehnt die Datei
/// dann ab (T15) — und muss einen Ausweg anbieten, sonst ist die
/// Installation damit lahmgelegt.
#[test]
fn test_weak_parameters_count_as_unusable_not_as_a_wrong_password() {
    let dir = Dir::new("health-params");
    let db = dir.db();
    set_up_master_password(
        &db,
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();

    let path = wrapping_file_path(&db);
    let mut bytes = std::fs::read(&path).unwrap();
    // `m_cost` (u32 little endian) auf 8 KiB herunterschrauben — Offset 10
    // laut Format (s. `key_wrapping`).
    bytes[10..14].copy_from_slice(&8u32.to_le_bytes());
    std::fs::write(&path, &bytes).unwrap();

    assert_eq!(wrapping_health(&db), WrappingHealth::Unusable);
    assert!(wrapping_health(&db).allows_starting_over());
    assert!(
        matches!(
            unlock(&db, &good()),
            Err(MasterPasswordError::UnusableWrappingFile)
        ),
        "geschwächte Parameter sind ein Formatfehler, nicht „Passwort falsch“ (A17)"
    );
}

/// Klarstellung 10a, **Fehlerart „hängende Verknüpfung"**: Die Datei liegt
/// an ihrem Platz, ihr Ziel gibt es nicht — *nicht erreichbar* (D1), nicht
/// *ungültig*.
///
/// Dass hier `NotFound` herauskommt, macht es nicht zu „es gibt keine
/// Verpackung": `key_mode` sieht die Verknüpfung und sagt `Password`. Käme
/// `NotInPasswordMode` heraus, widersprächen sich beide Funktionen über
/// denselben Zustand.
#[cfg(unix)]
#[test]
fn test_a_dangling_symlink_at_the_wrapping_path_is_unreachable_not_invalid() {
    let dir = Dir::new("health-dangling");
    let db = dir.db();
    std::os::unix::fs::symlink(dir.path.join("gibt-es-nicht"), wrapping_file_path(&db)).unwrap();

    assert_eq!(key_mode(&db), KeyMode::Password);
    assert_eq!(wrapping_health(&db), WrappingHealth::Unreachable);
    assert!(
        !wrapping_health(&db).allows_starting_over(),
        "A3 führt *nicht erreichbar* nach D1 — ein neuer K entsteht hier nicht \
         (Klarstellung 10a)"
    );
    assert!(
        matches!(
            unlock(&db, &good()),
            Err(MasterPasswordError::WrappingFileUnreachable { .. })
        ),
        "die Fehlerart muss „nicht lesbar“ sein, nicht „kein Master-Passwort eingerichtet“"
    );
}

/// Klarstellung 10a, **Fehlerart „Rechte"**: `chmod 000` auf eine
/// vollkommen gültige Verpackung. Genau der Fall, den der Review-Lauf 6 als
/// Abweichung gefunden hat.
///
/// Der Inhalt ist in Ordnung — nur das Leserecht fehlt. Früher führte das
/// in „Neu anfangen" und damit dazu, dass Datenbank, `-wal`, `-shm` **und
/// die intakte Verpackung** umbenannt wurden und ein neuer K entstand. A3
/// verlangt für diese Spalte D1: erneut versuchen, nichts verändern.
#[cfg(unix)]
#[test]
fn test_a_wrapping_file_without_read_permission_is_unreachable_not_invalid() {
    use std::os::unix::fs::PermissionsExt;

    let dir = Dir::new("health-perm");
    let db = dir.db();
    set_up_master_password(
        &db,
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();
    let path = wrapping_file_path(&db);
    let intact = std::fs::read(&path).unwrap();

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Als `root` greift `chmod` nicht — dann prüfte der Test nichts, und das
    // soll man sehen, statt es für ein Grün zu nehmen.
    if std::fs::read(&path).is_ok() {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        eprintln!("übersprungen: dieser Benutzer darf die Datei trotz 0000 lesen (root?)");
        return;
    }

    assert_eq!(key_mode(&db), KeyMode::Password);
    assert_eq!(wrapping_health(&db), WrappingHealth::Unreachable);
    assert!(
        !wrapping_health(&db).allows_starting_over(),
        "ein entzogenes Leserecht darf nicht in „Neu anfangen“ führen: der Inhalt ist in \
         Ordnung, und ein neuer K machte den bisherigen Verlauf unlesbar (Klarstellung 10a)"
    );
    assert!(matches!(
        unlock(&db, &good()),
        Err(MasterPasswordError::WrappingFileUnreachable { .. })
    ));

    // „Nichts verändern" wörtlich: Die Datei ist nach dem Urteil dieselbe,
    // und mit dem Leserecht gibt sie K wieder her.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), intact);
    assert_eq!(wrapping_health(&db), WrappingHealth::Usable);
    assert_eq!(unlock(&db, &good()).unwrap().expose(), &ROOT_KEY);
}

/// Klarstellung 10a, **Fehlerart „etwas anderes als eine Datei"**: Ein
/// Verzeichnis am Ort der Verpackung.
///
/// Der Fall steht für jeden Lesefehler, der kein `NotFound` und kein
/// Rechtefehler ist (E/A-Fehler, ein von einem anderen Programm gehaltenes
/// Handle unter Windows) — keiner davon ist ein Urteil über den Inhalt, und
/// alle gehen durch denselben Zweig. Er läuft auf allen Plattformen.
#[test]
fn test_a_directory_at_the_wrapping_path_is_unreachable_not_invalid() {
    let dir = Dir::new("health-dir");
    let db = dir.db();
    std::fs::create_dir(wrapping_file_path(&db)).unwrap();

    assert_eq!(key_mode(&db), KeyMode::Password);
    assert_eq!(wrapping_health(&db), WrappingHealth::Unreachable);
    assert!(!wrapping_health(&db).allows_starting_over());
    assert!(matches!(
        unlock(&db, &good()),
        Err(MasterPasswordError::WrappingFileUnreachable { .. })
    ));
}

/// Klarstellung 10a, die **Gegenprobe zu den drei Tests darüber**: Ein
/// *gelesener*, aber unbrauchbarer Inhalt bleibt *ungültig* und behält den
/// Ausweg. Ohne diese Aussage wäre „alles ist *nicht erreichbar*" eine
/// bestehende Umsetzung — und die Installation mit einer zerstörten
/// Verpackung dauerhaft ausgesperrt.
#[test]
fn test_a_read_but_broken_wrapping_file_stays_invalid_and_keeps_the_way_out() {
    let dir = Dir::new("health-kinds");
    for (what, bytes) in [
        ("fremder Inhalt", vec![0xAB; 40]),
        ("leere Datei", Vec::new()),
        ("abgeschnittener Kopf", vec![0x01; 8]),
    ] {
        let db = dir.path.join(format!("kind-{}.db", bytes.len()));
        std::fs::write(wrapping_file_path(&db), &bytes).unwrap();
        assert_eq!(
            wrapping_health(&db),
            WrappingHealth::Unusable,
            "{what} wurde gelesen und gibt kein K her — das ist A3 *ungültig*"
        );
        assert!(
            wrapping_health(&db).allows_starting_over(),
            "{what}: ohne Ausweg wäre die Installation dauerhaft ausgesperrt"
        );
    }
}

/// T17 (Spec 0101 §7, ERHÖHT), Passwort-Modus: **Weder das Log noch das
/// Diagnosepaket tragen K, den Datenbankschlüssel, das Master-Passwort oder
/// ein Secret** — gefahren über den ganzen Modus (einrichten, falsches
/// Passwort, entsperren, Schlüsselbund aufräumen, Passwort ändern, zurück
/// auf den Schlüsselbund, unbrauchbare Datei).
///
/// **Warum echtes Log und nicht nur Fehlertexte:** `test_no_error_text_
/// carries_secret_material` prüft die Texte, die der Nutzer sieht. T17 fragt
/// nach dem, was auf die Platte geht. Aufgezeichnet wird deshalb der
/// tatsächliche `tracing`-Strom dieses Threads im selben JSON-Format, das
/// `logging::init_logging` schreibt, und genau dieser Strom geht danach in
/// `build_diagnostics_bundle` — dieselbe Form, in der
/// `logging::read_last_log_lines` ihn dem Diagnose-Kommando liefert.
///
/// **Zwei Aussagen über das Paket**, und beide sind nötig: Die Positivliste
/// aus Spec 0063 kennt keine Zeile dieses Moduls, das Paket lässt sie also
/// alle draußen (erste Aussage, fail-closed). Käme je eine davon auf die
/// Liste, bliebe die zweite Aussage — keiner der Suchbegriffe im Paket —
/// als Wächter übrig.
///
/// Die drei Zeilen, die der Test selbst schreibt, sind ausdrücklich
/// gekennzeichnet: Sie bilden die Produktiv-Aufrufstellen in
/// `app_shell::commands::master_password` nach (`detail_for_log`, `?state`,
/// `?health`), die aus diesem Crate nicht erreichbar sind.
#[test]
fn test_t17_no_key_password_or_secret_in_the_log_or_the_diagnostics_bundle() {
    use crate::diagnostics::{build_diagnostics_bundle, DiagnosticsInput};
    use crate::test_support::{key_leak_needles, log_capture};
    use ssh_manager_core::ai::DefaultOutputRedactor;
    use ssh_manager_core::crypto::DatabaseKey;

    /// Nicht `[0x11; 32]` wie oben: ein Schlüssel mit lauter verschiedenen
    /// Bytes, damit auch ein **halb** durchgesickerter Schlüssel auffällt
    /// (bei einem Wiederholungsmuster wäre jede Teilfolge dieselbe).
    const T17_KEY: [u8; 32] = [
        0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce,
        0xcf, 0xd0, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd,
        0xde, 0xdf,
    ];
    const T17_PASSWORD: &str = "Passwort-0101-Mondlicht";
    const T17_NEW_PASSWORD: &str = "Passwort-0101-Sonnenwind";
    const T17_SECRET: &str = "Secret-0101";

    log_capture::start_recording();

    let dir = Dir::new("t17-log");
    let db = dir.db();
    let keyring = CountingKeychain::with_root_key(&T17_KEY);
    // Ein Secret, das nichts mit K zu tun hat, im selben Speicher: Wer den
    // Speicher oder ein `get`-Ergebnis als Ganzes loggte, nähme es mit.
    keyring.entries.lock().unwrap().insert(
        "server/0101/password".to_string(),
        SecretString::from(T17_SECRET.to_string()),
    );

    let password = pw(T17_PASSWORD);
    let new_password = pw(T17_NEW_PASSWORD);

    // T13: einrichten.
    set_up_master_password(
        &db,
        &ssh_manager_core::crypto::RootKey::for_tests(T17_KEY),
        &password,
        &password,
        CONFIRMED,
        Some(&keyring),
    )
    .unwrap();

    // A17: falsches Passwort — und die Fehler-Nutzlast so ins Log, wie
    // `to_command_error` es tut.
    let wrong = unlock(&db, &pw("Passwort-0101-falsch-aber-lang")).unwrap_err();
    if let Some(detail) = wrong.detail_for_log() {
        tracing::warn!(detail, "nachgebildete Aufrufstelle: to_command_error");
    }
    tracing::warn!(error = %wrong, "nachgebildete Aufrufstelle: to_command_error");

    // A16/A17: entsperren und den Schlüsselbund aufräumen. Dafür ein
    // Schlüsselbund, in dem K noch liegt — der oben ist nach dem Einrichten
    // leer; genau das ist der abgebrochene Wechsel aus T16.
    let root_key = unlock(&db, &password).unwrap();
    assert_eq!(root_key.expose(), &T17_KEY, "sonst prüft der Rest nichts");
    let aborted_switch = CountingKeychain::with_root_key(&T17_KEY);
    assert_eq!(
        tidy_up_keychain_after_unlock(&aborted_switch, &root_key),
        KeychainTidyResult::Deleted
    );

    // Die Stelle, an der ein `#[derive(Debug)]` auf `RootKeyState` K ins
    // Log schriebe — `app_shell::commands::master_password` loggt `?other`
    // genau so. Hier mit `Present`, also dem einzigen Fall, der K trägt.
    let keyring_with_key = CountingKeychain::with_root_key(&T17_KEY);
    let state = ssh_manager_core::crypto::read_root_key(&keyring_with_key);
    tracing::warn!(?state, "nachgebildete Aufrufstelle: ?state im Kommando");

    // T16: ein abweichender Eintrag wird nicht gelöscht (eigene Log-Zeile).
    let other_keyring = CountingKeychain::with_root_key(&OTHER_KEY);
    assert_eq!(
        tidy_up_keychain_after_unlock(&other_keyring, &root_key),
        KeychainTidyResult::LeftAlone
    );

    // A15: Passwort ändern und zurück auf einen Schlüsselbund, der nicht
    // antwortet (`KeychainFailed` mit Nutzlast im `detail`).
    change_master_password(&db, &password, &new_password, &new_password).unwrap();
    let unreadable = CountingKeychain {
        fail_read: true,
        ..Default::default()
    };
    let failed = switch_to_keychain(
        &db,
        &new_password,
        &unreadable,
        KeychainOverwrite::OnlyAfterConfirmation,
    )
    .unwrap_err();
    if let Some(detail) = failed.detail_for_log() {
        tracing::warn!(detail, "nachgebildete Aufrufstelle: to_command_error");
    }

    // Klarstellung 9: eine unbrauchbare Datei beurteilen (eigene Log-Zeile).
    let junk_dir = Dir::new("t17-junk");
    std::fs::write(wrapping_file_path(&junk_dir.db()), b"keine Verpackung").unwrap();
    let health = wrapping_health(&junk_dir.db());
    assert_eq!(health, WrappingHealth::Unusable);
    tracing::warn!(?health, "nachgebildete Aufrufstelle: ?health im Kommando");

    let log = log_capture::recorded_text();

    // Zuerst der Beweis, dass überhaupt aufgezeichnet wurde: ohne ihn wäre
    // jede Abwesenheits-Aussage unten wertlos.
    for expected in [
        "master password set up",
        "removed the root key from the OS keychain after unlocking",
        "differs from the unwrapped one",
        "master password changed",
        "cannot yield the root key with any password",
        "nachgebildete Aufrufstelle",
    ] {
        assert!(
            log.contains(expected),
            "die Aufzeichnung hat „{expected}“ nicht gesehen — der Test prüfte nichts"
        );
    }

    let log_lines: Vec<String> = log.lines().map(str::to_string).collect();
    let bundle = build_diagnostics_bundle(
        &DiagnosticsInput {
            version_display: "0.5.1 (test)".to_string(),
            build_type: "Dev",
            edition: "Community".to_string(),
            os: "macos",
            arch: "aarch64",
            os_version: Some("15.1".to_string()),
            db_path: db.display().to_string(),
            log_dir: dir.path.display().to_string(),
            host_key_path: dir.path.join("host_keys.json").display().to_string(),
            provider_types: Some(vec!["anthropic".to_string()]),
            server_count: Some(1),
        },
        &log_lines,
        &DefaultOutputRedactor::new(),
    );

    // Spec 0063, Positivliste: keine Zeile dieses Moduls gehört ins Paket.
    assert!(
        !bundle.contains("master password set up"),
        "die Positivliste des Diagnosepakets darf keine Zeile dieses Moduls durchlassen"
    );

    // Die Suchbegriffe kommen aus `key_leak_needles` — dieselbe Liste, die
    // die T17-Tests der Pfade T1, T3 und T11 verwenden (Klarstellung 10d).
    key_leak_needles::assert_absent(
        &key_leak_needles::for_root_key(&T17_KEY, &[T17_PASSWORD, T17_NEW_PASSWORD], &[T17_SECRET]),
        &[("das Log", &log), ("das Diagnosepaket", &bundle)],
    );

    // Und die beiden Typen, über die es am kürzesten gehen würde: ein
    // `{:?}` auf dem Schlüssel selbst.
    assert_eq!(
        format!(
            "{:?}",
            DatabaseKey::from_root_key(&ssh_manager_core::crypto::RootKey::for_tests(T17_KEY))
        ),
        "DatabaseKey(<nicht anzeigbar>)"
    );
    assert_eq!(
        format!("{root_key:?}"),
        "RootKey(<nicht anzeigbar>)",
        "RootKey darf K nicht über Debug zeigen"
    );
}

/// #266 (1): Ein Link am `.new`-Pfad wird nicht verfolgt — das Schreiben
/// scheitert, und das Ziel bleibt unverändert.
#[cfg(unix)]
#[test]
fn test_a_symlink_at_the_temporary_path_fails_the_write_and_keeps_its_target() {
    let dir = Dir::new("tmp-symlink");
    let db = dir.db();
    let victim = dir.path.join("victim");
    std::fs::write(&victim, b"unberuehrt").unwrap();
    let tmp = temporary_path(&wrapping_file_path(&db));
    std::os::unix::fs::symlink(&victim, &tmp).unwrap();

    let result = set_up_master_password(
        &db,
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    );
    assert!(
        matches!(result, Err(MasterPasswordError::FileFailed { .. })),
        "{result:?}"
    );
    assert_eq!(std::fs::read(&victim).unwrap(), b"unberuehrt");
    assert!(
        !wrapping_file_path(&db).exists(),
        "es darf keine Verpackungsdatei entstanden sein"
    );
}

/// #266 (1): Eine liegengebliebene reguläre `.new`-Datei blockiert nicht.
#[test]
fn test_a_stale_temporary_file_does_not_block_a_later_write() {
    let dir = Dir::new("tmp-stale");
    let db = dir.db();
    let tmp = temporary_path(&wrapping_file_path(&db));
    std::fs::write(&tmp, b"Rest eines abgebrochenen Schreibens").unwrap();
    // Schreibgeschützt: Das alte `truncate` scheiterte daran, das explizite
    // Entfernen nicht (Gegenbeweis gegen den Stand vor #266).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o400)).unwrap();
    }

    set_up_master_password(
        &db,
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();
    assert_eq!(unlock(&db, &good()).unwrap().expose(), &ROOT_KEY);
    assert!(!tmp.exists());
}

/// #266 (4): Ein Fehler von `symlink_metadata`, der kein `NotFound` ist,
/// heißt nicht „keine Datei“.
#[cfg(unix)]
#[test]
fn test_a_metadata_error_other_than_not_found_is_not_read_as_absent() {
    use std::os::unix::fs::PermissionsExt;

    let dir = Dir::new("meta-err");
    let sub = dir.path.join("sub");
    std::fs::create_dir(&sub).unwrap();
    let db = sub.join("smart-ssh.db");
    set_up_master_password(
        &db,
        &ssh_manager_core::crypto::RootKey::for_tests(ROOT_KEY),
        &good(),
        &good(),
        CONFIRMED,
        None,
    )
    .unwrap();

    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o000)).unwrap();
    let restore = || std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o700));
    if std::fs::symlink_metadata(wrapping_file_path(&db)).is_ok() {
        restore().unwrap();
        eprintln!("übersprungen: dieser Benutzer darf trotz 0000 hinein (root?)");
        return;
    }

    let health = wrapping_health(&db);
    let mode = key_mode(&db);
    restore().unwrap();
    assert_eq!(health, WrappingHealth::Unreachable);
    assert_eq!(mode, KeyMode::Password);
}
