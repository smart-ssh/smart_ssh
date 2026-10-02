//! Spec 0101, A10/A11: der einmalige Umzug der Secrets aus dem
//! Schlüsselbund des Betriebssystems in die verschlüsselte Datenbank.
//!
//! Läuft im Startablauf (§5, Schritt 7) nach dem Öffnen der Datenbank und
//! vor dem übrigen Zustand. Tauri-frei, damit der gesamte Ablauf ohne
//! Fenster prüfbar ist — die Dialoge kommen über [`StartupPrompt`], genau
//! wie bei A3/A5.
//!
//! **Die drei Zustände und warum es sie braucht** (A10):
//!
//! | Zustand | Bedeutung | Was ein Start damit tut |
//! |---|---|---|
//! | `open` | nichts umgezogen | lesen, schreiben, zurücklesen, vergleichen |
//! | `moved` | alles umgezogen und gleich | die festgehaltene Liste im Schlüsselbund löschen |
//! | `done` | Einträge weg | nichts |
//! | `skipped` | A11.1 (Etappe 3) | nichts, und **nie** löschen |
//!
//! Der Übergang nach `moved` passiert **erst, wenn jede** Referenz
//! zurückgelesen und gleich war. Vorher wird kein einziger
//! Schlüsselbund-Eintrag gelöscht — sonst könnte ein Abbruch in der Mitte
//! ein Secret vernichten, das noch nirgends sonst steht.
//!
//! Beim Übergang wird die Liste der zu löschenden Referenzen
//! **festgehalten** und später nach dieser Liste gelöscht, nicht nach dem
//! dann aktuellen Datenbankstand. Grund: Löscht der Nutzer einen Server
//! zwischen `moved` und `done`, nennt die Datenbank seine Referenzen nicht
//! mehr — sein Passwort bliebe für immer im Schlüsselbund liegen.

use ssh_manager_core::profiles::{AuthMethod, CredentialError, CredentialRef, CredentialStore};
use ssh_manager_core::shared::ServerId;

use persistence_sqlite::{ConnectFailureKind, SqliteProfileStore};

use crate::database_startup::{StartupAbort, StartupChoice, StartupDialog, StartupPrompt};

pub const STATE_OPEN: &str = "open";
pub const STATE_MOVED: &str = "moved";
pub const STATE_DONE: &str = "done";
pub const STATE_SKIPPED: &str = "skipped";

/// A10: Die Referenzen, die ein Server im Schlüsselbund haben kann.
///
/// Der feste Sudo-Slot ist **immer** dabei, unabhängig von der Anmeldeart
/// (A10 nennt ihn ausdrücklich gesondert): Ein Sudo-Passwort kann zu jedem
/// Server gehören, auch zu einem mit `Agent`-Anmeldung, und steht in keiner
/// Spalte, die man danach fragen könnte.
fn refs_of_server(id: ServerId, auth: &AuthMethod) -> Vec<CredentialRef> {
    let mut refs = vec![crate::server_credentials::sudo_password_credential_ref(id)];
    match auth {
        AuthMethod::Password { credential_ref } => refs.push(credential_ref.clone()),
        AuthMethod::PrivateKey {
            credential_ref,
            passphrase_ref,
        } => {
            refs.push(credential_ref.clone());
            refs.extend(passphrase_ref.clone());
        }
        AuthMethod::Certificate { cert_ref, key_ref } => {
            refs.push(cert_ref.clone());
            refs.push(key_ref.clone());
        }
        // Ein Pfad ist kein Secret (Spec 0076, B-4) — nur die Passphrase
        // liegt im Schlüsselbund.
        AuthMethod::IdentityFile { passphrase_ref, .. } => refs.extend(passphrase_ref.clone()),
        AuthMethod::Agent => {}
    }
    refs
}

