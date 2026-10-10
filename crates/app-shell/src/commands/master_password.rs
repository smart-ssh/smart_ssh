//! Spec 0101, A13/A15–A18: die Kommandos rund um das Master-Passwort —
//! Entsperren beim Start und der Moduswechsel in den Einstellungen.
//!
//! **Zwei Gruppen, und das ist keine Kosmetik:**
//!
//! Die erste Gruppe läuft **vor** dem `AppState` und steht deshalb in der
//! Positivliste des [`StartupGate`](crate::startup_gate::StartupGate)
//! (A16): `get_startup_state`, `unlock_with_master_password`,
//! `answer_startup_prompt`, `quit_application`. Keines davon nimmt
//! `State<AppState>` — es gibt ihn noch nicht.
//!
//! Die zweite Gruppe sind die Einstellungen (A15/A18). Sie laufen im
//! normalen Betrieb, also **nach** der Entsperrung, und sind damit vom Tor
//! nicht betroffen.
//!
//! **Kein Passwort verlässt das Backend** (A19): Die Kommandos nehmen es
//! als `String` an (das IPC kann nichts anderes) und verpacken es
//! unmittelbar in ein `SecretString`, das beim Freigeben überschrieben
//! wird. Kein Rückgabewert und kein DTO dieses Moduls trägt ein Passwort
//! oder einen Schlüssel.

use std::sync::Arc;

use secrecy::SecretString;
use serde::Serialize;
use tauri::{Emitter, Manager};

use app_logic::database_startup::{NewMasterPassword, RootKeyAccess, StartupAbort};
use app_logic::error::{CommandError, CommandResult};
use app_logic::master_password::{self, KeyMode};
use app_logic::state::AppState;

use crate::startup_gate::StartupGate;
use crate::window_prompt::{StartupPromptAnswer, WindowStartupPrompt};

/// Das Ereignis, mit dem das Fenster erfährt, dass der Zustand jetzt steht.
pub const UNLOCKED_EVENT: &str = "startup:unlocked";

/// Fehlercodes dieser Etappe (A20).
pub const WRONG_MASTER_PASSWORD_CODE: &str = "WRONG_MASTER_PASSWORD";
pub const MASTER_PASSWORD_FILE_FAILED_CODE: &str = "MASTER_PASSWORD_FILE_FAILED";
/// review-09 (Runde 2, Teil A, Fund 2 „Rest"): `refuse_to_start_over` meldete
/// für `WrappingHealth::Unreachable` **und** `WrappingHealth::Absent`
/// denselben Code und Text — „gerade nicht zu lesen oder zu schreiben".
/// Stimmt für `Unreachable` (die Datei liegt da, ein I/O- oder Rechte-Fehler
/// verhindert den Zugriff), aber nicht für `Absent`: Dort liegt **keine**
/// Datei an diesem Ort, der Modus ist längst der Schlüsselbund-Modus
/// ([`mode_for`]), und „war gerade nicht lesbar" behauptet
/// einen Zustand, der nicht vorliegt — derselbe Fehlertyp wie bei
/// `KEYCHAIN_KEY_MISMATCH_CODE` (Klarstellung 9, Punkt 5).
pub const MASTER_PASSWORD_FILE_ABSENT_CODE: &str = "MASTER_PASSWORD_FILE_ABSENT";
pub const MASTER_PASSWORD_REJECTED_CODE: &str = "MASTER_PASSWORD_REJECTED";
pub const STARTUP_FAILED_CODE: &str = "STARTUP_FAILED";
/// Klarstellung 10b: Der Wechsel auf den Schlüsselbund würde einen fremden
/// Schlüssel ersetzen und wartet auf eine ausdrückliche Bestätigung.
///
/// **Ein eigener Code, kein Dateifehler:** Die Oberfläche muss diesen Fall
/// an der Antwort erkennen können, denn nur er hat einen zweiten,
/// bestätigenden Aufruf als Fortsetzung — alles andere ist „erneut
/// versuchen". Der Text dazu entsteht mit dem Dialog in Commit 11.
pub const KEYCHAIN_HOLDS_ANOTHER_KEY_CODE: &str = "KEYCHAIN_HOLDS_ANOTHER_KEY";
/// Klarstellung 9, Punkt 5 („Fehlertexte stimmen mit dem Zustand überein,
/// den sie beschreiben"), Review-Fund Runde 1 zu Commit 11: Der Schlüssel im
/// Schlüsselbund ist **nicht** der, mit dem diese Datenbank offen ist.
///
/// **Ein eigener Code, obwohl die Datei-Fehler bewusst einen gemeinsamen
/// haben:** Die Oberfläche übersetzt den Code und zeigt den Text des
/// Backends nicht (Spec 0024, Abschnitt 5 — sonst stünde im englischen
/// Fenster ein deutscher Satz). Unter `MASTER_PASSWORD_FILE_FAILED` las der
/// Nutzer deshalb „Die Schlüsseldatei neben deiner Datenbank ließ sich nicht
/// lesen" — im Schlüsselbund-Modus gibt es gar keine, und die Ursache liegt
/// woanders. Ein Orakel ist dieser Code nicht: Er sagt nichts über die
/// Existenz einer Verpackungsdatei, nur etwas über den Schlüsselbund, dessen
/// Modus die Oberfläche ohnehin anzeigt (A18).
pub const KEYCHAIN_KEY_MISMATCH_CODE: &str = "KEYCHAIN_KEY_MISMATCH";
/// Derselbe Grund: Der Vorgang passt nicht zum aktiven Modus (A18) —
/// einrichten, obwohl schon ein Passwort gilt, oder wechseln, obwohl keines
/// gilt. Das ist kein Dateifehler und soll nicht als einer gemeldet werden.
pub const MASTER_PASSWORD_MODE_MISMATCH_CODE: &str = "MASTER_PASSWORD_MODE_MISMATCH";
/// Klarstellung 12 (A13/E10): Die Warnung ist nicht ausdrücklich bestätigt.
///
/// **Nicht `MASTER_PASSWORD_REJECTED`**, obwohl die Prüfung neben Länge und
/// Wiederholung steht: Dessen Text nennt die Mindestlänge und die
/// Wiederholung — beide können hier in Ordnung sein, und ein Text, der vom
/// falschen Zustand spricht, ist genau der Fehler aus Klarstellung 9,
/// Punkt 5. Über die Oberfläche ist dieser Code unerreichbar (der Knopf
/// bleibt ohne Häkchen aus); er beschreibt einen Aufruf, der die Maske
/// umgangen hat, und soll deshalb auch so heißen.
pub const MASTER_PASSWORD_WARNING_NOT_CONFIRMED_CODE: &str =
    "MASTER_PASSWORD_WARNING_NOT_CONFIRMED";

/// Klarstellung 11: Nach wie vielen gescheiterten Entsperrversuchen eines
/// **Programmlaufs** die Maske „Neu anfangen" anbietet.
///
/// **Warum es diese Zahl überhaupt gibt** (Q-BL-0314-01, K3 entschieden):
/// Nach A17 sind „Passwort vergessen" und „ein Byte im Chiffrat gekippt"
/// nicht unterscheidbar. Der Riegel aus Klarstellung 9 lehnt beide ab,
/// solange die Datei einen brauchbaren Kopf hat — richtig gegen den
/// Fehlgriff, aber damit hatte dauerhaft gescheiterte Authentifizierung in
/// der App **keinen Ausweg**. Drei Versuche sind die Schwelle, ab der ein
/// Tippfehler als Erklärung ausfällt.
///
/// **Was den Ausweg weiter eng hält:** Gezählt wird nur eine gescheiterte
/// *Authentifizierung* (nicht ein Datei- oder Schlüsselbundfehler), der
/// Zähler liegt nur im Speicher und beginnt bei jedem Start bei null, eine
/// *nicht erreichbare* Verpackungsdatei bietet den Ausweg **nie**, und
/// dahinter liegen unverändert die zweite Bestätigung aus A5 und — im
/// Passwort-Modus — das Einrichten eines neuen Passworts.
const ATTEMPTS_BEFORE_STARTING_OVER_IS_OFFERED: u32 = 3;

