//! Spec 0101, §2 Frage 3: die Startdialoge **im Fenster**, für den
//! Passwort-Modus.
//!
//! **Warum nicht die nativen Dialoge aus `startup_prompt`:** Im
//! Passwort-Modus entsteht das Fenster, bevor der Zustand da ist (gemessen,
//! M5) — der ganze Startablauf läuft deshalb aus einem Kommando heraus, also
//! auf einem Arbeitsthread der Async-Laufzeit und nicht auf dem
//! Haupt-Thread. `rfd` trägt das auf macOS nicht. Dazu hat `rfd` keine
//! Texteingabe (§1), die Entsperr- und Einrichtemasken müssen also ohnehin
//! ins Fenster. Zwei Darstellungsarten im selben Ablauf wären die schlechtere
//! Antwort als eine.
//!
//! **Die Entscheidungslogik bleibt, wo sie ist.** Dieses Modul ist eine
//! zweite Implementierung von [`StartupPrompt`] — dasselbe Trait, das die
//! nativen Dialoge tragen. `app_logic::database_startup` und
//! `app_logic::secret_migration` sehen keinen Unterschied und wurden dafür
//! nicht geändert; §5 stellt genau diese Wahl zur Verfügung.
//!
//! **Wie das Warten funktioniert:** `ask` ist synchron (das Trait ist es),
//! läuft aber in einer asynchronen Aufgabe. Es schickt die Frage als
//! Ereignis ans Frontend und wartet auf dem Kanal — mit
//! `tokio::task::block_in_place`, derselben gemessenen Zusicherung wie beim
//! synchronen Secret-Speicher (§2 Frage 2: Tauris Laufzeit ist eine
//! Multi-Thread-Laufzeit, s. `runtime_assumptions`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::Emitter;
use tokio::sync::oneshot;

use app_logic::database_startup::{NewMasterPassword, StartupChoice, StartupDialog, StartupPrompt};
use app_logic::startup_choice_dialogs as texts;
use app_logic::startup_error_messages::Language;
use credentials_keyring::{KeychainAvailability, KeychainUnavailableReason};
use persistence_sqlite::{detect_database_file_state, DatabaseFileState};

/// Das Ereignis, mit dem das Fenster nach einer Entscheidung gefragt wird.
pub const STARTUP_PROMPT_EVENT: &str = "startup:prompt";

/// Wie lange auf die Antwort aus dem Fenster gewartet wird (Spec 0101,
/// Klarstellung 9, vierter Punkt; Spec 0059).
///
/// **Warum es eine Grenze braucht:** `emit` liefert `Ok`, auch wenn niemand
/// zuhört. Hat das Fenster den Zuhörer für `startup:prompt` noch nicht
/// registriert — oder verliert es ihn bei einem Reload —, geht das Ereignis
/// verloren, und ein Warten ohne Grenze wäre ein Dauerhänger im gesperrten
/// Zustand: kein Dialog, keine Meldung, kein Ausgang außer dem Beenden über
/// das Fenstersystem. A16 verlangt an jeder Stelle eine **sichtbare**
/// Meldung, und Spec 0059 ist genau gegen diese Art von stillem Start
/// geschrieben.
///
/// **Warum fünf Minuten und nicht fünf Sekunden:** Hinter der Frage steht
/// ein Mensch, der ein Master-Passwort eintippt oder sich überlegt, ob er
/// seinen Verlauf aufgibt. Die Grenze darf keine dieser Entscheidungen
/// abschneiden; sie ist gegen den Fall „niemand hat die Frage überhaupt
/// gesehen" gerichtet, nicht gegen langsames Nachdenken. Ein kurzer Wert
/// mit einer Rückmeldung aus der Oberfläche wäre genauer, bräuchte aber ein
/// zusätzliches Kommando — und damit eine Zusage, die die Maske aus
/// Commit 11 erst einlösen müsste. Scheitert sie daran, liefe jeder Dialog
/// in die Grenze. Eine großzügige Grenze ohne neue Zusage ist der sichere
/// Zuschnitt.
const PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Welche Knöpfe die Maske anbieten darf. Das Frontend bildet sie ab, es
/// erfindet keine — jeder Knopf, der hier nicht steht, darf dort nicht
/// erscheinen.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PromptKind {
    /// „Erneut versuchen“ / „Beenden“ (D1, A11).
    RetryOrQuit,
    /// „Erneut versuchen“ / „Master-Passwort einrichten“ / „Beenden“ — D1
    /// mit dem dritten Knopf ab Etappe 3 (A3, D1; A13).
    ///
    /// **Eine eigene Art und nicht ein Zusatzfeld an [`Self::RetryOrQuit`]:**
    /// Die Zuordnung Antwort → Wahl unten ist je Art eingeschränkt. Hinge
    /// der dritte Knopf an einem Wahrheitswert, könnte eine Oberfläche
    /// „Einrichten" auf einen Dialog schicken, der es nicht anbietet — und
    /// Einrichten ist der Weg, auf dem im Fall *fehlt* ein neuer K entsteht
    /// (A3).
    RetrySetUpOrQuit,
    /// „Erneut versuchen“ / „Ohne Übernahme fortfahren“ / „Beenden“ (A11.1).
    RetrySkipOrQuit,
    /// „Neu anfangen“ / „Beenden“ (D2, D3).
    StartOverOrQuit,
    /// „Neuen Schlüssel erzeugen“ / „Beenden“ (D4).
    NewKeyOrQuit,
    /// Zweite Bestätigung zu „Neu anfangen“ (A5).
    ConfirmStartOver,
    /// Zweite Bestätigung zu „Neuen Schlüssel erzeugen“ (A5/D4).
    ConfirmNewKey,
    /// Ein neues Master-Passwort eingeben (A13).
    NewMasterPassword,
    /// Nur eine Meldung mit „OK“ (A5, nach dem Umbenennen).
    Notice,
}

