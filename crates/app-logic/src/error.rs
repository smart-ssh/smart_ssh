//! Fehlertyp für Tauri-Commands. Tauri serialisiert das `Err`-Ergebnis
//! eines `#[tauri::command]` per `serde::Serialize` zurück ans Frontend —
//! ein flacher `{ message: String }` reicht für Teil 1 (die einzelnen
//! Rust-Fehlertypen aus `core`/`persistence-sqlite` unterscheiden sich zu
//! sehr, um sie 1:1 über die IPC-Grenze zu spiegeln; das Frontend braucht
//! ohnehin nur eine anzeigbare Meldung). Ein blanket `From<E: Display>`
//! deckt alle projektinternen Fehlertypen ab (`ProfileError`,
//! `CredentialError`, `persistence_sqlite::AiProviderStoreError`,
//! `keyring`-Fehler über `credentials-keyring`), die allesamt `Display`
//! implementieren — kein separater `From`-Impl pro Fehlertyp nötig.

use serde::Serialize;

use ssh_manager_core::entitlements::FeatureLocked;
use ssh_manager_core::profiles::CredentialError;
use ssh_manager_core::ssh::SshError;

/// Spec 0071, A13: stabiler Code für "der OS-Schlüsselbund ist bei diesem
/// Programmlauf nicht erreichbar". Das Frontend übersetzt ihn über
/// `errorCodes.ts` und zeigt den eigenen Text, statt den englischen
/// `Display`-Text der `keyring`-Crate durchzureichen.
pub const KEYCHAIN_UNAVAILABLE: &str = "KEYCHAIN_UNAVAILABLE";

/// Spec 0098, A1: „der Schlüsselbund war beim Start da, dieser Zugriff ist
/// trotzdem gescheitert". Die Konstante wohnt in `core`
/// ([`ssh_manager_core::profiles::KEYCHAIN_ACCESS_FAILED`]), weil
/// [`ssh_manager_core::ssh::SshError::code`] sie für die Verbindungskette
/// (A4) ebenfalls vergibt — ein zweites Literal hier könnte abdriften.
pub use ssh_manager_core::profiles::KEYCHAIN_ACCESS_FAILED;

/// Spec 0101, A9.1: „der Secret-Speicher hat den Zugriff nicht ausgeführt".
/// Wohnt aus demselben Grund in `core` wie [`KEYCHAIN_ACCESS_FAILED`] —
/// `SshError::code` vergibt ihn für die Verbindungskette ebenfalls.
pub use ssh_manager_core::profiles::SECRET_STORE_FAILED;

/// Spec 0098 A1/A2/A7, seit Spec 0101 A9.1 auf den Secret-Speicher
/// umgestellt: der eine Ort, an dem ein [`CredentialError::Backend`] zu
/// Code und Meldung wird.
///
/// **Die Nutzlast bleibt hier liegen** (A5, Ausweitung von Spec 0071 X2).
/// Sie geht nur noch in eine `debug`-Zeile (A7) — und die läuft durch den
/// Redactor. `debug` ist ohne `RUST_LOG` aus (Spec 0094, A3).
///
/// **Die Code-Wahl hängt an der Variante, nie am Text** (A1,
/// Angriffsrichtung T10): Eine Nutzlast, die selbst wie ein Code aussieht,
/// fließt nicht in diese Entscheidung ein — sie wird gar nicht gelesen.
/// Seit A9.1 gibt es überhaupt nur noch **einen** Code auf diesem Weg; der
/// Startzustand des Schlüsselbunds spielt keine Rolle mehr.
fn backend_failure_to_command_error(payload: &str) -> CommandError {
    // Durch den Redactor, wie die Zeile in `core`s `credential_lookup_error`
    // — Begründung dort (spec-reviewer Runde 1): A5 erklärt diese Nutzlast
    // für unzustellbar, und ein Log ist eine Datensenke.
    tracing::debug!(
        error = %{
            use ssh_manager_core::ai::OutputRedactor;
            ssh_manager_core::ai::default_log_redactor().redact_text(payload)
        },
        "secret store backend failure (Spec 0101, A9.1)"
    );
    // Kein „Schlüsselbund" im Text (A9.1). Den ausführlichen Text liefert
    // das Frontend über den Code; diese Meldung ist der Fallback für einen
    // Aufrufer, der ihn nicht kennt.
    CommandError::with_code(
        "Der Zugriff auf den Secret-Speicher ist fehlgeschlagen. \
         Die gespeicherten Zugangsdaten konnten nicht gelesen oder \
         geschrieben werden.",
        SECRET_STORE_FAILED,
    )
}

