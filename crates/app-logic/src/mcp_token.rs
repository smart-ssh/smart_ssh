//! Spec 0101, A12: Das MCP-Server-Token liegt in der verschlüsselten
//! Datenbank, nicht in `settings.json`.
//!
//! **Warum das überhaupt ein Thema ist:** Das Bearer-Token schaltet die
//! gesamte MCP-Angriffsfläche frei — wer es hat, ist ein vollwertiger
//! MCP-Client. Bisher lag es im Klartext in `settings.json`; die Datei
//! wurde best-effort auf 0600 gehärtet, aber Klartext auf der Platte blieb
//! Klartext (s. `app_shell::mcp_settings::harden_settings_store_permissions`
//! und der dort genannte Review-Befund, der genau diesen Schritt
//! vorschlug).
//!
//! **Der Umzug (A12):** Ein Token aus `settings.json` wird mit gleichem
//! Wert übernommen — schreiben, **zurücklesen, vergleichen** —, und erst
//! danach dort entfernt. Dieselbe Reihenfolge wie beim Secret-Umzug (A10):
//! Erst wenn der neue Ort den Wert nachweislich hergibt, verschwindet der
//! alte. Ein Abbruch in der Mitte lässt das Token in `settings.json`
//! stehen; der nächste Start holt den Umzug nach.
//!
//! **Tauri-frei**, damit T12 ohne Fenster und ohne echte `settings.json`
//! läuft: Der alte Ablageort steckt hinter [`LegacyMcpTokenFile`].

use secrecy::{ExposeSecret, SecretString};

use ssh_manager_core::profiles::{
    CredentialError, CredentialRef, CredentialStore, MCP_SERVER_TOKEN_REF,
};

use crate::error::{secret_store_error, CommandResult};

/// Der alte Ablageort des Tokens (`settings.json`).
///
/// Ein Trait, weil `tauri-plugin-store` nur in `app-shell` erreichbar ist —
/// die Reihenfolge „schreiben, zurücklesen, vergleichen, dann entfernen"
/// soll aber hier geprüft werden können, wo kein Fenster nötig ist.
pub trait LegacyMcpTokenFile {
    /// Das Token aus `settings.json`, falls dort noch eines steht.
    fn read_token(&self) -> CommandResult<Option<String>>;

    /// Entfernt den Schlüssel aus `settings.json` und schreibt die Datei.
    fn remove_token(&self) -> CommandResult<()>;
}

fn token_reference() -> CredentialRef {
    CredentialRef::new(MCP_SERVER_TOKEN_REF.to_string())
}

/// Schreiben, zurücklesen, vergleichen (A12).
///
/// Der Vergleich ist nicht Zierde: Ein Secret-Speicher, der schreibt, ohne
/// zu speichern, würde dem Nutzer sonst ein Token anzeigen, mit dem sich
/// kein Client anmelden kann — und beim Umzug wäre das alte schon gelöscht.
///
/// Scheitert der Vergleich, geht **kein** Wert in die Meldung: Das Token
/// ist selbst das Geheimnis.
fn write_and_verify(
    credentials: &(dyn CredentialStore + Send + Sync),
    token: &str,
) -> CommandResult<()> {
    let reference = token_reference();
    credentials
        .set(&reference, SecretString::from(token.to_string()))
        .map_err(secret_store_error)?;
    let read_back = credentials.get(&reference).map_err(secret_store_error)?;
    if read_back.expose_secret() != token {
        return Err(secret_store_error(CredentialError::Backend(
            "das MCP-Token war nach dem Schreiben nicht unverändert lesbar (Spec 0101, A12)"
                .to_string(),
        )));
    }
    Ok(())
}

