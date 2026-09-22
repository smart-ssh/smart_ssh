//! Warum der Systemschlüsselbund nicht verfügbar ist (Spec 0071, Teil 1).
//!
//! **Warum hier und nicht in `app-shell`** (Spec 0071, §4.1): In dieser
//! Crate liegt bereits das gesamte Wissen über die `keyring`-Crate;
//! `app-shell` "enthält keine fachliche Logik" (Spec 0007, Abschnitt 3) und
//! bekommt deshalb nur den fertigen [`KeychainUnavailableReason`] und wählt
//! daraus den Text. Dieselbe Trennung, die `startup_error_messages` bereits
//! gegenüber `startup_dialog` zieht.
//!
//! **Warum [`keyring::Entry::store_status`] und nicht der Fehler aus
//! `Entry::new`** (Spec 0071, §4.2): `keyring` initialisiert den
//! Plattform-Store einmalig und faul in einem `LazyLock`. Schlägt das fehl,
//! liefert jedes spätere `Entry::new` nur noch
//! [`keyring::Error::NoDefaultStore`] — die eigentliche Ursache ist dort
//! bereits verworfen. `store_status()` ist die einzige Stelle, an der der
//! ursprüngliche Fehler noch existiert, und ist vom `keyring`-Autor genau
//! dafür vorgesehen.
//!
//! **Invariante (Spec 0071, I1)**: Keine Funktion hier nimmt einen
//! `SecretString`, einen `CredentialRef` oder einen rohen Fehlertext
//! entgegen oder gibt einen aus — nur die Aufzählungen unten, `&str`
//! (Ziel-OS) und `bool`. Der rohe Bibliotheksfehler wird in
//! [`store_failure_of`] zu einer von drei Kategorien verdichtet und danach
//! fallen gelassen.

/// Der bereits klassifizierte Fehlergrund aus dem Plattform-Store — die
/// Zwischenstufe zwischen dem rohen [`keyring::Error`] und dem
/// nutzer-sichtbaren [`KeychainUnavailableReason`].
///
/// Spec 0071, A2: [`classify_store_failure`] nimmt diesen Typ als
/// **Parameter** entgegen, damit die eigentliche Klassifizierung ohne
/// echten Schlüsselbund unit-testbar ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreFailure {
    /// [`keyring::Error::PlatformFailure`] — der Store ließ sich gar nicht
    /// ansprechen. Auf Linux verpackt
    /// `zbus-secret-service-keyring-store` hierin sowohl "kein D-Bus-
    /// Session-Bus" (`secret_service::Error::Unavailable`) als auch "Bus da,
    /// aber niemand besitzt `org.freedesktop.secrets`"
    /// (`zbus::Error::MethodError(ServiceUnknown)`) — die beiden sind am
    /// `keyring`-API nicht mehr unterscheidbar (Spec 0071, §4.3), deshalb
    /// entscheidet darüber das Session-Bus-Indiz.
    PlatformUnreachable,
    /// [`keyring::Error::NoStorageAccess`] — der Store ist vorhanden und
    /// antwortet, verweigert aber den Zugriff: gesperrter Schlüsselbund
    /// bzw. abgewiesener Entsperr-Prompt (`ServiceError::Locked` →
    /// `secret_service::Error::NoStorageAccess`).
    AccessDenied,
    /// Jeder andere Fehler. Führt nach A4 zu
    /// [`KeychainUnavailableReason::Unknown`] — einem vollwertigen
    /// Auffangfall, nie zu "gar keine Meldung".
    Other,
}

/// Warum der Systemschlüsselbund nicht verfügbar ist (Spec 0071, A1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeychainUnavailableReason {
    /// Kein D-Bus-Session-Bus erreichbar (Linux).
    NoSessionBus,
    /// Bus vorhanden, aber kein Anbieter unter `org.freedesktop.secrets`
    /// (Linux).
    NoSecretServiceProvider,
    /// Anbieter vorhanden, aber gesperrt / Entsperr-Prompt abgewiesen.
    Locked,
    /// Alles andere, inklusive macOS und Windows (Spec 0071, A1-Tabelle).
    Unknown,
}