/// Die Frage, wie sie im Fenster erscheint.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupPromptRequest {
    pub kind: PromptKind,
    pub title: String,
    pub message: String,
}

/// Die Antwort aus dem Fenster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupPromptAnswer {
    Retry,
    Quit,
    StartOver,
    GenerateNewKey,
    ContinueWithoutMigration,
    /// A3, D1, dritter Knopf: „Master-Passwort einrichten“ (A13).
    SetUpMasterPassword,
    /// Zweite Bestätigung bejaht.
    Confirm,
    /// Zweite Bestätigung abgelehnt, Maske abgebrochen, Meldung quittiert.
    Cancel,
}

/// A3: Welche Knöpfe eine Dialogart zeigt — die Zuordnung Fall → Knöpfe.
///
/// Als freie Funktion neben [`choice_for`], weil die beiden zusammen die
/// ganze Einschränkung tragen: Diese hier entscheidet, welche Wahlen ein
/// Fall überhaupt anbietet, jene, welche Antwort er annimmt. Beide ohne
/// Tauri und damit prüfbar.
///
/// **Die Zusage, auf die es ankommt** (A3, T13): Der Umzugs-Dialog aus A11
/// bietet „Master-Passwort einrichten" nie an — er ist eine eigene Variante
/// und nicht `D1 { offers_password_setup: false }`, also kann er es per
/// Konstruktion nicht.
fn kind_for(dialog: &StartupDialog) -> PromptKind {
    match dialog {
        // Der dritte Knopf erscheint **nur**, wenn der Startablauf ihn
        // anbietet — die Bedingung dafür (`password_setup_is_safe`) liegt in
        // `database_startup`, nicht hier.
        StartupDialog::D1 {
            offers_password_setup: true,
        } => PromptKind::RetrySetUpOrQuit,
        StartupDialog::D1 {
            offers_password_setup: false,
        } => PromptKind::RetryOrQuit,
        StartupDialog::MigrationUnreadable {
            offers_skip_migration: true,
        } => PromptKind::RetrySkipOrQuit,
        StartupDialog::MigrationUnreadable {
            offers_skip_migration: false,
        } => PromptKind::RetryOrQuit,
        StartupDialog::D2 | StartupDialog::D3 => PromptKind::StartOverOrQuit,
        StartupDialog::D4 => PromptKind::NewKeyOrQuit,
    }
}

/// A3: Welche Antwort aus dem Fenster **darf** welche Wahl bedeuten.
///
/// **Je Dialogart eingeschränkt, nicht frei.** Eine Oberfläche, die „Neu
/// anfangen" auf D1 schickt, bekommt „beenden". Dieselbe Regel wie im
/// nativen Dialog, nur hier wichtiger: Dort wählt der Nutzer aus Knöpfen,
/// die das Betriebssystem gezeichnet hat; hier kommt die Antwort über IPC
/// und kann jeden Wert haben.
///
/// Als freie Funktion, damit die Tabelle unten ohne eine laufende Tauri-App
/// prüfbar ist — die Einschränkung ist der sicherheitsrelevante Teil, nicht
/// das Schicken des Ereignisses.
fn choice_for(kind: PromptKind, answer: StartupPromptAnswer) -> StartupChoice {
    match (kind, answer) {
        (
            PromptKind::RetryOrQuit | PromptKind::RetrySkipOrQuit | PromptKind::RetrySetUpOrQuit,
            StartupPromptAnswer::Retry,
        ) => StartupChoice::Retry,
        // **Nur** von D1 mit drittem Knopf (A3, D1): Einrichten ist der Weg,
        // auf dem im Fall *fehlt* ein neuer K entsteht.
        (PromptKind::RetrySetUpOrQuit, StartupPromptAnswer::SetUpMasterPassword) => {
            StartupChoice::SetUpMasterPassword
        }
        (PromptKind::RetrySkipOrQuit, StartupPromptAnswer::ContinueWithoutMigration) => {
            StartupChoice::ContinueWithoutMigration
        }
        (PromptKind::StartOverOrQuit, StartupPromptAnswer::StartOver) => StartupChoice::StartOver,
        (PromptKind::NewKeyOrQuit, StartupPromptAnswer::GenerateNewKey) => {
            StartupChoice::GenerateNewKey
        }
        (_, other) => {
            if other != StartupPromptAnswer::Quit && other != StartupPromptAnswer::Cancel {
                tracing::warn!(
                    ?kind,
                    "the window answered a startup dialog with a choice it does not offer; \
                     quitting without touching anything (Spec 0101, A3)"
                );
            }
            StartupChoice::Quit
        }
    }
}

/// Das Warten auf eine Antwort aus dem Fenster — **ohne Tauri**.
///
/// Hält genau eine offene Frage; mehr kann es nicht geben, weil der
/// Startablauf linear ist und auf jede Antwort wartet.
///
/// **Warum ein eigener Typ** (Klarstellung 9): Die Zeitgrenze ist der Teil,
/// auf den es sicherheitstechnisch ankommt, und genau der ließ sich am
/// [`WindowStartupPrompt`] nicht prüfen — der hängt an `tauri::AppHandle`
/// (also an `Wry`), und die Testlaufzeit von Tauri ist eine andere
/// (`MockRuntime`). Hier ist das Warten eine Sache für sich: Der
/// Tauri-Anteil schrumpft auf den `emit`-Aufruf, den der Aufrufer als
/// Abschluss mitgibt.
struct PromptChannel {
    pending: Mutex<Option<oneshot::Sender<StartupPromptAnswer>>>,
    /// Gesetzt, wenn eine Frage in die Zeitgrenze gelaufen ist.
    timed_out: AtomicBool,
    /// Die Zeitgrenze. Als Feld und nicht als Konstante am Verwendungsort,
    /// damit ein Test sie herunterdrehen kann — eine Prüfung, die fünf
    /// Minuten wartet, wird nie gefahren, und `tokio::time::pause` trägt das
    /// verschachtelte `block_on` hier nicht.
    timeout: std::time::Duration,
}

