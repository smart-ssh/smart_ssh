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
    /// Die Datei ließ sich nicht schreiben oder ersetzen.
    FileFailed { detail: String },
    /// Die Verpackungsdatei liegt an ihrem Platz, ließ sich aber nicht
    /// **lesen** — in der Tabelle A3 der Zustand *nicht erreichbar* (D1,
    /// Klarstellung 10a).
    ///
    /// Ein eigener Fehler und nicht [`Self::FileFailed`]: Der Text für den
    /// Nutzer ist ein anderer („nicht lesbar, versuche es erneut“ statt
    /// „ließ sich nicht schreiben“), und es ist der Fall, in dem **nichts**
    /// verändert wurde.
    WrappingFileUnreachable { detail: String },
    /// Der Schlüsselbund hat nicht geantwortet (A15: dann wird die
    /// Verpackungsdatei **nicht** entfernt).
    KeychainFailed { detail: String },
    /// A15/Klarstellung 10b: Im Schlüsselbund liegt schon ein Schlüssel,
    /// und es ist **nicht** der, der gerade gilt.
    ///
    /// Der Wechsel bricht hier ab, ohne irgendetwas anzufassen. Erst eine
    /// ausdrückliche Bestätigung
    /// ([`KeychainOverwrite::ConfirmedByTheUser`]) ersetzt den fremden
    /// Eintrag.
    KeychainHoldsAnotherKey,
    /// Es gibt keine Verpackungsdatei — der Vorgang passt nicht zum Modus.
    NotInPasswordMode,
    /// Es gibt schon eine Verpackungsdatei.
    AlreadyInPasswordMode,
    /// Klarstellung 12 (A13/E10): Die Warnung „ohne Passwort sind alle Daten
    /// verloren" ist nicht ausdrücklich bestätigt.
    ///
    /// Es wird nichts verändert — der Riegel sitzt vor jedem Schreibzugriff.
    LossWarningNotConfirmed,
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
            Self::WrappingFileUnreachable { .. } => write!(
                f,
                "die Schlüsseldatei neben der Datenbank ist gerade nicht lesbar — es wurde \
                 nichts verändert; versuche es erneut"
            ),
            Self::KeychainFailed { .. } => {
                write!(f, "der Schlüsselbund hat nicht geantwortet")
            }
            // Der Wortlaut aus Klarstellung 10b. Die Frage selbst stellt
            // die Oberfläche (Commit 11); bis dahin ist dies der Text zum
            // Code.
            Self::KeychainHoldsAnotherKey => write!(
                f,
                "im Schlüsselbund liegt ein anderer Schlüssel. Backups, die mit diesem \
                 Schlüssel verschlüsselt sind, werden danach unlesbar. Es ist nichts verändert"
            ),
            Self::NotInPasswordMode => write!(f, "es ist kein Master-Passwort eingerichtet"),
            Self::AlreadyInPasswordMode => {
                write!(f, "es ist schon ein Master-Passwort eingerichtet")
            }
            // Klarstellung 12: Der Wortlaut sagt beides — was fehlt und
            // dass nichts passiert ist.
            Self::LossWarningNotConfirmed => write!(
                f,
                "die Warnung zum Master-Passwort ist nicht bestätigt. Ohne dieses Passwort sind \
                 alle Daten verloren, und es gibt keine Wiederherstellung — bestätige das \
                 ausdrücklich. Es ist nichts verändert"
            ),
        }
    }
}

impl std::error::Error for MasterPasswordError {}

