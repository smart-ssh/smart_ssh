//! Spec 0101, A3/A5: Der Startablauf entscheidet nach Dateizustand und
//! Schlüsselzustand, **bevor** eine Migration läuft — und fasst in keinem
//! Dialogfall etwas an, solange der Nutzer nicht gewählt hat.
//!
//! **Hier und nicht in `app-shell`:** Die Entscheidung ist fachliche Logik
//! mit vier Dateizuständen × vier Schlüsselzuständen; sie muss vollständig
//! testbar sein (T3 verlangt „je Feld der Tabelle ein Fall“). `app-shell`
//! bringt nur die native Dialogdarstellung mit ([`StartupPrompt`]) und den
//! echten Schlüsselbund.
//!
//! **Die Tabelle steht an genau einer Stelle** ([`decide_startup`]) und ist
//! rein: keine Datei, kein Schlüsselbund, kein Dialog. Alles, was I/O
//! macht, liegt in [`open_or_prepare_database`] und ruft die Tabelle, statt
//! ihre Fälle nachzubilden.

use std::path::{Path, PathBuf};

use credentials_keyring::{KeychainAvailability, KeychainUnavailableReason};
use persistence_sqlite::{
    detect_database_file_state, ConnectFailureKind, DataDirLock, DataDirLockError,
    DatabaseFileState, PersistenceError, SqliteProfileStore,
};
use ssh_manager_core::crypto::{DatabaseKey, RootKey, RootKeyState};
use ssh_manager_core::profiles::CredentialStore;

/// Spec 0101, A3: der Schlüsselzustand, wie ihn die Tabelle braucht.
///
/// **Maßgeblich ist das Ergebnis des Lesens von K, nicht die Probe beim
/// Start** (A3, wörtlich): `Unreachable` heißt „das `get` scheitert mit
/// `Backend`" **oder** „es wurde wegen `Unavailable` gar nicht versucht“.
/// Deshalb trägt die Variante den Grund mit, soweit er bekannt ist — D1
/// bietet das Einrichten eines Master-Passworts nur bei zwei bestimmten
/// Gründen an.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyState {
    Present,
    NotFound,
    Unreachable(Option<KeychainUnavailableReason>),
    Invalid,
}

/// Welcher Startdialog zu zeigen ist (Spec 0101, A3: D1–D4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupDialog {
    /// „Erneut versuchen“ / „Beenden“; ab Etappe 3 zusätzlich
    /// „Master-Passwort einrichten“, wenn `offers_password_setup`.
    D1 { offers_password_setup: bool },
    /// Datei mit dem vorhandenen Schlüssel nicht lesbar bzw. kein Schlüssel
    /// zu einer verschlüsselten Datei. „Beenden“ / „Neu anfangen“.
    D2,
    /// Schlüssel unbrauchbar. „Beenden“ / „Neu anfangen“.
    D3,
    /// Wie D3, aber die Klartext-Datei bleibt lesbar: „Beenden“ / „Neuen
    /// Schlüssel erzeugen".
    D4,
    /// A11/A11.1: Beim Secret-Umzug ist ein Lesen aus dem Schlüsselbund
    /// gescheitert. „Erneut versuchen“ / „Beenden“, im Passwort-Modus
    /// zusätzlich „Ohne Übernahme fortfahren“.
    ///
    /// **Eine eigene Variante und nicht `D1 { offers_password_setup: false }`**
    /// (T13: „Umzugs-Dialog (A11) nie“): So kann dieser Dialog das
    /// Einrichten eines Master-Passworts per Konstruktion nicht anbieten —
    /// es hinge sonst an einem `false`, das jemand später anders setzt. Die
    /// Texte sind dieselben wie bei D1 (A11: „Dialog D1 ohne Einrichten“).
    MigrationUnreadable { offers_skip_migration: bool },
}

/// Was der Start als Nächstes tun soll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupPlan {
    /// Datei fehlt, K liegt vor: eine neue, verschlüsselte Datenbank anlegen.
    CreateFresh,
    /// Datei fehlt, kein K-Eintrag: K erzeugen, dann anlegen. Eines der
    /// **zwei** Felder, in denen ohne Nutzerwahl ein K entsteht (A3).
    GenerateKeyThenCreateFresh,
    /// Klartext-Datei, K liegt vor: umwandeln (A6), dann öffnen.
    Convert,
    /// Klartext-Datei, kein K-Eintrag: K erzeugen, dann umwandeln. Das
    /// zweite der beiden Felder aus A3.
    GenerateKeyThenConvert,
    /// Verschlüsselte (oder sonstige) Datei, K liegt vor: öffnen; ist sie
    /// nicht lesbar, folgt D2.
    OpenExisting,
    /// Nichts anfassen, Dialog zeigen.
    Dialog(StartupDialog),
}

/// Spec 0101, A3 — **die** Entscheidungstabelle, als eine reine Funktion.
///
/// Rein und ohne Nebenwirkung: Sie liest keine Datei, fragt keinen
/// Schlüsselbund und zeigt keinen Dialog. Dadurch lässt sich jedes der
/// zwölf Felder einzeln prüfen (T3), und es gibt keine zweite Stelle, an
/// der die Tabelle „noch einmal anders“ nachgebildet wäre.
pub fn decide_startup(file: DatabaseFileState, key: &KeyState) -> StartupPlan {
    match (file, key) {
        // --- Zeile „fehlt“
        (DatabaseFileState::Missing, KeyState::Present) => StartupPlan::CreateFresh,
        (DatabaseFileState::Missing, KeyState::NotFound) => StartupPlan::GenerateKeyThenCreateFresh,
        (DatabaseFileState::Missing, KeyState::Unreachable(reason)) => {
            StartupPlan::Dialog(StartupDialog::D1 {
                offers_password_setup: password_setup_is_safe(*reason),
            })
        }
        (DatabaseFileState::Missing, KeyState::Invalid) => StartupPlan::Dialog(StartupDialog::D3),

        // --- Zeile „Klartext“
        (DatabaseFileState::Plaintext, KeyState::Present) => StartupPlan::Convert,
        (DatabaseFileState::Plaintext, KeyState::NotFound) => StartupPlan::GenerateKeyThenConvert,
        (DatabaseFileState::Plaintext, KeyState::Unreachable(reason)) => {
            StartupPlan::Dialog(StartupDialog::D1 {
                offers_password_setup: password_setup_is_safe(*reason),
            })
        }
        // D4 statt D3: Die Klartext-Datei bleibt lesbar, es geht nur der
        // feldweise verschlüsselte Verlauf verloren — ein anderer Preis als
        // bei D3 und deshalb eine andere Frage.
        (DatabaseFileState::Plaintext, KeyState::Invalid) => StartupPlan::Dialog(StartupDialog::D4),

        // --- Zeile „sonst“
        (DatabaseFileState::Other, KeyState::Present) => StartupPlan::OpenExisting,
        // Kein Schlüssel zu einer verschlüsselten Datei: **nicht** einen
        // neuen erzeugen (das wäre die Datei für immer verloren), sondern
        // D2 fragen.
        (DatabaseFileState::Other, KeyState::NotFound) => StartupPlan::Dialog(StartupDialog::D2),
        // „Dialog D1 ohne Einrichten“ (A3, Tabelle): Hier kann kein
        // erreichbarer K existieren, der diese Datei öffnet — ein neu
        // eingerichtetes Master-Passwort würde einen neuen K verpacken und
        // die vorhandene Datei damit endgültig unlesbar machen.
        (DatabaseFileState::Other, KeyState::Unreachable(_)) => {
            StartupPlan::Dialog(StartupDialog::D1 {
                offers_password_setup: false,
            })
        }
        (DatabaseFileState::Other, KeyState::Invalid) => StartupPlan::Dialog(StartupDialog::D3),
    }
}