/// Was zum Nachbauen des Zustands nach der Entsperrung gebraucht wird.
///
/// **Wird immer verwaltet**, auch im Schlüsselbund-Modus: Sonst wäre
/// `get_startup_state` ein Kommando mit nicht verwaltetem `State` und
/// scheiterte dort mit einer Meldung über `.manage()` statt mit einer
/// Antwort (gemessen, M1).
pub struct PendingStartup {
    inputs: crate::StartupInputs,
    entitlements: Arc<dyn ssh_manager_core::entitlements::EntitlementProvider>,
    /// Der Fragesteller im Fenster. Entsteht beim ersten Entsperrversuch,
    /// weil er den `AppHandle` braucht.
    prompt: std::sync::Mutex<Option<Arc<WindowStartupPrompt>>>,
    /// Serialisiert jeden Weg, der den Zustand aufbaut (spec-reviewer
    /// Lauf 4, Fund 6).
    ///
    /// **Ein `tokio::sync::Mutex`, nicht der aus `std`:** Der kritische
    /// Abschnitt enthält `await`-Punkte (Datenbank öffnen, Secret-Umzug,
    /// Startdialoge). Eine `std`-Sperre über ein `await` zu halten blockiert
    /// einen Laufzeit-Thread und ist in einem `Send`-Future nicht einmal
    /// erlaubt.
    ///
    /// Die Sperre deckt **beide** Wege: Entsperren und „Neu anfangen". Das
    /// ist der Punkt — der zweite benennt Dateien um, und zwei davon
    /// gleichzeitig wäre der schlechteste Augenblick für ein Rennen.
    unlock_lock: tokio::sync::Mutex<()>,
    /// Klarstellung 11: gescheiterte Entsperrversuche **dieses
    /// Programmlaufs**.
    ///
    /// **Nur im Speicher, und das ist die Zusage:** Vor der Entsperrung
    /// wird für diesen Zähler nichts geschrieben — keine Datei, keine
    /// Einstellung, kein Datenbankeintrag (vor der Entsperrung gibt es
    /// ohnehin keine offene Datenbank). Ein Neustart beginnt bei null. Ein
    /// dauerhafter Zähler wäre eine neue Datensenke neben der Datenbank und
    /// zugleich ein Weg, den Ausweg durch Warten statt durch Versuche
    /// freizuschalten.
    failed_unlock_attempts: std::sync::atomic::AtomicU32,
}

impl PendingStartup {
    pub fn new(
        inputs: crate::StartupInputs,
        entitlements: Arc<dyn ssh_manager_core::entitlements::EntitlementProvider>,
    ) -> Self {
        Self {
            inputs,
            entitlements,
            prompt: std::sync::Mutex::new(None),
            unlock_lock: tokio::sync::Mutex::new(()),
            failed_unlock_attempts: std::sync::atomic::AtomicU32::new(0),
        }
    }

    fn failed_unlock_attempts(&self) -> u32 {
        self.failed_unlock_attempts
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Klarstellung 11: **nur** eine gescheiterte Authentifizierung zählt.
    ///
    /// Ein Datei- oder Schlüsselbundfehler zählt nicht: Er sagt nichts
    /// darüber, ob das Passwort passt, und würde den Ausweg öffnen, obwohl
    /// die Verpackung vielleicht vollkommen in Ordnung ist (Klarstellung
    /// 10a).
    fn note_failed_unlock_attempt(&self) -> u32 {
        let count = self
            .failed_unlock_attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        tracing::info!(
            attempts = count,
            threshold = ATTEMPTS_BEFORE_STARTING_OVER_IS_OFFERED,
            "a master-password unlock attempt failed to authenticate (Spec 0101, A17, \
             Klarstellung 11)"
        );
        count
    }

    fn prompt_for(&self, app: &tauri::AppHandle) -> Arc<WindowStartupPrompt> {
        let mut slot = self.prompt.lock().expect("Prompt-Sperre");
        if let Some(prompt) = slot.as_ref() {
            return prompt.clone();
        }
        let prompt = Arc::new(WindowStartupPrompt::new(
            app.clone(),
            self.inputs.db_path.clone(),
            self.inputs.language,
            self.inputs.keychain,
        ));
        *slot = Some(prompt.clone());
        prompt
    }
}

/// Welche Maske der Start zeigt — **ein** Feld, nicht mehrere Flaggen.
///
/// Vorher trug das DTO `unlocked`, `needs_unlock` und `wrapping_unusable`:
/// drei Wahrheitswerte, aus denen die Oberfläche einen Zustand
/// zusammensetzen musste — und einer der vier Zustände aus A3 war darin
/// nicht abbildbar (ADR 0097 §4.2: *nicht erreichbar* führte zu
/// `needs_unlock = true`, der Nutzer sah also ein Passwortfeld für eine
/// Datei, die gar nicht gelesen werden konnte). Ein Feld mit vier Werten
/// kann sich nicht selbst widersprechen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupScreen {
    /// Der Zustand steht; die Oberfläche zeigt die App.
    Unlocked,
    /// A16: Entsperrmaske mit Passwortfeld (A3-Spalte *da* nach dem
    /// Entsperren).
    Unlock,
    /// A3 *ungültig* (D3/D4), Klarstellung 9/10a: Die Datei wurde gelesen
    /// und gibt mit **keinem** Passwort K her. Kein Passwortfeld, sondern
    /// der Ausweg „Neu anfangen".
    UnusableWrapping,
    /// A3 *nicht erreichbar* (D1), Klarstellung 10a/11: Die Datei liegt an
    /// ihrem Platz, ließ sich aber nicht lesen. „Erneut versuchen" /
    /// „Beenden", **kein** „Neu anfangen" — über den Inhalt ist nichts
    /// gesagt, und morgen ist die Datei vielleicht wieder lesbar.
    UnreachableWrapping,
    /// §2 Frage 3 / A13: Schlüsselbund-Modus, aber der Zustand steht
    /// nicht — der Start ist in D1 gelandet und soll mit dem Einrichten
    /// eines Master-Passworts im Fenster weitergehen.
    SetUpMasterPassword,
}

impl StartupScreen {
    fn name(self) -> &'static str {
        match self {
            Self::Unlocked => "unlocked",
            Self::Unlock => "unlock",
            Self::UnusableWrapping => "unusableWrapping",
            Self::UnreachableWrapping => "unreachableWrapping",
            Self::SetUpMasterPassword => "setUpMasterPassword",
        }
    }
}

/// Die Entscheidung über die Startmaske — **eine** Stelle für zwei
/// Verbraucher.
///
/// `offers_start_over` beantwortet dieselbe Frage für die Maske (darf der
/// Knopf erscheinen?) und für [`start_over_from_unlock_screen`] (darf der
/// Aufruf angenommen werden?). Zwei getrennte Fassungen davon wären zwei
/// Gelegenheiten, sie auseinanderlaufen zu lassen — und die gefährliche
/// Richtung ist die, in der der Aufruf mehr annimmt als die Maske anbietet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartupScreenDecision {
    screen: StartupScreen,
    offers_start_over: bool,
}

/// A3/A16, Klarstellung 9 + 11 — rein, damit die Tabelle unten prüfbar ist.
///
/// `health` ist `None`, solange der Zustand schon steht (entsperrt): Dann
/// ist über die Datei nichts zu sagen, und ein Lesefehler wäre bloß Lärm.
fn decide_startup_screen(
    health: Option<master_password::WrappingHealth>,
    failed_unlock_attempts: u32,
) -> StartupScreenDecision {
    use master_password::WrappingHealth as H;
    let Some(health) = health else {
        return StartupScreenDecision {
            screen: StartupScreen::Unlocked,
            offers_start_over: false,
        };
    };
    // **Die alte Prüfung bleibt wörtlich stehen und wird mit der neuen
    // ODER-verknüpft** (Klarstellung 11 lockert Klarstellung 9, und das ist
    // die einzige Lockerung dieser Spec): `allows_starting_over` ist eine
    // Positivliste über `WrappingHealth` — ein künftiger Zustand, den
    // niemand bedacht hat, führt dort **nicht** zu einem neuen K. Diese
    // Fassung kann damit per Konstruktion nicht weniger ablehnen als die
    // alte; sie kommt nur in der einen, entschiedenen Lage dazu.
    //
    // Der Aufruf steht hier und nicht bloß in einem Test (Review-Fund Runde
    // 1 zu Commit 11): Sonst wäre der dokumentierte Riegel Testcode, und
    // wer ihn verschärft, härtete die falsche Funktion.
    let after_three_failed_attempts =
        health == H::Usable && failed_unlock_attempts >= ATTEMPTS_BEFORE_STARTING_OVER_IS_OFFERED;
    let offers_start_over = health.allows_starting_over() || after_three_failed_attempts;
    match health {
        // Keine Verpackungsdatei: Schlüsselbund-Modus. Dass der Zustand
        // trotzdem nicht steht, heißt, dass der Start in D1 gelandet ist
        // (§2 Frage 3).
        H::Absent => StartupScreenDecision {
            screen: StartupScreen::SetUpMasterPassword,
            offers_start_over: false,
        },
        // Klarstellung 11: der einzige Zustand, in dem Versuche zählen.
        H::Usable => StartupScreenDecision {
            screen: StartupScreen::Unlock,
            offers_start_over,
        },
        // Klarstellung 9: Hier gibt es nichts zu entsperren — kein
        // Passwortfeld, und der Ausweg steht sofort offen.
        H::Unusable => StartupScreenDecision {
            screen: StartupScreen::UnusableWrapping,
            offers_start_over,
        },
        // Klarstellung 11, letzter Satz: **nie** „Neu anfangen", auch nicht
        // nach beliebig vielen Versuchen. Ein Rechte- oder E/A-Fehler mit
        // dem Aufgeben von K zu beantworten wäre der schlechteste Tausch
        // dieser Spec.
        H::Unreachable => StartupScreenDecision {
            screen: StartupScreen::UnreachableWrapping,
            offers_start_over: false,
        },
    }
}

