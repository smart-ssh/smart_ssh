//! Spec 0101, T12 (A12) — das MCP-Token in der Datenbank.
//!
//! Der Marker `Token-0101` kommt aus §7 der Spec.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use ssh_manager_core::profiles::SECRET_STORE_FAILED;

use crate::error::CommandError;
use crate::test_support::InMemoryCredentialStore;

use super::*;

const TOKEN_MARKER: &str = "Token-0101";

/// Ein `settings.json`-Ersatz, der zählt, was mit ihm passiert.
#[derive(Default)]
struct FakeSettingsFile {
    token: Mutex<Option<String>>,
    removes: AtomicUsize,
    fail_read: bool,
    fail_remove: bool,
}

impl FakeSettingsFile {
    fn with_token(token: &str) -> Self {
        Self {
            token: Mutex::new(Some(token.to_string())),
            ..Self::default()
        }
    }

    fn empty() -> Self {
        Self::default()
    }

    fn current(&self) -> Option<String> {
        self.token.lock().unwrap().clone()
    }

    fn removes(&self) -> usize {
        self.removes.load(Ordering::SeqCst)
    }
}

impl LegacyMcpTokenFile for FakeSettingsFile {
    fn read_token(&self) -> CommandResult<Option<String>> {
        if self.fail_read {
            return Err(CommandError::from("settings.json nicht lesbar"));
        }
        Ok(self.current())
    }

    fn remove_token(&self) -> CommandResult<()> {
        self.removes.fetch_add(1, Ordering::SeqCst);
        if self.fail_remove {
            return Err(CommandError::from("settings.json nicht schreibbar"));
        }
        *self.token.lock().unwrap() = None;
        Ok(())
    }
}

/// Ein Speicher, der **annimmt und etwas anderes zurückgibt**. Genau den
/// Fall soll „zurücklesen, vergleichen" aus A12 abfangen.
#[derive(Default)]
struct LyingCredentialStore {
    written: Mutex<Option<String>>,
}

impl CredentialStore for LyingCredentialStore {
    fn get(&self, r: &CredentialRef) -> ssh_manager_core::profiles::CredentialResult<SecretString> {
        match &*self.written.lock().unwrap() {
            Some(_) => Ok(SecretString::from("ein-anderer-wert".to_string())),
            None => Err(CredentialError::NotFound(r.clone())),
        }
    }

    fn set(
        &self,
        _r: &CredentialRef,
        value: SecretString,
    ) -> ssh_manager_core::profiles::CredentialResult<()> {
        *self.written.lock().unwrap() = Some(value.expose_secret().to_string());
        Ok(())
    }

    fn delete(&self, _r: &CredentialRef) -> ssh_manager_core::profiles::CredentialResult<()> {
        Ok(())
    }
}

fn reference() -> CredentialRef {
    CredentialRef::new(MCP_SERVER_TOKEN_REF.to_string())
}

fn never_generate() -> String {
    panic!("hier darf kein neues Token erzeugt werden — das alte muss übernommen werden");
}

/// **T12, erste Hälfte:** `settings.json` mit Token → Token in der
/// Datenbank, **gleicher Wert**, Schlüssel aus `settings.json` entfernt.
///
/// Der gleiche Wert ist die eigentliche Zusicherung: Ein frisch erzeugtes
/// Token wäre technisch auch „ein Token in der Datenbank", machte aber jede
/// bestehende MCP-Client-Konfiguration ungültig, ohne dass es jemand merkt.
/// Deshalb scheitert dieser Test (`never_generate`), sobald der Umzug
/// stattdessen neu erzeugt.
#[test]
fn test_t12_a_token_in_settings_json_moves_into_the_database_unchanged() {
    let credentials = InMemoryCredentialStore::new();
    let legacy = FakeSettingsFile::with_token(TOKEN_MARKER);

    let token = load_or_init_token(&credentials, &legacy, &never_generate)
        .expect("der Umzug muss gelingen");

    assert_eq!(
        token, TOKEN_MARKER,
        "der Wert muss unverändert übernommen werden"
    );
    assert_eq!(
        credentials
            .get(&reference())
            .expect("das Token muss in der Datenbank liegen")
            .expose_secret(),
        TOKEN_MARKER
    );
    assert_eq!(
        legacy.current(),
        None,
        "der Schlüssel muss aus settings.json entfernt sein"
    );
    assert_eq!(legacy.removes(), 1, "genau ein Entfernen");
}