/// D1, Einschränkung (A3): „Master-Passwort einrichten“ nur bei
/// `NoSecretServiceProvider` oder `NoSessionBus`.
///
/// Begründung aus der Spec, und sie ist der Kern dieser Einschränkung: Nur
/// bei diesen beiden Gründen kann **kein** erreichbarer K existieren — es
/// gibt gar keinen Schlüsselbund, in dem einer liegen könnte. Bei `Locked`,
/// `Unknown` oder einem Backend-Fehler ist der Schlüsselbund da und enthält
/// möglicherweise den K, der den feldweise verschlüsselten Verlauf
/// aufschließt; ein neu erzeugter K würde ihn unlesbar machen.
///
/// `None` (Grund unbekannt, weil das `get` selbst scheiterte) zählt
/// bewusst **nicht** dazu — im Zweifel kein Einrichten.
fn password_setup_is_safe(reason: Option<KeychainUnavailableReason>) -> bool {
    matches!(
        reason,
        Some(KeychainUnavailableReason::NoSecretServiceProvider)
            | Some(KeychainUnavailableReason::NoSessionBus)
    )
}

/// Spec 0101, A3: Ermittelt den K-Zustand.
///
/// **Der Schlüsselbund wird nicht angefasst, wenn er beim Start als nicht
/// verfügbar erkannt wurde** — dann gilt `Unreachable` mit dem Grund aus
/// der Probe (A3: „wird wegen `Unavailable` gar nicht versucht“). Sonst
/// entscheidet ausschließlich das Ergebnis des `get`.
pub fn read_key_state(
    credential_store: &dyn CredentialStore,
    keychain: KeychainAvailability,
) -> (KeyState, Option<[u8; 32]>) {
    if !keychain.is_available() {
        return (KeyState::Unreachable(keychain.unavailable_reason()), None);
    }
    match ssh_manager_core::crypto::read_root_key(credential_store) {
        RootKeyState::Present(key) => (KeyState::Present, Some(key)),
        RootKeyState::NotFound => (KeyState::NotFound, None),
        // Der Grund bleibt `None`: Die Startprobe sagte „verfügbar“, das
        // `get` scheiterte trotzdem — welcher der Gründe aus
        // `KeychainUnavailableReason` das war, ist von hier aus nicht zu
        // wissen, und zu raten wäre gerade bei D1 falsch (s.
        // `password_setup_is_safe`).
        RootKeyState::Unreachable(_) => (KeyState::Unreachable(None), None),
        RootKeyState::Invalid => (KeyState::Invalid, None),
    }
}

/// Die Wahl des Nutzers in einem Startdialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupChoice {
    /// D1: ohne Neustart erneut prüfen.
    Retry,
    /// Jeder Dialog: beenden, nichts anfassen.
    Quit,
    /// D2/D3: „Neu anfangen“ (A5).
    StartOver,
    /// D4: „Neuen Schlüssel erzeugen“.
    GenerateNewKey,
    /// D1, dritter Knopf ab Etappe 3 (A13): „Master-Passwort einrichten“ —
    /// **nur**, wenn [`StartupDialog::D1::offers_password_setup`] gilt.
    SetUpMasterPassword,
    /// A11.1, dritte Wahl im Umzugs-Dialog: „Ohne Übernahme fortfahren“.
    /// Nur im Passwort-Modus angeboten.
    ContinueWithoutMigration,
}

/// Woher K beim Start kommt (A3/A16).
///
/// **Ein eigener Typ statt eines zweiten `CredentialStore`:** Im
/// Passwort-Modus gibt es keinen Store, aus dem K zu lesen wäre — er kommt
/// aus der Verpackungsdatei und liegt nach der Entsperrung schon vor. Ihn
/// über einen In-Memory-Store einzuschmuggeln hätte funktioniert und wäre
/// die gefährlichste Abkürzung dieser Spec gewesen: Die Fälle „K erzeugen“
/// der Tabelle A3 hätten dann einen neuen Schlüssel in diesen
/// Speicher-Store geschrieben, den niemand je in eine Verpackungsdatei
/// übernimmt — ein neuer K, der still entsteht und beim nächsten Start weg
/// ist (A3: „nie still ein neuer K“).
pub enum RootKeyAccess<'a> {
    /// Schlüsselbund-Modus (Standard, E8): K wird gelesen und darf in den
    /// zwei Feldern „K erzeugen“ der Tabelle A3 entstehen.
    /// `+ Send + Sync` aus demselben Grund wie bei [`StartupPrompt`].
    Keychain(&'a (dyn CredentialStore + Send + Sync)),
    /// Passwort-Modus, entsperrt (A16): K liegt vor.
    ///
    /// **Als [`RootKey`] und nicht als `[u8; 32]`** (A19, spec-reviewer
    /// Lauf 4, Fund 11): Der Typ überschreibt sich beim Freigeben und lässt
    /// sich nicht ausgeben. Vorher kopierte die Aufrufstelle K in ein
    /// gewöhnliches Array — der `RootKey` daneben wurde korrekt
    /// überschrieben, die Kopie nicht.
    ///
    /// Besitzend: Der Startablauf hält K für seine Dauer und gibt ihn danach
    /// frei — eine geliehene Fassung ginge nicht, weil der Moduswechsel aus
    /// D1 (A13) mitten in der Schleife einen **neuen** K einsetzt.
    Unlocked(RootKey),
    /// Passwort-Modus, aber die Verpackungsdatei ist nach Format oder
    /// Version nicht lesbar — in der Tabelle A3 der Zustand *ungültig*.
    UnusableWrapping,
}