/// Spec 0076, C-6: Die Überführung einer Schlüsseldatei in den
/// Schlüsselbund ist gescheitert, **und** der eben geschriebene Schlüssel
/// ließ sich nicht wieder entfernen — es liegt jetzt ein privater Schlüssel
/// im Schlüsselbund, auf den kein Server zeigt.
///
/// **Eigener Code, nicht [`KEYCHAIN_UNAVAILABLE`]** (spec-reviewer-Fund,
/// Runde 2): Das Frontend **ersetzt** bei einem bekannten Code die Meldung
/// durch seinen eigenen Text (`errorCodes.ts`). Mit
/// `KEYCHAIN_UNAVAILABLE` hätte der Nutzer „Der Systemschlüsselbund ist
/// nicht verfügbar" gelesen — und ausgerechnet **nicht** erfahren, welcher
/// Eintrag liegen geblieben ist. Das ist die Auskunft, für die dieser Weg
/// überhaupt existiert.
///
/// Solange Schritt 5 der Spec keine Übersetzung dafür ergänzt, ist der Code
/// dem Frontend unbekannt — und genau dann zeigt es die `message`, die den
/// Slot nennt. Die Übersetzung sollte ihn als Parameter führen.
pub const IDENTITY_FILE_ROLLBACK_LEFT_KEY_BEHIND: &str = "IDENTITY_FILE_ROLLBACK_LEFT_KEY_BEHIND";

/// Spec 0077, 3.1.3: Das Muster einer Filterregel lässt sich nicht
/// übersetzen — die Regel wurde deshalb **nicht** gespeichert.
pub const FILTER_RULE_PATTERN_INVALID: &str = "FILTER_RULE_PATTERN_INVALID";

/// Spec 0077, 3.1.3: Wandelt einen [`RuleWriteError`] in einen
/// [`CommandError`] und hängt für ein ungültiges Muster den Code
/// [`FILTER_RULE_PATTERN_INVALID`] an.
///
/// **Ein `From`-Impl ist hier möglich, weil `RuleWriteError` kein `Display`
/// implementiert** (Issue #260, ADR 0068 §7): Der blanket
/// `impl<E: Display> From<E>` unten greift deshalb für diesen Typ nicht,
/// und ein bloßes `?` behält den Code. Ein späteres `Display` auf
/// `RuleWriteError` wäre ein Kohärenzfehler (E0119), kein stiller
/// Codeverlust.
///
/// Als `message` steht der Fehlertext der Bibliothek (mit der Stelle im
/// Muster). Das Frontend ersetzt bei bekanntem Code zwar den Text, zeigt
/// die `message` für diesen Code aber zusätzlich darunter an (3.1.4) —
/// genau dafür wird sie hier mitgegeben.
impl From<crate::filter_rules::RuleWriteError> for CommandError {
    fn from(err: crate::filter_rules::RuleWriteError) -> Self {
        match err {
            crate::filter_rules::RuleWriteError::InvalidPattern(pattern_err) => {
                CommandError::with_code(pattern_err.to_string(), FILTER_RULE_PATTERN_INVALID)
            }
            crate::filter_rules::RuleWriteError::Store(
                persistence_sqlite::PolicyStoreError::InvalidPattern(pattern_err),
            ) => CommandError::with_code(pattern_err.to_string(), FILTER_RULE_PATTERN_INVALID),
            crate::filter_rules::RuleWriteError::Store(store_err) => CommandError::from(store_err),
        }
    }
}