impl PromptChannel {
    fn new(timeout: std::time::Duration) -> Self {
        Self {
            pending: Mutex::new(None),
            timed_out: AtomicBool::new(false),
            timeout,
        }
    }

    /// Nimmt die Antwort an. `false`, wenn gerade keine Frage offen ist —
    /// dann ist der Aufruf verspätet oder erfunden und wird verworfen, statt
    /// eine spätere Frage vorab zu beantworten.
    fn answer(&self, answer: StartupPromptAnswer) -> bool {
        let sender = self.pending.lock().expect("Prompt-Sperre").take();
        match sender {
            Some(sender) => sender.send(answer).is_ok(),
            None => {
                tracing::warn!("a startup-prompt answer arrived with no question open");
                false
            }
        }
    }

    fn timed_out(&self) -> bool {
        self.timed_out.load(Ordering::SeqCst)
    }

    /// Klarstellung 10e: **Ein reiner Hinweis wartet auf keine Antwort.**
    ///
    /// Anzeigen und zurückkehren — ohne eine Frage offen zu halten, ohne
    /// Zeitgrenze, ohne Kennzeichnung. Vorher lief der Hinweis aus A5 („die
    /// Dateien heißen jetzt …") durch [`Self::show_and_wait`]: Er hielt den
    /// ganzen Startablauf auf, bis jemand „OK" drückte, und lief nach fünf
    /// Minuten in die Zeitgrenze — mit dem Ergebnis, dass der Start danach
    /// als „keine Antwort erhalten" abbrach, **obwohl das Umbenennen schon
    /// passiert war**. Ein Hinweis hat keine Antwort, auf die es ankommt;
    /// er steht im Fenster, bis der Nutzer ihn wegklickt, und der Start
    /// läuft weiter.
    ///
    /// Die Kennzeichnung wird hier **nicht** angefasst: Sie gehört der
    /// letzten *Frage*, und ein Hinweis ist keine.
    fn show_only<E: std::fmt::Display>(&self, show: impl FnOnce() -> Result<(), E>) {
        if let Err(err) = show() {
            // Kein Ausgang nötig: Es ist nichts zu entscheiden. Dass der
            // Hinweis nicht ankam, darf aber nicht still bleiben — sonst
            // erfährt der Nutzer den neuen Dateinamen aus A5 nirgends.
            tracing::error!(error = %err, "could not show the startup notice in the window");
        }
        debug_assert!(
            self.pending.lock().expect("Prompt-Sperre").is_none(),
            "ein Hinweis darf keine Frage offen lassen (Klarstellung 10e)"
        );
    }

    /// Frage offen halten, `show` aufrufen, auf die Antwort warten.
    fn show_and_wait<E: std::fmt::Display>(
        &self,
        show: impl FnOnce() -> Result<(), E>,
    ) -> StartupPromptAnswer {
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().expect("Prompt-Sperre");
            // Eine noch offene Frage kann es nicht geben (linearer Ablauf).
            // Käme es doch dazu, ist „beenden“ die sichere Antwort auf die
            // alte — nicht ihr stilles Verschwinden.
            if let Some(previous) = pending.take() {
                let _ = previous.send(StartupPromptAnswer::Quit);
                tracing::warn!("a second startup prompt replaced one that was still open");
            }
            *pending = Some(tx);
            // **Die Kennzeichnung gilt der Frage, nicht dem Programmlauf**
            // (spec-reviewer Runde 6): Der Fragesteller lebt den ganzen
            // Lauf. Blieb die Kennzeichnung stehen, bekäme ein *bewusstes*
            // „Beenden" auf die zweite Frage die Meldung über die
            // ausgebliebene Antwort auf die erste — falsch gegenüber
            // jemandem, der gerade selbst entschieden hat. Belegt von
            // `test_a_timed_out_marker_does_not_outlive_its_question`.
            self.timed_out.store(false, Ordering::SeqCst);
        }

        if let Err(err) = show() {
            // Ohne Fenster gibt es keine Antwort. „Beenden“ ist der einzige
            // Ausgang, der nichts anfasst.
            tracing::error!(error = %err, "could not show the startup prompt in the window");
            let _ = self.pending.lock().expect("Prompt-Sperre").take();
            return StartupPromptAnswer::Quit;
        }

        // s. Modulkommentar: synchrones Warten in einer asynchronen
        // Aufgabe, auf der gemessenen Multi-Thread-Laufzeit — jetzt mit
        // einer Zeitgrenze (s. [`PROMPT_TIMEOUT`]).
        let waited = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { tokio::time::timeout(self.timeout, rx).await })
        });
        match waited {
            Ok(Ok(answer)) => answer,
            // Der Kanal ist zugegangen — das Fenster ist weg. Wieder:
            // beenden, nichts anfassen.
            Ok(Err(_)) => {
                tracing::warn!("the window closed while a startup prompt was open");
                StartupPromptAnswer::Quit
            }
            Err(_elapsed) => {
                // **Sichtbar scheitern statt hängen.** Der Rückgabewert ist
                // `Quit`, weil das der einzige Ausgang ist, der nichts
                // anfasst — die Kennzeichnung sorgt dafür, dass der
                // Aufrufer daraus keine Nutzerentscheidung macht, sondern
                // eine Meldung über die ausgebliebene Antwort.
                self.timed_out.store(true, Ordering::SeqCst);
                // Den verwaisten Sender wegräumen: Eine Antwort, die jetzt
                // noch käme, soll „es ist gerade keine Frage offen" ergeben
                // und nicht eine längst abgelaufene Frage beantworten.
                let _ = self.pending.lock().expect("Prompt-Sperre").take();
                tracing::error!(
                    seconds = self.timeout.as_secs(),
                    "no answer to the startup prompt arrived in time; the window may never \
                     have received it (Spec 0101, A16; Spec 0059)"
                );
                StartupPromptAnswer::Quit
            }
        }
    }
}