/// Was die Oberfläche beim Start anzeigen soll (A16/A18).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupStateDto {
    /// Welche Maske — s. [`StartupScreen`]. `"unlocked"` heißt: die App.
    pub screen: &'static str,
    /// A18: der aktive Modus, `"password"` oder `"keychain"`.
    pub mode: &'static str,
    /// Klarstellung 11: Darf die Maske „Neu anfangen" anbieten? Die
    /// Oberfläche entscheidet das nicht selbst — sie fragt, und
    /// [`start_over_from_unlock_screen`] prüft dieselbe Bedingung noch
    /// einmal selbst.
    pub offers_start_over: bool,
    /// Klarstellung 11: gescheiterte Entsperrversuche dieses Programmlaufs.
    /// Für den Hinweistext der Maske („nach drei Versuchen …"); die
    /// Entscheidung hängt nicht an diesem Wert, sondern an
    /// `offers_start_over`.
    pub failed_unlock_attempts: u32,
    /// Die Sprache der Startmasken, `"de"` oder `"en"`.
    ///
    /// **Warum aus dem Backend und nicht aus der Einstellungsdatei:**
    /// Klarstellung 9 verlangt, dass Einstellungsdateien vor der
    /// Entsperrung nicht lesbar sind — das `store`-Plugin ist dort noch
    /// nicht registriert (s. `crate::run`). Die Sprache ist deshalb die,
    /// die der Start einmal aus der Umgebung bestimmt hat (Spec 0071,
    /// A11a/A11b: **eine** Sprachwahl für alle Startdialoge eines
    /// Programmlaufs). Nach der Entsperrung gilt wieder die gespeicherte
    /// Wahl aus Spec 0024.
    pub language: &'static str,
}

fn mode_name(mode: KeyMode) -> &'static str {
    match mode {
        KeyMode::Password => "password",
        KeyMode::Keychain => "keychain",
    }
}

/// Der Modus aus demselben Blick auf die Datei, aus dem auch ihr Zustand
/// kommt (s. [`startup_state`]). Dasselbe Kriterium wie
/// [`master_password::key_mode`]: Es gibt eine Verpackungsdatei an diesem
/// Ort oder nicht.
fn mode_for(health: master_password::WrappingHealth) -> KeyMode {
    match health {
        master_password::WrappingHealth::Absent => KeyMode::Keychain,
        _ => KeyMode::Password,
    }
}

/// A16/A18: Was ist zu zeigen? Das erste Kommando, das die Oberfläche beim
/// Start aufruft.
#[tauri::command]
pub fn get_startup_state(
    gate: tauri::State<'_, Arc<StartupGate>>,
    pending: tauri::State<'_, PendingStartup>,
) -> StartupStateDto {
    startup_state(&gate, &pending)
}

/// Derselbe Inhalt wie [`get_startup_state`], aber ohne `tauri::State` —
/// damit die Kommandos unten ihn nach dem Entsperren noch bilden können,
/// ohne die Zustandsgriffe weiterzugeben.
fn startup_state(gate: &StartupGate, pending: &PendingStartup) -> StartupStateDto {
    let db_path = &pending.inputs.db_path;
    let unlocked = gate.is_unlocked();
    // **Eine Lesung entscheidet über Modus und Dateizustand**, solange der
    // Zustand nicht steht. `key_mode` und `wrapping_health` stellen
    // dieselbe Frage an dieselbe Datei (`symlink_metadata`); getrennt
    // gefragt könnten sie sich widersprechen, wenn die Datei dazwischen
    // verschwindet — und der Widerspruch landete in der Maske. Nach der
    // Entsperrung wird die Datei nicht mehr gelesen: Der Zustand ist gebaut,
    // ein Lesefehler wäre bloß Lärm.
    let (mode, health) = if unlocked {
        (master_password::key_mode(db_path), None)
    } else {
        let health = master_password::wrapping_health(db_path);
        (mode_for(health), Some(health))
    };
    let decision = decide_startup_screen(health, pending.failed_unlock_attempts());
    StartupStateDto {
        screen: decision.screen.name(),
        mode: mode_name(mode),
        offers_start_over: decision.offers_start_over,
        failed_unlock_attempts: pending.failed_unlock_attempts(),
        language: match pending.inputs.language {
            app_logic::startup_error_messages::Language::De => "de",
            app_logic::startup_error_messages::Language::En => "en",
        },
    }
}

/// A16: Entsperren und den Zustand nachbauen.
///
/// **Was hier nie passiert** (A16, wörtlich): Bei einem falschen Passwort
/// wird die Datenbank nicht geöffnet, nichts umbenannt und **nie** ein
/// neuer Schlüssel erzeugt. Der Fehler geht an die Maske zurück, das Tor
/// bleibt zu, und der Nutzer kann es erneut versuchen.
#[tauri::command]
pub async fn unlock_with_master_password(
    app: tauri::AppHandle,
    password: String,
    gate: tauri::State<'_, Arc<StartupGate>>,
    pending: tauri::State<'_, PendingStartup>,
) -> CommandResult<StartupStateDto> {
    // **Ein Entsperrvorgang zur Zeit** (spec-reviewer Lauf 4, Fund 6).
    // Die `is_unlocked()`-Prüfung allein genügt nicht: Zwei gleichzeitige
    // Aufrufe kämen beide daran vorbei und bauten beide einen vollständigen
    // Zustand auf — zwei Verbindungspools, zwei Secret-Umzüge, zwei
    // Aufräumläufe auf dem Schlüsselbund. Das Schloss wird **vor** der
    // Prüfung genommen, damit der zweite Aufruf den fertigen Zustand sieht.
    let _serialized = pending.unlock_lock.lock().await;
    if gate.is_unlocked() {
        // Schon entsperrt — ein zweiter Aufruf darf den Zustand nicht
        // ersetzen (gemessen M3: `manage` würde ihn ohnehin nicht
        // ersetzen, aber der ganze Aufbau liefe erneut).
        return Ok(startup_state(&gate, &pending));
    }

    let password = SecretString::from(password);
    let db_path = pending.inputs.db_path.clone();
    let mode = master_password::key_mode(&db_path);

    // Im Passwort-Modus zuerst die Verpackung öffnen; ohne sie gibt es kein
    // K und damit keinen Weg weiter. Im Modus „Einrichten aus D1" (Teil 0
    // Frage 3) gibt es noch keine Verpackung — dort fährt der Ablauf mit
    // `Keychain` weiter und läuft erneut in D1, diesmal mit der Maske im
    // Fenster.
    let keyring = credentials_keyring::KeyringCredentialStore::new();
    let access = match mode {
        KeyMode::Password => {
            let key = match master_password::unlock(&db_path, &password) {
                Ok(key) => key,
                Err(err) => {
                    // Klarstellung 11: Gezählt wird **nur** eine
                    // gescheiterte Authentifizierung — „Passwort falsch
                    // oder Datei beschädigt" (A17). Ein Datei- oder
                    // Schlüsselbundfehler sagt nichts darüber, ob das
                    // Passwort passt; ihn mitzuzählen hieße, den Ausweg
                    // über eine dreimal nicht lesbare, vielleicht
                    // vollkommen intakte Datei freizuschalten.
                    if matches!(
                        err,
                        master_password::MasterPasswordError::WrongPasswordOrDamagedFile
                    ) {
                        pending.note_failed_unlock_attempt();
                    }
                    return Err(to_command_error(err));
                }
            };
            // A17: Ein liegengebliebener Schlüsselbund-Eintrag wird
            // aufgeräumt — aber nur, wenn er gleich K ist.
            master_password::tidy_up_keychain_after_unlock(&keyring, &key);
            // A19 (Fund 11): K wird **verschoben**, nicht in ein
            // gewöhnliches Array kopiert.
            RootKeyAccess::Unlocked(key)
        }
        KeyMode::Keychain => RootKeyAccess::Keychain(&keyring),
    };

    // A11.1 gilt nur im Passwort-Modus.
    let offers_skip = mode == KeyMode::Password;
    assemble_and_open_the_gate(&app, &gate, &pending, access, offers_skip).await
}