/// Spec 0101, A9.1 (zuvor Spec 0071 A13/X2 und Spec 0098 A1/A2): Wandelt
/// einen [`CredentialError`] des **Secret-Speichers** in einen
/// [`CommandError`] und hängt jedem [`CredentialError::Backend`] den
/// stabilen Code [`SECRET_STORE_FAILED`] an.
///
/// **Warum der Schlüsselbund hier nicht mehr vorkommt:** Seit A9 liegen die
/// Secrets in der verschlüsselten Datenbank
/// ([`persistence_sqlite::SqliteCredentialStore`]); der Schlüsselbund trägt
/// nur noch K. Jeder Aufrufer dieser Funktion arbeitet auf
/// `AppState.credential_store`, also auf der Datenbank. Hinge der Code
/// weiterhin an `AppState.keychain`, meldete die App für einen
/// Datenbankfehler „Schlüsselbund gesperrt" und schickte den Nutzer in die
/// Systemeinstellungen, wo es nichts zu reparieren gibt. A9.1 verlangt
/// deshalb ausdrücklich: eigener Code, kein `KEYCHAIN_*`, kein
/// „Schlüsselbund" im Text, **und keine Abhängigkeit von
/// `AppState.keychain`** — der Parameter ist mit dieser Spec entfallen.
///
/// **Der Code ersetzt den Text, er ergänzt ihn nicht** (unverändert aus
/// Spec 0098 A5/X2): Die Nutzlast von [`CredentialError::Backend`] erreicht
/// das Frontend auf keinem Weg, auch nicht im Feld `message`. Enthielte ein
/// Backend-Fehler jemals einen Secret-artigen Wert, käme er über diesen Pfad
/// nicht ins Frontend. Zur Diagnose bleibt er auf `debug`
/// (A7, s. [`backend_failure_to_command_error`]).
///
/// [`CredentialError::NotFound`] bleibt bewusst unverändert: „kein Eintrag
/// vorhanden" ist eine fachliche Aussage über den Bestand, keine Störung des
/// Speichers — sie mit einem Störungscode zu überschreiben wäre genau die
/// Verwechslung, die Spec 0071 A14/I4 verbietet.
pub fn secret_store_error(err: CredentialError) -> CommandError {
    match err {
        CredentialError::Backend(payload) => backend_failure_to_command_error(&payload),
        // **Ausdrücklich `NotFound`, kein Catch-all** (spec-reviewer Runde 1):
        // `CommandError::from` setzt `code: None` und nimmt den `Display`-Text
        // als Meldung. Für eine dritte `CredentialError`-Variante wäre das
        // genau der Zustand, den Spec 0098 abgeschafft hat — roher Text, kein
        // stabiler Code. So scheitert stattdessen die Übersetzung.
        not_found @ CredentialError::NotFound(_) => CommandError::from(not_found),
    }
}

/// Spec 0098 A4/A5, seit Spec 0101 A9.1 ohne Schlüsselbund-Bezug: ein
/// [`SshError`] als [`CommandError`].
///
/// **Die Umdeutung nach `KEYCHAIN_UNAVAILABLE` ist ersatzlos entfallen.**
/// Sie war die eine Stelle, an der der Startzustand des Schlüsselbunds den
/// Code eines Verbindungsfehlers verändern konnte. Mit A9 liegt hinter
/// [`SshError::CredentialStoreFailed`] die Datenbank, nicht der
/// Schlüsselbund — die Umdeutung hätte also ab jetzt systematisch die
/// falsche Ursache benannt. [`SshError::code`] vergibt
/// [`SECRET_STORE_FAILED`] endgültig; keine Schicht darüber ändert ihn noch.
///
/// Die `message` bleibt `SshError`s `Display`. Für
/// [`SshError::CredentialStoreFailed`] ist das per Konstruktion fester Text
/// plus Secret-Art plus Hop-Angabe (Spec 0076, A-8) — die Nutzlast der
/// Bibliothek hat dort kein Feld (A5).
pub fn ssh_command_error(err: &SshError) -> CommandError {
    CommandError::with_code(err.to_string(), err.code())
}

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub message: String,
    /// Spec 0024, Abschnitt 5: stabiler, sprachunabhängiger Bezeichner fürs
    /// Frontend-Mapping auf Übersetzungs-Keys — `None` für die (weit
    /// überwiegende) Mehrheit der Fehler, die weiterhin nur über den
    /// blanket `From<E: Display>`-Impl unten entstehen (unverändert wie vor
    /// Spec 0024: `message` reicht dafür aus, ein `code` pro Fehlertyp wäre
    /// hier unverhältnismäßiger Aufwand, s. Moduldoc). Gezielt `Some` nur an
    /// den Stellen, die explizit `CommandError::with_code` verwenden — aktuell
    /// die Validierungsfehler aus den Server-/Gruppen-Formularen (Spec 0008,
    /// s. `groups.rs`/`server_credentials.rs`).
    pub code: Option<&'static str>,
    /// Spec 0037, Abschnitt 2: strukturiert eingebettet (nicht nur als
    /// `message`-String), damit das Frontend einen gesperrten Feature-Fehler
    /// eindeutig von einem fachlichen Fehler unterscheiden kann — inkl.
    /// `feature`/`tier`, nicht nur "irgendein Fehler ist aufgetreten".
    /// `None` für jeden anderen Fehler (die weit überwiegende Mehrheit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_locked: Option<FeatureLocked>,
    /// Issue #51: das Schritt-Protokoll eines gescheiterten
    /// Verbindungsaufbaus (`connect`), damit die Oberfläche es unter der
    /// Fehlermeldung zeigen kann. `None` für jeden anderen Fehler.
    ///
    /// Nur für die Oberfläche: Wer einen `CommandError` loggt, loggt
    /// `code`/`message`, nie dieses Feld (Spec 0094).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_log: Option<Vec<ssh_manager_core::ssh::ConnectStepRecord>>,
}