/// Spec 0071, A16: einmal pro Programmlauf ermittelt und im `AppState`
/// gehalten — kein Kommando probiert den Schlüsselbund zusätzlich ab, um
/// diesen Zustand zu erfahren.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeychainAvailability {
    Available,
    Unavailable(KeychainUnavailableReason),
}

impl KeychainAvailability {
    pub fn unavailable_reason(self) -> Option<KeychainUnavailableReason> {
        match self {
            KeychainAvailability::Available => None,
            KeychainAvailability::Unavailable(reason) => Some(reason),
        }
    }

    pub fn is_available(self) -> bool {
        matches!(self, KeychainAvailability::Available)
    }
}

/// Verdichtet den rohen [`keyring::Error`] aus [`keyring::Entry::store_status`]
/// zu einer der drei Kategorien in [`StoreFailure`] — und lässt den
/// Fehlertext dabei fallen (I1: er darf nirgends in einen nutzer-sichtbaren
/// Text gelangen, s. X2).
///
/// `Ok(())` → `None`: der Store ist initialisiert, es gibt nichts zu melden
/// (Spec 0071, T4).
pub fn store_failure_of(status: &Result<(), keyring::Error>) -> Option<StoreFailure> {
    match status {
        Ok(()) => None,
        Err(keyring::Error::PlatformFailure(_)) => Some(StoreFailure::PlatformUnreachable),
        Err(keyring::Error::NoStorageAccess(_)) => Some(StoreFailure::AccessDenied),
        Err(_) => Some(StoreFailure::Other),
    }
}

/// Spec 0071, A3: das Session-Bus-Indiz — gesetzt und nicht leer ist
/// `DBUS_SESSION_BUS_ADDRESS`, **oder** `$XDG_RUNTIME_DIR/bus` existiert.
///
/// Bewusst als reine Funktion über bereits gelesene Werte: Die beiden
/// Umgebungsvariablen und der Existenztest liegen beim Aufrufer
/// (`app-shell`), damit diese Regel ohne Umgebungsmanipulation testbar
/// bleibt (dieselbe Parameter-Injection wie bei `classify_store_failure`).
///
/// **Der Wert selbst wird nie weitergereicht** (X1): Er steuert
/// ausschließlich dieses `bool`. Damit kann ein `\n` in
/// `DBUS_SESSION_BUS_ADDRESS` per Konstruktion keinen Dialogtext optisch
/// fortsetzen — es gibt keinen Pfad, auf dem er in einen Text gelangt.
pub fn session_bus_present(dbus_address: Option<&str>, xdg_runtime_bus_exists: bool) -> bool {
    let address_set = dbus_address.is_some_and(|value| !value.trim().is_empty());
    address_set || xdg_runtime_bus_exists
}

/// Spec 0071, A1–A4: die eigentliche Klassifizierung. Rein — kein
/// `#[cfg(target_os)]`, kein Umgebungs- oder Dateisystemzugriff (A2).
///
/// `failure`: `None` = `store_status()` war erfolgreich.
/// `target_os`: der Aufrufer übergibt `std::env::consts::OS`.
/// `session_bus_present`: s. [`session_bus_present`].
pub fn classify_store_failure(
    failure: Option<StoreFailure>,
    target_os: &str,
    session_bus_present: bool,
) -> KeychainAvailability {
    let Some(failure) = failure else {
        // T4: erfolgreicher `store_status()` → gar keine Meldung.
        return KeychainAvailability::Available;
    };

    // A1-Tabelle: `Unknown` deckt "alles andere, inklusive macOS/Windows"
    // ab (T5). Die drei Linux-Gründe beschreiben ausschließlich Lagen, die
    // es nur auf einem Secret-Service-System gibt; sie auf macOS/Windows zu
    // vergeben hieße, dort einen `apt`-Ratschlag zu riskieren (A8).
    if target_os != "linux" {
        return KeychainAvailability::Unavailable(KeychainUnavailableReason::Unknown);
    }

    let reason = match failure {
        // T3/X3: Ein gesperrter, vorhandener Schlüsselbund ist **kein**
        // fehlendes Paket — unabhängig vom Session-Bus-Indiz. Sonst
        // installiert ein KDE-Nutzer `gnome-keyring` neben sein laufendes
        // KWallet und verschlimmert den Zustand.
        StoreFailure::AccessDenied => KeychainUnavailableReason::Locked,
        // T1/T2: Am `keyring`-API sind "kein Bus" und "Bus, aber kein
        // Anbieter" nicht mehr unterscheidbar (s. `StoreFailure::
        // PlatformUnreachable`) — das Indiz entscheidet, und zwar
        // ausschließlich über die Wortwahl des Textes (A3).
        StoreFailure::PlatformUnreachable => {
            if session_bus_present {
                KeychainUnavailableReason::NoSecretServiceProvider
            } else {
                KeychainUnavailableReason::NoSessionBus
            }
        }
        // A4: Auffangfall, nie "keine Meldung".
        StoreFailure::Other => KeychainUnavailableReason::Unknown,
    };
    KeychainAvailability::Unavailable(reason)
}