/// **T12, zweite Hälfte:** „Erneuern schreibt nicht dorthin."
///
/// Dass gar nicht nach `settings.json` geschrieben werden *kann*, ist
/// strukturell: [`LegacyMcpTokenFile`] hat keine Schreibmethode, nur Lesen
/// und Entfernen.
///
/// Die Datei trägt hier absichtlich ein altes Token (spec-reviewer Runde 3:
/// mit leerer Datei wäre „bleibt leer" eine leere Aussage). Nach dem
/// Erneuern muss sie **entleert** sein — das alte Token ist jetzt wertlos,
/// aber es ist eine Klartext-Kopie eines Geheimnisses und gehört weg.
#[test]
fn test_t12_regenerating_writes_only_into_the_database() {
    let credentials = InMemoryCredentialStore::new().with_secret(&reference(), TOKEN_MARKER);
    let legacy = FakeSettingsFile::with_token("Token-0101-alt");
    let fresh = format!("{TOKEN_MARKER}-neu");

    let token = regenerate_token(&credentials, &legacy, &|| fresh.clone())
        .expect("das Erneuern muss gelingen");

    assert_eq!(token, fresh);
    assert_eq!(
        credentials
            .get(&reference())
            .expect("das neue Token muss in der Datenbank liegen")
            .expose_secret(),
        fresh.as_str(),
        "das alte Token muss ersetzt sein"
    );
    assert_eq!(
        legacy.current(),
        None,
        "nach settings.json wird nichts geschrieben, und die alte Kopie muss weg"
    );
    assert_eq!(legacy.removes(), 1);
}

/// **Ein leeres Token wird nicht übernommen** (spec-reviewer Runde 3).
///
/// Steht in `settings.json` `"mcpServerToken": ""`, würde „mit gleichem
/// Wert übernehmen" den Leerstring dauerhaft in die Datenbank schreiben.
/// Ein leeres erwartetes Token heißt MCP ohne Geheimnis — die gesamte
/// Angriffsfläche offen. Stattdessen: ein richtiges Token erzeugen und den
/// leeren Eintrag entfernen.
///
/// **Gegenbeweis geführt:** Ohne den Filter liefert der Aufruf den
/// Leerstring zurück und die erste Zusicherung scheitert.
#[test]
fn test_a12_an_empty_legacy_token_is_never_adopted() {
    let credentials = InMemoryCredentialStore::new();
    let legacy = FakeSettingsFile::with_token("   ");

    let token = load_or_init_token(&credentials, &legacy, &|| TOKEN_MARKER.to_string())
        .expect("muss gelingen");

    assert_eq!(
        token, TOKEN_MARKER,
        "statt des leeren Werts muss ein neues Token erzeugt werden"
    );
    assert_eq!(
        credentials
            .get(&reference())
            .expect("das neue Token muss in der Datenbank liegen")
            .expose_secret(),
        TOKEN_MARKER
    );
    assert_eq!(
        legacy.current(),
        None,
        "der leere Eintrag gehört trotzdem aus der Datei"
    );
}

/// A12: Die Datenbank ist die Quelle. Ein Rest in `settings.json` — ein
/// früheres Entfernen ist gescheitert — wird nachgeholt, nicht ignoriert:
/// Es ist eine Klartext-Kopie eines Geheimnisses auf der Platte.
#[test]
fn test_a12_the_database_wins_and_a_leftover_in_settings_json_is_removed() {
    let database_token = format!("{TOKEN_MARKER}-db");
    let credentials = InMemoryCredentialStore::new().with_secret(&reference(), &database_token);
    let legacy = FakeSettingsFile::with_token("Token-0101-alt");

    let token = load_or_init_token(&credentials, &legacy, &never_generate).expect("muss gelingen");

    assert_eq!(token, database_token, "die Datenbank entscheidet");
    assert_eq!(
        legacy.current(),
        None,
        "die alte Kopie in settings.json muss verschwinden"
    );
    assert_eq!(legacy.removes(), 1);
}

/// A12 mit A9.1: Ein **Störung** des Speichers darf nicht wie „kein Token"
/// aussehen.
///
/// Sonst erzeugte ein vorübergehend klemmender Speicher ein neues Token,
/// machte die bestehende Client-Konfiguration still ungültig — und das alte
/// Token wäre aus `settings.json` entfernt, also unwiederbringlich weg.
#[test]
fn test_a12_a_store_failure_never_silently_mints_a_new_token() {
    let credentials = InMemoryCredentialStore::new().with_failing_get();
    let legacy = FakeSettingsFile::with_token(TOKEN_MARKER);

    let err = load_or_init_token(&credentials, &legacy, &never_generate)
        .expect_err("ein klemmender Speicher muss sichtbar scheitern");

    assert_eq!(
        err.code,
        Some(SECRET_STORE_FAILED),
        "A9.1: der stabile Code des Secret-Speichers"
    );
    assert_eq!(
        legacy.current(),
        Some(TOKEN_MARKER.to_string()),
        "solange der neue Ort nicht antwortet, bleibt das Token, wo es ist"
    );
    assert_eq!(legacy.removes(), 0, "nichts entfernt");
}