impl CommandError {
    pub fn with_code(message: impl Into<String>, code: &'static str) -> Self {
        Self {
            message: message.into(),
            code: Some(code),
            feature_locked: None,
            connect_log: None,
        }
    }

    /// Issue #51: hängt das Schritt-Protokoll eines Verbindungsversuchs an.
    pub fn with_connect_log(
        mut self,
        steps: Vec<ssh_manager_core::ssh::ConnectStepRecord>,
    ) -> Self {
        self.connect_log = Some(steps);
        self
    }

    /// Spec 0037, Abschnitt 3 (D5): Gating-Konvention. Kein `impl
    /// From<FeatureLocked> for CommandError` (und damit kein bloßes `?` wie
    /// in der Spec-Skizze): `FeatureLocked` implementiert `Display` (über
    /// `thiserror`), der blanket `impl<E: Display> From<E>` unten deckt es
    /// also bereits ab — ein zweiter, spezifischerer `From`-Impl für exakt
    /// diesen einen `Display`-Typ wäre eine von Rusts Kohärenzregeln
    /// verbotene überlappende Impl (E0119). Ein gegatetes Command ruft
    /// deshalb explizit `.map_err(CommandError::feature_locked)?` statt nur
    /// `?` auf `require(...)`.
    ///
    /// `#[allow(dead_code)]`: noch kein einziger tatsächlich gegateter
    /// Command in diesem Schritt (s. `AppState::entitlements`-Doc-
    /// Kommentar) — bleibt bis zum ersten echten `require(...)`-Aufruf
    /// unvermeidlich ungenutzt.
    #[allow(dead_code)]
    pub fn feature_locked(err: FeatureLocked) -> Self {
        Self {
            message: err.to_string(),
            code: Some("FEATURE_LOCKED"),
            feature_locked: Some(err),
            connect_log: None,
        }
    }
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(err: E) -> Self {
        Self {
            message: err.to_string(),
            code: None,
            feature_locked: None,
            connect_log: None,
        }
    }
}

pub type CommandResult<T> = Result<T, CommandError>;