/// A10: alle Referenzen, die laut Datenbank ein Secret haben könnten.
///
/// Reihenfolge und Eindeutigkeit sind zugesichert: Die Liste wird
/// festgehalten und später abgearbeitet, doppelte Einträge hieße doppeltes
/// Löschen (harmlos, weil idempotent) und eine unübersichtliche Liste im
/// Log.
async fn refs_in_database(store: &SqliteProfileStore) -> Result<Vec<CredentialRef>, StartupAbort> {
    use ssh_manager_core::profiles::ProfileStore;

    let servers = store
        .list_servers()
        .await
        .map_err(|err| StartupAbort::Fatal {
            // **`Other`, nicht `SecretMigrationFailed`** (spec-reviewer
            // Runde 3): Die realistische Ursache ist hier eine nicht
            // dekodierbare Zeile (`auth_method`-JSON, Spaltentyp), also
            // beschädigter Inhalt. Der Umzugstext sagt „es ist keine Datei
            // beschädigt" und „starte erneut" — das führte endlos in
            // denselben Fehler. Hier ist der Backup-Rat der richtige.
            kind: ConnectFailureKind::Other,
            detail: format!("Server für den Secret-Umzug nicht lesbar: {err}"),
        })?;
    let providers = store
        .ai_provider_store()
        .list()
        .await
        .map_err(|err| StartupAbort::Fatal {
            // `Other` aus demselben Grund wie eine Zeile höher.
            kind: ConnectFailureKind::Other,
            detail: format!("Provider für den Secret-Umzug nicht lesbar: {err}"),
        })?;

    let mut refs: Vec<CredentialRef> = Vec::new();
    for server in &servers {
        for reference in refs_of_server(server.id, &server.auth) {
            if is_migratable(&reference) && !refs.contains(&reference) {
                refs.push(reference);
            }
        }
    }
    for provider in &providers {
        if is_migratable(&provider.credential_ref) && !refs.contains(&provider.credential_ref) {
            refs.push(provider.credential_ref.clone());
        }
    }
    Ok(refs)
}

/// **Die Liste wird gegen das erwartete Schema geprüft, bevor irgendetwas
/// gelesen oder gelöscht wird** (spec-reviewer, Runde 1 — Angriffsrichtung
/// „Abgebrochener Moduswechsel löscht die einzige Kopie von K").
///
/// Die Referenzen kommen **aus der Datenbank**, wörtlich wie sie dort
/// stehen (`ai_provider_configs.credential_ref`, das `auth_method`-JSON der
/// Server) — sie werden nicht aus der Server-ID neu berechnet. Bis A6 die
/// Datei umwandelt, liegt dieses JSON im Klartext auf der Platte. Stünde
/// dort `app:chat_content_encryption_key`, zöge der Umzug **K** in die
/// Secrets-Tabelle, vergliche erfolgreich und löschte ihn danach aus dem
/// Schlüsselbund. Beim nächsten Start wäre die Datei verschlüsselt und der
/// Schlüssel weg: D2, und die ganze Datenbank verloren.
///
/// Deshalb: Nur die beiden Präfixe aus §1 werden angefasst, und K
/// ausdrücklich nie — die Prüfung ist eine Positivliste, keine Ausnahme von
/// einer, damit ein künftiger dritter `app:`-Eintrag ebenfalls nicht
/// hineinfällt. Eine Abweichung wird **übersprungen und geloggt**, nicht
/// zum Startfehler: Sie heißt nicht, dass etwas kaputt ist, sondern dass
/// etwas nicht hierher gehört.
fn is_migratable(reference: &CredentialRef) -> bool {
    let raw = reference.as_str();
    let ok = (raw.starts_with("server:") || raw.starts_with("ai-provider:"))
        && raw != ssh_manager_core::crypto::CHAT_CONTENT_ENCRYPTION_KEY_REF;
    if !ok {
        tracing::warn!(
            reference = raw,
            "a credential reference outside the expected schema is not migrated and never \
             deleted (Spec 0101, A10)"
        );
    }
    ok
}

