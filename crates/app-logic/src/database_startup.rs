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
    detect_database_file_state, ConnectFailureKind, DatabaseFileState, PersistenceError,
    SqliteProfileStore,
};
use ssh_manager_core::crypto::{DatabaseKey, RootKeyState};
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
}

/// Was `app-shell` an nativen Dialogen beisteuert. Als Trait, damit T3/T7/T8
/// den gesamten Ablauf ohne Fenster prüfen können — und damit ein Test
/// **zählen** kann, was gefragt wurde.
pub trait StartupPrompt {
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
}

/// Warum der Start nicht zu einer offenen Datenbank geführt hat.
#[derive(Debug)]
pub enum StartupAbort {
    /// Der Nutzer hat „Beenden“ gewählt — kein Fehlerdialog mehr, es ist
    /// bereits alles gesagt.
    UserQuit,
    /// Ein Fehler, für den [`crate::startup_error_messages::
    /// db_connect_failure_text`] den Text liefert.
    Fatal {
        kind: ConnectFailureKind,
        /// Für die Log-Zeile; nie für einen Dialog.
        detail: String,
    },
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
pub async fn open_or_prepare_database(
    db_path: &Path,
    credential_store: &dyn CredentialStore,
    keychain: KeychainAvailability,
    prompt: &dyn StartupPrompt,
) -> Result<OpenedDatabase, StartupAbort> {
    // spec-reviewer Runde 1: A6 verweigert die Umwandlung eines Symlinks
    // (T20), aber `detect_database_file_state` folgt ihm — eine
    // Verknuepfung auf ein **nicht vorhandenes** Ziel ergab damit *fehlt*,
    // und `create_if_missing` legte die neue, verschluesselte Datenbank am
    // Ziel der Verknuepfung an. Diese Asymmetrie war unbeabsichtigt. Jetzt
    // gilt die Regel aus A6 fuer **jeden** Weg: Ist `smart-ssh.db` eine
    // Verknuepfung, endet der Start mit derselben Meldung, und es wird
    // nichts angelegt, geoeffnet oder veraendert.
    if std::fs::symlink_metadata(db_path)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(StartupAbort::Fatal {
            kind: ConnectFailureKind::SymlinkedDatabase,
            detail: "die Datenbankdatei ist eine symbolische Verknuepfung".to_string(),
        });
    }

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
        let (key_state, root_key) = read_key_state(credential_store, keychain);
        let plan = decide_startup(file, &key_state);
        tracing::info!(
            ?file,
            ?key_state,
            ?plan,
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
                        start_over(db_path, prompt, StartupDialog::D2)?;
                        return open_encrypted(db_path, &key)
                            .await
                            .map_err(fatal_after_start_over);
                    }
                    Err(Abort::Fatal(abort)) => return Err(abort),
                }
            }
            StartupPlan::GenerateKeyThenCreateFresh => {
                let key = generate_key(credential_store)?;
                return open_encrypted(db_path, &key)
                    .await
                    .map_err(fatal_after_start_over);
            }
            StartupPlan::Convert => {
                let Some(key) = root_key else {
                    return Err(missing_key_despite_present());
                };
                return convert_then_open(db_path, &key).await;
            }
            StartupPlan::GenerateKeyThenConvert => {
                let key = generate_key(credential_store)?;
                return convert_then_open(db_path, &key).await;
            }
            StartupPlan::Dialog(StartupDialog::D1 {
                offers_password_setup,
            }) => match prompt.ask(StartupDialog::D1 {
                offers_password_setup,
            }) {
                // „Erneut versuchen“ prüft ohne Neustart erneut (A3, D1) —
                // der einzige Weg zurück an den Anfang der Schleife.
                StartupChoice::Retry => continue,
                _ => return Err(StartupAbort::UserQuit),
            },
            // Verschlüsselte Datei, kein K-Eintrag: dieselbe Frage wie
            // oben, nur schon vor dem Öffnen entschieden.
            StartupPlan::Dialog(StartupDialog::D2) => {
                start_over(db_path, prompt, StartupDialog::D2)?;
                let key = generate_key(credential_store)?;
                return open_encrypted(db_path, &key)
                    .await
                    .map_err(fatal_after_start_over);
            }
            StartupPlan::Dialog(StartupDialog::D3) => {
                start_over(db_path, prompt, StartupDialog::D3)?;
                // D3: „Der vorhandene Eintrag bzw. die Datei wird **erst
                // nach dieser Wahl** ersetzt." Der unbrauchbare Eintrag wird
                // jetzt überschrieben — ohne das bliebe der Zustand
                // *ungültig* und derselbe Dialog käme beim nächsten Start
                // wieder.
                let key = generate_key(credential_store)?;
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
                // Verlauf ist verloren.
                let key = generate_key(credential_store)?;
                return convert_then_open(db_path, &key).await;
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
        detail: "Schluesselzustand Present, aber kein Schluessel geliefert".to_string(),
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
fn start_over(
    db_path: &Path,
    prompt: &dyn StartupPrompt,
    dialog: StartupDialog,
) -> Result<(), StartupAbort> {
    if prompt.ask(dialog) != StartupChoice::StartOver {
        return Err(StartupAbort::UserQuit);
    }
    // Der Name steht **vor** der Bestätigung fest, damit der Dialog ihn
    // nennen kann (A5: „nennt im Dialog den neuen Dateinamen“).
    //
    // `None`, wenn es gar keine Datei zum Umbenennen gibt — das Feld
    // *fehlt* × *ungültig* der Tabelle A3 führt ebenfalls über D3 hierher
    // (spec-reviewer Runde 1). Vorher nannten Bestätigung und Meldung dort
    // einen Dateinamen, den es nicht gab.
    let plan = plan_start_over_renames(db_path);
    let renamed_to = plan.main_target_name();
    if !prompt.confirm_start_over(renamed_to.as_deref()) {
        return Err(StartupAbort::UserQuit);
    }
    plan.execute().map_err(|err| StartupAbort::Fatal {
        kind: ConnectFailureKind::Other,
        detail: format!("Umbenennen fehlgeschlagen: {err}"),
    })?;
    if let Some(renamed_to) = renamed_to {
        prompt.notify_started_over(&renamed_to);
    }
    Ok(())
}

fn generate_key(credential_store: &dyn CredentialStore) -> Result<[u8; 32], StartupAbort> {
    ssh_manager_core::crypto::generate_and_store_root_key(credential_store).map_err(|err| {
        // Der Text von `CipherError::KeyStoreAccessFailed` trägt die
        // Bibliotheks-Nutzlast — die geht ins Log, nie in einen Dialog
        // (Spec 0098, A5).
        StartupAbort::Fatal {
            kind: ConnectFailureKind::Other,
            detail: format!("Wurzelschlüssel nicht anlegbar: {err}"),
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
) -> Result<OpenedDatabase, StartupAbort> {
    let db_key = DatabaseKey::from_root_key(root_key);
    tracing::info!("converting the plaintext database (Spec 0101, A6)");
    if let Err(err) = persistence_sqlite::convert_plaintext_database(db_path, &db_key).await {
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
}

impl StartOverPlan {
    /// Der Dateiname (ohne Verzeichnis), den die bisherige Datenbank
    /// bekommt — `None`, wenn es keine Datenbankdatei gibt.
    pub fn main_target_name(&self) -> Option<String> {
        if self.renames.is_empty() {
            return None;
        }
        self.main_target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
    }

    /// Benennt um — **löscht nichts** (A5).
    ///
    /// Die Prüfung direkt vor jedem `rename` ist der eigentliche Beweis
    /// dafür (spec-reviewer Runde 1): Der Zähler in
    /// [`plan_start_over_renames`] kann theoretisch auslaufen, und zwischen
    /// Planen und Ausführen liegt die zweite Bestätigung des Nutzers, also
    /// beliebig viel Zeit. `fs::rename` würde ein vorhandenes Ziel
    /// stillschweigend überschreiben; hier bricht es stattdessen ab.
    pub fn execute(&self) -> std::io::Result<()> {
        for (from, to) in &self.renames {
            if std::fs::symlink_metadata(to).is_ok() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "{} existiert bereits — es wird nichts überschrieben",
                        to.display()
                    ),
                ));
            }
            std::fs::rename(from, to)?;
        }
        Ok(())
    }
}

/// A5: baut den Plan. Benennt Datei, `-wal` und `-shm` um (die
/// Verpackungsdatei kommt in Etappe 3 dazu); vorhandene Dateien nur, denn
/// `rename` auf eine fehlende Datei wäre ein Fehler, der den ganzen Vorgang
/// abbräche.
pub fn plan_start_over_renames(db_path: &Path) -> StartOverPlan {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let mut suffix = format!(".unreadable-{stamp}");
    // „löscht nichts“: Trägt schon etwas diesen Namen (zwei Versuche in
    // derselben Sekunde, oder ein Rest eines früheren Versuchs), wird ein
    // Zähler angehängt, statt die vorhandene Datei zu überschreiben.
    for attempt in 1..1000 {
        if !any_target_exists(db_path, &suffix) {
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

    StartOverPlan {
        renames,
        main_target,
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

#[cfg(test)]
mod tests;