#[cfg(test)]
mod code_tests {
    /// Spec 0024, Abschnitt 5: Codes müssen stabil und eindeutig sein — kein
    /// Code darf für zwei unterschiedliche Validierungsfehler doppelt
    /// vergeben sein. Enumeriert alle aktuell über `CommandError::with_code`
    /// vergebenen Codes (Server-/Gruppen-Formulare, Spec 0008); s.
    /// `groups.rs`/`server_credentials.rs` für die jeweiligen
    /// Einzeltests, die zusätzlich prüfen, dass der *richtige* Code am
    /// jeweiligen Fehlerfall hängt.
    #[test]
    fn test_command_error_with_code_values_are_unique() {
        let codes = [
            "GROUP_SELF_PARENT",
            "GROUP_CYCLE_DETECTED",
            "SERVER_PASSWORD_REQUIRED",
            "SERVER_PRIVATE_KEY_REQUIRED",
            "SERVER_CERTIFICATE_REQUIRED",
            "SERVER_CERTIFICATE_KEY_REQUIRED",
            "FIRST_RUN_NOTICE_NOT_ACKNOWLEDGED",
            "SERVER_JUMP_HOST_LOCAL",
            // Spec 0069, Teil A4:
            "AI_NO_ACTIVE_PROVIDER",
            "SSH_HOST_KEY_NOT_TRUSTED",
            "SSH_HOST_KEY_CONFIRM_TIMEOUT",
            // Spec 0069, Teil A5 (spec-reviewer-Fund):
            "SSH_CONNECTION_ABANDONED",
            // Spec 0071, A13.
            super::KEYCHAIN_UNAVAILABLE,
            // Spec 0098, A1/A6 (BL-0244/BL-0205/BL-0206).
            super::KEYCHAIN_ACCESS_FAILED,
            // Spec 0101, A9.1 (BL-0314). Kommt sowohl aus
            // `secret_store_error` als auch aus
            // `ssh_manager_core::ssh::SshError::code` — derselbe Wert,
            // deshalb nur ein Eintrag.
            super::SECRET_STORE_FAILED,
            // Spec 0076 (BL-0221/BL-0222). Die `KEY_FILE_*`-Codes kommen
            // aus `ssh_manager_core::ssh::KeyFileError::code()` und werden
            // über `identity_file::identity_file_error_to_command_error`
            // in einen `CommandError` gehängt — sie gehören deshalb in
            // dieselbe Eindeutigkeitsprüfung wie die hier direkt
            // vergebenen.
            "SERVER_IDENTITY_FILE_REQUIRED",
            "SERVER_NOT_AN_IDENTITY_FILE",
            super::IDENTITY_FILE_ROLLBACK_LEFT_KEY_BEHIND,
            "KEY_FILE_NOT_FOUND",
            "KEY_FILE_NOT_READABLE",
            "KEY_FILE_PERMISSIONS_TOO_OPEN",
            "KEY_FILE_TOO_LARGE",
            "KEY_FILE_NOT_A_REGULAR_FILE",
            "KEY_FILE_PATH_NOT_ABSOLUTE",
            "KEY_FILE_INVALID_KEY",
            // Spec 0077, 3.1.3 (BL-0249).
            super::FILTER_RULE_PATTERN_INVALID,
        ];
        let mut unique = codes.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            codes.len(),
            unique.len(),
            "doppelt vergebener CommandError-Code: {codes:?}"
        );
    }

    /// Spec 0077, T-6d: Die Umwandlung aus 3.1.3 hängt für ein ungültiges
    /// Muster den stabilen Code an und reicht den Fehlertext der
    /// Bibliothek als `message` durch.
    ///
    /// Scheitert, wenn `RuleWriteError` wieder `Display` bekäme (E0119) oder
    /// die Umwandlung den Code nicht setzt.
    #[test]
    fn test_spec_0077_t6d_invalid_pattern_keeps_its_code_and_library_message() {
        let pattern_err =
            ssh_manager_core::filter::Pattern::Regex("^systemctl stop (.*".to_string())
                .validate()
                .unwrap_err();
        let expected_message = pattern_err.to_string();

        let err = crate::error::CommandError::from(
            crate::filter_rules::RuleWriteError::InvalidPattern(pattern_err),
        );

        assert_eq!(err.code, Some(super::FILTER_RULE_PATTERN_INVALID));
        assert_eq!(err.message, expected_message);
        assert!(
            err.message.contains("unclosed group"),
            "die Stelle im Muster gehört in die Meldung: {}",
            err.message
        );
    }

    /// Issue #260: Ein bloßes `?` über einen `RuleWriteError` liefert einen
    /// `CommandError` mit Code — auch wenn das Muster erst im Speicher
    /// (Schicht 2) abgewiesen wurde.
    ///
    /// *Gegenbeweis:* Vor #260 lief `?` über den blanket `From<E: Display>`
    /// und ergab `code: None`.
    #[test]
    fn test_issue_260_question_mark_keeps_the_code() {
        fn store_layer_rejects() -> Result<(), crate::filter_rules::RuleWriteError> {
            let pattern_err = ssh_manager_core::filter::Pattern::Regex("^a(".to_string())
                .validate()
                .unwrap_err();
            Err(persistence_sqlite::PolicyStoreError::InvalidPattern(
                pattern_err,
            ))?
        }
        fn command() -> crate::error::CommandResult<()> {
            store_layer_rejects()?;
            Ok(())
        }
        let err = command().unwrap_err();
        assert_eq!(err.code, Some(super::FILTER_RULE_PATTERN_INVALID));
    }

    /// Spec 0077, 3.1.3: Ein Speicher-Fehler wird umgewandelt wie bisher —
    /// ohne Code. Belegt, dass die neue Umwandlung nicht pauschal den
    /// Muster-Code an jeden Schreibfehler hängt.
    #[test]
    fn test_spec_0077_store_error_keeps_converting_without_a_code() {
        let err = crate::error::CommandError::from(crate::filter_rules::RuleWriteError::Store(
            persistence_sqlite::PolicyStoreError::NotFound(ssh_manager_core::filter::RuleId(
                "irgendeine-regel".to_string(),
            )),
        ));

        assert_eq!(err.code, None);
    }
}