impl MasterPasswordError {
    /// Die Bibliotheks-Nutzlast für das Log — nie für einen Dialog oder ein
    /// DTO (§6, Spec 0098 A5).
    pub fn detail_for_log(&self) -> Option<&str> {
        match self {
            Self::FileFailed { detail }
            | Self::WrappingFileUnreachable { detail }
            | Self::KeychainFailed { detail } => Some(detail),
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

/// Spec 0101, Klarstellung 9: Kann die Verpackungsdatei **überhaupt** K
/// hergeben?
///
/// Die Frage ist nötig, weil „Neu anfangen" im Passwort-Modus einen neuen K
/// erzeugt und damit den bisherigen Verlauf aufgibt. Angeboten werden darf
/// es deshalb **nur**, wenn kein Passwort der Welt die vorhandene Datei
/// öffnen würde — sonst wäre der Knopf ein Weg, sich bei bloß vergessenem
/// Passwort selbst die Daten zu löschen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrappingHealth {
    /// Es gibt keine Verpackungsdatei — Schlüsselbund-Modus.
    Absent,
    /// Kopf und Parameter sind brauchbar. Scheitert das Entsperren, war es
    /// das Passwort (oder eine Veränderung am Chiffrat, A17: nicht
    /// unterscheidbar) — **kein** Grund für „Neu anfangen".
    Usable,
    /// Die Datei **wurde gelesen**, und Marke, Formatversion, KDF-Kennung,
    /// Parameter oder die Länge des Chiffrats sind unbrauchbar. In der
    /// Tabelle A3 der Zustand *ungültig* (D3).
    Unusable,
    /// Die Datei liegt an ihrem Platz, ihr Inhalt ließ sich aber **nicht
    /// lesen** — Rechte, E/A-Fehler, von einem anderen Programm gesperrt,
    /// eine hängende Verknüpfung.
    ///
    /// In der Tabelle A3 der Zustand *nicht erreichbar* (D1): „Erneut
    /// versuchen“ / „Beenden“, **nichts verändern** (Klarstellung 10a).
    /// Über den Inhalt ist damit nichts gesagt — die Datei kann vollkommen
    /// in Ordnung sein und morgen wieder lesbar. Sie als *ungültig* zu
    /// behandeln hieße, einen Rechte- oder E/A-Fehler mit dem Aufgeben von
    /// K zu beantworten.
    Unreachable,
}

impl WrappingHealth {
    /// Darf „Neu anfangen" (D3/D4) im gesperrten Zustand angeboten werden?
    ///
    /// **Nur** [`Self::Unusable`] — also nur, wenn die Datei gelesen wurde
    /// und ihr Inhalt kein K hergibt. Die Positivliste-Richtung ist hier
    /// Absicht: Ein künftiger Zustand, den niemand bedacht hat, führt
    /// **nicht** zu einem neuen K.
    pub fn allows_starting_over(self) -> bool {
        matches!(self, Self::Unusable)
    }
}

/// Spec 0101, Klarstellung 9/10a: liest die Verpackungsdatei und urteilt
/// über sie, **ohne** ein Passwort zu brauchen.
///
/// Rührt nichts an: nur `symlink_metadata` und `read`.
///
/// **Die Fehlerart entscheidet** (Klarstellung 10a, A3): Ein gescheitertes
/// `read` sagt nichts über den Inhalt der Datei und führt deshalb nach D1,
/// nicht nach D3. Nur ein *gelesener* Inhalt kann *ungültig* sein.
pub fn wrapping_health(db_path: &Path) -> WrappingHealth {
    let path = wrapping_file_path(db_path);
    if std::fs::symlink_metadata(&path).is_err() {
        return WrappingHealth::Absent;
    }
    match std::fs::read(&path) {
        Ok(bytes) => match crypto::inspect_wrapped_key(&bytes) {
            Ok(()) => WrappingHealth::Usable,
            Err(err) => {
                tracing::warn!(
                    reason = %err,
                    "the wrapping file next to the database cannot yield the root key with any \
                     password (Spec 0101, A3 „ungültig“)"
                );
                WrappingHealth::Unusable
            }
        },
        // Die Datei ist über `symlink_metadata` da, lässt sich aber nicht
        // lesen — eine hängende Verknüpfung ergibt hier `NotFound`, ein
        // entzogenes Leserecht `PermissionDenied`, ein von einem anderen
        // Programm gehaltenes Handle unter Windows `PermissionDenied`
        // (os error 32), ein Verzeichnis an diesem Ort `IsADirectory`.
        // **Keiner dieser Fälle** ist ein Urteil über den Inhalt.
        Err(err) => {
            tracing::warn!(
                detail = %err,
                "the wrapping file next to the database exists but cannot be read; nothing is \
                 changed and the user may retry (Spec 0101, A3 „nicht erreichbar“, \
                 Klarstellung 10a)"
            );
            WrappingHealth::Unreachable
        }
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

/// Ist die Warnung aus A13/E10 ausdrücklich bestätigt? (Klarstellung 12)
///
/// **Ein eigener Typ und kein `bool`**, aus demselben Grund wie bei
/// [`KeychainOverwrite`]: An der Aufrufstelle soll stehen, *was* bestätigt
/// wurde, und die Voreinstellung muss die vorsichtige sein. Ein `bool`
/// würde beim Durchreichen durch drei Schichten zu `false` verrutschen,
/// ohne dass es jemand liest — und `true` wäre an der Aufrufstelle nicht
/// zuzuordnen.
///
/// **Warum das Backend es überhaupt prüft** (Klarstellung 12,
/// Q-BL-0314-02, K3 entschieden): Länge und Wiederholung prüft es längst
/// doppelt, weil ein Kommandoaufruf die Maske umgeht. Für die Bestätigung
/// galt das nicht — das Häkchen hielt allein die Oberfläche, und ein
/// direkter Aufruf von `set_up_master_password` richtete ein
/// Master-Passwort ein, ohne dass die Warnung je gelesen worden sein muss.
/// Nach E10 gibt es danach keine Wiederherstellung; genau dieser Schritt
/// braucht die Zusage am wenigsten umgehbaren Ort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossWarning {
    /// Standard: Es wird kein Master-Passwort eingerichtet und nichts
    /// verändert.
    NotConfirmed,
    /// Der Nutzer hat die Warnung gelesen und ausdrücklich bestätigt.
    ConfirmedByTheUser,
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
    warning: LossWarning,
    delete_from_keychain: Option<&dyn CredentialStore>,
) -> Result<(), MasterPasswordError> {
    // **Der erste Riegel, vor allem anderen** (Klarstellung 12): „Jeder
    // Weg, der ein Master-Passwort einrichtet, lehnt ohne diese Bestätigung
    // ab und verändert nichts." Hier ist der eine Ort, durch den alle drei
    // Wege gehen (Einstellungen, D1, A5/D4 im Passwort-Modus) — kein
    // Aufrufer kann ihn überspringen, und der Typ zwingt jeden, die Frage
    // zu beantworten.
    check_loss_warning(warning)?;
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

/// Darf [`switch_to_keychain`] einen **fremden** Schlüssel im Schlüsselbund
/// ersetzen? (A15/Klarstellung 10b)
///
/// Ein eigener Typ und kein `bool`: Am Aufruf soll stehen, *was* bestätigt
/// wurde. `switch_to_keychain(…, true)` wäre an der Aufrufstelle nicht
/// lesbar, und die Voreinstellung muss die vorsichtige sein.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeychainOverwrite {
    /// Standard: Liegt dort ein anderer Schlüssel, bricht der Wechsel ab
    /// und es wird nichts verändert.
    OnlyAfterConfirmation,
    /// Der Nutzer hat das Ersetzen ausdrücklich bestätigt, nachdem ihm die
    /// Folge genannt wurde.
    ConfirmedByTheUser,
}

/// A15, erste Hälfte: zurück auf den Schlüsselbund — „aktuelles Passwort, K
/// in den Schlüsselbund schreiben, zurücklesen, vergleichen, dann
/// Verpackungsdatei entfernen".
///
/// Scheitert eine der drei ersten Stufen, bleibt die Verpackungsdatei
/// liegen und der Modus damit unverändert.
///
/// **Vorher wird gelesen** (Klarstellung 10b): Es gibt genau einen Platz je
/// Benutzer für K im Schlüsselbund. Liegt dort schon ein **anderer**
/// Schlüssel, gehört er zu irgendetwas anderem — einer zweiten
/// Installation, einem Backup, einem Verlauf, der sonst unlesbar wird. Ihn
/// zu überschreiben ist unwiderruflich und braucht deshalb eine
/// ausdrückliche Bestätigung. `tidy_up_keychain_after_unlock` schützt genau
/// diesen fremden Eintrag beim Entsperren byteweise (A17); der Wechsel darf
/// nicht das Gegenteil tun.
pub fn switch_to_keychain(
    db_path: &Path,
    current: &SecretString,
    keyring: &dyn CredentialStore,
    overwrite: KeychainOverwrite,
) -> Result<(), MasterPasswordError> {
    let root_key = unlock(db_path, current)?;

    // **Gelesen wird immer**, auch mit Bestätigung (spec-reviewer Runde 7):
    // Das Ersetzen eines fremden Wurzelschlüssels ist der einzige
    // unumkehrbare Schritt dieses Vorgangs, und er darf im Log nicht wie
    // der Normalfall aussehen. Ohne die Lesung gäbe es keine Zeile, die ihn
    // benennt.
    match crypto::read_root_key(keyring) {
        // Gleich K: „Ist der Eintrag gleich K, entfällt die Frage."
        crypto::RootKeyState::Present(existing) if &existing == root_key.expose() => {}
        crypto::RootKeyState::NotFound => {}
        // Ein anderer Schlüssel — und ein unbrauchbarer Eintrag ist auch
        // „nicht gleich K". Beides fragt nach, statt zu raten, wessen
        // Eintrag dort liegt.
        crypto::RootKeyState::Present(_) | crypto::RootKeyState::Invalid => {
            if overwrite == KeychainOverwrite::OnlyAfterConfirmation {
                tracing::warn!(
                    "refusing to switch back to the OS keychain: it already holds a different \
                     root key, and replacing it would make anything encrypted with that key \
                     unreadable; nothing was changed (Spec 0101, A15, Klarstellung 10b)"
                );
                return Err(MasterPasswordError::KeychainHoldsAnotherKey);
            }
            tracing::warn!(
                "replacing a different root key in the OS keychain after an explicit \
                 confirmation; anything that was encrypted with the previous key stays \
                 unreadable (Spec 0101, A15, Klarstellung 10b)"
            );
        }
        // Nicht erreichbar: Dann ist **unbekannt**, ob dort etwas liegt —
        // und unbekannt ist kein Grund zu schreiben. Der Wechsel scheiterte
        // ohnehin am Zurücklesen; jetzt scheitert er davor, also ohne `set`.
        // Auch mit Bestätigung: Bestätigt wurde das Ersetzen eines
        // *bekannten* Eintrags, nicht das Schreiben ins Ungewisse.
        //
        // `{other:?}` und nicht `{reason:?}`: Die `Debug`-Ausgabe von
        // `RootKeyState` verbirgt den Bibliothekstext absichtlich; den
        // ausgepackten Grund roh zu formatieren umgeht genau diesen Schutz
        // (spec-reviewer Runde 7).
        other @ crypto::RootKeyState::Unreachable(_) => {
            return Err(MasterPasswordError::KeychainFailed {
                detail: format!("Schlüsselbund vor dem Wechsel nicht lesbar: {other:?}"),
            })
        }
    }

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
    let path = wrapping_file_path(db_path);
    let tmp = temporary_path(&path);
    let wrapped = crypto::wrap_root_key(root_key, password).map_err(classify_unwrap_error)?;

    // **Alles Prüfbare auf der `.new`-Datei, bevor die alte weicht**
    // (spec-reviewer Lauf 4, Fund 3). Vorher lief der Vergleich auf der
    // **Zieldatei** — also erst, nachdem das `rename` die einzige andere
    // Kopie von K schon überschrieben hatte. Scheiterte er dann, war die
    // alte Verpackung fort und die neue gab K nicht her: Totalverlust. Der
    // Modulkommentar oben („neuen Zustand herstellen, zurücklesen und
    // vergleichen, **erst dann** den alten entfernen") gilt mit dieser
    // Reihenfolge auch für `change_master_password`.
    let verified = write_and_read_back(&tmp, &wrapped, root_key, password);
    if let Err(err) = verified {
        // **Kein Rest** (Fund 4): Eine liegengebliebene `.new`-Datei ist
        // eine vollständige, gültige Verpackung von K — nach einem
        // Passwortwechsel unter dem *anderen* der beiden Passwörter. Wer
        // das Dateisystem lesen kann, greift dann offline das schwächere
        // von beiden an. Sie verschwindet deshalb auf **jedem** Fehlerweg,
        // nicht nur wenn das `rename` scheitert.
        remove_leftover(&tmp);
        return Err(err);
    }

    // Eine vorhandene Datei wird hier **absichtlich** ersetzt: Das ist der
    // Fall „Passwort ändern" (A15, „atomar ersetzen"). Das Umbenennen des
    // alten Stands gehört zu A5, nicht hierher.
    std::fs::rename(&tmp, &path).map_err(|err| {
        remove_leftover(&tmp);
        MasterPasswordError::FileFailed {
            detail: format!("Verpackungsdatei ersetzen: {err}"),
        }
    })?;

    #[cfg(unix)]
    {
        // Nach dem `rename` erneut, falls die Datei schon vorher existierte
        // und laxere Rechte trug (Passwortwechsel auf einer alten Datei).
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    // Fund 4, zweite Hälfte: Der Inhalt der neuen Datei ist durch
    // `sync_all` auf der Platte, der **Verzeichniseintrag** aber noch
    // nicht. Ein Stromausfall direkt danach könnte deshalb den alten Namen
    // auf die alte Datei zeigen lassen — die Folge wäre „das alte Passwort
    // gilt noch", kein Verlust, aber eine verwirrende Überraschung.
    sync_directory_of(&path);
    Ok(())
}

/// Der prüfbare Teil: schreiben, **dieselbe Datei** zurücklesen, entpacken
/// und mit K vergleichen.
///
/// Der Vergleich ist nicht Zierde: Er ist der Beweis, dass die Datei auf der
/// Platte K wirklich hergibt, **bevor** die andere Kopie entfernt wird.
/// Ohne ihn wäre ein Schreibfehler, den das Dateisystem erst beim Lesen
/// zeigt, ein Totalverlust.
fn write_and_read_back(
    tmp: &Path,
    wrapped: &[u8],
    root_key: &[u8; 32],
    password: &SecretString,
) -> Result<(), MasterPasswordError> {
    write_file_with_owner_only_permissions(tmp, wrapped)?;

    let read_back = std::fs::read(tmp).map_err(|err| MasterPasswordError::FileFailed {
        detail: format!("neue Verpackungsdatei nicht zurücklesbar: {err}"),
    })?;

    // Prüfpunkt **nur** für Tests: Ob die alte Verpackung einen
    // fehlgeschlagenen Vergleich übersteht, ist sonst nicht prüfbar — ein
    // Schreibfehler, den das Dateisystem erst beim Lesen zeigt, lässt sich
    // mit Dateien und Verknüpfungen allein nicht herstellen (eine
    // Verknüpfung auf `/dev/null` scheitert schon am `sync_all`, also
    // **vor** dem `rename`, und unterscheidet die beiden Reihenfolgen
    // deshalb nicht). Derselbe Weg wie bei `CountingKeychain`s
    // `fail_delete` in T13 — nur ohne Trait, weil es hier keinen gibt.
    //
    // `cargo build --workspace` läuft ohne `test-support` und fängt einen
    // Produktiv-Aufruf dieses Punktes ab (s. CLAUDE.md).
    #[cfg(any(test, feature = "test-support"))]
    if fail_the_next_wrapping_check::is_armed() {
        return Err(MasterPasswordError::FileFailed {
            detail: "Vergleich der neuen Verpackung absichtlich fehlgeschlagen (Test)".to_string(),
        });
    }
    let unwrapped = crypto::unwrap_root_key(&read_back, password).map_err(classify_unwrap_error)?;
    if unwrapped.expose() != root_key {
        return Err(MasterPasswordError::FileFailed {
            detail: "entpackter Schlüssel weicht von K ab".to_string(),
        });
    }
    Ok(())
}

/// Fehlerinjektion für den Vergleich der neuen Verpackung — **nur** mit
/// `test-support`.
///
/// Ein Schalter, der sich einmal auslöst und sich dann selbst zurücksetzt:
/// So kann ein Test den Fehlerfall fahren und danach im selben Verzeichnis
/// prüfen, dass das **alte** Passwort noch gilt, ohne den Schalter von Hand
/// aufzuräumen.
///
/// **Thread-lokal, nicht global** (gemessen: ein `static AtomicBool` ließ den
/// Schalter in einen nebenläufig laufenden Test überlaufen und machte dessen
/// Verpacken grundlos rot). Jeder `#[test]` läuft auf seinem eigenen Thread,
/// also wirkt der Schalter genau dort, wo er gesetzt wurde.
#[cfg(any(test, feature = "test-support"))]
pub mod fail_the_next_wrapping_check {
    use std::cell::Cell;

    thread_local! {
        static ARMED: Cell<bool> = const { Cell::new(false) };
    }

    /// Der nächste Vergleich auf **diesem** Thread scheitert.
    pub fn arm() {
        ARMED.set(true);
    }

    pub(super) fn is_armed() -> bool {
        ARMED.replace(false)
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".new");
    PathBuf::from(tmp)
}

/// Eine halbe Verpackung wegräumen. Scheitert das, ist es eine Log-Zeile und
/// kein Abbruch: Der Vorgang selbst ist schon gescheitert, und ein zweiter
/// Fehler darüber würde die Ursache verdecken.
fn remove_leftover(tmp: &Path) {
    match std::fs::remove_file(tmp) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => tracing::warn!(
            detail = %err,
            "a half-written wrapping file could not be removed; it may still hold the root key \
             under the other password (Spec 0101, A14)"
        ),
    }
}

/// `fsync` auf das Verzeichnis, damit der neue Verzeichniseintrag hält.
///
/// Best-effort und ohne Fehlerweg: Unter Windows lässt sich ein Verzeichnis
/// nicht wie eine Datei öffnen, und die Folge eines fehlenden Syncs ist
/// „das alte Passwort gilt noch" — unschön, aber kein Datenverlust. Den
/// Vorgang daran scheitern zu lassen wäre die schlechtere Antwort.
fn sync_directory_of(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    match std::fs::File::open(parent) {
        Ok(dir) => {
            if let Err(err) = dir.sync_all() {
                tracing::debug!(
                    detail = %err,
                    "could not fsync the data directory after replacing the wrapping file"
                );
            }
        }
        Err(err) => tracing::debug!(
            detail = %err,
            "could not open the data directory to fsync it after replacing the wrapping file"
        ),
    }
}

fn read_wrapping_file(db_path: &Path) -> Result<Vec<u8>, MasterPasswordError> {
    let path = wrapping_file_path(db_path);
    match std::fs::read(&path) {
        Ok(bytes) => Ok(bytes),
        // **An diesem Ort liegt nichts** — dann, und nur dann, passt der
        // Vorgang nicht zum Modus. Gefragt wird dasselbe wie in
        // [`key_mode`] (`symlink_metadata`), damit beide Funktionen nie
        // Verschiedenes über denselben Zustand sagen: Eine hängende
        // Verknüpfung ist für `key_mode` der Passwort-Modus, und ihr
        // `NotFound` beim Lesen darf deshalb nicht als „kein Master-Passwort
        // eingerichtet" herauskommen (Klarstellung 10a).
        Err(err)
            if err.kind() == std::io::ErrorKind::NotFound
                && std::fs::symlink_metadata(&path).is_err() =>
        {
            Err(MasterPasswordError::NotInPasswordMode)
        }
        // Alles andere ist A3 *nicht erreichbar* (D1): Es wurde nichts
        // verändert, und ein erneuter Versuch kann gelingen.
        Err(err) => Err(MasterPasswordError::WrappingFileUnreachable {
            detail: format!("Verpackungsdatei nicht lesbar: {err}"),
        }),
    }
}

/// Von Anfang an mit 0600, und mit `sync_all`, bevor der Aufrufer die Datei
/// weiterverwendet.
///
/// **Die Rechte werden beim Anlegen gesetzt, nicht danach** — anders als in
/// `host_key_store`. Zwischen `write` und `set_permissions` läge die Datei
/// mit den Rechten aus der `umask` auf der Platte; bei einem Chiffrat, das
/// ein Offline-Rateangriff braucht, ist dieses Fenster eines zu viel.
fn write_file_with_owner_only_permissions(
    path: &Path,
    bytes: &[u8],
) -> Result<(), MasterPasswordError> {
    let file_failed = |step: &str, err: std::io::Error| MasterPasswordError::FileFailed {
        detail: format!("{step}: {err}"),
    };

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|err| file_failed("Verpackungsdatei anlegen", err))?;
    use std::io::Write;
    file.write_all(bytes)
        .map_err(|err| file_failed("Verpackungsdatei schreiben", err))?;
    // `sync_all`, bevor umbenannt wird: Sonst könnte das `rename` sichtbar
    // sein, während der Inhalt noch im Puffer steht — ein Absturz dazwischen
    // ließe eine leere Verpackungsdatei zurück, und K wäre weg.
    file.sync_all()
        .map_err(|err| file_failed("Verpackungsdatei sichern", err))?;
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

/// Klarstellung 12: der Riegel aus [`set_up_master_password`], einzeln.
///
/// Eigene Funktion, damit genau **eine** Stelle entscheidet, was als
/// bestätigt gilt — der Vorab-Prüfer unten und das Einrichten selbst
/// dürfen sich hier nicht auseinanderentwickeln.
fn check_loss_warning(warning: LossWarning) -> Result<(), MasterPasswordError> {
    if warning != LossWarning::ConfirmedByTheUser {
        tracing::warn!(
            "refusing to set up a master password: the data-loss warning was not confirmed; \
             nothing was changed (Spec 0101, A13/E10, Klarstellung 12)"
        );
        return Err(MasterPasswordError::LossWarningNotConfirmed);
    }
    Ok(())
}

/// Dieselben beiden Prüfungen wie am Anfang von [`set_up_master_password`],
/// aber aufrufbar, **bevor** der Aufrufer selbst etwas anfasst
/// (spec-reviewer Runde 2 zu Klarstellung 12).
///
/// **Warum es das braucht:** Auf den Wegen über A5 und D4 liegt zwischen
/// der Passworteingabe und dem Einrichten ein `rename` — die Datenbank und
/// die alte Verpackung werden zur Seite gelegt, weil die Reihenfolge das
/// verlangt (ADR 0095 §4). Der Riegel im Einrichten greift dort erst
/// danach, und „lehnt ab und **verändert nichts**" (Klarstellung 12) wäre
/// dann schon nicht mehr wahr: Zurück bliebe ein Datenverzeichnis ohne
/// Verpackungsdatei, also beim nächsten Start der Schlüsselbund-Modus —
/// ein Moduswechsel, den niemand gewählt hat.
///
/// **Sie ersetzt die Prüfung im Einrichten nicht**, sie kommt davor. Beide
/// rufen dieselben beiden Funktionen auf; eine doppelte Prüfung kann nicht
/// weniger erkennen als eine.
pub fn check_new_password_before_touching_files(
    password: &SecretString,
    repeated: &SecretString,
    warning: LossWarning,
) -> Result<(), MasterPasswordError> {
    check_loss_warning(warning)?;
    check_new_password(password, repeated)
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
