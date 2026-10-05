//! Spec 0101, Teil 0 Frage 3: die Startdialoge **im Fenster**, für den
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
//! synchronen Secret-Speicher (Teil 0 Frage 2: Tauris Laufzeit ist eine
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
    /// Zweite Bestätigung bejaht.
    Confirm,
    /// Zweite Bestätigung abgelehnt, Maske abgebrochen, Meldung quittiert.
    Cancel,
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
        let (kind, text) = match dialog {
            StartupDialog::D1 {
                offers_password_setup,
            } => (
                PromptKind::RetryOrQuit,
                texts::d1_keychain_unreachable_text(
                    self.keychain
                        .unavailable_reason()
                        .unwrap_or(KeychainUnavailableReason::Unknown),
                    std::env::consts::OS,
                    offers_password_setup,
                    self.database_is_plaintext(),
                    self.language,
                ),
            ),
            // A11: „Dialog D1 ohne Einrichten“ — derselbe Text, andere
            // Knöpfe (A11.1 kommt im Passwort-Modus dazu).
            StartupDialog::MigrationUnreadable {
                offers_skip_migration,
            } => (
                if offers_skip_migration {
                    PromptKind::RetrySkipOrQuit
                } else {
                    PromptKind::RetryOrQuit
                },
                texts::d1_keychain_unreachable_text(
                    self.keychain
                        .unavailable_reason()
                        .unwrap_or(KeychainUnavailableReason::Unknown),
                    std::env::consts::OS,
                    false,
                    self.database_is_plaintext(),
                    self.language,
                ),
            ),
            StartupDialog::D2 => (
                PromptKind::StartOverOrQuit,
                texts::d2_not_readable_text(path, self.language),
            ),
            StartupDialog::D3 => (
                PromptKind::StartOverOrQuit,
                texts::d3_unusable_key_text(path, self.language),
            ),
            StartupDialog::D4 => (
                PromptKind::NewKeyOrQuit,
                texts::d4_new_key_for_plaintext_text(path, self.language),
            ),
        };

        let answer = self.request(StartupPromptRequest {
            kind,
            title: text.title,
            message: text.message,
        });
        // **Die Abbildung ist je Dialogart eingeschränkt**, nicht frei: Eine
        // Oberfläche, die „Neu anfangen“ auf D1 schickt, bekommt „beenden“.
        // Dieselbe Regel wie im nativen Dialog, nur hier wichtiger, weil die
        // Antwort über IPC kommt.
        match (kind, answer) {
            (PromptKind::RetryOrQuit | PromptKind::RetrySkipOrQuit, StartupPromptAnswer::Retry) => {
                StartupChoice::Retry
            }
            (PromptKind::RetrySkipOrQuit, StartupPromptAnswer::ContinueWithoutMigration) => {
                StartupChoice::ContinueWithoutMigration
            }
            (PromptKind::StartOverOrQuit, StartupPromptAnswer::StartOver) => {
                StartupChoice::StartOver
            }
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

    fn notify_started_over(&self, renamed_to: &str) {
        let text = texts::started_over_notice_text(renamed_to, self.language);
        let _ = self.request(StartupPromptRequest {
            kind: PromptKind::Notice,
            title: text.title,
            message: text.message,
        });
    }

    fn ask_for_new_master_password(&self) -> Option<NewMasterPassword> {
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