/// Der Startschritt. `keyring` ist der Schlüsselbund des Betriebssystems,
/// `database` der neue Store aus A9.
///
/// Gibt `Ok(())` zurück, wenn der Start weitergehen darf — auch dann, wenn
/// ein **Löschen** gescheitert ist (A11: Warnung ins Log, App startet,
/// Löschen bei jedem Start erneut). Ein **Lesefehler** dagegen hält den
/// Start an und zeigt D1 ohne Einrichten, bis der Nutzer „Erneut versuchen"
/// wählt und es gelingt, oder „Beenden".
pub async fn migrate_secrets_into_database(
    store: &SqliteProfileStore,
    keyring: &(dyn CredentialStore + Send + Sync),
    database: &(dyn CredentialStore + Send + Sync),
    prompt: &dyn StartupPrompt,
) -> Result<(), StartupAbort> {
    loop {
        let (state, pending) =
            store
                .secret_migration_state()
                .await
                .map_err(|err| StartupAbort::Fatal {
                    kind: ConnectFailureKind::SecretMigrationFailed,
                    detail: format!("Umzugszustand nicht lesbar: {err}"),
                })?;

        match state.as_str() {
            STATE_DONE => return Ok(()),
            // A11.1: In diesem Zustand wird **nie** ein Eintrag gelöscht,
            // auch wenn der Schlüsselbund später erreichbar ist. Deshalb
            // hier ein eigener Zweig und nicht der `moved`-Zweig mit leerer
            // Liste — eine leere Liste könnte jemand später wieder füllen.
            STATE_SKIPPED => {
                tracing::info!("secret migration was skipped by the user; nothing is deleted");
                return Ok(());
            }
            STATE_MOVED => {
                delete_moved_entries(store, keyring, &pending).await?;
                return Ok(());
            }
            STATE_OPEN => {}
            other => {
                return Err(StartupAbort::Fatal {
                    // `Other`: Ein Zustand, den dieser Code nicht kennt,
                    // steht so in der Datenbank — das ist beschädigter
                    // Inhalt, nicht ein gescheiterter Umzug. „Starte
                    // erneut" hilft hier nicht (spec-reviewer Runde 3).
                    kind: ConnectFailureKind::Other,
                    detail: format!("unbekannter Umzugszustand: {other}"),
                });
            }
        }

        let refs = refs_in_database(store).await?;
        let mut moved: Vec<String> = Vec::new();
        let mut read_failed = false;

        for reference in &refs {
            let value = match keyring.get(reference) {
                Ok(value) => value,
                // A10: „`NotFound` → ausgelassen". Kein Fehler — ein Slot,
                // der nie belegt war, ist der Normalfall (ein Server ohne
                // Sudo-Passwort hat sechs davon).
                Err(CredentialError::NotFound(_)) => continue,
                // A11: Lesefehler → Dialog, nichts gelöscht, Zustand bleibt
                // *offen*. Die Nutzlast geht nicht in den Dialog (Spec 0098,
                // A5) und auch nicht ins `warn` (Spec 0094).
                Err(CredentialError::Backend(_)) => {
                    tracing::warn!(
                        reference = reference.as_str(),
                        "reading a secret from the OS keychain failed during the migration \
                         (Spec 0101, A11)"
                    );
                    read_failed = true;
                    break;
                }
            };

            // Schreiben, zurücklesen, vergleichen (A10). Erst dieser
            // Vergleich macht aus „geschrieben" ein „umgezogen".
            if let Err(err) = database.set(reference, value.clone()) {
                return Err(store_write_failed(reference, &err));
            }
            let read_back = database
                .get(reference)
                .map_err(|err| store_write_failed(reference, &err))?;
            use secrecy::ExposeSecret;
            if read_back.expose_secret() != value.expose_secret() {
                // Kann nach heutigem Code nicht vorkommen (dieselbe Zeile,
                // dieselbe Spalte) — und endet deshalb sichtbar, statt den
                // Umzug für gelungen zu erklären und danach den
                // Schlüsselbund-Eintrag zu löschen.
                return Err(StartupAbort::Fatal {
                    kind: ConnectFailureKind::SecretMigrationFailed,
                    detail: format!("zurückgelesenes Secret weicht ab: {}", reference.as_str()),
                });
            }
            moved.push(reference.as_str().to_string());
        }

        if read_failed {
            // D1 **ohne** Einrichten (A11): Ein neuer K würde den feldweise
            // verschlüsselten Verlauf unlesbar machen, und das Problem ist
            // ohnehin ein anderes — der Schlüsselbund antwortet nicht.
            match prompt.ask(StartupDialog::D1 {
                offers_password_setup: false,
            }) {
                // Erneut prüfen, ohne Neustart — dieselbe Zusicherung wie
                // bei D1 in A3. Der Zustand ist noch *offen*, also beginnt
                // der nächste Durchlauf von vorn.
                StartupChoice::Retry => continue,
                // **Erschöpfend, kein `_`-Zweig** (spec-reviewer Runde 1):
                // A11.1 fügt in Etappe 3 genau hier eine dritte Wahl hinzu
                // („Ohne Übernahme fortfahren", Zustand *übersprungen*).
                // Als Catch-all gälte sie stillschweigend als „Beenden" —
                // der Compiler würde nichts sagen, und der Fehler fiele
                // erst im Betrieb auf. So scheitert der Bau, bis die neue
                // Wahl hier bewusst behandelt ist.
                StartupChoice::Quit => return Err(StartupAbort::UserQuit),
                // D1 bietet diese beiden nicht an (`offers_password_setup:
                // false`, kein „Neu anfangen" in diesem Dialog). Käme eine
                // davon trotzdem zurück, wäre die Lage unklar — dann nichts
                // anfassen und beenden, statt zu raten.
                StartupChoice::StartOver | StartupChoice::GenerateNewKey => {
                    tracing::warn!(
                        "the migration dialog returned a choice it does not offer; quitting \
                         without touching anything (Spec 0101, A11)"
                    );
                    return Err(StartupAbort::UserQuit);
                }
            }
        }

        // Alles gelesen, geschrieben und gleich zurückgelesen → *umgezogen*,
        // mit festgehaltener Löschliste.
        store
            .set_secret_migration_state(STATE_MOVED, &moved)
            .await
            .map_err(|err| StartupAbort::Fatal {
                kind: ConnectFailureKind::SecretMigrationFailed,
                detail: format!("Umzugszustand nicht schreibbar: {err}"),
            })?;
        tracing::info!(
            count = moved.len(),
            "secrets moved into the encrypted database (Spec 0101, A10)"
        );
        delete_moved_entries(store, keyring, &moved).await?;
        return Ok(());
    }
}