#[cfg(test)]
mod secret_store_code_tests {
    use super::*;
    use ssh_manager_core::profiles::CredentialRef;

    /// Spec 0101, T10 / Spec 0071 A13: Ein Störungsfehler des
    /// Secret-Speichers erreicht das Frontend mit einem stabilen Code —
    /// nicht mehr als codeloser, englischer Bibliothekstext.
    ///
    /// **Gegen den Stand vor Spec 0101 scheitert das:** dort war der Code
    /// `KEYCHAIN_ACCESS_FAILED` bzw. `KEYCHAIN_UNAVAILABLE`, je nach
    /// Startzustand des Schlüsselbunds.
    #[test]
    fn test_backend_error_gets_the_stable_secret_store_code() {
        let err = secret_store_error(CredentialError::Backend("database is locked".to_string()));
        assert_eq!(err.code, Some(SECRET_STORE_FAILED));
    }

    /// Spec 0071, X2: Der Code **ersetzt** den Text. Ein Platzhalter-Secret
    /// aus der Fehlerkette darf in keiner `CommandError.message` mit diesem
    /// Code auftauchen — sonst hätte die Redaktionszusage genau hier ein
    /// Loch, an einer Stelle, die direkt ins UI geht.
    #[test]
    fn test_backend_error_text_never_reaches_the_frontend_with_this_code() {
        let err = secret_store_error(CredentialError::Backend(
            "token sk-live-hunter2 rejected by the store".to_string(),
        ));
        assert_eq!(err.code, Some(SECRET_STORE_FAILED));
        assert!(
            !err.message.contains("hunter2") && !err.message.contains("sk-live"),
            "der rohe Backend-Fehlertext darf nicht mitgereicht werden: {}",
            err.message
        );
    }

    /// Die Nutzlast, die laut Spec §7 in jedem Test dieses Laufs steht.
    /// Enthält absichtlich **beide** Teile — den „Bibliothekstext" und ein
    /// „Geheimnis" —, damit ein Test, der nur einen davon sucht, nicht aus
    /// Versehen grün ist.
    const MARKER: &str = "LIBTEXT-0098 Geheim-0098";

    /// Spec 0098, T1/T2 (A1), seit Spec 0101 A9.1 mit dem neuen Code:
    /// Ein Schreib- oder Lesefehler des Secret-Speichers reicht den rohen
    /// Bibliothekstext **nicht** ins Frontend durch, sondern bekommt einen
    /// stabilen Code.
    #[test]
    fn test_spec_0101_a9_1_backend_error_gets_the_secret_store_code() {
        let err = secret_store_error(CredentialError::Backend(MARKER.to_string()));

        assert_eq!(err.code, Some(SECRET_STORE_FAILED));
        assert!(
            !err.message.contains("LIBTEXT-0098") && !err.message.contains("Geheim-0098"),
            "die Nutzlast der Bibliothek darf das Frontend nicht erreichen: {}",
            err.message
        );
    }

    /// Spec 0101, A9.1 / T10: Die Meldung nennt den **Schlüsselbund
    /// nicht**. Seit A9 liegen die Secrets in der Datenbank; „Schlüsselbund
    /// gesperrt" schickte den Nutzer in die Systemeinstellungen, wo es
    /// nichts zu reparieren gibt. Ebensowenig behauptet sie „nicht
    /// verfügbar" oder rät zu einer Paketinstallation.
    ///
    /// **Gegen den Stand vor Spec 0101 scheitert das:** dort lautete die
    /// Meldung wörtlich „Der Zugriff auf den Systemschlüsselbund ist
    /// fehlgeschlagen …".
    #[test]
    fn test_spec_0101_a9_1_message_never_mentions_the_os_keychain() {
        let err = secret_store_error(CredentialError::Backend(MARKER.to_string()));

        let message = err.message.to_lowercase();
        assert!(
            !message.contains("schlüsselbund") && !message.contains("keychain"),
            "A9.1: kein „Schlüsselbund\" im Text: {}",
            err.message
        );
        assert!(
            !message.contains("nicht verfügbar"),
            "„nicht verfügbar\" ist genau die falsche Auskunft: {}",
            err.message
        );
        assert!(
            !message.contains("apt ") && !message.contains("install"),
            "kein Paket- oder Installationshinweis: {}",
            err.message
        );
    }