/// A13: ein neu eingegebenes Master-Passwort, zweimal.
///
/// Der Vergleich der beiden Eingaben passiert im Backend
/// ([`crate::master_password`]), nicht in der Maske — eine Prüfung, die nur
/// dort steht, umgeht ein Kommandoaufruf.
pub struct NewMasterPassword {
    pub password: secrecy::SecretString,
    pub repeated: secrecy::SecretString,
    /// Klarstellung 12: dasselbe Argument wie für die Wiederholung — die
    /// Bestätigung der Warnung aus A13/E10 reist mit, und das Backend prüft
    /// sie, statt sie der Maske zu glauben.
    pub warning: crate::master_password::LossWarning,
}

/// Was `app-shell` an nativen Dialogen beisteuert. Als Trait, damit T3/T7/T8
/// den gesamten Ablauf ohne Fenster prüfen können — und damit ein Test
/// **zählen** kann, was gefragt wurde.
/// `Send + Sync` seit Etappe 3: Der Startablauf läuft im Passwort-Modus aus
/// einem `#[tauri::command]` heraus, und dessen Future muss `Send` sein
/// (Teil 0 Frage 3). Beide Implementierungen erfüllen es ohnehin.
pub trait StartupPrompt: Send + Sync {
    /// Zeigt D1–D4 und liefert die Wahl.
    fn ask(&self, dialog: StartupDialog) -> StartupChoice;

    /// A5: zweite Bestätigung für „Neu anfangen“. `renamed_to` ist der Name,
    /// den die bisherige Datei bekommt — er gehört in den Text, damit der
    /// Nutzer sie wiederfindet. `None`, wenn es keine Datei umzubenennen
    /// gibt (Feld *fehlt* × *ungültig*): Dann darf der Text auch keine
    /// nennen. `false` heißt: es passiert nichts.
    fn confirm_start_over(&self, renamed_to: Option<&str>) -> bool;

    /// D4: zweite Bestätigung für „Neuen Schlüssel erzeugen“ (A5,
    /// „Zweite Bestätigung wie A5“). Die Datenbank wird dabei **nicht**
    /// umbenannt, nur der bisherige Verlauf ist verloren.
    fn confirm_generate_new_key(&self) -> bool;

    /// A5: nennt nach dem Umbenennen den neuen Dateinamen.
    fn notify_started_over(&self, renamed_to: &str);

    /// A13: fragt ein neues Master-Passwort ab — zweimal, mit Warnung und
    /// ausdrücklicher Bestätigung (die Maske zeigt beides, s.
    /// [`StartupDialog::D1`] und A5 im Passwort-Modus; geprüft werden beide
    /// im Backend, Klarstellung 12 — ein Fragesteller, der die Bestätigung
    /// nicht einholt, liefert [`crate::master_password::LossWarning::
    /// NotConfirmed`], und dann wird nichts eingerichtet).
    ///
    /// `None` heißt abgebrochen; dann bleibt **alles** unverändert (A5).
    fn ask_for_new_master_password(&self) -> Option<NewMasterPassword>;

    /// Kann dieser Fragesteller überhaupt eine Texteingabe zeigen?
    ///
    /// Die nativen Dialoge können es nicht (§1: rfd hat keine
    /// Texteingabe). Statt dort ein `None` zurückzugeben, das sich von
    /// „abgebrochen“ nicht unterscheiden ließe und den Start stillschweigend
    /// beendete, sagt diese Frage es vorher — und der Startablauf antwortet
    /// mit [`StartupAbort::NeedsWindow`], ohne etwas anzufassen.
    fn can_ask_for_a_password(&self) -> bool;
}

/// Warum der Start nicht zu einer offenen Datenbank geführt hat.
#[derive(Debug)]
pub enum StartupAbort {
    /// Der Nutzer hat „Beenden“ gewählt — kein Fehlerdialog mehr, es ist
    /// bereits alles gesagt.
    UserQuit,
    /// Teil 0 Frage 3: Der Nutzer hat etwas gewählt, das eine Texteingabe
    /// braucht (A13, „Master-Passwort einrichten“ aus D1) — und der
    /// Startablauf läuft gerade mit den nativen Dialogen, die keine haben
    /// (§1: „rfd … keine Texteingabe“).
    ///
    /// **Nichts ist verändert.** Der Aufrufer startet das Fenster und
    /// wiederholt den Ablauf von vorn, dann mit der Maske im Fenster.
    NeedsWindow,
    /// Ein Fehler, für den [`crate::startup_error_messages::
    /// db_connect_failure_text`] den Text liefert.
    Fatal {
        kind: ConnectFailureKind,
        /// Für die Log-Zeile; nie für einen Dialog.
        detail: String,
    },
}

/// Issue #19: sperrt das Datenverzeichnis von `db_path` exklusiv für diesen
/// Prozess — der erste Schritt des Startablaufs, **vor** jedem Zugriff auf
/// die Datenbank (Öffnen, Migrieren, Umwandeln) und vor der
/// Verpackungsdatei.
///
/// Hält ein anderer Prozess die Sperre, endet der Start mit
/// [`ConnectFailureKind::AlreadyRunning`]; die Datenbank bleibt unberührt.
/// Jeder andere Fehler beim Sperren endet ebenfalls im Startfehler —
/// **nie** in einem Start ohne Sperre.
pub fn lock_data_directory(db_path: &Path) -> Result<DataDirLock, StartupAbort> {
    DataDirLock::acquire_for_database(db_path).map_err(|err| {
        let kind = match &err {
            DataDirLockError::HeldByAnotherProcess => ConnectFailureKind::AlreadyRunning,
            DataDirLockError::Io(io) if io.kind() == std::io::ErrorKind::PermissionDenied => {
                ConnectFailureKind::PermissionDenied
            }
            DataDirLockError::Io(_) => ConnectFailureKind::Other,
        };
        StartupAbort::Fatal {
            kind,
            detail: err.to_string(),
        }
    })
}

/// Ergebnis eines erfolgreichen Starts.
pub struct OpenedDatabase {
    pub store: SqliteProfileStore,
    /// Der Wurzelschlüssel K — für den Chat-Cipher (Spec 0036, E11).
    pub root_key: [u8; 32],
}

