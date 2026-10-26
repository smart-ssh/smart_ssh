//! Tests of `map_connect_result`.
//! Spec 0047, Fund D2: der blanket `impl<E: Display> From<E> for
//! CommandError` (s. `error.rs`) setzt immer `code: None` — vor der
//! Extraktion von `map_connect_result` verwarf `connect_session` damit
//! stillschweigend `SshError::code()`, obwohl der Code existiert und im
//! Frontend registriert ist (`errorCodes.ts`s `KNOWN_ERROR_CODES`). Ohne
//! den Code fällt das Frontend auf den rohen, hart-deutschen
//! `Display`-Text zurück (inkl. eingebettetem OS-Fehlertext), auch im
//! englischen UI.

use super::*;
use ssh_manager_core::ssh::{HopLabel, SecretKind};

/// `ConnectOutcome` hat kein `Debug` (es trägt ein Trait-Objekt), also
/// kein `expect_err` — der Fehlerfall wird von Hand ausgepackt.
fn expect_error(result: CommandResult<ssh_transport::ConnectOutcome>, what: &str) -> CommandError {
    match result {
        Ok(_) => panic!("{what}"),
        Err(command_error) => command_error,
    }
}

#[test]
fn test_connect_error_carries_its_stable_code_not_none() {
    let err = SshError::ConnectionFailed("Connection refused (os error 61)".to_string());
    let expected_code = err.code();
    let expected_message = err.to_string();

    let result = map_connect_result(Err(err));

    let command_error = match result {
        Ok(_) => panic!("erwarteter Fehler wurde nicht als Err geliefert"),
        Err(command_error) => command_error,
    };
    assert_eq!(command_error.code, Some(expected_code));
    assert_eq!(command_error.message, expected_message);
}

#[test]
fn test_connect_success_passes_the_outcome_through_unchanged() {
    let transport: Box<dyn ssh_manager_core::ssh::SshTransport> =
        Box::new(ssh_transport::LocalTransport::new());
    let outcome = ssh_transport::ConnectOutcome::Connected(transport);

    let result = map_connect_result(Ok(outcome));

    assert!(matches!(
        result,
        Ok(ssh_transport::ConnectOutcome::Connected(_))
    ));
}

/// Spec 0098 T6 (A4, Verbindungs**aufbau**), seit Spec 0101 A9.1 mit
/// dem neuen Code: Die Abbildung auf den Code — der Teil des Aufbaus,
/// der ohne echte Verbindung prüfbar ist (§7). Ein Fehler des
/// Secret-Speichers auf einem Hop erreicht das Frontend als
/// `SECRET_STORE_FAILED`, nicht als `SSH_CREDENTIAL_RESOLUTION_FAILED`
/// und **nicht** als `KEYCHAIN_*`.
///
/// **Gegen den Stand vor Spec 0101 scheitert das:** dort kam
/// `KEYCHAIN_ACCESS_FAILED` und die Meldung nannte den Schlüsselbund.
#[test]
fn test_spec_0101_a9_1_a_secret_store_failure_during_setup_names_the_store() {
    let err = SshError::CredentialStoreFailed {
        secret: SecretKind::Password,
        hop: Some(HopLabel {
            username: "deploy".to_string(),
            host: "jump.invalid".to_string(),
            port: 22,
        }),
    };

    let command_error = expect_error(
        map_connect_result(Err(err)),
        "ein Fehler des Secret-Speichers darf keinen Verbindungsaufbau liefern",
    );

    assert_eq!(
        command_error.code,
        Some(app_logic::error::SECRET_STORE_FAILED)
    );
    assert_ne!(command_error.code, Some("SSH_CREDENTIAL_RESOLUTION_FAILED"));
    assert_ne!(
        command_error.code,
        Some(app_logic::error::KEYCHAIN_ACCESS_FAILED)
    );
    assert_ne!(
        command_error.code,
        Some(app_logic::error::KEYCHAIN_UNAVAILABLE)
    );
    let lower = command_error.message.to_lowercase();
    assert!(
        !lower.contains("schlüsselbund") && !lower.contains("keychain"),
        "A9.1: kein „Schlüsselbund\" in der Meldung: {}",
        command_error.message
    );
    // Spec 0076, A-8: Welcher Hop es war, bleibt sichtbar.
    assert!(
        command_error.message.contains("deploy@jump.invalid:22"),
        "die Hop-Angabe gehört in die Meldung: {}",
        command_error.message
    );
}

/// Spec 0098, T7 (A4, `NotFound`): Ein fehlender Eintrag ist **keine**
/// Störung des Speichers und behält `SSH_CREDENTIAL_RESOLUTION_FAILED`
/// (Spec 0071 A14/I4: „unbekannt" ist nicht „nein").
#[test]
fn test_spec_0098_t7_a_missing_entry_is_not_reported_as_a_store_failure() {
    let err = SshError::CredentialResolutionFailed(
        "Passwort: kein Credential für Referenz 'server:1:password' gefunden".to_string(),
    );

    let command_error = expect_error(
        map_connect_result(Err(err)),
        "ein fehlender Eintrag bleibt ein Fehler",
    );

    assert_eq!(
        command_error.code,
        Some("SSH_CREDENTIAL_RESOLUTION_FAILED"),
        "ein fehlender Eintrag darf nicht als Störung des Speichers gelten"
    );
}