/// Ein Rest in `settings.json` ist kein Grund, die Einstellungen nicht
/// zu laden — aber auch keiner, ihn stillschweigend liegen zu lassen.
///
/// Dasselbe Muster wie A11 für gescheiterte Schlüsselbund-Löschungen:
/// Warnung ins Log, Aufruf läuft weiter, **jeder** folgende Aufruf
/// versucht es erneut. Die Datenbank ist ab hier die Quelle; der Rest in
/// der Datei ist eine alte Kopie, die weg soll.
fn remove_legacy_token_best_effort(legacy: &dyn LegacyMcpTokenFile) {
    match legacy.read_token() {
        Ok(None) => {}
        Ok(Some(_)) => {
            if let Err(err) = legacy.remove_token() {
                tracing::warn!(
                    code = err.code.unwrap_or("none"),
                    "removing the MCP token from settings.json failed; it is retried on the \
                     next call (Spec 0101, A12)"
                );
            } else {
                tracing::info!("the MCP token was removed from settings.json (Spec 0101, A12)");
            }
        }
        Err(err) => {
            tracing::warn!(
                code = err.code.unwrap_or("none"),
                "reading settings.json for the MCP token cleanup failed (Spec 0101, A12)"
            );
        }
    }
}

/// A12: Das Token aus der Datenbank — übernimmt bei Bedarf eines aus
/// `settings.json`, erzeugt sonst eines.
///
/// `generate` kommt von außen, damit der Test einen festen Wert setzen
/// kann; produktiv ist das die UUID-Erzeugung aus `app-shell`.
///
/// Reihenfolge der Fälle, und warum sie so ist:
/// 1. **Die Datenbank hat ein Token** → es gilt. Steht trotzdem noch eines
///    in `settings.json`, ist ein früheres Entfernen gescheitert; das wird
///    hier nachgeholt, nicht ignoriert.
/// 2. **Kein Token in der Datenbank, eines in `settings.json`** → Umzug:
///    schreiben, zurücklesen, vergleichen, dann dort entfernen. Der Wert
///    bleibt **gleich** (A12) — ein neues Token würde jede bestehende
///    Client-Konfiguration ungültig machen, ohne dass der Nutzer es
///    merkt.
/// 3. **Nirgends eines** → erzeugen und nur in die Datenbank schreiben.
///
/// Ein [`CredentialError::Backend`] des Speichers wird **nicht** als „kein
/// Token" gelesen (A9.1): Sonst erzeugte ein vorübergehend klemmender
/// Speicher ein neues Token und machte die bestehende Client-Konfiguration
/// still ungültig.
pub fn load_or_init_token(
    credentials: &(dyn CredentialStore + Send + Sync),
    legacy: &dyn LegacyMcpTokenFile,
    generate: &dyn Fn() -> String,
) -> CommandResult<String> {
    match credentials.get(&token_reference()) {
        Ok(token) => {
            remove_legacy_token_best_effort(legacy);
            Ok(token.expose_secret().to_string())
        }
        Err(CredentialError::NotFound(_)) => {
            if let Some(token) = legacy.read_token()? {
                write_and_verify(credentials, &token)?;
                // Erst jetzt — der neue Ort hat den Wert nachweislich
                // hergegeben.
                legacy.remove_token()?;
                tracing::info!(
                    "the MCP token moved from settings.json into the database (Spec 0101, A12)"
                );
                return Ok(token);
            }
            let token = generate();
            write_and_verify(credentials, &token)?;
            Ok(token)
        }
        Err(err @ CredentialError::Backend(_)) => Err(secret_store_error(err)),
    }
}

/// A12: „Erzeugen und Erneuern schreiben nur in die Datenbank."
///
/// Das neue Token wird ebenfalls zurückgelesen und verglichen — ein
/// Erneuern, das nicht ankommt, würde den Nutzer mit einem Token
/// dastehen lassen, das kein Client benutzen kann.
pub fn regenerate_token(
    credentials: &(dyn CredentialStore + Send + Sync),
    legacy: &dyn LegacyMcpTokenFile,
    generate: &dyn Fn() -> String,
) -> CommandResult<String> {
    let token = generate();
    write_and_verify(credentials, &token)?;
    // Ein altes Token in `settings.json` wäre nach dem Erneuern ohnehin
    // wertlos — aber es ist eine Kopie eines Geheimnisses im Klartext und
    // gehört weg.
    remove_legacy_token_best_effort(legacy);
    Ok(token)
}

#[cfg(test)]
mod tests;