/// Der Fragesteller. Hält genau **eine** offene Frage — mehr kann es nicht
/// geben, weil der Startablauf linear ist und auf jede Antwort wartet.
pub struct WindowStartupPrompt {
    app: tauri::AppHandle,
    pub db_path: PathBuf,
    pub language: Language,
    pub keychain: KeychainAvailability,
    /// Das Warten selbst (Frage offen halten, Antwort annehmen,
    /// Zeitgrenze) — ohne Tauri, s. [`PromptChannel`].
    channel: PromptChannel,
    /// Das zuletzt eingegebene neue Master-Passwort (A13). Steht getrennt
    /// von der offenen Frage, weil es nicht in das Antwort-Ereignis gehört:
    /// Ein Passwort in einem `Serialize`-Typ wäre genau der Weg in ein DTO,
    /// den §6 ausschließt. Es kommt über ein eigenes Kommando.
    new_password: Mutex<Option<NewMasterPassword>>,
}

impl WindowStartupPrompt {
    pub fn new(
        app: tauri::AppHandle,
        db_path: PathBuf,
        language: Language,
        keychain: KeychainAvailability,
    ) -> Self {
        Self {
            app,
            db_path,
            language,
            keychain,
            channel: PromptChannel::new(PROMPT_TIMEOUT),
            new_password: Mutex::new(None),
        }
    }

    /// Nimmt die Antwort aus dem Fenster an. `false`, wenn gerade keine
    /// Frage offen ist — dann ist der Aufruf verspätet oder erfunden und
    /// wird verworfen, statt eine spätere Frage vorab zu beantworten.
    pub fn answer(&self, answer: StartupPromptAnswer) -> bool {
        self.channel.answer(answer)
    }

    /// A13: das neue Master-Passwort hinterlegen, bevor die Maske ihre
    /// Antwort schickt.
    ///
    /// Die Bestätigung der Warnung (A13/E10, Klarstellung 12) liegt **im
    /// selben Wert** und nicht daneben. Das ist der Punkt: Sie wird mit dem
    /// Passwort zusammen gesetzt, zusammen geleert und zusammen
    /// entnommen — eine Bestätigung aus einem früheren Versuch kann so
    /// nicht an ein neues Passwort geraten.
    pub fn provide_new_password(&self, password: NewMasterPassword) {
        *self.new_password.lock().expect("Passwort-Sperre") = Some(password);
    }

    fn request(&self, request: StartupPromptRequest) -> StartupPromptAnswer {
        // Tauri-Anteil: das Ereignis schicken. Das Warten darauf liegt in
        // [`PromptChannel`] und kennt Tauri nicht.
        self.channel.show_and_wait(|| {
            self.app
                .emit(STARTUP_PROMPT_EVENT, &request)
                .map_err(|err| err.to_string())
        })
    }

    /// Ist eine Frage in die Zeitgrenze gelaufen?
    ///
    /// Der Startablauf übersetzt jede abgelehnte Frage in
    /// `StartupAbort::UserQuit` — das ist richtig (es ist nichts angefasst),
    /// aber die Meldung „Der Start wurde abgebrochen" wäre hier falsch: Der
    /// Nutzer hat nichts abgebrochen, er hat die Frage vermutlich nie
    /// gesehen. Der Aufrufer fragt deshalb nach.
    pub fn timed_out(&self) -> bool {
        self.channel.timed_out()
    }

    /// D1 nennt bei einer vorhandenen Klartext-Datenbank den Verlust des
    /// bisherigen Verlaufs (A3, D1) — dieselbe Regel wie im nativen Dialog.
    fn database_is_plaintext(&self) -> bool {
        matches!(
            detect_database_file_state(&self.db_path),
            Ok(DatabaseFileState::Plaintext)
        )
    }
}

impl StartupPrompt for WindowStartupPrompt {
    fn ask(&self, dialog: StartupDialog) -> StartupChoice {
        let path = self.db_path.as_path();
        let kind = kind_for(&dialog);
        let text = match dialog {
            StartupDialog::D1 {
                offers_password_setup,
            } => texts::d1_keychain_unreachable_text(
                self.keychain
                    .unavailable_reason()
                    .unwrap_or(KeychainUnavailableReason::Unknown),
                std::env::consts::OS,
                offers_password_setup,
                self.database_is_plaintext(),
                self.language,
            ),
            // A11: „Dialog D1 ohne Einrichten“ — derselbe Text, andere
            // Knöpfe (A11.1 kommt im Passwort-Modus dazu).
            StartupDialog::MigrationUnreadable { .. } => texts::d1_keychain_unreachable_text(
                self.keychain
                    .unavailable_reason()
                    .unwrap_or(KeychainUnavailableReason::Unknown),
                std::env::consts::OS,
                false,
                self.database_is_plaintext(),
                self.language,
            ),
            StartupDialog::D2 => texts::d2_not_readable_text(path, self.language),
            StartupDialog::D3 => texts::d3_unusable_key_text(path, self.language),
            StartupDialog::D4 => texts::d4_new_key_for_plaintext_text(path, self.language),
        };

        let answer = self.request(StartupPromptRequest {
            kind,
            title: text.title,
            message: text.message,
        });
        choice_for(kind, answer)
    }

    fn confirm_start_over(&self, renamed_to: Option<&str>) -> bool {
        let text = texts::start_over_confirmation_text(renamed_to, self.language);
        self.request(StartupPromptRequest {
            kind: PromptKind::ConfirmStartOver,
            title: text.title,
            message: text.message,
        }) == StartupPromptAnswer::Confirm
    }