/// Spec 0101, Klarstellung 9 + A16 („Neu anfangen"): der Ausweg aus einer
/// Verpackungsdatei, die **kein** Passwort mehr öffnet.
///
/// **Warum ein eigenes Kommando und nicht ein Zweig von
/// [`unlock_with_master_password`]:** Entsperren und Neuanfangen sind
/// gegensätzliche Vorgänge — das eine holt K, das andere gibt ihn auf. Sie
/// in einen Aufruf zu legen hieße, dass ein Tippfehler im Passwortfeld in
/// der Nähe eines Codepfads landet, der Daten aufgibt. Getrennt ist
/// „Daten aufgeben" eine eigene, ausdrückliche Nutzerwahl, wie A5 sie
/// verlangt — mit der zweiten Bestätigung aus dem Startablauf dahinter.
///
/// **Der Riegel:** Der Aufruf wird abgelehnt, solange die Datei brauchbar
/// ist. Die Oberfläche entscheidet das nicht; sie kann es nur anfragen.
/// Sonst wäre dieses Kommando bei bloß vergessenem Passwort ein Knopf, der
/// den Verlauf wegwirft, obwohl K noch zu holen wäre.
#[tauri::command]
pub async fn start_over_from_unlock_screen(
    app: tauri::AppHandle,
    gate: tauri::State<'_, Arc<StartupGate>>,
    pending: tauri::State<'_, PendingStartup>,
) -> CommandResult<StartupStateDto> {
    let _serialized = pending.unlock_lock.lock().await;
    if gate.is_unlocked() {
        return Ok(startup_state(&gate, &pending));
    }

    let db_path = pending.inputs.db_path.clone();
    let health = master_password::wrapping_health(&db_path);
    if let Some(refusal) = refuse_to_start_over(health, pending.failed_unlock_attempts()) {
        return Err(refusal);
    }

    // `UnusableWrapping` führt die Tabelle A3 nach *ungültig* und damit nach
    // D3 bzw. D4. Der Dialog, die zweite Bestätigung und die Abfrage des
    // neuen Master-Passworts laufen über den Fragesteller im Fenster und
    // werden mit `answer_startup_prompt` beantwortet — beides steht in der
    // Positivliste des Tors.
    tracing::info!(
        ?health,
        "starting over from the unlock screen because the wrapping file cannot yield the root \
         key (Spec 0101, A3 „ungültig“, A16)"
    );
    assemble_and_open_the_gate(
        &app,
        &gate,
        &pending,
        RootKeyAccess::UnusableWrapping,
        // A11.1 gilt im Passwort-Modus.
        true,
    )
    .await
}

/// Der Riegel vor „Neu anfangen" — `Some`, wenn der Aufruf abzulehnen ist.
///
/// **Dieselbe Bedingung wie die Maske** ([`decide_startup_screen`]): Was
/// dort nicht angeboten wird, wird hier nicht angenommen. Die Oberfläche
/// entscheidet es nicht; sie kann es nur anfragen. Sonst wäre dieses
/// Kommando bei bloß vergessenem Passwort ein Knopf, der den Verlauf
/// wegwirft, obwohl K noch zu holen wäre.
fn refuse_to_start_over(
    health: master_password::WrappingHealth,
    failed_unlock_attempts: u32,
) -> Option<CommandError> {
    if decide_startup_screen(Some(health), failed_unlock_attempts).offers_start_over {
        return None;
    }
    tracing::warn!(
        ?health,
        failed_unlock_attempts,
        "refusing to start over: the wrapping file next to the database may still yield the \
         root key (Spec 0101, A5/A16, Klarstellung 10a/11)"
    );
    // **Drei Gründe, drei Texte** (review-09, Runde 2 Teil A, Fund 2
    // „Rest" — vorher zwei Texte, `Unreachable` und `Absent` teilten sich
    // einen). Eine Datei, die sich gerade nicht *lesen* lässt, ist nicht
    // dasselbe wie eine, die an diesem Ort gar nicht (mehr) existiert, und
    // keines von beidem ist dasselbe wie eine, die in Ordnung ist und nur
    // ein anderes Passwort braucht (Klarstellung 10a). Alle drei enden in
    // „nichts verändert, versuche es erneut" (D1) — aber einem Nutzer zu
    // sagen, seine Datei sei lesbar oder vorhanden, während das Gegenteil
    // zutrifft, führt ihn in die falsche Fehlersuche.
    Some(match health {
        master_password::WrappingHealth::Unreachable => CommandError::with_code(
            "Die Schlüsseldatei neben deiner Datenbank liegt an ihrem Platz, ist aber \
             gerade nicht lesbar — vielleicht hält sie ein anderes Programm offen oder \
             die Rechte stimmen nicht. Es wurde nichts verändert. Versuche es erneut.",
            MASTER_PASSWORD_FILE_FAILED_CODE,
        ),
        // Keine Datei an diesem Ort: Dann gibt es auch nichts umzubenennen,
        // und der Modus ist inzwischen der Schlüsselbund-Modus. „Erneut
        // versuchen" liest den Zustand neu und führt dorthin.
        master_password::WrappingHealth::Absent => CommandError::with_code(
            "An diesem Ort liegt keine Schlüsseldatei mehr — Smart SSH läuft für diese \
             Datenbank jetzt im Schlüsselbund-Modus. Es wurde nichts verändert. Versuche \
             es erneut.",
            MASTER_PASSWORD_FILE_ABSENT_CODE,
        ),
        // Brauchbare Datei, noch zu wenige Fehlversuche (Klarstellung 11).
        _ => CommandError::with_code(
            "Deine Schlüsseldatei ist in Ordnung — nur das Passwort passt nicht. Versuche \
             es erneut; es wird nichts verändert.",
            WRONG_MASTER_PASSWORD_CODE,
        ),
    })
}

/// Der gemeinsame Rest von [`unlock_with_master_password`] und
/// [`start_over_from_unlock_screen`]: Zustand bauen, verwalten, Tor öffnen.
///
/// Als eigene Funktion, damit die **Reihenfolge** (Zustand, dann Tor) an
/// genau einer Stelle steht. Zwei Kopien davon wären zwei Gelegenheiten,
/// sie zu vertauschen.
async fn assemble_and_open_the_gate(
    app: &tauri::AppHandle,
    gate: &StartupGate,
    pending: &PendingStartup,
    access: RootKeyAccess<'_>,
    offers_skip: bool,
) -> CommandResult<StartupStateDto> {
    let prompt = pending.prompt_for(app);
    let state = crate::open_and_assemble(
        &pending.entitlements,
        &pending.inputs,
        access,
        prompt.as_ref(),
        offers_skip,
    )
    .await
    .map_err(|abort| startup_abort_to_command_error(&pending.inputs, prompt.as_ref(), abort))?;

    // **Die Reihenfolge ist wichtig:** erst der Zustand, dann das Tor.
    // Andersherum gäbe es ein Fenster, in dem ein Kommando durchkäme, für
    // das `State<AppState>` noch nicht verwaltet ist.
    if !app.manage(state) {
        // Gemessen (M3): Ein zweites `manage` ersetzt nichts. Mit dem
        // Schloss um beide Kommandos kann es dazu nur kommen, wenn der
        // Zustand aus einer früheren Entsperrung desselben Programmlaufs
        // schon steht — dann gilt die erste, und diese hier hat nichts
        // getan.
        tracing::warn!("the application state was already managed; this unlock changed nothing");
    }
    gate.unlock();
    // Klarstellung 9: Erst jetzt dürfen die Plugins da sein, die Dateien,
    // Einstellungen oder das Betriebssystem berühren (A16).
    crate::register_unlocked_plugins(app);
    // A16, zweite Hälfte: **nach** der Entsperrung gilt der Normalbetrieb
    // (spec-reviewer Runde 6). Ohne diesen Aufruf liefen MCP-Autostart
    // (Spec 0028 §9), das Aufräumen alter Sitzungen (0034) und die
    // Migration der Klartext-Zeilen (0040) im Passwort-Modus für die ganze
    // Sitzung nicht — der Doc-Kommentar von `spawn_post_startup_tasks`
    // behauptete diesen Aufruf, und es gab ihn nicht.
    crate::spawn_post_startup_tasks(app);
    tracing::info!("unlocked and assembled the application state (Spec 0101, A16)");
    let _ = app.emit(UNLOCKED_EVENT, ());

    Ok(startup_state(gate, pending))
}