    /// Spec 0101, A9.1: Es gibt **keine** zweite Fassung dieses Codes mehr.
    /// Vor dieser Spec entschied der Startzustand des Schlüsselbunds
    /// zwischen `KEYCHAIN_ACCESS_FAILED` und `KEYCHAIN_UNAVAILABLE`; jetzt
    /// gibt es nur noch einen Ausgang, und zwar immer denselben.
    ///
    /// Dass der Startzustand nicht mehr einfließen **kann**, hängt an der
    /// Signatur: [`secret_store_error`] bekommt ihn gar nicht mehr. Der
    /// Test hält fest, was daraus folgt — ein Fehler, beliebig oft, immer
    /// derselbe Code, nie ein `KEYCHAIN_*`.
    #[test]
    fn test_spec_0101_a9_1_there_is_no_second_code_for_this_path() {
        for attempt in 1..=3 {
            let err = secret_store_error(CredentialError::Backend(MARKER.to_string()));

            assert_eq!(
                err.code,
                Some(SECRET_STORE_FAILED),
                "Versuch {attempt}: immer derselbe Code"
            );
            assert_ne!(err.code, Some(KEYCHAIN_UNAVAILABLE));
            assert_ne!(err.code, Some(KEYCHAIN_ACCESS_FAILED));
        }
    }

    /// Spec 0098, T10 (adversarial): Die Code-Wahl hängt an der
    /// **Variante**, nie am Text der Nutzlast. Eine Nutzlast, die selbst
    /// wie ein anderer Code aussieht, ändert daran nichts — und landet
    /// insbesondere nicht in der `message`, wo sie wie eine Aussage der App
    /// über ihren eigenen Zustand gelesen würde.
    #[test]
    fn test_spec_0098_t10_payload_that_looks_like_a_code_does_not_pick_the_code() {
        let err = secret_store_error(CredentialError::Backend(KEYCHAIN_UNAVAILABLE.to_string()));

        assert_eq!(
            err.code,
            Some(SECRET_STORE_FAILED),
            "die Variante entscheidet, nicht die Nutzlast"
        );
        assert!(
            !err.message.contains(KEYCHAIN_UNAVAILABLE),
            "die Nutzlast darf auch dann nicht durchkommen, wenn sie wie ein Code aussieht: {}",
            err.message
        );
    }

    /// Spec 0098, T9 (adversarial): Eine Nutzlast mit Zeilenumbruch und
    /// Anführungszeichen — die Form, in der ein Bibliothekstext ein
    /// mitgeliefertes Geheimnis wie ein eigenes Feld aussehen lässt — kommt
    /// in **keinem** Feld ans Frontend.
    #[test]
    fn test_spec_0098_t9_multiline_payload_reaches_no_field() {
        let payload = "x\nPasswort: Geheim-0098\"";

        let err = secret_store_error(CredentialError::Backend(payload.to_string()));

        // Nicht nur `message`: Das ganze serialisierte `CommandError`,
        // also exakt das, was über die IPC-Grenze geht.
        let serialised = serde_json::to_string(&err).expect("CommandError ist serialisierbar");
        assert!(
            !serialised.contains("Geheim-0098"),
            "die Nutzlast darf in keinem Feld des DTO stehen: {serialised}"
        );
    }

    /// Spec 0071, A14/I4: „nicht vorhanden" ist keine Störung des
    /// Speichers und bekommt deshalb **keinen** Störungscode.
    #[test]
    fn test_not_found_is_never_reported_as_a_store_failure() {
        let err = secret_store_error(CredentialError::NotFound(CredentialRef::new(
            "server:1:sudo_password".to_string(),
        )));
        assert_eq!(err.code, None);
    }