/// Spec 0101, A3/A5/A6 — der Startablauf, bis die Datenbank offen ist.
///
/// Fährt die Tabelle aus [`decide_startup`], führt die dort vorgesehene
/// Handlung aus und fragt sonst den Nutzer. Die Schleife existiert nur für
/// „Erneut versuchen“ (D1): Sie beginnt wieder beim Dateizustand, prüft
/// also tatsächlich neu, statt einen gemerkten Zustand zu wiederholen.
///
/// **Was hier niemals passiert:** Kein `set` auf K außer in den beiden
/// „K erzeugen“-Feldern und nach einer ausdrücklichen Wahl; kein Öffnen,
/// Umbenennen oder Verändern einer Datei in einem Dialogfall, solange nicht
/// gewählt ist.
///
/// **Issue #19:** `lock` ist die Sperre auf das Datenverzeichnis aus
/// [`lock_data_directory`]. Der Aufrufer nimmt sie **vor** diesem Aufruf
/// und hält sie, solange der Prozess läuft; die Umwandlung (A6) verlangt
/// sie ausdrücklich.
pub async fn open_or_prepare_database(
    db_path: &Path,
    access: RootKeyAccess<'_>,
    keychain: KeychainAvailability,
    prompt: &dyn StartupPrompt,
    lock: &DataDirLock,
) -> Result<OpenedDatabase, StartupAbort> {
    // `mut`, weil genau ein Übergang vorkommt: D1 → „Master-Passwort
    // einrichten“ wechselt den Modus mitten im Start (A13). Danach ist K
    // entpackt, und der zweite Schleifendurchlauf fährt die Tabelle mit
    // `Unlocked`.
    let mut access = access;
    loop {
        let file = match detect_database_file_state(db_path) {
            Ok(state) => state,
            Err(err) => {
                return Err(StartupAbort::Fatal {
                    kind: if err.kind() == std::io::ErrorKind::PermissionDenied {
                        ConnectFailureKind::PermissionDenied
                    } else {
                        ConnectFailureKind::Other
                    },
                    detail: format!("Dateizustand nicht ermittelbar: {err}"),
                })
            }
        };
        // A6 verweigert die Umwandlung eines Symlinks (T20), aber
        // `detect_database_file_state` folgt ihm: Eine Verknüpfung auf ein
        // **nicht vorhandenes** Ziel ergab *fehlt*, und
        // `create_if_missing` legte die neue, verschlüsselte Datenbank am
        // Ziel der Verknüpfung an (spec-reviewer Runde 1). Dieselbe Regel
        // gilt deshalb für jeden Weg, auf dem eine Datei **angelegt oder
        // umbenannt** wird — also für *fehlt* und *Klartext*.
        //
        // **Nicht** für *sonst* (spec-reviewer Runde 2): Eine bewusst
        // gesetzte Verknüpfung auf eine bereits verschlüsselte Datenbank
        // wird nur geöffnet; es entsteht keine Datei und es wird nichts
        // umbenannt. Sie weiter abzulehnen wäre eine Verschärfung ohne
        // Anlass — und der Dialogtext („wird nicht automatisch
        // verschlüsselt") wäre für diesen Fall auch die falsche Begründung.
        if matches!(
            file,
            DatabaseFileState::Missing | DatabaseFileState::Plaintext
        ) && std::fs::symlink_metadata(db_path)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(StartupAbort::Fatal {
                kind: ConnectFailureKind::SymlinkedDatabase,
                detail: "die Datenbankdatei ist eine symbolische Verknüpfung".to_string(),
            });
        }

        let (key_state, root_key) = match &access {
            RootKeyAccess::Keychain(store) => read_key_state(*store, keychain),
            // A3: „Im Passwort-Modus ist K erst nach der Entsperrung *da*.“
            // Die Kopie in das `Option<[u8; 32]>` bleibt: Der Bestand
            // (`read_key_state`, `DatabaseKey::from_root_key`) arbeitet mit
            // `[u8; 32]`, und das umzustellen wäre ein eigener Schritt
            // (ADR 0095 §3).
            RootKeyAccess::Unlocked(key) => (KeyState::Present, Some(*key.expose())),
            // A3: „*ungültig* (… Verpackungsdatei, deren Format oder
            // Version nicht lesbar ist)“.
            RootKeyAccess::UnusableWrapping => (KeyState::Invalid, None),
        };
        let plan = decide_startup(file, &key_state);
        tracing::info!(
            ?file,
            ?key_state,
            ?plan,
            password_mode = !matches!(access, RootKeyAccess::Keychain(_)),
            "decided how to open the database (Spec 0101, A3)"
        );

        // **Jeder Zweig außer D1-„Erneut versuchen“ endet.** Die Schleife
        // ist ausschließlich für das erneute Prüfen da; ein Zweig, der nach
        // einer Nutzerwahl bloß zurück an den Anfang springt, könnte
        // denselben Dialog endlos zeigen (D3 etwa bleibt nach dem
        // Umbenennen weiter bei einem unbrauchbaren Schlüssel).
        match plan {
            StartupPlan::CreateFresh | StartupPlan::OpenExisting => {
                // spec-reviewer Runde 1: kein `expect` im Startpfad. Die
                // Zusicherung haelt (nur `KeyState::Present` fuehrt
                // hierher, und `read_key_state` liefert Zustand und
                // Schluessel als Paar), aber ein Panic an dieser Stelle
                // laeuft vor jedem Fenster — also genau das
                // undiagnostizierbare Aufblitzen, das Spec 0059 beseitigt
                // hat. Lieber ein sichtbarer Startfehler.
                let Some(key) = root_key else {
                    return Err(missing_key_despite_present());
                };
                match open_encrypted(db_path, &key).await {
                    Ok(opened) => return Ok(opened),
                    // D2: Die Datei ist mit dem vorhandenen Schlüssel nicht
                    // lesbar.
                    Err(Abort::NotReadable) => {
                        // `Keep`: Der Schlüssel ist brauchbar, nur die Datei
                        // gehört nicht zu ihm — s. [`StartOverKey`].
                        let key = start_over(
                            db_path,
                            &access,
                            prompt,
                            StartupDialog::D2,
                            StartOverKey::Keep(key),
                        )?;
                        return open_encrypted(db_path, &key)
                            .await
                            .map_err(fatal_after_start_over);
                    }
                    Err(Abort::Fatal(abort)) => return Err(abort),
                }
            }
            StartupPlan::GenerateKeyThenCreateFresh => {
                let key = generate_key(db_path, &access, prompt)?;
                return open_encrypted(db_path, &key)
                    .await
                    .map_err(fatal_after_start_over);
            }
            StartupPlan::Convert => {
                let Some(key) = root_key else {
                    return Err(missing_key_despite_present());
                };
                return convert_then_open(db_path, &key, lock).await;
            }
            StartupPlan::GenerateKeyThenConvert => {
                let key = generate_key(db_path, &access, prompt)?;
                return convert_then_open(db_path, &key, lock).await;
            }
            StartupPlan::Dialog(StartupDialog::D1 {
                offers_password_setup,
            }) => match prompt.ask(StartupDialog::D1 {
                offers_password_setup,
            }) {
                // „Erneut versuchen“ prüft ohne Neustart erneut (A3, D1) —
                // der einzige Weg zurück an den Anfang der Schleife.
                StartupChoice::Retry => continue,
                // A13, Einrichten aus D1. **Die Einschränkung aus A3 wird
                // hier noch einmal geprüft**, nicht nur beim Anbieten: Der
                // Knopf kommt aus einer Oberfläche, und ein Einrichten bei
                // einem bloß gesperrten Schlüsselbund würde einen neuen K
                // erzeugen und den vorhandenen Verlauf unlesbar machen
                // (Angriffsrichtung „Einrichten bei verschlüsselter Datei“).
                StartupChoice::SetUpMasterPassword if offers_password_setup => {
                    // Teil 0 Frage 3: Die nativen Dialoge haben keine
                    // Texteingabe. Hier endet der Ablauf **ohne etwas
                    // anzufassen**; der Aufrufer wiederholt ihn mit dem
                    // Fenster.
                    if !prompt.can_ask_for_a_password() {
                        tracing::info!(
                            "the master-password setup needs a text field; restarting the \
                             startup sequence with the in-window dialog (Spec 0101, A13)"
                        );
                        return Err(StartupAbort::NeedsWindow);
                    }
                    // „Aus D1 gibt es keinen K im Schlüsselbund; dort wird K
                    // neu erzeugt (A3).“
                    let mut key = ssh_manager_core::crypto::generate_root_key();
                    let Some(new_password) = prompt.ask_for_new_master_password() else {
                        // A5: Abbruch heißt, dass nichts verändert ist.
                        return Err(StartupAbort::UserQuit);
                    };
                    set_up_password(db_path, &key, &new_password, None, FilesAlreadyMoved::No)?;
                    // Ab jetzt Passwort-Modus mit entpacktem K; der nächste
                    // Durchlauf fährt die Tabelle neu und landet in
                    // „neu anlegen“ bzw. „umwandeln“.
                    // A19: K wandert in den schützenden Typ, das Array
                    // wird dabei überschrieben.
                    access = RootKeyAccess::Unlocked(RootKey::take_from(&mut key));
                    continue;
                }
                StartupChoice::SetUpMasterPassword => {
                    tracing::warn!(
                        "a master-password setup was requested for a dialog that does not offer \
                         it; nothing was touched (Spec 0101, A3 D1)"
                    );
                    return Err(StartupAbort::UserQuit);
                }
                StartupChoice::Quit
                | StartupChoice::StartOver
                | StartupChoice::GenerateNewKey
                | StartupChoice::ContinueWithoutMigration => return Err(StartupAbort::UserQuit),
            },
            // Verschlüsselte Datei, kein K-Eintrag: dieselbe Frage wie
            // oben, nur schon vor dem Öffnen entschieden.
            StartupPlan::Dialog(StartupDialog::D2) => {
                let key = start_over(
                    db_path,
                    &access,
                    prompt,
                    StartupDialog::D2,
                    StartOverKey::Issue,
                )?;
                return open_encrypted(db_path, &key)
                    .await
                    .map_err(fatal_after_start_over);
            }
            StartupPlan::Dialog(StartupDialog::D3) => {
                // D3: „Der vorhandene Eintrag bzw. die Datei wird **erst
                // nach dieser Wahl** ersetzt." Der unbrauchbare Eintrag bzw.
                // die unbrauchbare Verpackungsdatei wird jetzt ersetzt —
                // ohne das bliebe der Zustand *ungültig* und derselbe Dialog
                // käme beim nächsten Start wieder.
                let key = start_over(
                    db_path,
                    &access,
                    prompt,
                    StartupDialog::D3,
                    StartOverKey::Issue,
                )?;
                return open_encrypted(db_path, &key)
                    .await
                    .map_err(fatal_after_start_over);
            }
            StartupPlan::Dialog(StartupDialog::D4) => {
                if prompt.ask(StartupDialog::D4) != StartupChoice::GenerateNewKey {
                    return Err(StartupAbort::UserQuit);
                }
                // A5: zweite Bestätigung. Ohne sie passiert nichts — kein
                // neuer Schlüssel, keine Umwandlung.
                if !prompt.confirm_generate_new_key() {
                    return Err(StartupAbort::UserQuit);
                }
                // D4: Die Klartext-Datei bleibt lesbar und wird mit dem
                // neuen K umgewandelt; allein der feldweise verschlüsselte
                // Verlauf ist verloren. Im Passwort-Modus heißt „neuer
                // Schlüssel“ auch „neue Verpackung“ — mit derselben
                // Reihenfolge wie in A5 (s. `generate_key`).
                let key = generate_key(db_path, &access, prompt)?;
                return convert_then_open(db_path, &key, lock).await;
            }
            // [`decide_startup`] liefert diesen Dialog nie — er gehört zum
            // Secret-Umzug (A11), der erst nach dem Öffnen läuft. Der Zweig
            // steht hier, damit das Hinzufügen einer weiteren Dialogart den
            // Bau anhält, statt in einem `_` zu verschwinden.
            StartupPlan::Dialog(StartupDialog::MigrationUnreadable { .. }) => {
                return Err(StartupAbort::Fatal {
                    kind: ConnectFailureKind::Other,
                    detail: "Umzugs-Dialog aus der Entscheidungstabelle A3".to_string(),
                });
            }
        }
    }
}

