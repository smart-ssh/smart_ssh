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
    set_up_master_password(&db, &ROOT_KEY, &good(), &good(), None).unwrap();

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

    set_up_master_password(&db, &ROOT_KEY, &good(), &good(), None).unwrap();
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
    set_up_master_password(&db, &ROOT_KEY, &good(), &good(), None).unwrap();
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
    set_up_master_password(&db, &ROOT_KEY, &good(), &good(), None).unwrap();

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

/// Klarstellung 9: Eine Datei, die an ihrem Platz liegt, aber nicht zu lesen
/// ist (hängende Verknüpfung), ist ebenfalls *ungültig*.
///
/// Vor dieser Änderung endete derselbe Zustand in `NotInPasswordMode` bzw.
/// `FileFailed` — und damit in einem Fehler ohne Ausweg, obwohl der Modus
/// weiter `Password` war.
#[cfg(unix)]
#[test]
fn test_an_unreadable_wrapping_file_is_a_way_out_too() {
    let dir = Dir::new("health-dangling");
    let db = dir.db();
    std::os::unix::fs::symlink(dir.path.join("gibt-es-nicht"), wrapping_file_path(&db)).unwrap();

    assert_eq!(key_mode(&db), KeyMode::Password);
    assert_eq!(wrapping_health(&db), WrappingHealth::Unreadable);
    assert!(
        wrapping_health(&db).allows_starting_over(),
        "sonst sperrt eine hängende Verknüpfung die Installation dauerhaft aus"
    );
}