/// Die Antwort auf einen Startdialog im Fenster (§2 Frage 3).
///
/// Das neue Master-Passwort kommt getrennt mit — es gehört nicht in das
/// Ereignis, mit dem gefragt wurde (§6: kein Passwort in einem DTO).
///
/// `warning_confirmed` ist die Bestätigung aus A13/E10 (Klarstellung 12).
/// Sie zählt nur zusammen mit einem Passwort und wird **nicht**
/// vorausgesetzt: Fehlt sie, reist [`LossWarning::NotConfirmed`] mit, und
/// der Startablauf richtet nichts ein.
#[tauri::command]
pub fn answer_startup_prompt(
    app: tauri::AppHandle,
    answer: StartupPromptAnswer,
    password: Option<String>,
    repeated: Option<String>,
    warning_confirmed: Option<bool>,
    pending: tauri::State<'_, PendingStartup>,
) -> CommandResult<()> {
    let prompt = pending.prompt_for(&app);
    if let (Some(password), Some(repeated)) = (password, repeated) {
        prompt.provide_new_password(NewMasterPassword {
            password: SecretString::from(password),
            repeated: SecretString::from(repeated),
            warning: loss_warning(warning_confirmed.unwrap_or(false)),
        });
    }
    if !prompt.answer(answer) {
        return Err(CommandError::with_code(
            "Es ist gerade keine Frage offen.",
            STARTUP_FAILED_CODE,
        ));
    }
    Ok(())
}

/// A16: „Beenden“ aus der Entsperrmaske.
///
/// Rückgabewert 0: Ein bewusstes Beenden ist kein Fehler.
#[tauri::command]
pub fn quit_application(app: tauri::AppHandle) {
    tracing::info!("the user quit from the unlock screen (Spec 0101, A16)");
    // **Puffer schreiben, bevor beendet wird** (spec-reviewer Lauf 4,
    // Fund 8). `app.exit(0)` führt keine Destruktoren aus — die
    // Ereignisschleife kommt auf manchen Plattformen nicht zurück. Ohne das
    // hier ginge gerade die Zeile darüber verloren, also die, die erklärt,
    // warum die Sitzung endete. An den beiden `process::exit`-Stellen in
    // `run()` ist dasselbe ausdrücklich behandelt; hier fehlte es.
    match app.try_state::<app_logic::logging::LogFlushOnDemand>() {
        Some(flush) => {
            flush.flush_now();
        }
        // Darf nicht vorkommen (`run` verwaltet ihn immer). Sichtbar
        // machen statt stillschweigend ohne Flush zu beenden — auch wenn
        // diese Zeile dann selbst die ist, die verloren geht.
        None => tracing::warn!("no log flush guard is managed; the last lines may be lost"),
    }
    app.exit(0);
}

/// A18: Welcher Modus ist aktiv? Für die Einstellungen.
#[tauri::command]
pub fn get_master_password_mode(state: tauri::State<'_, AppState>) -> &'static str {
    let _ = &state;
    mode_name(master_password::key_mode(
        &persistence_sqlite::default_db_path(),
    ))
}

/// Die Bestätigung aus A13/E10 vom IPC in den Typ, den `app-logic` verlangt
/// (Klarstellung 12).
///
/// Ein `bool` ist alles, was über das IPC kommen kann; ab hier trägt der
/// Typ die Bedeutung, und die vorsichtige Lesart ist die Voreinstellung.
fn loss_warning(confirmed: bool) -> master_password::LossWarning {
    if confirmed {
        master_password::LossWarning::ConfirmedByTheUser
    } else {
        master_password::LossWarning::NotConfirmed
    }
}

/// Die Vorprüfungen von [`set_up_master_password`], getrennt vom
/// `State<AppState>` und vom Schlüsselbund, damit die Entscheidung ohne
/// Tauri-Laufzeit testbar ist (ADR 0099 §7, Punkt 1). Sie schreibt nichts.
///
/// Reihenfolge, und sie ist Teil der Zusage:
///
/// 1. **Bestätigung vor dem Schlüsselbund** (Klarstellung 12): Ohne sie
///    wird abgelehnt, bevor `read_key` überhaupt aufgerufen wird — „verändert
///    nichts" fängt beim Nichtstun an, und die Fehlermeldung bliebe sonst
///    von der Erreichbarkeit des Schlüsselbunds abhängig.
/// 2. **K wird gelesen, nicht erzeugt** (A3/A13): Ist er nicht lesbar,
///    entsteht hier auf keinen Fall ein neuer — die Datenbank wäre verloren.
/// 3. **Es muss der K sein, mit dem die Datenbank gerade offen ist**
///    (spec-reviewer Lauf 4, Fund 10). Weicht der Eintrag seit dem Start
///    ab — zweite Installation, manuelle Änderung, ein Rest aus A17 —,
///    verpackte die App sonst einen Schlüssel, mit dem die offene Datenbank
///    **nicht** zu öffnen ist: beim nächsten Start D2 und Totalverlust.
///    Verglichen wird über die Kennung, nicht über K selbst — K für die
///    ganze Sitzung aufzubewahren wäre das Gegenteil von A19 (s.
///    `crypto::root_key_fingerprint`).
fn check_set_up_preconditions(
    warning_confirmed: bool,
    open_root_key_fingerprint: &[u8; 32],
    read_key: impl FnOnce() -> ssh_manager_core::crypto::RootKeyState,
) -> Result<
    (
        master_password::LossWarning,
        ssh_manager_core::crypto::RootKey,
    ),
    CommandError,
> {
    let warning = loss_warning(warning_confirmed);
    if warning != master_password::LossWarning::ConfirmedByTheUser {
        return Err(to_command_error(
            master_password::MasterPasswordError::LossWarningNotConfirmed,
        ));
    }
    match read_key() {
        ssh_manager_core::crypto::RootKeyState::Present(key)
            if &ssh_manager_core::crypto::root_key_fingerprint(&key)
                == open_root_key_fingerprint =>
        {
            Ok((warning, key))
        }
        ssh_manager_core::crypto::RootKeyState::Present(_) => {
            tracing::error!(
                "refusing to set up a master password: the root key in the OS keychain is not \
                 the one this database is open with; wrapping it would make the database \
                 unopenable (Spec 0101, A3/A13)"
            );
            Err(CommandError::with_code(
                "Der Schlüssel im Schlüsselbund gehört nicht zu dieser Datenbank. Smart SSH \
                 richtet das Master-Passwort deshalb nicht ein — es würde den falschen \
                 Schlüssel sichern und die Datenbank beim nächsten Start unlesbar machen. \
                 Es ist nichts verändert.",
                KEYCHAIN_KEY_MISMATCH_CODE,
            ))
        }
        other => {
            tracing::warn!(
                ?other,
                "refusing to set up a master password without a readable root key \
                 (Spec 0101, A3)"
            );
            Err(CommandError::with_code(
                "Der Schlüssel zu deiner Datenbank ist gerade nicht lesbar. Richte das \
                 Master-Passwort später erneut ein — es wird kein neuer Schlüssel erzeugt.",
                MASTER_PASSWORD_FILE_FAILED_CODE,
            ))
        }
    }
}

/// A13: Master-Passwort aus den Einstellungen einrichten.
///
/// K kommt dabei aus dem Schlüsselbund — es ist dasselbe K, mit dem die
/// Datenbank gerade offen ist, und es bleibt es (E9). Die Reihenfolge
/// (verpacken, schreiben, zurücklesen, vergleichen, dann löschen) liegt in
/// `app_logic::master_password`.
///
/// `warning_confirmed` ist die Bestätigung aus A13/E10 (Klarstellung 12).
/// Ohne sie wird abgelehnt, bevor irgendetwas geschrieben wird — auch
/// bevor der Schlüsselbund gelesen wird.
#[tauri::command]
pub fn set_up_master_password(
    password: String,
    repeated: String,
    warning_confirmed: bool,
    state: tauri::State<'_, AppState>,
) -> CommandResult<&'static str> {
    let db_path = persistence_sqlite::default_db_path();
    let keyring = credentials_keyring::KeyringCredentialStore::new();
    let (warning, root_key) =
        check_set_up_preconditions(warning_confirmed, &state.root_key_fingerprint, || {
            ssh_manager_core::crypto::read_root_key(&keyring)
        })?;

    master_password::set_up_master_password(
        &db_path,
        &root_key,
        &SecretString::from(password),
        &SecretString::from(repeated),
        warning,
        Some(&keyring),
    )
    .map_err(to_command_error)?;
    Ok("password")
}