enum Abort {
    NotReadable,
    Fatal(StartupAbort),
}

/// Kann nach heutigem Code nicht vorkommen (s. Aufrufstellen) — und endet
/// deshalb sichtbar statt in einem Panic vor dem ersten Fenster.
fn missing_key_despite_present() -> StartupAbort {
    StartupAbort::Fatal {
        kind: ConnectFailureKind::Other,
        detail: "Schlüsselzustand Present, aber kein Schlüssel geliefert".to_string(),
    }
}

/// Nach „Neu anfangen“ bzw. nach dem Anlegen eines neuen K darf die Datei
/// nicht mehr unlesbar sein. Passiert es doch (jemand hat zwischenzeitlich
/// eine Datei untergeschoben), endet der Start sichtbar statt in einer
/// weiteren Runde Dialoge.
fn fatal_after_start_over(err: Abort) -> StartupAbort {
    match err {
        Abort::NotReadable => StartupAbort::Fatal {
            kind: ConnectFailureKind::KeyMismatch,
            detail: "Datei direkt nach dem Neuanfang nicht lesbar".to_string(),
        },
        Abort::Fatal(abort) => abort,
    }
}

/// D2/D3 → A5. Fragt, bestätigt, benennt um, meldet den neuen Namen.
///
/// **Die Reihenfolge ist Teil der Anforderung:** Erst die Wahl, dann die
/// zweite Bestätigung, dann das Umbenennen. Bricht der Nutzer an einer der
/// beiden Stellen ab, ist keine Datei angefasst — beides ergibt
/// [`StartupAbort::UserQuit`], und `Ok(())` heißt: umbenannt.
/// Was „Neu anfangen“ mit dem Schlüssel macht.
///
/// **Die Unterscheidung ist wichtig und nicht bloß Bequemlichkeit:** D2 aus
/// dem Feld *sonst* × *da* heißt „die Datei passt nicht zu diesem
/// Schlüssel“ — der Schlüssel selbst ist in Ordnung. Ihn dabei zu ersetzen
/// würde den feldweise verschlüsselten Verlauf **und** jede Chance
/// vernichten, die umbenannte Datei später doch noch zu öffnen. D3 dagegen
/// heißt „der Schlüssel ist unbrauchbar“; dort muss ein neuer entstehen,
/// sonst käme derselbe Dialog beim nächsten Start wieder.
enum StartOverKey {
    /// K bleibt, nur die Datei geht aus dem Weg.
    Keep([u8; 32]),
    /// Es gibt keinen brauchbaren K — ein neuer entsteht (Schlüsselbund
    /// oder neue Verpackung, je nach Modus).
    Issue,
}