/// A12, die Reihenfolge: **zurücklesen und vergleichen, erst dann
/// entfernen.**
///
/// Der Speicher nimmt hier an und gibt etwas anderes zurück. Würde der
/// Umzug nach dem `set` sofort entfernen, wäre das Token hier verloren:
/// nicht mehr in `settings.json` und in der Datenbank nur als falscher
/// Wert. Der Test hält fest, dass das nicht passiert — und dass der Wert
/// nicht in der Meldung landet.
#[test]
fn test_a12_the_legacy_token_stays_until_the_database_gives_the_value_back() {
    let credentials = LyingCredentialStore::default();
    let legacy = FakeSettingsFile::with_token(TOKEN_MARKER);

    let err = load_or_init_token(&credentials, &legacy, &never_generate)
        .expect_err("ein nicht zurücklesbarer Wert muss den Umzug abbrechen");

    assert_eq!(err.code, Some(SECRET_STORE_FAILED));
    assert!(
        !err.message.contains(TOKEN_MARKER),
        "das Token ist selbst das Geheimnis und gehört nicht in die Meldung: {}",
        err.message
    );
    assert_eq!(
        legacy.current(),
        Some(TOKEN_MARKER.to_string()),
        "nicht entfernen, solange der neue Ort den Wert nicht hergibt"
    );
    assert_eq!(legacy.removes(), 0);
}

/// A12: Nirgends ein Token → eines erzeugen, nur in die Datenbank.
#[test]
fn test_a12_a_fresh_installation_generates_the_token_into_the_database() {
    let credentials = InMemoryCredentialStore::new();
    let legacy = FakeSettingsFile::empty();

    let token = load_or_init_token(&credentials, &legacy, &|| TOKEN_MARKER.to_string())
        .expect("muss gelingen");

    assert_eq!(token, TOKEN_MARKER);
    assert_eq!(
        credentials
            .get(&reference())
            .expect("das Token muss in der Datenbank liegen")
            .expose_secret(),
        TOKEN_MARKER
    );
    assert_eq!(
        legacy.removes(),
        0,
        "ohne alten Eintrag gibt es nichts zu entfernen"
    );
}

/// A12: Ein gescheitertes Entfernen nach erfolgreichem Umzug hält die App
/// nicht auf — dasselbe Muster wie A11 für Schlüsselbund-Löschungen. Der
/// nächste Aufruf versucht es erneut.
#[test]
fn test_a12_a_failed_cleanup_does_not_block_the_settings() {
    let database_token = format!("{TOKEN_MARKER}-db");
    let credentials = InMemoryCredentialStore::new().with_secret(&reference(), &database_token);
    let legacy = FakeSettingsFile {
        token: Mutex::new(Some("Token-0101-alt".to_string())),
        fail_remove: true,
        ..FakeSettingsFile::default()
    };

    let token = load_or_init_token(&credentials, &legacy, &never_generate)
        .expect("ein gescheitertes Aufräumen darf die Einstellungen nicht blockieren");
    assert_eq!(token, database_token);
    assert_eq!(legacy.removes(), 1);

    // Zweiter Aufruf: erneuter Versuch, nicht aufgegeben.
    load_or_init_token(&credentials, &legacy, &never_generate).expect("muss wieder gelingen");
    assert_eq!(legacy.removes(), 2, "jeder Aufruf versucht es erneut");
}

/// A12: Ein unlesbares `settings.json` auf dem Aufräumweg (Datenbank hat
/// das Token schon) blockiert die Einstellungen ebenfalls nicht.
#[test]
fn test_a12_an_unreadable_settings_file_does_not_block_the_cleanup_path() {
    let database_token = format!("{TOKEN_MARKER}-db");
    let credentials = InMemoryCredentialStore::new().with_secret(&reference(), &database_token);
    let legacy = FakeSettingsFile {
        fail_read: true,
        ..FakeSettingsFile::default()
    };

    let token = load_or_init_token(&credentials, &legacy, &never_generate)
        .expect("der Aufräumweg darf das Laden nicht aufhalten");
    assert_eq!(token, database_token);
    assert_eq!(legacy.removes(), 0);
}
