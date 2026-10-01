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

use credentials_keyring::KeychainAvailability;
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

/// Spec 0098, A1/A2/A7: der eine Ort, an dem ein
/// [`CredentialError::Backend`] zu Code und Meldung wird.
///
/// **Die Nutzlast bleibt hier liegen** (A5, Ausweitung von Spec 0071 X2).
/// Sie geht nur noch in eine `debug`-Zeile (A7) — Spec 0094 nimmt
/// Schlüsselbund-Fehlerwerte ausdrücklich aus (§3, Nicht-Ziele), und die
/// bestehenden `warn`-Zeilen der Aufrufer bleiben unverändert. `debug` ist
/// ohne `RUST_LOG` aus (Spec 0094, A3).
///
/// **Die Code-Wahl hängt an Variante und Startzustand, nie am Text** (A1,
/// Angriffsrichtung T10): Eine Nutzlast, die selbst wie ein Code aussieht,
/// fließt nicht in diese Entscheidung ein — sie wird gar nicht gelesen.
fn backend_failure_to_command_error(payload: &str, keychain: KeychainAvailability) -> CommandError {
    // Durch den Redactor, wie die Zeile in `core`s `credential_lookup_error`
    // — Begründung dort (spec-reviewer Runde 1): A5 erklärt diese Nutzlast
    // für unzustellbar, und ein Log ist eine Datensenke.
    tracing::debug!(
        keychain_available = keychain.is_available(),
        error = %{
            use ssh_manager_core::ai::OutputRedactor;
            ssh_manager_core::ai::default_log_redactor().redact_text(payload)
        },
        "credential store backend failure (Spec 0098, A7)"
    );
    if keychain.is_available() {
        // A6: kein „nicht verfügbar", kein Paket- oder Installationshinweis
        // — der Schlüsselbund ist da, dieser eine Zugriff ging schief. Den
        // ausführlichen Text liefert das Frontend über den Code; diese
        // Meldung ist der Fallback für einen Aufrufer, der ihn nicht kennt.
        CommandError::with_code(
            "Der Zugriff auf den Systemschlüsselbund ist fehlgeschlagen. \
             Möglicherweise ist er gesperrt oder der Zugriff wurde abgelehnt.",
            KEYCHAIN_ACCESS_FAILED,
        )
    } else {
        CommandError::with_code(
            "Der Systemschlüsselbund ist nicht verfügbar.",
            KEYCHAIN_UNAVAILABLE,
        )
    }
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
/// **Warum eine ausdrückliche Funktion und kein `From`-Impl:** Der blanket
/// `impl<E: Display> From<E> for CommandError` weiter unten deckt
/// [`RuleWriteError`] bereits ab (er implementiert `Display` über
/// `thiserror`); ein zweiter, spezifischerer `From`-Impl wäre eine von
/// Rusts Kohärenzregeln verbotene überlappende Impl (E0119). Ein blosses
/// `?` oder `.map_err(Into::into)` liefe deshalb **still** über den
/// blanket-Impl und setzte `code: None` — der Code ginge verloren, ohne
/// dass irgendwo etwas scheitert. Dasselbe Muster wie
/// [`keychain_aware_credential_error`] und
/// [`CommandError::feature_locked`].
///
/// Als `message` steht der Fehlertext der Bibliothek (mit der Stelle im
/// Muster). Das Frontend ersetzt bei bekanntem Code zwar den Text, zeigt
/// die `message` für diesen Code aber zusätzlich darunter an (3.1.4) —
/// genau dafür wird sie hier mitgegeben.
pub fn rule_write_error(err: crate::filter_rules::RuleWriteError) -> CommandError {
    match err {
        crate::filter_rules::RuleWriteError::InvalidPattern(pattern_err) => {
            CommandError::with_code(pattern_err.to_string(), FILTER_RULE_PATTERN_INVALID)
        }
        crate::filter_rules::RuleWriteError::Store(store_err) => CommandError::from(store_err),
    }
}

/// Spec 0071 A13/X2, erweitert durch Spec 0098 A1/A2: Wandelt einen
/// [`CredentialError`] in einen [`CommandError`] und hängt **jedem**
/// [`CredentialError::Backend`] einen stabilen Code an —
/// [`KEYCHAIN_UNAVAILABLE`], wenn der Schlüsselbund bei diesem
/// Programmstart schon als nicht verfügbar erkannt wurde (A16 — `keychain`
/// kommt aus dem `AppState`, es wird hier **nicht** erneut probiert),
/// sonst [`KEYCHAIN_ACCESS_FAILED`].
///
/// **Was Spec 0098 ändert:** Vorher fiel der Fall „Schlüsselbund verfügbar,
/// Zugriff trotzdem gescheitert" durch auf den pauschalen
/// `From<E: Display>` und lieferte `code: None` mit „Credential-Backend-
/// Fehler: <Text der Bibliothek>". Genau dieser Weg ist der häufige: ein
/// nach dem Start gesperrter Schlüsselbund, ein abgelehnter Dialog.
///
/// **X2, ausgeweitet (A5) — der Code ersetzt den Text, er ergänzt ihn
/// nicht:** Die Nutzlast von [`CredentialError::Backend`] erreicht das
/// Frontend auf **keinem** Zustand mehr, auch nicht im Feld `message`.
/// Enthielte ein Backend-Fehler jemals einen Secret-artigen Wert, käme er
/// über diesen Pfad nicht ins Frontend. Zur Diagnose bleibt er auf `debug`
/// (A7, s. [`backend_failure_to_command_error`]).
///
/// **Der Zustand bleibt unberührt (A3):** `keychain` kommt als Wert herein
/// und wird nur gelesen. Ein einzelner fehlgeschlagener Zugriff eskaliert
/// nichts — der nächste Zugriff gelingt, sobald der Store wieder antwortet.
///
/// [`CredentialError::NotFound`] bleibt bewusst unverändert: "kein Eintrag
/// vorhanden" ist eine fachliche Aussage und hat mit der Verfügbarkeit des
/// Schlüsselbunds nichts zu tun — sie mit einem der beiden Codes zu
/// überschreiben wäre genau die Verwechslung, die A14/I4 verbietet.
pub fn keychain_aware_credential_error(
    err: CredentialError,
    keychain: KeychainAvailability,
) -> CommandError {
    match err {
        CredentialError::Backend(payload) => backend_failure_to_command_error(&payload, keychain),
        // **Ausdrücklich `NotFound`, kein Catch-all** (spec-reviewer Runde 1):
        // `CommandError::from` setzt `code: None` und nimmt den `Display`-Text
        // als Meldung. Für eine dritte `CredentialError`-Variante wäre das
        // genau der Zustand, den diese Spec abschafft — roher Text, kein
        // stabiler Code. So scheitert stattdessen die Übersetzung.
        not_found @ CredentialError::NotFound(_) => CommandError::from(not_found),
    }
}

/// Spec 0098, A4: dieselbe Entscheidung wie in
/// [`keychain_aware_credential_error`], nur für einen Fehler, der die
/// Verbindungskette durchlaufen hat.
///
/// `core` kennt den `AppState` nicht (§5) und vergibt über
/// [`SshError::code`] deshalb [`KEYCHAIN_ACCESS_FAILED`] — die Fassung, die
/// **nie** fälschlich „nicht verfügbar" behauptet. Hier, wo der Startzustand
/// bekannt ist, wird daraus [`KEYCHAIN_UNAVAILABLE`], wenn der Schlüsselbund
/// schon beim Start fehlte (A2).
///
/// **Nur [`SshError::CredentialStoreFailed`] wird umgedeutet.** Jede andere
/// Variante behält ihren Code unverändert — insbesondere
/// [`SshError::CredentialResolutionFailed`], unter der ein fehlender Eintrag
/// (`NotFound`) weiterhin als `SSH_CREDENTIAL_RESOLUTION_FAILED` ankommt (A4
/// letzter Satz, Spec 0071 I4).
pub fn keychain_aware_ssh_error_code(
    err: &SshError,
    keychain: KeychainAvailability,
) -> &'static str {
    match err {
        SshError::CredentialStoreFailed { .. } if !keychain.is_available() => KEYCHAIN_UNAVAILABLE,
        other => other.code(),
    }
}