fn start_over(
    db_path: &Path,
    access: &RootKeyAccess<'_>,
    prompt: &dyn StartupPrompt,
    dialog: StartupDialog,
    key: StartOverKey,
) -> Result<[u8; 32], StartupAbort> {
    if prompt.ask(dialog) != StartupChoice::StartOver {
        return Err(StartupAbort::UserQuit);
    }
    let in_password_mode = !matches!(access, RootKeyAccess::Keychain(_));
    // A5 nennt die Verpackungsdatei im Umbenennungssatz — **aber nur, wenn
    // sie selbst das Unbrauchbare ist** (D3, `UnusableWrapping`). Bei D2 mit
    // brauchbarem K wäre ihr Umbenennen der Verlust von K: Die Datenbank
    // wird neu angelegt, und ohne Verpackung gäbe es kein Passwort mehr, mit
    // dem sie sich öffnen ließe. Strenger als der Wortlaut wäre hier also
    // schädlich; die Regel bleibt „nie überschreiben“, nicht „immer
    // umbenennen“.
    let include_wrapping = in_password_mode && matches!(key, StartOverKey::Issue);
    // Der Name steht **vor** der Bestätigung fest, damit der Dialog ihn
    // nennen kann (A5: „nennt im Dialog den neuen Dateinamen“).
    //
    // `None`, wenn es gar keine Datei zum Umbenennen gibt — das Feld
    // *fehlt* × *ungültig* der Tabelle A3 führt ebenfalls über D3 hierher
    // (spec-reviewer Runde 1). Vorher nannten Bestätigung und Meldung dort
    // einen Dateinamen, den es nicht gab.
    let plan = plan_start_over_renames(db_path, include_wrapping);
    let renamed_to = plan.main_target_name();
    if !prompt.confirm_start_over(renamed_to.as_deref()) {
        return Err(StartupAbort::UserQuit);
    }

    // **A5 im Passwort-Modus:** Das neue Passwort wird eingegeben, *bevor*
    // eine Datei angefasst wird — „Bricht der Nutzer das Einrichten ab,
    // bleibt alles unverändert.“ Deshalb steht die Abfrage hier, zwischen
    // der zweiten Bestätigung und dem ersten `rename`.
    let new_password = if include_wrapping {
        match prompt.ask_for_new_master_password() {
            Some(password) => {
                // **Und geprüft, bevor das erste `rename` läuft**
                // (Klarstellung 12, spec-reviewer Runde 2): Sonst stünde
                // die Datenbank schon unter ihrem neuen Namen, wenn das
                // Einrichten die fehlende Bestätigung ablehnt — „verändert
                // nichts" wäre nicht mehr wahr, und zurück bliebe ein
                // Verzeichnis ohne Verpackungsdatei, also beim nächsten
                // Start der Schlüsselbund-Modus.
                check_new_password_upfront(&password)?;
                Some(password)
            }
            None => return Err(StartupAbort::UserQuit),
        }
    } else {
        None
    };

    // Spec 0101, A5: **eigener Fall**, nicht `Other`. Der `Other`-Text rät
    // dazu, ein Backup einzuspielen — hier ist das falsch: `execute()` ist
    // alles-oder-nichts, die bisherige Datenbank liegt also unberührt an
    // ihrem Platz, und eine Namenskollision im Datenverzeichnis löst ein
    // Backup ohnehin nicht auf.
    plan.execute().map_err(|err| StartupAbort::Fatal {
        kind: ConnectFailureKind::StartOverFailed,
        detail: format!("Umbenennen fehlgeschlagen: {err}"),
    })?;

    // Erst nach dem Umbenennen ist der Platz der Verpackungsdatei frei —
    // A5: „alte Verpackungsdatei umbenannt (nicht überschrieben)“.
    let key = match (key, new_password) {
        (StartOverKey::Keep(key), _) => key,
        (StartOverKey::Issue, Some(password)) => {
            let key = ssh_manager_core::crypto::generate_root_key();
            // **Kein `set` auf den Schlüsselbund** (T7, wörtlich): Der Modus
            // bleibt, was er war (A5).
            set_up_password(db_path, &key, &password, None, FilesAlreadyMoved::Yes)?;
            key
        }
        (StartOverKey::Issue, None) => match access {
            RootKeyAccess::Keychain(store) => store_new_key_in_keychain(*store)?,
            // Kann nicht vorkommen: `include_wrapping` ist im
            // Passwort-Modus bei `Issue` wahr, also liegt ein Passwort vor.
            // Sichtbar scheitern statt einen Schlüssel irgendwohin zu
            // schreiben, wo ihn niemand erwartet.
            _ => {
                return Err(StartupAbort::Fatal {
                    kind: ConnectFailureKind::Other,
                    detail: "Passwort-Modus ohne neues Passwort beim Neuanfang".to_string(),
                })
            }
        },
    };

    if let Some(renamed_to) = renamed_to {
        prompt.notify_started_over(&renamed_to);
    }
    Ok(key)
}