    /// Spec 0101, A9.1, zweiter Weg (§1): derselbe Code aus der
    /// Verbindungskette. [`ssh_command_error`] deutet ihn **nicht** mehr
    /// um — vor dieser Spec wurde daraus bei nicht verfügbarem
    /// Schlüsselbund `KEYCHAIN_UNAVAILABLE`.
    #[test]
    fn test_spec_0101_a9_1_the_connection_path_reports_the_same_code() {
        use ssh_manager_core::ssh::SecretKind;

        let err = ssh_command_error(&SshError::CredentialStoreFailed {
            secret: SecretKind::Password,
            hop: None,
        });

        assert_eq!(err.code, Some(SECRET_STORE_FAILED));
        let message = err.message.to_lowercase();
        assert!(
            !message.contains("schlüsselbund") && !message.contains("keychain"),
            "A9.1: auch auf diesem Weg kein „Schlüsselbund\" im Text: {}",
            err.message
        );
    }

    /// Spec 0098, A4 letzter Satz / Spec 0071 I4: Ein fehlender Eintrag
    /// bleibt auf dem Verbindungsweg `SSH_CREDENTIAL_RESOLUTION_FAILED` —
    /// die Umstellung aus A9.1 färbt nicht auf andere Varianten ab.
    #[test]
    fn test_spec_0101_a9_1_other_ssh_errors_keep_their_code() {
        let err = ssh_command_error(&SshError::CredentialResolutionFailed(
            "Passwort: kein Credential".to_string(),
        ));
        assert_eq!(err.code, Some("SSH_CREDENTIAL_RESOLUTION_FAILED"));
    }
}

#[cfg(test)]
mod feature_locked_tests {
    use super::*;
    use ssh_manager_core::entitlements::{Feature, Tier};

    /// Spec 0037, Abschnitt 2: `FeatureLocked` muss strukturiert (nicht nur
    /// als generischer `message`-String über den blanket `From<E: Display>`)
    /// im `CommandError` landen, damit das Frontend ihn eindeutig von
    /// fachlichen Fehlern unterscheiden kann.
    #[test]
    fn test_command_error_feature_locked_carries_structured_feature_and_tier() {
        let err = ssh_manager_core::entitlements::FeatureLocked {
            feature: Feature::DocumentExport,
            tier: Tier::Free,
        };

        let command_error = CommandError::feature_locked(err);

        assert_eq!(command_error.code, Some("FEATURE_LOCKED"));
        let feature_locked = command_error
            .feature_locked
            .expect("feature_locked muss gesetzt sein");
        assert_eq!(feature_locked.feature, Feature::DocumentExport);
        assert_eq!(feature_locked.tier, Tier::Free);
    }

    /// Gegentest: ein gewöhnlicher Fehler (über den blanket `From<E:
    /// Display>`) darf `feature_locked` nicht setzen — sonst könnte das
    /// Frontend jeden Fehler fälschlich als gesperrtes Feature behandeln.
    #[test]
    fn test_ordinary_error_leaves_feature_locked_none() {
        let command_error: CommandError = "irgendein fachlicher Fehler".into();

        assert!(command_error.feature_locked.is_none());
        assert_eq!(command_error.code, None);
    }
}

#[cfg(test)]
mod connect_log_tests {
    //! Issue #51: das Schritt-Protokoll reist nur am Verbindungsfehler mit.
    use super::*;
    use ssh_manager_core::ssh::{ConnectStep, ConnectStepRecord, StepStatus};

    #[test]
    fn test_issue_51_connect_log_is_serialized_only_when_attached() {
        let plain = serde_json::to_value(CommandError::with_code("x", "SSH_TIMEOUT")).unwrap();
        assert!(plain.get("connect_log").is_none(), "{plain}");

        let step = ConnectStepRecord {
            hop_index: 0,
            hop: "deploy@example.invalid:22".into(),
            step: ConnectStep::TcpConnect {
                address: None,
                port: 22,
            },
            status: StepStatus::Failed {
                code: "SSH_TIMEOUT".into(),
            },
            duration_ms: Some(10_000),
        };
        let with_log = serde_json::to_value(
            CommandError::with_code("x", "SSH_TIMEOUT").with_connect_log(vec![step]),
        )
        .unwrap();
        assert_eq!(with_log["code"], "SSH_TIMEOUT");
        assert_eq!(with_log["connect_log"][0]["step"]["kind"], "tcpConnect");
        assert_eq!(with_log["connect_log"][0]["status"]["code"], "SSH_TIMEOUT");
    }
}