/// Spec 0098, A4/A5: ein [`SshError`] als [`CommandError`], mit dem
/// schlüsselbund-bewussten Code aus [`keychain_aware_ssh_error_code`].
///
/// Die `message` bleibt `SshError`s `Display`. Für
/// [`SshError::CredentialStoreFailed`] ist das per Konstruktion fester Text
/// plus Secret-Art plus Hop-Angabe (Spec 0076, A-8) — die Nutzlast der
/// Bibliothek hat dort kein Feld (A5). Für jede andere Variante ändert sich
/// gegenüber `CommandError::with_code(err.to_string(), err.code())` nichts.
pub fn keychain_aware_ssh_error(err: &SshError, keychain: KeychainAvailability) -> CommandError {
    CommandError::with_code(
        err.to_string(),
        keychain_aware_ssh_error_code(err, keychain),
    )
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
}

impl CommandError {
    pub fn with_code(message: impl Into<String>, code: &'static str) -> Self {
        Self {
            message: message.into(),
            code: Some(code),
            feature_locked: None,
        }
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
        }
    }
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(err: E) -> Self {
        Self {
            message: err.to_string(),
            code: None,
            feature_locked: None,
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
            // Spec 0098, A1/A6 (BL-0244/BL-0205/BL-0206). Kommt sowohl aus
            // `keychain_aware_credential_error` als auch aus
            // `ssh_manager_core::ssh::SshError::code` — derselbe Wert,
            // deshalb nur ein Eintrag.
            super::KEYCHAIN_ACCESS_FAILED,
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
    /// Scheitert, wenn jemand die ausdrückliche Umwandlung durch `?` oder
    /// `.map_err(Into::into)` ersetzt: Dann liefe der Fehler über den
    /// pauschalen `From<E: Display>`, und `code` wäre still `None`.
    #[test]
    fn test_spec_0077_t6d_invalid_pattern_keeps_its_code_and_library_message() {
        let pattern_err =
            ssh_manager_core::filter::Pattern::Regex("^systemctl stop (.*".to_string())
                .validate()
                .unwrap_err();
        let expected_message = pattern_err.to_string();

        let err = super::rule_write_error(crate::filter_rules::RuleWriteError::InvalidPattern(
            pattern_err,
        ));

        assert_eq!(err.code, Some(super::FILTER_RULE_PATTERN_INVALID));
        assert_eq!(err.message, expected_message);
        assert!(
            err.message.contains("unclosed group"),
            "die Stelle im Muster gehört in die Meldung: {}",
            err.message
        );
    }

    /// Spec 0077, 3.1.3: Ein Speicher-Fehler wird umgewandelt wie bisher —
    /// ohne Code. Belegt, dass die neue Umwandlung nicht pauschal den
    /// Muster-Code an jeden Schreibfehler hängt.
    #[test]
    fn test_spec_0077_store_error_keeps_converting_without_a_code() {
        let err = super::rule_write_error(crate::filter_rules::RuleWriteError::Store(
            persistence_sqlite::PolicyStoreError::NotFound(ssh_manager_core::filter::RuleId(
                "irgendeine-regel".to_string(),
            )),
        ));

        assert_eq!(err.code, None);
    }
}

#[cfg(test)]
mod keychain_code_tests {
    use super::*;
    use credentials_keyring::KeychainUnavailableReason;
    use ssh_manager_core::profiles::CredentialRef;

    const UNAVAILABLE: KeychainAvailability =
        KeychainAvailability::Unavailable(KeychainUnavailableReason::NoSecretServiceProvider);

    /// Spec 0071, A13: Ist der Schlüsselbund nicht verfügbar, erreicht der
    /// Fehler das Frontend mit dem stabilen Code — nicht mehr als
    /// codeloser, englischer Bibliothekstext.
    #[test]
    fn test_backend_error_gets_the_stable_code_when_the_keychain_is_unavailable() {
        let err = keychain_aware_credential_error(
            CredentialError::Backend(
                "No default store has been set, so cannot search or create entries".to_string(),
            ),
            UNAVAILABLE,
        );
        assert_eq!(err.code, Some(KEYCHAIN_UNAVAILABLE));
    }

    /// Spec 0071, X2: Der Code **ersetzt** den Text. Ein Platzhalter-Secret
    /// aus der Fehlerkette darf in keiner `CommandError.message` mit diesem
    /// Code auftauchen — sonst hätte die Redaktionszusage genau hier ein
    /// Loch, an einer Stelle, die direkt ins UI geht.
    #[test]
    fn test_backend_error_text_never_reaches_the_frontend_with_this_code() {
        let err = keychain_aware_credential_error(
            CredentialError::Backend("token sk-live-hunter2 rejected by keyring".to_string()),
            UNAVAILABLE,
        );
        assert_eq!(err.code, Some(KEYCHAIN_UNAVAILABLE));
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

    /// Spec 0098, T1/T2 (A1) — **ersetzt**
    /// `test_backend_error_keeps_its_message_when_the_keychain_is_available`,
    /// der genau das Gegenteil verlangte (`code == None`, Bibliothekstext in
    /// `message`). Das war das Verhalten aus BL-0244: Ein Schreib- oder
    /// Lesefehler bei verfügbarem Schlüsselbund reichte den rohen englischen
    /// `keyring`-Text ins Frontend durch.
    ///
    /// Scheitert am Stand vor dieser Spec in **beiden** Zusicherungen.
    #[test]
    fn test_spec_0098_t1_backend_error_gets_the_access_code_when_the_keychain_is_available() {
        let err = keychain_aware_credential_error(
            CredentialError::Backend(MARKER.to_string()),
            KeychainAvailability::Available,
        );

        assert_eq!(err.code, Some(KEYCHAIN_ACCESS_FAILED));
        assert!(
            !err.message.contains("LIBTEXT-0098") && !err.message.contains("Geheim-0098"),
            "die Nutzlast der Bibliothek darf das Frontend nicht erreichen: {}",
            err.message
        );
    }

    /// Spec 0098, A6: Die Meldung dieses Codes behauptet **nicht** „nicht
    /// verfügbar" und schickt niemanden zu einer Paketinstallation — der
    /// Schlüsselbund ist ja da. Ein Test gegen den Wortlaut wäre brüchig;
    /// geprüft wird nur, was nicht drinstehen darf.
    #[test]
    fn test_spec_0098_a6_access_failure_does_not_claim_an_unavailable_keychain() {
        let err = keychain_aware_credential_error(
            CredentialError::Backend(MARKER.to_string()),
            KeychainAvailability::Available,
        );

        let message = err.message.to_lowercase();
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

    /// Spec 0098, T3 (A2): Der Fall „schon beim Start nicht verfügbar"
    /// bleibt **unverändert** bei `KEYCHAIN_UNAVAILABLE`. Gegenprobe zu T1 —
    /// die neue Code-Vergabe darf den alten Fall nicht überschreiben, sonst
    /// verlöre der Nutzer den Hinweis auf die Diagnose-Ansicht.
    #[test]
    fn test_spec_0098_t3_unavailable_keychain_still_reports_unavailable() {
        let err = keychain_aware_credential_error(
            CredentialError::Backend(MARKER.to_string()),
            UNAVAILABLE,
        );

        assert_eq!(err.code, Some(KEYCHAIN_UNAVAILABLE));
        assert!(
            !err.message.contains("LIBTEXT-0098") && !err.message.contains("Geheim-0098"),
            "auch hier bleibt die Nutzlast draußen: {}",
            err.message
        );
    }

    /// Spec 0098, T10 (adversarial): Die Code-Wahl hängt an **Variante und
    /// Startzustand**, nie am Text der Nutzlast. Eine Nutzlast, die selbst
    /// wie der andere Code aussieht, ändert daran nichts — und landet
    /// insbesondere nicht in der `message`, wo sie wie eine Aussage der App
    /// über ihren eigenen Zustand gelesen würde.
    #[test]
    fn test_spec_0098_t10_payload_that_looks_like_a_code_does_not_pick_the_code() {
        let err = keychain_aware_credential_error(
            CredentialError::Backend(KEYCHAIN_UNAVAILABLE.to_string()),
            KeychainAvailability::Available,
        );

        assert_eq!(
            err.code,
            Some(KEYCHAIN_ACCESS_FAILED),
            "der Startzustand entscheidet, nicht die Nutzlast"
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

        for keychain in [KeychainAvailability::Available, UNAVAILABLE] {
            let err = keychain_aware_credential_error(
                CredentialError::Backend(payload.to_string()),
                keychain,
            );

            // Nicht nur `message`: Das ganze serialisierte `CommandError`,
            // also exakt das, was über die IPC-Grenze geht.
            let serialised = serde_json::to_string(&err).expect("CommandError ist serialisierbar");
            assert!(
                !serialised.contains("Geheim-0098"),
                "die Nutzlast darf in keinem Feld des DTO stehen: {serialised}"
            );
        }
    }

    /// Spec 0098, T4 (A3), Hälfte „keine falsche Behauptung": Ein einzelner
    /// fehlgeschlagener Zugriff meldet nicht „nicht verfügbar" — und zwar
    /// beliebig oft hintereinander, es gibt keinen Zähler, der irgendwann
    /// umkippt.
    ///
    /// Die zweite Hälfte („der nächste Zugriff gelingt wirklich") braucht
    /// einen Store, der nur einmal scheitert, und steht deshalb in
    /// `server_credentials.rs`
    /// (`test_spec_0098_t4_a_single_rejection_does_not_make_the_keychain_unavailable`).
    ///
    /// **Dass der Zustand unberührt bleibt, ist hier nicht prüfbar** und
    /// stand früher als `assert!(keychain.is_available())` da — eine
    /// Zusicherung, die nicht scheitern kann, weil `keychain` eine lokale
    /// `Copy`-Variable ist (spec-reviewer Runde 1). Die Garantie hängt an der
    /// Signatur: Die Funktion bekommt den Zustand als Wert und hat keinen Weg
    /// zum `AppState`.
    #[test]
    fn test_spec_0098_t4_a_single_failure_never_claims_unavailability() {
        let keychain = KeychainAvailability::Available;

        for attempt in 1..=3 {
            let err = keychain_aware_credential_error(
                CredentialError::Backend(MARKER.to_string()),
                keychain,
            );

            assert_ne!(
                err.code,
                Some(KEYCHAIN_UNAVAILABLE),
                "Versuch {attempt}: ein Laufzeitfehler darf nicht „nicht verfügbar\" behaupten"
            );
            assert_eq!(
                err.code,
                Some(KEYCHAIN_ACCESS_FAILED),
                "Versuch {attempt}: und er bleibt bei demselben Code"
            );
        }
    }

    /// Spec 0071, A14/I4: "nicht vorhanden" ist keine Schlüsselbund-Störung.
    #[test]
    fn test_not_found_is_never_reported_as_an_unavailable_keychain() {
        let err = keychain_aware_credential_error(
            CredentialError::NotFound(CredentialRef::new("server:1:sudo_password".to_string())),
            UNAVAILABLE,
        );
        assert_eq!(err.code, None);
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