/// A15: Passwort ändern. Die Datenbank wird dabei nicht angefasst.
#[tauri::command]
pub fn change_master_password(
    current: String,
    password: String,
    repeated: String,
    state: tauri::State<'_, AppState>,
) -> CommandResult<()> {
    let _ = &state;
    master_password::change_master_password(
        &persistence_sqlite::default_db_path(),
        &SecretString::from(current),
        &SecretString::from(password),
        &SecretString::from(repeated),
    )
    .map_err(to_command_error)
}

/// A15: zurück auf den Schlüsselbund.
///
/// `replace_another_key` ist die Bestätigung aus Klarstellung 10b. Die
/// Oberfläche ruft das Kommando zuerst **ohne** sie; kommt
/// `KEYCHAIN_HOLDS_ANOTHER_KEY` zurück, stellt sie die Frage und ruft
/// erneut. Der Dialog selbst entsteht in Commit 11 — bis dahin ist der
/// Rückweg der Fehlercode, und das ist der sichere Ausgang: ohne Antwort
/// bleibt alles, wie es war.
#[tauri::command]
pub fn switch_to_os_keychain(
    current: String,
    replace_another_key: bool,
    state: tauri::State<'_, AppState>,
) -> CommandResult<&'static str> {
    let _ = &state;
    master_password::switch_to_keychain(
        &persistence_sqlite::default_db_path(),
        &SecretString::from(current),
        &credentials_keyring::KeyringCredentialStore::new(),
        if replace_another_key {
            master_password::KeychainOverwrite::ConfirmedByTheUser
        } else {
            master_password::KeychainOverwrite::OnlyAfterConfirmation
        },
    )
    .map_err(to_command_error)?;
    Ok("keychain")
}

/// A17/A20: Der Fehler für das Frontend — **ohne** Nutzlast im Text, mit
/// einem stabilen Code.
///
/// „Passwort falsch" und „Datei beschädigt" sind derselbe Code: Sie sind
/// nicht unterscheidbar (A17), und ein zweiter Code wäre ein Orakel darüber,
/// welcher der beiden Fälle vorliegt.
fn to_command_error(err: master_password::MasterPasswordError) -> CommandError {
    use master_password::MasterPasswordError as E;
    if let Some(detail) = err.detail_for_log() {
        tracing::warn!(
            detail,
            "a master-password operation failed (Spec 0101, A13–A17)"
        );
    }
    let code = match err {
        E::WrongPasswordOrDamagedFile => WRONG_MASTER_PASSWORD_CODE,
        E::PasswordRejected(_) => MASTER_PASSWORD_REJECTED_CODE,
        E::KeychainHoldsAnotherKey => KEYCHAIN_HOLDS_ANOTHER_KEY_CODE,
        E::UnusableWrappingFile
        | E::FileFailed { .. }
        // Klarstellung 10a: **derselbe** Code wie die übrigen Dateifehler.
        // Ein eigener Code wäre ein Orakel darüber, ob die Datei existiert
        // und nur gerade gesperrt ist — und die Oberfläche tut in beiden
        // Fällen dasselbe („Erneut versuchen"). Der Unterschied steht im
        // Text und im Log.
        | E::WrappingFileUnreachable { .. }
        | E::KeychainFailed { .. } => MASTER_PASSWORD_FILE_FAILED_CODE,
        // Kein Dateifehler, sondern ein Vorgang, der nicht zum aktiven
        // Modus passt (Review-Fund Runde 1 zu Commit 11).
        E::NotInPasswordMode | E::AlreadyInPasswordMode => MASTER_PASSWORD_MODE_MISMATCH_CODE,
        // Klarstellung 12: eigener Code, s. dessen Konstante.
        E::LossWarningNotConfirmed => MASTER_PASSWORD_WARNING_NOT_CONFIRMED_CODE,
    };
    CommandError::with_code(err.to_string(), code)
}