    fn confirm_generate_new_key(&self) -> bool {
        let text = texts::new_key_confirmation_text(self.language);
        self.request(StartupPromptRequest {
            kind: PromptKind::ConfirmNewKey,
            title: text.title,
            message: text.message,
        }) == StartupPromptAnswer::Confirm
    }

    /// Klarstellung 10e: **wartet auf keine Antwort** — s.
    /// [`PromptChannel::show_only`].
    fn notify_started_over(&self, renamed_to: &str) {
        let text = texts::started_over_notice_text(renamed_to, self.language);
        let request = StartupPromptRequest {
            kind: PromptKind::Notice,
            title: text.title,
            message: text.message,
        };
        self.channel.show_only(|| {
            self.app
                .emit(STARTUP_PROMPT_EVENT, &request)
                .map_err(|err| err.to_string())
        });
    }

    /// Issue #113: ein Hinweis wie A5 — über `show_only`, nicht `request`
    /// (Klarstellung 10e: ein Hinweis wartet auf keine Antwort).
    fn notify_unreadable_history_removed(&self, removed: u64) {
        let text = texts::unreadable_history_removed_notice_text(removed, self.language);
        let request = StartupPromptRequest {
            kind: PromptKind::Notice,
            title: text.title,
            message: text.message,
        };
        self.channel.show_only(|| {
            self.app
                .emit(STARTUP_PROMPT_EVENT, &request)
                .map_err(|err| err.to_string())
        });
    }

    fn ask_for_new_master_password(&self) -> Option<NewMasterPassword> {
        // Klarstellung 10e, zweite Hälfte: **ein geöffnetes Passwortfeld ist
        // leer.** Die Maske im Fenster leert ihre Felder selbst; hier wird
        // der Platz im Backend geleert, damit ein Passwort aus einem früheren,
        // abgebrochenen Versuch nicht als Antwort auf diese Frage gilt. Ohne
        // das genügte ein Aufruf von `answer_startup_prompt` mit `Confirm`
        // und ohne Passwort, um das alte zu bestätigen — die Maske zeigte
        // ein leeres Feld, und eingerichtet würde das Passwort von vorhin.
        *self.new_password.lock().expect("Passwort-Sperre") = None;
        let text = texts::master_password_setup_text(self.language);
        let answer = self.request(StartupPromptRequest {
            kind: PromptKind::NewMasterPassword,
            title: text.title,
            message: text.message,
        });
        if answer != StartupPromptAnswer::Confirm {
            // A5/A13: Abbruch heißt, dass nichts verändert ist — auch kein
            // hinterlegtes Passwort bleibt liegen.
            *self.new_password.lock().expect("Passwort-Sperre") = None;
            return None;
        }
        self.new_password.lock().expect("Passwort-Sperre").take()
    }