/// A11, zweite Hälfte: Löschfehler sind **kein** Startabbruch. Die App
/// startet, eine Warnung geht ins Log, und der nächste Start versucht es
/// erneut — die Liste bleibt dafür stehen.
async fn delete_moved_entries(
    store: &SqliteProfileStore,
    keyring: &(dyn CredentialStore + Send + Sync),
    pending: &[String],
) -> Result<(), StartupAbort> {
    let mut still_pending: Vec<String> = Vec::new();
    for raw in pending {
        let reference = CredentialRef::new(raw.clone());
        // Zweite Schranke, an der Stelle, die tatsächlich löscht
        // (spec-reviewer Runde 1): Die Liste in der Datenbank ist selbst
        // eine Datenquelle — eine Zeile, die jemand dort verändert, darf
        // kein `delete` auf einen fremden Schlüsselbund-Eintrag auslösen.
        // Dieselbe Prüfung wie beim Aufsammeln, damit sie nicht an einer
        // einzigen Stelle hängt.
        if !is_migratable(&reference) {
            continue;
        }
        match keyring.delete(&reference) {
            // `delete` ist idempotent (A9): „war schon weg" ist Erfolg.
            Ok(()) => {}
            Err(CredentialError::NotFound(_)) => {}
            Err(CredentialError::Backend(_)) => {
                tracing::warn!(
                    reference = raw.as_str(),
                    "deleting a migrated secret from the OS keychain failed; it stays on the \
                     list and is retried at the next start (Spec 0101, A11)"
                );
                still_pending.push(raw.clone());
            }
        }
    }

    let (state, pending_now) = if still_pending.is_empty() {
        (STATE_DONE, Vec::new())
    } else {
        (STATE_MOVED, still_pending)
    };
    store
        .set_secret_migration_state(state, &pending_now)
        .await
        .map_err(|err| StartupAbort::Fatal {
            kind: ConnectFailureKind::SecretMigrationFailed,
            detail: format!("Umzugszustand nicht schreibbar: {err}"),
        })?;
    Ok(())
}

/// Der Secret-Speicher selbst hat nicht geschrieben oder nicht
/// zurückgelesen. Das ist kein Schlüsselbund-Problem und auch keines, das
/// „Erneut versuchen" löst — die Datenbank ist gerade erst erfolgreich
/// geöffnet worden. Also ein sichtbarer Startfehler; die Nutzlast geht nur
/// in den `detail`, der ausschließlich ins Log wandert.
fn store_write_failed(reference: &CredentialRef, err: &CredentialError) -> StartupAbort {
    tracing::warn!(
        reference = reference.as_str(),
        "writing a migrated secret into the database failed (Spec 0101, A10)"
    );
    StartupAbort::Fatal {
        kind: ConnectFailureKind::SecretMigrationFailed,
        detail: format!("Secret-Umzug: {} ({err})", reference.as_str()),
    }
}

#[cfg(test)]
mod tests;