/// Ein Startabbruch aus dem Entsperr-Kommando. Anders als in `run()` endet
/// er **nicht** in `process::exit`: Das Fenster steht schon, und die
/// Entsperrmaske kann den Grund zeigen (A16: „sichtbare Meldung").
fn startup_abort_to_command_error(
    inputs: &crate::StartupInputs,
    prompt: &WindowStartupPrompt,
    abort: StartupAbort,
) -> CommandError {
    match abort {
        // Klarstellung 9: „Der Start wurde abgebrochen" wäre hier eine
        // falsche Aussage, wenn die Frage nie im Fenster ankam — der Nutzer
        // hat nichts abgebrochen, er hat vermutlich nichts gesehen. Der
        // Startablauf kann die beiden Fälle nicht unterscheiden (er sieht
        // nur „nicht bestätigt"), der Fragesteller schon.
        StartupAbort::UserQuit if prompt.timed_out() => CommandError::with_code(
            "Smart SSH hat auf eine Rückfrage gewartet, aber keine Antwort erhalten. Es ist \
             nichts verändert. Starte Smart SSH erneut; bleibt es dabei, nennt das Log die \
             Ursache.",
            STARTUP_FAILED_CODE,
        ),
        StartupAbort::UserQuit => CommandError::with_code(
            "Der Start wurde abgebrochen. Es ist nichts verändert.",
            STARTUP_FAILED_CODE,
        ),
        // Kann hier nicht vorkommen: Der Fenster-Fragesteller **kann** nach
        // einem Passwort fragen (`can_ask_for_a_password`). Sichtbar
        // scheitern statt stillschweigend nichts zu tun.
        StartupAbort::NeedsWindow => CommandError::with_code(
            "Der Start braucht eine Eingabe, die hier nicht möglich ist.",
            STARTUP_FAILED_CODE,
        ),
        StartupAbort::Fatal { kind, detail } => {
            let log_dir = app_logic::logging::default_log_dir();
            let text = app_logic::startup_error_messages::db_connect_failure_text(
                &kind,
                &inputs.db_path,
                &log_dir,
                inputs.language,
            );
            // Wie in `run()`: `detail` nur ins Log.
            tracing::error!(detail, ?kind, "the startup failed after unlocking");
            CommandError::with_code(text.message, STARTUP_FAILED_CODE)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use master_password::WrappingHealth as H;

    /// A3/A16, Klarstellung 9 + 11: die **ganze** Entscheidungstabelle der
    /// Startmaske, Zeile für Zeile.
    ///
    /// Der Fund, gegen den der dritte Zustand steht (ADR 0097 §4.2): *nicht
    /// erreichbar* war im DTO nicht abbildbar und führte zum Passwortfeld —
    /// der Nutzer tippte ein Passwort für eine Datei, die gar nicht gelesen
    /// werden konnte. Die Zeile `Unreachable` unten ist genau das.
    #[test]
    fn test_the_startup_screen_follows_the_state_of_the_wrapping_file() {
        // Entsperrt: über die Datei ist nichts zu sagen.
        assert_eq!(
            decide_startup_screen(None, 99),
            StartupScreenDecision {
                screen: StartupScreen::Unlocked,
                offers_start_over: false,
            },
            "ist der Zustand gebaut, zeigt die Oberfläche die App — und „Neu anfangen“ hat dort \
             nichts zu suchen"
        );

        assert_eq!(
            decide_startup_screen(Some(H::Absent), 0).screen,
            StartupScreen::SetUpMasterPassword,
            "keine Verpackungsdatei und trotzdem kein Zustand: der Start ist in D1 gelandet \
             (§2 Frage 3)"
        );
        assert_eq!(
            decide_startup_screen(Some(H::Usable), 0).screen,
            StartupScreen::Unlock,
            "brauchbare Datei: Passwortfeld (A16)"
        );
        assert_eq!(
            decide_startup_screen(Some(H::Unusable), 0).screen,
            StartupScreen::UnusableWrapping,
            "A3 *ungültig*: kein Passwortfeld, sondern der Ausweg (Klarstellung 9)"
        );
        assert_eq!(
            decide_startup_screen(Some(H::Unreachable), 0).screen,
            StartupScreen::UnreachableWrapping,
            "A3 *nicht erreichbar* (D1): weder Passwortfeld noch Ausweg — über den Inhalt der \
             Datei ist nichts gesagt (Klarstellung 10a)"
        );
    }

    /// Klarstellung 11: **Der Ausweg erscheint erst nach dem dritten
    /// gescheiterten Versuch** — und bei einer nicht erreichbaren Datei nie.
    #[test]
    fn test_starting_over_is_offered_only_after_three_failed_attempts() {
        for attempts in 0..ATTEMPTS_BEFORE_STARTING_OVER_IS_OFFERED {
            assert!(
                !decide_startup_screen(Some(H::Usable), attempts).offers_start_over,
                "nach {attempts} Fehlversuchen darf es den Ausweg noch nicht geben — der Riegel \
                 aus Klarstellung 9 schützt genau hier vor dem vergessenen Passwort"
            );
            assert!(
                refuse_to_start_over(H::Usable, attempts).is_some(),
                "und das Kommando muss denselben Aufruf ablehnen; sonst bietet die Maske den \
                 Knopf nicht an, aber ein erfundener Aufruf käme durch"
            );
        }
        assert!(
            decide_startup_screen(Some(H::Usable), ATTEMPTS_BEFORE_STARTING_OVER_IS_OFFERED)
                .offers_start_over,
            "nach dem dritten Fehlversuch gibt es den Ausweg (Q-BL-0314-01, entschieden) — sonst \
             ist ein vergessenes Passwort eine dauerhafte Aussperrung"
        );
        assert!(
            refuse_to_start_over(H::Usable, ATTEMPTS_BEFORE_STARTING_OVER_IS_OFFERED).is_none(),
            "und das Kommando nimmt ihn dann an"
        );

        // Die Zeile, die auch nach beliebig vielen Versuchen stehen bleibt.
        for attempts in [0, 3, 99] {
            assert!(
                !decide_startup_screen(Some(H::Unreachable), attempts).offers_start_over,
                "Klarstellung 11, letzter Satz: eine *nicht erreichbare* Datei bietet „Neu \
                 anfangen“ nie — ein Rechte- oder E/A-Fehler darf nicht mit dem Aufgeben von K \
                 beantwortet werden"
            );
            assert!(
                refuse_to_start_over(H::Unreachable, attempts).is_some(),
                "und das Kommando lehnt ihn ab, egal wie oft gefragt wird"
            );
            assert!(
                !decide_startup_screen(Some(H::Absent), attempts).offers_start_over,
                "ohne Datei gibt es nichts umzubenennen"
            );
            assert!(
                refuse_to_start_over(H::Absent, attempts).is_some(),
                "und auch das lehnt das Kommando ab"
            );
        }

        // Und die Gegenrichtung: Bei *ungültiger* Datei steht der Ausweg vom
        // ersten Augenblick an offen (Klarstellung 9) — ohne diese Zeile käme
        // eine Fassung durch, die ihn überall erst nach drei Versuchen
        // anbietet.
        assert!(decide_startup_screen(Some(H::Unusable), 0).offers_start_over);
        assert!(refuse_to_start_over(H::Unusable, 0).is_none());
    }

    /// Die drei Texte der Ablehnung sagen die **Wahrheit über den
    /// Zustand**, den sie beschreiben (Klarstellung 10a; `Absent` seit
    /// review-09 Runde 2 Teil A, Fund 2 „Rest" mit eigenem Code/Text statt
    /// unter `MASTER_PASSWORD_FILE_FAILED` mitzulaufen).
    #[test]
    fn test_the_refusal_names_the_right_reason() {
        let unreachable = refuse_to_start_over(H::Unreachable, 99).expect("abgelehnt");
        assert_eq!(unreachable.code, Some(MASTER_PASSWORD_FILE_FAILED_CODE));
        assert!(
            unreachable.message.contains("nicht lesbar"),
            "eine Datei, die sich nicht lesen lässt, ist nicht „in Ordnung, nur das Passwort \
             passt nicht“ — das führte in die falsche Fehlersuche. Text: {}",
            unreachable.message
        );

        let absent = refuse_to_start_over(H::Absent, 99).expect("abgelehnt");
        assert_eq!(
            absent.code,
            Some(MASTER_PASSWORD_FILE_ABSENT_CODE),
            "keine Datei an diesem Ort ist ein anderer Zustand als eine, die da liegt und \
             gerade nicht lesbar ist — beide unter demselben Code zu melden, behauptet im \
             Absent-Fall einen I/O-/Rechte-Fehler, den es nicht gibt"
        );
        assert_ne!(
            absent.code, unreachable.code,
            "sonst wäre die Aufteilung nur eine Umbenennung, kein eigener Zustand"
        );
        assert!(
            !absent.message.contains("nicht lesbar"),
            "ohne Datei an diesem Ort gibt es nichts, das „nicht lesbar“ wäre — der Text \
             behauptet sonst eine Datei, die nicht existiert. Text: {}",
            absent.message
        );

        let wrong_password = refuse_to_start_over(H::Usable, 0).expect("abgelehnt");
        assert_eq!(wrong_password.code, Some(WRONG_MASTER_PASSWORD_CODE));
        assert!(wrong_password.message.contains("in Ordnung"));
    }

    fn pending_for(db_path: std::path::PathBuf) -> PendingStartup {
        PendingStartup::new(
            crate::StartupInputs {
                data_dir_lock: persistence_sqlite::DataDirLock::acquire_for_database(&db_path)
                    .expect("lock the test data directory"),
                db_path,
                language: app_logic::startup_error_messages::Language::De,
                keychain: credentials_keyring::KeychainAvailability::Available,
            },
            Arc::new(ssh_manager_core::entitlements::FixedEntitlements(
                ssh_manager_core::entitlements::Entitlements::free(),
            )),
        )
    }

    /// Dasselbe am echten Dateisystem: Der Zustand im DTO kommt aus dem
    /// Zustand der Datei — und `mode` widerspricht ihm nicht.
    #[test]
    fn test_the_startup_state_reports_the_state_of_the_file_next_to_the_database() {
        let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
        let db_path = dir.path().join("smart-ssh.db");
        let wrapping = master_password::wrapping_file_path(&db_path);
        let gate = StartupGate::new(false);
        let pending = pending_for(db_path.clone());

        // Keine Datei: Schlüsselbund-Modus.
        let state = startup_state(&gate, &pending);
        assert_eq!(state.screen, "setUpMasterPassword");
        assert_eq!(state.mode, "keychain");
        assert!(!state.offers_start_over);

        // Gelesen, aber unbrauchbar (A3 *ungültig*).
        std::fs::write(&wrapping, b"kein Verpackungsformat-0101").expect("schreiben");
        let state = startup_state(&gate, &pending);
        assert_eq!(state.screen, "unusableWrapping");
        assert_eq!(
            state.mode, "password",
            "eine Datei an diesem Ort heißt Passwort-Modus, auch wenn ihr Inhalt nichts hergibt \
             — sonst entstünde im Schlüsselbund-Modus ein neuer K (A3)"
        );
        assert!(state.offers_start_over);

        // Eine brauchbare Verpackung: Passwortfeld, kein Ausweg.
        std::fs::remove_file(&wrapping).expect("entfernen");
        master_password::set_up_master_password(
            &db_path,
            &ssh_manager_core::crypto::RootKey::for_tests([7u8; 32]),
            &SecretString::from("Passwort-0101-lang"),
            &SecretString::from("Passwort-0101-lang"),
            master_password::LossWarning::ConfirmedByTheUser,
            None,
        )
        .expect("einrichten");
        let state = startup_state(&gate, &pending);
        assert_eq!(state.screen, "unlock");
        assert_eq!(state.mode, "password");
        assert!(!state.offers_start_over);
        assert_eq!(state.failed_unlock_attempts, 0);

        // Klarstellung 11: drei gescheiterte Authentifizierungen, dann der
        // Ausweg — an derselben, unveränderten Datei.
        let before = std::fs::read(&wrapping).expect("lesen");
        for _ in 0..ATTEMPTS_BEFORE_STARTING_OVER_IS_OFFERED {
            pending.note_failed_unlock_attempt();
        }
        let state = startup_state(&gate, &pending);
        assert_eq!(
            state.screen, "unlock",
            "das Passwortfeld bleibt — der Ausweg kommt daneben, er ersetzt es nicht"
        );
        assert!(state.offers_start_over);
        assert_eq!(state.failed_unlock_attempts, 3);
        assert_eq!(
            std::fs::read(&wrapping).expect("lesen"),
            before,
            "bis zur Nutzerwahl wird an der Datei nichts verändert (A3)"
        );

        // Und entsperrt ist der Schirm die App, ohne dass die Datei noch
        // gelesen wird.
        let open = StartupGate::new(true);
        assert_eq!(startup_state(&open, &pending).screen, "unlocked");
    }

    /// A3 *nicht erreichbar* am echten Dateisystem (Klarstellung 10a): eine
    /// **vollkommen gültige** Verpackung ohne Leserechte.
    ///
    /// Das ist der Fall, der vor Klarstellung 10a „Neu anfangen" anbot und
    /// damit eine intakte Verpackung samt Datenbank umbenannte.
    #[cfg(unix)]
    #[test]
    fn test_a_valid_wrapping_file_without_read_permission_is_unreachable_not_unusable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("Temp-Verzeichnis");
        let db_path = dir.path().join("smart-ssh.db");
        master_password::set_up_master_password(
            &db_path,
            &ssh_manager_core::crypto::RootKey::for_tests([9u8; 32]),
            &SecretString::from("Passwort-0101-lang"),
            &SecretString::from("Passwort-0101-lang"),
            master_password::LossWarning::ConfirmedByTheUser,
            None,
        )
        .expect("einrichten");
        let wrapping = master_password::wrapping_file_path(&db_path);
        std::fs::set_permissions(&wrapping, std::fs::Permissions::from_mode(0o000))
            .expect("Rechte setzen");

        let gate = StartupGate::new(false);
        let pending = pending_for(db_path);
        // Auch nach beliebig vielen Fehlversuchen.
        for _ in 0..10 {
            pending.note_failed_unlock_attempt();
        }
        let state = startup_state(&gate, &pending);

        // Vor dem Prüfen zurücksetzen: Ein scheiterndes `assert!` würde das
        // Temp-Verzeichnis sonst mit einer unlesbaren Datei zurücklassen.
        std::fs::set_permissions(&wrapping, std::fs::Permissions::from_mode(0o600))
            .expect("Rechte zurücksetzen");

        assert_eq!(
            state.screen, "unreachableWrapping",
            "eine Datei ohne Leserechte ist *nicht erreichbar*, nicht *ungültig* — ihr Inhalt \
             kann vollkommen in Ordnung sein"
        );
        assert!(
            !state.offers_start_over,
            "und sie bietet „Neu anfangen“ nie an: sonst beantwortet ein Rechtefehler sich \
             selbst mit dem Aufgeben von K"
        );
    }

    /// Klarstellung 11: **Nur eine gescheiterte Authentifizierung zählt.**
    ///
    /// Quelltextlesend, weil `unlock_with_master_password` ein Tauri-Kommando
    /// mit `State`-Griffen ist und außerhalb einer laufenden App nicht
    /// aufrufbar (ADR 0096 §3). Die Aussage ist trotzdem falsifizierbar: Es
    /// gibt genau eine Stelle, die zählt, sie liegt in diesem Kommando, und
    /// die Prüfung auf die Fehlerart steht davor.
    #[test]
    fn test_only_a_failed_authentication_counts_towards_the_way_out() {
        let source = include_str!("master_password.rs");
        let production = &source[..source
            .find("#[cfg(test)]")
            .expect("das Testmodul ist nicht mehr zu finden")];

        let counting = "pending.note_failed_unlock_attempt()";
        assert_eq!(
            production.matches(counting).count(),
            1,
            "Klarstellung 11: Gezählt wird an genau einer Stelle. Eine zweite braucht dieselbe \
             Prüfung auf die Fehlerart — und dieser Test die zweite Stelle."
        );

        let unlocking = {
            let start = production
                .find("pub async fn unlock_with_master_password(")
                .expect("das Entsperr-Kommando gibt es nicht mehr");
            let rest = &production[start..];
            &rest[..rest.find("\n}\n").expect("Funktionsende nicht gefunden")]
        };
        let guard = unlocking
            .find("MasterPasswordError::WrongPasswordOrDamagedFile")
            .expect(
                "Klarstellung 11: Gezählt werden darf nur „Passwort falsch oder Datei \
                 beschädigt“ (A17) — ohne diese Prüfung öffnete eine dreimal nicht lesbare, \
                 vielleicht intakte Datei den Ausweg",
            );
        let counts = unlocking
            .find(counting)
            .expect("hier wird nicht mehr gezählt — s. Doc-Kommentar");
        assert!(
            guard < counts,
            "die Prüfung auf die Fehlerart muss **vor** dem Zählen stehen"
        );
    }

    fn key_state(key: ssh_manager_core::crypto::RootKey) -> ssh_manager_core::crypto::RootKeyState {
        ssh_manager_core::crypto::RootKeyState::Present(key)
    }

    /// Spec 0101 E10/Klarstellung 12: ohne Bestätigung wird abgelehnt, und
    /// der Schlüsselbund wird nicht einmal gelesen.
    #[test]
    fn test_set_up_without_confirmation_is_rejected_before_the_keychain_is_read() {
        let key = ssh_manager_core::crypto::RootKey::for_tests([3u8; 32]);
        let fp = ssh_manager_core::crypto::root_key_fingerprint(&key);
        let read = std::cell::Cell::new(false);
        let err = check_set_up_preconditions(false, &fp, || {
            read.set(true);
            key_state(key)
        })
        .expect_err("ohne Bestätigung abgelehnt");
        assert!(!read.get(), "der Schlüsselbund darf nicht gelesen werden");
        assert_ne!(err.code, Some(KEYCHAIN_KEY_MISMATCH_CODE));
        assert_eq!(
            err.code,
            to_command_error(master_password::MasterPasswordError::LossWarningNotConfirmed).code
        );
    }

    /// Klarstellung 9: ein anderer K im Schlüsselbund als der der offenen
    /// Datenbank wird mit `KEYCHAIN_KEY_MISMATCH` abgelehnt.
    #[test]
    fn test_set_up_with_a_foreign_keychain_key_is_rejected_with_mismatch() {
        let fp = ssh_manager_core::crypto::root_key_fingerprint(
            &ssh_manager_core::crypto::RootKey::for_tests([3u8; 32]),
        );
        let err = check_set_up_preconditions(true, &fp, || {
            key_state(ssh_manager_core::crypto::RootKey::for_tests([4u8; 32]))
        })
        .expect_err("fremder Schlüssel abgelehnt");
        assert_eq!(err.code, Some(KEYCHAIN_KEY_MISMATCH_CODE));
    }

    /// A3: ein nicht lesbarer K wird nie durch einen neuen ersetzt.
    #[test]
    fn test_set_up_without_a_readable_keychain_key_is_rejected() {
        use ssh_manager_core::crypto::RootKeyState;
        let fp = ssh_manager_core::crypto::root_key_fingerprint(
            &ssh_manager_core::crypto::RootKey::for_tests([3u8; 32]),
        );
        for state in [
            RootKeyState::NotFound,
            RootKeyState::Invalid,
            RootKeyState::Unreachable("x".into()),
        ] {
            let err = check_set_up_preconditions(true, &fp, || state)
                .expect_err("kein lesbarer Schlüssel");
            assert_eq!(err.code, Some(MASTER_PASSWORD_FILE_FAILED_CODE));
        }
    }

    /// Der Erfolgsfall: Bestätigung da, Kennung passt — K wird zurückgegeben.
    #[test]
    fn test_set_up_preconditions_pass_for_the_key_the_database_is_open_with() {
        let key = ssh_manager_core::crypto::RootKey::for_tests([3u8; 32]);
        let fp = ssh_manager_core::crypto::root_key_fingerprint(&key);
        let (warning, got) = check_set_up_preconditions(true, &fp, || key_state(key.duplicate()))
            .expect("zugelassen");
        assert_eq!(warning, master_password::LossWarning::ConfirmedByTheUser);
        assert_eq!(got.expose(), key.expose());
    }
}