/// Die Felder „K erzeugen“ der Tabelle A3 und D4.
///
/// Im Passwort-Modus heißt das: neues Passwort abfragen, K erzeugen, die
/// alte Verpackungsdatei umbenennen (nie überschreiben, A5), dann die neue
/// schreiben. Bricht der Nutzer ab, ist nichts verändert.
fn generate_key(
    db_path: &Path,
    access: &RootKeyAccess<'_>,
    prompt: &dyn StartupPrompt,
) -> Result<[u8; 32], StartupAbort> {
    match access {
        RootKeyAccess::Keychain(store) => store_new_key_in_keychain(*store),
        RootKeyAccess::Unlocked(_) | RootKeyAccess::UnusableWrapping => {
            let Some(password) = prompt.ask_for_new_master_password() else {
                return Err(StartupAbort::UserQuit);
            };
            // Dasselbe wie in `start_over`: **vor** dem Umbenennen der
            // alten Verpackung prüfen (Klarstellung 12, spec-reviewer
            // Runde 2).
            check_new_password_upfront(&password)?;
            let key = ssh_manager_core::crypto::generate_root_key();
            // Die alte Verpackung aus dem Weg räumen, bevor die neue
            // entsteht — und **umbenennen**, nicht löschen (A5).
            let renamed = crate::master_password::rename_wrapping_file(
                db_path,
                &format!(
                    ".unreadable-{}",
                    chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
                ),
            )
            .map_err(|err| StartupAbort::Fatal {
                kind: ConnectFailureKind::StartOverFailed,
                detail: format!("alte Verpackungsdatei nicht umbenennbar: {err}"),
            })?;
            if let Some(renamed) = &renamed {
                tracing::info!(
                    renamed_to = %renamed.display(),
                    "the previous wrapping file was renamed, not overwritten (Spec 0101, A5)"
                );
            }
            set_up_password(db_path, &key, &password, None, FilesAlreadyMoved::Yes)?;
            Ok(key)
        }
    }
}

fn store_new_key_in_keychain(store: &dyn CredentialStore) -> Result<[u8; 32], StartupAbort> {
    ssh_manager_core::crypto::generate_and_store_root_key(store).map_err(|err| {
        // Der Text von `CipherError::KeyStoreAccessFailed` trägt die
        // Bibliotheks-Nutzlast — die geht ins Log, nie in einen Dialog
        // (Spec 0098, A5).
        StartupAbort::Fatal {
            kind: ConnectFailureKind::Other,
            detail: format!("Wurzelschlüssel nicht anlegbar: {err}"),
        }
    })
}

/// A13 aus dem Startablauf heraus. Die Reihenfolge und der Vergleich liegen
/// in [`crate::master_password::set_up_master_password`]; hier wird nur der
/// Fehler in einen Startfehler übersetzt.
/// Was beim Scheitern über den Zustand zu sagen ist (Klarstellung 9,
/// spec-reviewer Lauf 4, Fund 2).
///
/// Der Fehlertext zu A13 sagt „Es ist nichts verändert — dein Schlüssel
/// liegt weiter dort, wo er lag". Das stimmt für das Einrichten aus den
/// Einstellungen und aus D1, aber **nicht** auf den Wegen über A5: Dort sind
/// Datenbank und alte Verpackung beim Einrichten schon zur Seite gelegt (die
/// Reihenfolge ist zwingend, s. ADR 0095 §4). Ein eigener Wert statt eines
/// `bool`, damit an der Aufrufstelle lesbar steht, welcher der beiden Fälle
/// gemeint ist.
enum FilesAlreadyMoved {
    No,
    Yes,
}

/// Klarstellung 12/A13 **vor** dem ersten `rename` (spec-reviewer Runde 2).
///
/// Die Fehlerart ist `MasterPasswordSetupFailed`, also die Variante „es ist
/// nichts verändert" — und das stimmt hier per Konstruktion, denn der
/// Aufrufer hat an dieser Stelle noch keine Datei angefasst.
fn check_new_password_upfront(password: &NewMasterPassword) -> Result<(), StartupAbort> {
    crate::master_password::check_new_password_before_touching_files(
        &password.password,
        &password.repeated,
        password.warning,
    )
    .map_err(|err| {
        tracing::warn!(
            "the new master password was rejected before any file was touched \
             (Spec 0101, A13, Klarstellung 12)"
        );
        StartupAbort::Fatal {
            kind: ConnectFailureKind::MasterPasswordSetupFailed,
            detail: format!("Master-Passwort nicht einrichtbar: {err}"),
        }
    })
}

fn set_up_password(
    db_path: &Path,
    key: &[u8; 32],
    password: &NewMasterPassword,
    keyring: Option<&dyn CredentialStore>,
    moved: FilesAlreadyMoved,
) -> Result<(), StartupAbort> {
    crate::master_password::set_up_master_password(
        db_path,
        key,
        &password.password,
        &password.repeated,
        password.warning,
        keyring,
    )
    .map_err(|err| {
        tracing::warn!(
            detail = err.detail_for_log().unwrap_or("-"),
            "setting up the master password during startup failed (Spec 0101, A13)"
        );
        StartupAbort::Fatal {
            kind: match moved {
                FilesAlreadyMoved::No => ConnectFailureKind::MasterPasswordSetupFailed,
                FilesAlreadyMoved::Yes => ConnectFailureKind::MasterPasswordSetupFailedAfterRename,
            },
            detail: format!("Master-Passwort nicht einrichtbar: {err}"),
        }
    })
}

async fn open_encrypted(db_path: &Path, root_key: &[u8; 32]) -> Result<OpenedDatabase, Abort> {
    let db_key = DatabaseKey::from_root_key(root_key);
    match SqliteProfileStore::connect_encrypted(db_path, &db_key).await {
        Ok(store) => Ok(OpenedDatabase {
            store,
            root_key: *root_key,
        }),
        Err(PersistenceError::NotReadableWithKey) => Err(Abort::NotReadable),
        Err(err) => {
            let kind = err.classify();
            Err(Abort::Fatal(StartupAbort::Fatal {
                detail: err.to_string(),
                kind,
            }))
        }
    }
}

async fn convert_then_open(
    db_path: &Path,
    root_key: &[u8; 32],
    lock: &DataDirLock,
) -> Result<OpenedDatabase, StartupAbort> {
    let db_key = DatabaseKey::from_root_key(root_key);
    tracing::info!("converting the plaintext database (Spec 0101, A6)");
    if let Err(err) = persistence_sqlite::convert_plaintext_database(db_path, &db_key, lock).await {
        let as_persistence = PersistenceError::Conversion(err);
        let kind = as_persistence.classify();
        // A6: Kein Weiterlauf im Klartext — der Start endet hier.
        return Err(StartupAbort::Fatal {
            detail: as_persistence.to_string(),
            kind,
        });
    }
    tracing::info!("plaintext database converted");
    match open_encrypted(db_path, root_key).await {
        Ok(opened) => Ok(opened),
        Err(Abort::NotReadable) => Err(StartupAbort::Fatal {
            kind: ConnectFailureKind::KeyMismatch,
            detail: "umgewandelte Datei nicht mit ihrem Schlüssel lesbar".to_string(),
        }),
        Err(Abort::Fatal(abort)) => Err(abort),
    }
}