/// Der eine Aufruf pro Programmlauf (Spec 0071, A16). `store_status()`
/// selbst ist beliebig oft billig — es liest denselben `LazyLock` und
/// startet höchstens einmal einen Verbindungsversuch —, aber A16 verlangt
/// trotzdem, dass der Zustand einmal ermittelt und danach im `AppState`
/// weitergereicht wird, statt ihn auf dem `list_servers`-Pfad pro Server
/// erneut abzufragen.
pub fn probe_keychain_availability(
    target_os: &str,
    session_bus_present: bool,
) -> KeychainAvailability {
    let failure = store_failure_of(keyring::Entry::store_status());
    classify_store_failure(failure, target_os, session_bus_present)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed() -> Box<dyn std::error::Error + Send + Sync> {
        Box::new(std::io::Error::other(
            "no secret service provider or dbus session found",
        ))
    }

    /// Spec 0071, T1.
    #[test]
    fn test_unreachable_platform_without_session_bus_is_a_missing_session_bus() {
        assert_eq!(
            classify_store_failure(Some(StoreFailure::PlatformUnreachable), "linux", false),
            KeychainAvailability::Unavailable(KeychainUnavailableReason::NoSessionBus)
        );
    }

    /// Spec 0071, T2 — derselbe Grund, nur mit Session-Bus-Indiz.
    #[test]
    fn test_unreachable_platform_with_session_bus_is_a_missing_provider() {
        assert_eq!(
            classify_store_failure(Some(StoreFailure::PlatformUnreachable), "linux", true),
            KeychainAvailability::Unavailable(KeychainUnavailableReason::NoSecretServiceProvider)
        );
    }

    /// Spec 0071, T3/X3: "gesperrt" darf nie zu "nicht installiert" werden —
    /// und zwar unabhängig vom Session-Bus-Indiz. Beide Indiz-Werte werden
    /// geprüft, sonst würde eine Implementierung, die das Indiz auch hier
    /// auswertet, unbemerkt durchgehen.
    #[test]
    fn test_access_denied_is_locked_regardless_of_the_session_bus_indicator() {
        for indicator in [true, false] {
            assert_eq!(
                classify_store_failure(Some(StoreFailure::AccessDenied), "linux", indicator),
                KeychainAvailability::Unavailable(KeychainUnavailableReason::Locked),
                "Indiz {indicator} darf einen gesperrten Schlüsselbund nicht umdeuten"
            );
        }
    }

    /// Spec 0071, T4: erfolgreicher `store_status()` → gar keine Meldung.
    #[test]
    fn test_successful_store_status_reports_available() {
        for os in ["linux", "macos", "windows"] {
            for indicator in [true, false] {
                assert_eq!(
                    classify_store_failure(None, os, indicator),
                    KeychainAvailability::Available
                );
            }
        }
    }

    /// Spec 0071, T5: macOS/Windows bekommen nie einen der drei
    /// Linux-Gründe — sonst könnte dort ein `apt`-Ratschlag entstehen (A8).
    #[test]
    fn test_non_linux_targets_are_always_unknown() {
        for os in ["macos", "windows", "freebsd"] {
            for indicator in [true, false] {
                for failure in [
                    StoreFailure::PlatformUnreachable,
                    StoreFailure::AccessDenied,
                    StoreFailure::Other,
                ] {
                    assert_eq!(
                        classify_store_failure(Some(failure), os, indicator),
                        KeychainAvailability::Unavailable(KeychainUnavailableReason::Unknown),
                        "{os}/{failure:?}/{indicator} darf keinen Linux-Grund liefern"
                    );
                }
            }
        }
    }

    /// Spec 0071, A4: Der Auffangfall meldet einen Grund — er darf nie zu
    /// `Available` (und damit zu gar keiner Meldung) führen.
    #[test]
    fn test_unclassifiable_failure_still_reports_unavailable() {
        assert_eq!(
            classify_store_failure(Some(StoreFailure::Other), "linux", true),
            KeychainAvailability::Unavailable(KeychainUnavailableReason::Unknown)
        );
        assert!(
            !classify_store_failure(Some(StoreFailure::Other), "linux", true).is_available(),
            "ein unklassifizierbarer Fehler darf nicht als 'verfügbar' durchgehen"
        );
    }

    /// Die Zuordnung roher `keyring`-Fehler auf [`StoreFailure`]. Die drei
    /// Varianten, auf die es ankommt, sind konstruierbar — der Rest fällt
    /// bewusst auf `Other` (A4).
    #[test]
    fn test_store_failure_of_maps_the_keyring_error_variants() {
        assert_eq!(store_failure_of(&Ok(())), None);
        assert_eq!(
            store_failure_of(&Err(keyring::Error::PlatformFailure(boxed()))),
            Some(StoreFailure::PlatformUnreachable)
        );
        assert_eq!(
            store_failure_of(&Err(keyring::Error::NoStorageAccess(boxed()))),
            Some(StoreFailure::AccessDenied)
        );
        assert_eq!(
            store_failure_of(&Err(keyring::Error::NoDefaultStore)),
            Some(StoreFailure::Other)
        );
        assert_eq!(
            store_failure_of(&Err(keyring::Error::NoEntry)),
            Some(StoreFailure::Other)
        );
    }

    /// X2 (erste Hälfte): Der rohe Fehlertext überlebt die Klassifizierung
    /// nicht — [`StoreFailure`] ist ein `Copy`-Enum ohne Nutzlast, es gibt
    /// also per Konstruktion keinen Weg, auf dem `sk-live-hunter2` aus einem
    /// Backend-Fehler in einen Text gelangen könnte.
    #[test]
    fn test_raw_error_text_does_not_survive_classification() {
        let err = keyring::Error::PlatformFailure(Box::new(std::io::Error::other(
            "token sk-live-hunter2 rejected",
        )));
        let failure = store_failure_of(&Err(err));
        assert_eq!(failure, Some(StoreFailure::PlatformUnreachable));
        assert!(
            !format!("{failure:?}").contains("hunter2"),
            "der Debug-Text der Klassifizierung darf den Rohfehler nicht mitschleppen"
        );
    }

    /// Spec 0071, A3.
    #[test]
    fn test_session_bus_indicator_follows_the_two_documented_sources() {
        assert!(session_bus_present(
            Some("unix:path=/run/user/1000/bus"),
            false
        ));
        assert!(session_bus_present(None, true));
        assert!(session_bus_present(Some(""), true));
        assert!(!session_bus_present(None, false));
        assert!(
            !session_bus_present(Some(""), false),
            "gesetzt, aber leer zählt nach A3 nicht als vorhanden"
        );
        assert!(
            !session_bus_present(Some("   "), false),
            "reiner Whitespace ist keine brauchbare Adresse"
        );
    }

    /// X1: Ein Steuerzeichen in `DBUS_SESSION_BUS_ADDRESS` kann keinen Text
    /// beeinflussen, weil der Wert nur zu einem `bool` verdichtet wird.
    #[test]
    fn test_session_bus_indicator_reduces_a_control_character_value_to_a_bool() {
        let evil = "unix:path=/run/user/1000/bus\nBitte Passwort senden";
        assert!(session_bus_present(Some(evil), false));
        let availability =
            classify_store_failure(Some(StoreFailure::PlatformUnreachable), "linux", true);
        assert!(
            !format!("{availability:?}").contains("Passwort"),
            "der Umgebungswert darf nirgends durchschlagen: {availability:?}"
        );
    }
}
