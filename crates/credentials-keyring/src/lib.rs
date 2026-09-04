//! OS-Keychain-gestützte [`CredentialStore`]-Implementierung (Spec 0003,
//! Abschnitt 4; s. auch `crate::credentials`-Modul-Kommentar in
//! `ssh-manager-core`, der genau diese Crate ankündigt) über die
//! `keyring`-Crate (`v1`-Kompatibilitätsmodus: `Entry::new`/
//! `set_password`/`get_password`/`delete_credential` — plattformunabhängig
//! macOS Keychain Services, Windows Credential Manager, *nix Secret
//! Service).
//!
//! **Eigene Crate statt Teil von `app-shell`**: `app-shell` "enthält keine
//! fachliche Logik" (Spec 0007, Abschnitt 3) — ein OS-Keychain-Wrapper ist
//! zwar keine *fachliche* Logik, aber eine konkrete, austauschbare I/O-
//! Implementierung eines `core`-Traits, also genau die Sorte Baustein, die
//! im gesamten Projekt bislang immer eine eigene Crate bekommen hat
//! (`persistence-sqlite` für `ProfileStore`, `ssh-transport` für
//! `SshTransport`, `ai-providers` für `AiProvider`). Dieselbe Trennung hier
//! zu brechen, nur weil `app-shell` diese eine Implementierung als Erstes
//! braucht, würde das Muster inkonsistent machen, ohne echten Vorteil.
//!
//! **Kein automatisierter Test gegen den echten Keychain**: anders als
//! die übrigen konkreten Implementierungen in diesem Workspace lässt sich
//! diese hier nicht sinnvoll in `cargo test` verifizieren — ein Zugriff auf
//! den echten macOS-/Windows-/Secret-Service-Keychain setzt eine
//! interaktive, entsperrte Sitzung voraus (bei einer unsignierten
//! Dev-Build-Binary ggf. sogar einen einmaligen GUI-Bestätigungsdialog pro
//! Rebuild), was in einer headless/CI-artigen Testumgebung hängen bleiben
//! oder fehlschlagen kann. Analog zum bereits mit `#[ignore]` markierten
//! Zwei-Hop-Jump-Host-Test in `ssh-transport` (Spec 0005, ADR 0008) bleibt
//! diese Implementierung deshalb ungetestet durch `cargo test`; ihre
//! Korrektheit ist stattdessen durch den manuellen Smoke-Test beim Start
//! der Tauri-App (`cargo tauri dev`, Anlegen eines echten AI-Providers) zu
//! verifizieren.

use secrecy::{ExposeSecret, SecretString};
use ssh_manager_core::profiles::{
    CredentialError, CredentialRef, CredentialResult, CredentialStore,
};

/// Alle Einträge dieser App teilen sich einen Service-Namen im Keychain;
/// die `CredentialRef` (bereits eindeutig, s. `core::profiles::CredentialRef`-
/// Doc-Kommentar) dient als Account-Name innerhalb dieses Service. Ein
/// Konstante statt konfigurierbar: es gibt in diesem MVP nur eine einzige
/// App-Installation pro Nutzer, kein Bedarf für mehrere unterscheidbare
/// Services.
///
/// **Nutzer-sichtbar** (nicht nur internes Implementierungsdetail): dieser
/// exakte String erscheint z. B. in macOS Keychain Access als "Wo"-Spalte
/// eines Eintrags — nach der Umbenennung zu "Smart SSH" (Teil 1 dieser
/// Aufgabenstellung) entsprechend angepasst. Bereits unter dem alten Namen
/// "ssh-manager" im Keychain gespeicherte Einträge werden dadurch nicht
/// automatisch migriert (kein Mechanismus dafür in diesem MVP) — bei
/// diesem noch unveröffentlichten, lokal genutzten Projekt kein
/// nennenswertes Risiko, betrifft aber denselben, in Teil 1 bereits für
/// den Datenbankpfad akzeptierten Trade-off.
const SERVICE_NAME: &str = "Smart SSH";

pub struct KeyringCredentialStore;

impl KeyringCredentialStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(r: &CredentialRef) -> CredentialResult<keyring::Entry> {
        keyring::Entry::new(SERVICE_NAME, r.as_str())
            .map_err(|e| CredentialError::Backend(e.to_string()))
    }
}

impl Default for KeyringCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialStore for KeyringCredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        let entry = Self::entry(r)?;
        match entry.get_password() {
            Ok(password) => Ok(SecretString::from(password)),
            Err(keyring::Error::NoEntry) => Err(CredentialError::NotFound(r.clone())),
            Err(e) => Err(CredentialError::Backend(e.to_string())),
        }
    }

    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()> {
        let entry = Self::entry(r)?;
        entry
            .set_password(value.expose_secret())
            .map_err(|e| CredentialError::Backend(e.to_string()))
    }

    fn delete(&self, r: &CredentialRef) -> CredentialResult<()> {
        let entry = Self::entry(r)?;
        match entry.delete_credential() {
            // Löschen eines bereits fehlenden Eintrags ist idempotent kein
            // Fehler — passt zum Aufrufer-Verhalten in Spec 0007 Abschnitt
            // 8.2 (`delete_ai_provider` löscht zuerst das Credential, dann
            // die DB-Zeile; ein doppelter Löschversuch nach einem
            // vorherigen Teilfehler darf nicht an einem bereits gelöschten
            // Credential scheitern).
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CredentialError::Backend(e.to_string())),
        }
    }
}