    fn can_ask_for_a_password(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Erfolgsfall von `show`: Das Ereignis ging raus. Genau der Fall
    /// aus dem Fund — `emit` liefert `Ok`, auch wenn niemand zuhört.
    fn sent() -> Result<(), String> {
        Ok(())
    }

    /// Klarstellung 9, vierter Punkt (spec-reviewer Lauf 4, Fund 7): Eine
    /// Frage, deren Antwort ausbleibt, **scheitert sichtbar statt zu
    /// hängen**.
    ///
    /// Vorher wartete `rx.blocking_recv()` ohne Grenze. Hatte das Fenster
    /// den Zuhörer für `startup:prompt` nicht (oder nicht mehr), war der
    /// gesperrte Zustand ein Dauerhänger: kein Dialog, keine Meldung, kein
    /// Ausgang außer dem Beenden über das Fenstersystem.
    ///
    /// Die Prüfung ist selbst in eine Zeitgrenze gefasst: Trägt die
    /// Nachbesserung nicht, soll sie **scheitern** und nicht ihrerseits
    /// hängen.
    /// Führt `show_and_wait` auf einem **eigenen** Thread mit eigener
    /// Laufzeit aus und wartet höchstens `limit` darauf.
    ///
    /// Warum nicht `tokio::time::timeout` um die Aufgabe: Gemessen beim
    /// Gegenbeweis — ohne die Zeitgrenze im Produktivcode blieb der Test
    /// **hängen**, statt an der äußeren Grenze zu scheitern; das blockierte
    /// Warten lässt sich nicht von außen abbrechen. Ein eigener Thread lässt
    /// sich dagegen stehen lassen: Der Test scheitert dann sichtbar, und
    /// genau das soll er, wenn die Nachbesserung einmal zurückgedreht wird.
    fn wait_on_its_own_thread(
        channel: std::sync::Arc<PromptChannel>,
        limit: std::time::Duration,
    ) -> Result<StartupPromptAnswer, std::sync::mpsc::RecvTimeoutError> {
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Laufzeit");
            let answer = runtime.block_on(async { channel.show_and_wait(sent) });
            let _ = done.send(answer);
        });
        finished.recv_timeout(limit)
    }

    /// Wie [`wait_on_its_own_thread`], nur für einen Hinweis — der nichts
    /// zurückgibt.
    ///
    /// Auch hier ein eigener Thread mit eigener Laufzeit: Wartete der
    /// Hinweis doch auf eine Antwort, soll der Test **scheitern** und nicht
    /// die fünf Minuten der echten Zeitgrenze absitzen.
    fn notify_on_its_own_thread(
        channel: std::sync::Arc<PromptChannel>,
        limit: std::time::Duration,
    ) -> Result<(), std::sync::mpsc::RecvTimeoutError> {
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Laufzeit");
            runtime.block_on(async { channel.show_only(sent) });
            let _ = done.send(());
        });
        finished.recv_timeout(limit)
    }

    /// Der Rumpf einer Methode dieser Datei — vom Kopf bis zur ersten Zeile,
    /// die nur aus vier Leerzeichen und `}` besteht.
    fn method_body<'a>(source: &'a str, signature: &str) -> &'a str {
        let start = source
            .find(signature)
            .unwrap_or_else(|| panic!("`{signature}` gibt es nicht mehr — s. Doc-Kommentar"));
        let rest = &source[start..];
        let end = rest
            .find("\n    }\n")
            .expect("Methodenende nicht gefunden — s. Doc-Kommentar");
        &rest[..end]
    }

    /// Klarstellung 10e, erste Hälfte: **Ein reiner Hinweis wartet auf keine
    /// Antwort.**
    ///
    /// Die Kanal-Grenze ist hier die **echte** ([`PROMPT_TIMEOUT`]): Wartete
    /// der Hinweis, liefe dieser Test in seine eigene Grenze von fünf
    /// Sekunden und scheiterte — er sitzt nicht die fünf Minuten ab.
    #[test]
    fn test_a_notice_does_not_wait_for_an_answer() {
        let channel = std::sync::Arc::new(PromptChannel::new(PROMPT_TIMEOUT));

        notify_on_its_own_thread(channel.clone(), std::time::Duration::from_secs(5)).expect(
            "ein Hinweis hat keine Antwort, auf die es ankommt — er darf den Startablauf nicht \
             aufhalten (Klarstellung 10e)",
        );

        assert!(
            !channel.answer(StartupPromptAnswer::Cancel),
            "ein Hinweis darf keine Frage offen lassen — sonst beantwortet sein „OK“ die \
             nächste, echte Frage vorab"
        );
        assert!(
            !channel.timed_out(),
            "ein Hinweis kann nicht in die Zeitgrenze laufen; die Kennzeichnung gehört der \
             letzten Frage, und ein Hinweis ist keine"
        );
    }

    /// Klarstellung 10e im Produktivpfad: Der Hinweis aus A5 geht über
    /// [`PromptChannel::show_only`], und eine neue Passwortfrage beginnt mit
    /// leerem Platz.
    ///
    /// Gelesen wird die Quelle, weil [`WindowStartupPrompt`] an
    /// `tauri::AppHandle` (also `Wry`) hängt und mit Tauris Test-Laufzeit
    /// nicht zu bauen ist (ADR 0096 §3). Der Preis ist bekannt: Wird eine der
    /// beiden Methoden umbenannt, scheitert dieser Test, obwohl nichts
    /// kaputt ist — dann gehört die neue Form hier herein.
    #[test]
    fn test_the_notice_does_not_wait_and_a_new_password_question_starts_empty() {
        let source = include_str!("window_prompt.rs");

        let notice = method_body(source, "fn notify_started_over(&self, renamed_to: &str) {");
        assert!(
            notice.contains("self.channel.show_only("),
            "Klarstellung 10e: Der Hinweis aus A5 muss über `show_only` gehen"
        );
        // Issue #113: derselbe Grund für den Hinweis nach der Umstellung.
        let removed_notice = method_body(
            source,
            "fn notify_unreadable_history_removed(&self, removed: u64) {",
        );
        assert!(removed_notice.contains("self.channel.show_only("));
        assert!(!removed_notice.contains("self.request("));
        assert!(
            !notice.contains("self.request("),
            "Klarstellung 10e: `request` wartet auf eine Antwort und läuft in die Zeitgrenze — \
             genau das darf ein Hinweis nicht. Vorher brach der Start danach als „keine Antwort \
             erhalten“ ab, obwohl das Umbenennen schon passiert war."
        );

        let asking = method_body(
            source,
            "fn ask_for_new_master_password(&self) -> Option<NewMasterPassword> {",
        );
        let clears = asking
            .find("*self.new_password.lock().expect(\"Passwort-Sperre\") = None;")
            .expect(
                "Klarstellung 10e: Der Platz für das neue Passwort muss geleert werden, bevor \
                 gefragt wird — sonst gilt ein Passwort aus einem früheren, abgebrochenen \
                 Versuch als Antwort auf diese Frage",
            );
        let asks = asking
            .find("self.request(")
            .expect("hier wird nicht mehr gefragt — s. Doc-Kommentar");
        assert!(
            clears < asks,
            "Klarstellung 10e: geleert wird **vor** dem Fragen; danach wäre es die Antwort, die \
             weggeworfen wird"
        );
    }

    /// Klarstellung 9, vierter Punkt (spec-reviewer Lauf 4, Fund 7): Eine
    /// Frage, deren Antwort ausbleibt, **scheitert sichtbar statt zu
    /// hängen**.
    ///
    /// Vorher wartete `rx.blocking_recv()` ohne Grenze. Hatte das Fenster
    /// den Zuhörer für `startup:prompt` nicht (oder nicht mehr), war der
    /// gesperrte Zustand ein Dauerhänger: kein Dialog, keine Meldung, kein
    /// Ausgang außer dem Beenden über das Fenstersystem.
    #[test]
    fn test_a_startup_prompt_without_an_answer_fails_visibly_instead_of_hanging() {
        let channel = std::sync::Arc::new(PromptChannel::new(std::time::Duration::from_millis(50)));
        let answer = wait_on_its_own_thread(channel.clone(), std::time::Duration::from_secs(30));

        let answer = answer.expect(
            "die Frage hat nicht aufgehört zu warten — genau der Fund, den diese Prüfung \
             abdeckt",
        );
        assert_eq!(
            answer,
            StartupPromptAnswer::Quit,
            "ohne Antwort ist „beenden“ der einzige Ausgang, der nichts anfasst"
        );
        assert!(
            channel.timed_out(),
            "der Aufrufer muss die ausgebliebene Antwort von einem Abbruch unterscheiden \
             können — sonst meldet er „abgebrochen“, obwohl der Nutzer nichts gesehen hat"
        );
        // Der verwaiste Sender ist weg: Eine Antwort, die jetzt noch käme,
        // darf keine abgelaufene Frage beantworten.
        assert!(
            !channel.answer(StartupPromptAnswer::Confirm),
            "eine verspätete Antwort muss „keine Frage offen“ ergeben"
        );
    }

    /// Die Grenze darf keine echte Entscheidung abschneiden: Kommt eine
    /// Antwort, gilt sie — und `timed_out` bleibt aus.
    ///
    /// Ohne diesen Fall bewiese der Test oben nichts über den Normalbetrieb;
    /// eine Fassung, die **immer** sofort in die Grenze läuft, käme damit
    /// durch.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_an_answer_that_arrives_still_counts() {
        let channel = std::sync::Arc::new(PromptChannel::new(std::time::Duration::from_secs(30)));

        let waiting = {
            let channel = channel.clone();
            tokio::task::spawn_blocking(move || channel.show_and_wait(sent))
        };

        // Warten, bis die Frage wirklich offen ist — ein Schlaf wäre hier
        // ein Zeitrennen.
        loop {
            if channel.pending.lock().expect("Prompt-Sperre").is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(channel.answer(StartupPromptAnswer::Confirm));

        assert_eq!(
            waiting.await.expect("kein Panic"),
            StartupPromptAnswer::Confirm
        );
        assert!(
            !channel.timed_out(),
            "eine beantwortete Frage ist nicht in die Grenze gelaufen"
        );
    }

    /// A3: **Jede** Antwort, die eine Dialogart nicht anbietet, wird
    /// „beenden" — die vollständige Tabelle, nicht nur die erlaubten Paare.
    ///
    /// Das ist die Prüfung, auf die es bei einer Antwort über IPC ankommt:
    /// Die gefährlichen Wahlen (`StartOver`, `GenerateNewKey`,
    /// `SetUpMasterPassword`, `ContinueWithoutMigration`) erzeugen einen
    /// neuen K oder benennen Dateien um bzw. lassen Secrets liegen. Käme
    /// eine davon an einem Dialog durch, der sie nicht anbietet, wäre das
    /// ein Weg, A3 zu umgehen, ohne dass der Nutzer die Wahl gesehen hat.
    #[test]
    fn test_each_dialog_accepts_only_the_choices_it_offers() {
        use PromptKind as K;
        use StartupPromptAnswer as A;

        const ALL_KINDS: &[K] = &[
            K::RetryOrQuit,
            K::RetrySkipOrQuit,
            K::RetrySetUpOrQuit,
            K::StartOverOrQuit,
            K::NewKeyOrQuit,
            K::ConfirmStartOver,
            K::ConfirmNewKey,
            K::NewMasterPassword,
            K::Notice,
        ];
        const ALL_ANSWERS: &[A] = &[
            A::Retry,
            A::Quit,
            A::StartOver,
            A::GenerateNewKey,
            A::ContinueWithoutMigration,
            A::SetUpMasterPassword,
            A::Confirm,
            A::Cancel,
        ];
        /// Die erlaubten Paare — alles andere muss „beenden" werden.
        const ALLOWED: &[(K, A, StartupChoice)] = &[
            (K::RetryOrQuit, A::Retry, StartupChoice::Retry),
            (K::RetrySkipOrQuit, A::Retry, StartupChoice::Retry),
            (K::RetrySetUpOrQuit, A::Retry, StartupChoice::Retry),
            (
                K::RetrySkipOrQuit,
                A::ContinueWithoutMigration,
                StartupChoice::ContinueWithoutMigration,
            ),
            (
                K::RetrySetUpOrQuit,
                A::SetUpMasterPassword,
                StartupChoice::SetUpMasterPassword,
            ),
            (K::StartOverOrQuit, A::StartOver, StartupChoice::StartOver),
            (
                K::NewKeyOrQuit,
                A::GenerateNewKey,
                StartupChoice::GenerateNewKey,
            ),
        ];

        for kind in ALL_KINDS {
            for answer in ALL_ANSWERS {
                let expected = ALLOWED
                    .iter()
                    .find(|(k, a, _)| k == kind && a == answer)
                    .map(|(_, _, choice)| *choice)
                    .unwrap_or(StartupChoice::Quit);
                assert_eq!(
                    choice_for(*kind, *answer),
                    expected,
                    "A3: {kind:?} + {answer:?} muss {expected:?} ergeben — eine Wahl, die ein \
                     Dialog nicht anbietet, darf nie durchkommen"
                );
            }
        }
    }

    /// A3/A13: **Welcher Fall welche Knöpfe zeigt** — und dass der dritte
    /// Knopf nur an D1 hängt, wenn der Startablauf ihn anbietet.
    #[test]
    fn test_each_startup_dialog_shows_the_buttons_its_case_allows() {
        assert_eq!(
            kind_for(&StartupDialog::D1 {
                offers_password_setup: true
            }),
            PromptKind::RetrySetUpOrQuit,
            "A3, D1 ab Etappe 3: mit dem dritten Knopf „Master-Passwort einrichten“ (A13)"
        );
        assert_eq!(
            kind_for(&StartupDialog::D1 {
                offers_password_setup: false
            }),
            PromptKind::RetryOrQuit,
            "A3, D1 ohne Einrichten: Bei `Locked`/`Unknown`/Backend-Fehler würde ein neuer K den \
             feldweise verschlüsselten Verlauf unlesbar machen"
        );
        // T13, wörtlich: „Umzugs-Dialog (A11) nie“.
        assert_eq!(
            kind_for(&StartupDialog::MigrationUnreadable {
                offers_skip_migration: true
            }),
            PromptKind::RetrySkipOrQuit
        );
        assert_eq!(
            kind_for(&StartupDialog::MigrationUnreadable {
                offers_skip_migration: false
            }),
            PromptKind::RetryOrQuit
        );
        assert_eq!(kind_for(&StartupDialog::D2), PromptKind::StartOverOrQuit);
        assert_eq!(kind_for(&StartupDialog::D3), PromptKind::StartOverOrQuit);
        assert_eq!(kind_for(&StartupDialog::D4), PromptKind::NewKeyOrQuit);

        // Und die Aussage über den Umzugs-Dialog zu Ende geführt: Keine
        // seiner beiden Formen zeigt den dritten Knopf, egal was er sonst
        // anbietet.
        for offers_skip_migration in [true, false] {
            assert_ne!(
                kind_for(&StartupDialog::MigrationUnreadable {
                    offers_skip_migration
                }),
                PromptKind::RetrySetUpOrQuit,
                "A11/T13: Ein gescheiterter Secret-Umzug darf nie zum Einrichten eines \
                 Master-Passworts führen — dabei entstünde im Fall *fehlt* ein neuer K"
            );
        }
    }

    /// A3, D1 (A13): Der dritte Knopf ist **nur** an der Dialogart mit
    /// drittem Knopf zu haben.
    ///
    /// Eigens hervorgehoben, weil „Master-Passwort einrichten" im Fall
    /// *fehlt* einen neuen K erzeugt (A3: „Ein neuer K entsteht nur …").
    /// Würde der Umzugs-Dialog aus A11 ihn annehmen, entstünde ein neuer K
    /// aus einem gescheiterten Secret-Umzug — T13 verlangt ausdrücklich das
    /// Gegenteil („Umzugs-Dialog (A11) nie").
    #[test]
    fn test_only_d1_with_the_third_button_accepts_setting_up_a_master_password() {
        assert_eq!(
            choice_for(
                PromptKind::RetrySetUpOrQuit,
                StartupPromptAnswer::SetUpMasterPassword
            ),
            StartupChoice::SetUpMasterPassword
        );
        for kind in [
            PromptKind::RetryOrQuit,
            PromptKind::RetrySkipOrQuit,
            PromptKind::StartOverOrQuit,
            PromptKind::NewKeyOrQuit,
            PromptKind::Notice,
        ] {
            assert_eq!(
                choice_for(kind, StartupPromptAnswer::SetUpMasterPassword),
                StartupChoice::Quit,
                "{kind:?} bietet „Master-Passwort einrichten“ nicht an und darf es nicht \
                 annehmen — im Fall *fehlt* entsteht dabei ein neuer K"
            );
        }
    }

    /// **Die Kennzeichnung gilt der Frage, nicht dem Programmlauf** (ADR
    /// 0097 §4.3, zweiter der beiden fehlenden Tests).
    ///
    /// Der Fragesteller lebt den ganzen Lauf. Blieb die Kennzeichnung nach
    /// einer abgelaufenen Frage stehen, bekäme ein *bewusstes* „Beenden" auf
    /// die **nächste** Frage die Meldung über die ausgebliebene Antwort auf
    /// die erste („Smart SSH hat auf eine Rückfrage gewartet, aber keine
    /// Antwort erhalten") — falsch gegenüber jemandem, der gerade selbst
    /// entschieden hat, und irreführend bei der Fehlersuche.
    ///
    /// Geprüft wird deshalb der Übergang: erste Frage läuft in die Grenze,
    /// zweite Frage auf **demselben** Kanal wird beantwortet.
    #[test]
    fn test_a_timed_out_marker_does_not_outlive_its_question() {
        // Eine Sekunde: lang genug, dass die zweite Frage ohne Zeitrennen
        // beantwortet werden kann, kurz genug für einen Test.
        let channel = std::sync::Arc::new(PromptChannel::new(std::time::Duration::from_secs(1)));

        let first = wait_on_its_own_thread(channel.clone(), std::time::Duration::from_secs(30))
            .expect("die erste Frage hat nicht aufgehört zu warten");
        assert_eq!(first, StartupPromptAnswer::Quit);
        assert!(
            channel.timed_out(),
            "die erste Frage ist in die Grenze gelaufen — ohne diese Vorbedingung prüft der \
             Rest nichts"
        );

        // Ein Antwortgeber, der wartet, bis die zweite Frage wirklich offen
        // ist. Kein Schlaf: der wäre hier ein Zeitrennen.
        let answering = {
            let channel = channel.clone();
            std::thread::spawn(move || {
                loop {
                    if channel.pending.lock().expect("Prompt-Sperre").is_some() {
                        break;
                    }
                    std::thread::yield_now();
                }
                assert!(
                    channel.answer(StartupPromptAnswer::Cancel),
                    "die zweite Frage war offen und muss die Antwort annehmen"
                );
            })
        };

        let second = wait_on_its_own_thread(channel.clone(), std::time::Duration::from_secs(30))
            .expect("die zweite Frage hat nicht aufgehört zu warten");
        answering.join().expect("kein Panic im Antwortgeber");

        assert_eq!(
            second,
            StartupPromptAnswer::Cancel,
            "eine Antwort, die rechtzeitig kommt, gilt"
        );
        assert!(
            !channel.timed_out(),
            "die Kennzeichnung der ersten Frage darf die zweite nicht überdauern — sonst meldet \
             der Aufrufer „keine Antwort erhalten“, obwohl der Nutzer gerade selbst entschieden \
             hat"
        );
    }

    /// Scheitert schon das Anzeigen, wird die Frage nicht offen gelassen —
    /// sonst beantwortete eine spätere Nachricht eine Frage, die niemand
    /// gestellt hat.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_a_prompt_that_could_not_be_shown_leaves_no_question_open() {
        let answer = tokio::task::spawn_blocking(|| {
            let channel = PromptChannel::new(std::time::Duration::from_secs(30));
            let answer = channel.show_and_wait(|| Err::<(), String>("kein Fenster".to_string()));
            assert!(!channel.answer(StartupPromptAnswer::Confirm));
            assert!(
                !channel.timed_out(),
                "das ist kein Zeitablauf, sondern ein Anzeigefehler"
            );
            answer
        })
        .await
        .expect("kein Panic");

        assert_eq!(answer, StartupPromptAnswer::Quit);
    }
}