/// Spec 0101, A5: der Umbenennungsplan für „Neu anfangen“.
///
/// **Ein Plan statt einer Schleife:** Der Zielname der Hauptdatei muss im
/// Dialog stehen, **bevor** umbenannt wird — und alle Dateien müssen
/// denselben Zeitstempel bekommen, damit sie als Satz erkennbar bleiben.
/// Deshalb wird der Plan einmal gebaut, dann angezeigt, dann ausgeführt.
pub struct StartOverPlan {
    renames: Vec<(PathBuf, PathBuf)>,
    main_target: PathBuf,
    /// Gibt es überhaupt eine Datenbank**datei**? Eine verwaiste `-wal`
    /// ohne sie ergibt ebenfalls einen nicht leeren Plan — dann darf der
    /// Dialog aber keinen Namen für die Hauptdatei nennen (spec-reviewer
    /// Runde 2).
    main_present: bool,
}

impl StartOverPlan {
    /// Der Dateiname (ohne Verzeichnis), den die bisherige Datenbank
    /// bekommt — `None`, wenn es keine Datenbankdatei gibt.
    pub fn main_target_name(&self) -> Option<String> {
        if !self.main_present {
            return None;
        }
        self.main_target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
    }

    /// Benennt alle vier möglichen Dateien um — **löscht nichts** (A5).
    ///
    /// Die Prüfung vor dem ersten `rename` ist der eigentliche Beweis
    /// dafür (spec-reviewer Runde 1): Der Zähler in
    /// [`plan_start_over_renames`] kann theoretisch auslaufen, und zwischen
    /// Planen und Ausführen liegt die zweite Bestätigung des Nutzers, also
    /// beliebig viel Zeit. `fs::rename` würde ein vorhandenes Ziel
    /// stillschweigend überschreiben; hier bricht es stattdessen ab.
    pub fn execute(&self) -> std::io::Result<()> {
        // **Erst alle Ziele prüfen, dann umbenennen** (spec-reviewer
        // Runde 2): Prüfen und Umbenennen in einer Schleife konnte den Satz
        // zur Hälfte ausführen — Hauptdatei umbenannt, `-wal` nicht. Der
        // nächste Start hätte dann *fehlt* gesehen und eine neue Datenbank
        // direkt neben das fremde `-wal` der alten gelegt. Genau die
        // Angriffsrichtung „alte `-wal` wird auf die neue Datei angewandt".
        for (_, to) in &self.renames {
            if std::fs::symlink_metadata(to).is_ok() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "{} existiert bereits — es wird nichts überschrieben",
                        to.display()
                    ),
                ));
            }
        }
        // **Die Hauptdatei zuletzt.** Scheitert ein `rename` trotz der
        // Prüfung (Rechte, Sperre), liegt sie noch an ihrem Platz — der
        // nächste Start erkennt sie wieder und legt keine neue Datenbank
        // neben halb verschobene Journaldateien. Der Plan führt sie als
        // erstes Element (s. `plan_start_over_renames`), deshalb `rev()`.
        for (from, to) in self.renames.iter().rev() {
            std::fs::rename(from, to)?;
        }
        Ok(())
    }
}

/// A5: baut den Plan. Benennt Datei, `-wal`, `-shm` und `-journal` um;
/// vorhandene Dateien nur, denn `rename` auf eine fehlende Datei wäre ein
/// Fehler, der den ganzen Vorgang abbräche.
///
/// `include_wrapping` nimmt die Verpackungsdatei des Passwort-Modus mit
/// (A5). **Nur, wenn sie selbst unbrauchbar ist** — die Begründung steht
/// bei [`StartOverKey`].
pub fn plan_start_over_renames(db_path: &Path, include_wrapping: bool) -> StartOverPlan {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let mut suffix = format!(".unreadable-{stamp}");
    // „löscht nichts“: Trägt schon etwas diesen Namen (zwei Versuche in
    // derselben Sekunde, oder ein Rest eines früheren Versuchs), wird ein
    // Zähler angehängt, statt die vorhandene Datei zu überschreiben.
    for attempt in 1..1000 {
        if !any_target_exists(db_path, &suffix)
            && !(include_wrapping
                && std::fs::symlink_metadata(with_suffix(
                    &crate::master_password::wrapping_file_path(db_path),
                    &suffix,
                ))
                .is_ok())
        {
            break;
        }
        suffix = format!(".unreadable-{stamp}-{attempt}");
    }

    let mut renames = Vec::new();
    let main_target = with_suffix(db_path, &suffix);
    // `-journal` mit (spec-reviewer Runde 1): A5 nennt wörtlich nur Datei,
    // `-wal` und `-shm`, aber A6 Schritt 4 entfernt `-journal` „aus
    // demselben Grund mit" — ein altes Rollback-Journal neben einer neuen,
    // leeren Datenbank gehört zu einer anderen Datei. Strenger als der
    // Wortlaut, nie nachlässiger.
    for candidate in [
        db_path.to_path_buf(),
        sibling(db_path, "-wal"),
        sibling(db_path, "-shm"),
        sibling(db_path, "-journal"),
    ] {
        // `symlink_metadata`, nicht `exists`: Eine Verknüpfung ins Leere
        // soll ebenfalls umbenannt werden, statt liegen zu bleiben.
        if std::fs::symlink_metadata(&candidate).is_ok() {
            let target = with_suffix(&candidate, &suffix);
            renames.push((candidate, target));
        }
    }

    // A5: „… und (Passwort-Modus) die Verpackungsdatei“. Sie steht
    // **hinter** den Journaldateien, wird also vor der Hauptdatei
    // umbenannt (s. `execute`, `rev()`): Scheitert es dort, liegt die
    // Datenbank noch an ihrem Platz und der nächste Start sieht denselben
    // Zustand wieder, statt eine neue Datenbank ohne Verpackung anzulegen.
    if include_wrapping {
        let wrapping = crate::master_password::wrapping_file_path(db_path);
        if std::fs::symlink_metadata(&wrapping).is_ok() {
            let target = with_suffix(&wrapping, &suffix);
            renames.push((wrapping, target));
        }
    }

    let main_present = std::fs::symlink_metadata(db_path).is_ok();

    StartOverPlan {
        renames,
        main_target,
        main_present,
    }
}

fn any_target_exists(db_path: &Path, suffix: &str) -> bool {
    [
        db_path.to_path_buf(),
        sibling(db_path, "-wal"),
        sibling(db_path, "-shm"),
        sibling(db_path, "-journal"),
    ]
    .iter()
    .any(|p| std::fs::symlink_metadata(with_suffix(p, suffix)).is_ok())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn sibling(db_path: &Path, suffix: &str) -> PathBuf {
    with_suffix(db_path, suffix)
}

/// Issue #19: Sperre auf das Datenverzeichnis, mit einem echten zweiten
/// Prozess.
#[cfg(test)]
mod instance_lock_tests;
#[cfg(test)]
mod tests;
