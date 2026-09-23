//! Teil C der Spec 0076 (BL-0222): **Überführung einer Schlüsseldatei in
//! den Schlüsselbund.**
//!
//! Der Weg geht nur in eine Richtung. Aus einem gespeicherten Schlüssel
//! wieder eine Datei zu machen ist ausdrücklich kein Ziel (erstes
//! Nicht-Ziel in §2) — das hieße, Schlüsselmaterial aus dem Schlüsselbund
//! auf die Platte zu schreiben.
//!
//! Wie `crate::servers` von `tauri::State` losgelöst gehalten, damit sich
//! der Ablauf gegen einen In-Memory-`ProfileStore`/`CredentialStore`
//! prüfen lässt.

use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialStore, ProfileStore,
};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{KeyFileError, KeyFileReader};

use credentials_keyring::KeychainAvailability;
use secrecy::SecretString;

use crate::dto::{KeyFileFactsDto, ServerDto};
use crate::error::{keychain_aware_credential_error, CommandError, CommandResult};

/// Stabiler Code für den Fall, dass der Server gar keine Schlüsseldatei
/// benutzt — das Frontend bietet den Knopf dann erst gar nicht an, aber ein
/// Kommando darf sich nicht darauf verlassen, dass die Oberfläche mitdenkt.
pub const NOT_AN_IDENTITY_FILE: &str = "SERVER_NOT_AN_IDENTITY_FILE";

/// C-7/B-3: Was an einer Schlüsseldatei auffällt, **ohne** sie
/// herauszugeben.
///
/// Benutzt bewusst [`KeyFileReader::inspect`] und nicht `read`: Der Befund
/// braucht den Schlüssel nicht, und was nicht herausgegeben wird, kann auch
/// nicht versehentlich angezeigt oder gespeichert werden (§5.2, §4.2).
pub fn inspect_key_file(
    key_files: &(dyn KeyFileReader + Send + Sync),
    path: &str,
) -> KeyFileFactsDto {
    KeyFileFactsDto::from(key_files.inspect(path))
}

/// **C-3/C-6: Überführt die Schlüsseldatei eines Servers in den
/// Schlüsselbund.**
///
/// Ablauf und die beiden Abbruchstellen, die es geben muss (C-6):
///
/// 1. Datei **einmal** lesen — mit `enforce_permissions: false`. Eine zu
///    weit geöffnete Datei ist genau der Fall, für den es diesen Weg gibt
///    (A-4, letzter Absatz): Der Knopf behebt den Mangel, er darf nicht an
///    ihm scheitern.
/// 2. Inhalt **unverändert** in den Schlüsselbund legen (C-4) — kein
///    Trimmen, kein Entschlüsseln. Scheitert das, bleibt der Server
///    unverändert auf `IdentityFile`; geschrieben wurde nichts.
/// 3. Anmeldeart auf `PrivateKey` umstellen und speichern. Eine vorhandene
///    `passphrase_ref` wandert **unverändert** mit (C-3) — sie zeigt schon
///    auf den richtigen Slot.
/// 4. Scheitert das Speichern, wird der eben geschriebene
///    Schlüsselbund-Eintrag zurückgenommen. Sonst bliebe ein privater
///    Schlüssel liegen, auf den kein Server zeigt.
///
/// **Die Ursprungsdatei wird nicht angefasst** (C-5): nicht gelöscht, nicht
/// geändert, nicht in den Rechten verändert. Ein Löschangebot ist
/// ausdrücklich nicht Teil dieser Spec (E-4).
pub async fn convert_identity_file_to_keychain(
    store: &dyn ProfileStore,
    credential_store: &(dyn CredentialStore + Send + Sync),
    keychain: KeychainAvailability,
    key_files: &(dyn KeyFileReader + Send + Sync),
    server_id: ServerId,
) -> CommandResult<ServerDto> {
    let mut server = store.get_server(&server_id).await?;

    let AuthMethod::IdentityFile {
        path,
        passphrase_ref,
    } = server.auth.clone()
    else {
        return Err(CommandError::with_code(
            "Dieser Server meldet sich nicht mit einer Schlüsseldatei an",
            NOT_AN_IDENTITY_FILE,
        ));
    };

    // Schritt 1 (C-3, A-4 letzter Absatz).
    let content = key_files
        .read(&path, false)
        .map_err(identity_file_error_to_command_error)?;

    let key_ref = CredentialRef::new(format!("server:{}:private_key", server_id.0));

    // Was vorher unter diesem Slot stand — für den Rückweg in Schritt 4.
    // Ein `IdentityFile`-Server zeigt nie auf `private_key`, hier sollte
    // also nichts liegen; falls doch, wird es wiederhergestellt statt
    // gelöscht. Ein Rollback, der fremde Daten entfernt, wäre kein
    // Rollback.
    //
    // spec-reviewer-Fund: Hier stand `…get(&key_ref).ok()`. Das machte aus
    // „der Schlüsselbund konnte nicht antworten" dasselbe wie „der Slot ist
    // leer" — und ein späterer Rollback hätte dann **gelöscht**, statt
    // wiederherzustellen. Genau das Gegenteil dessen, was der Absatz
    // darüber zusichert. Ein `Backend`-Fehler bricht deshalb **vor** dem
    // Schreiben ab: Wer nicht weiß, was er überschreibt, überschreibt
    // nichts.
    let previous = match credential_store.get(&key_ref) {
        Ok(value) => Some(value),
        Err(CredentialError::NotFound(_)) => None,
        Err(err) => return Err(keychain_aware_credential_error(err, keychain)),
    };

    // Schritt 2 (C-4): byte-gleich, ohne Trimmen. `write_or_reuse_secret`
    // scheidet deshalb aus — die trimmt, und §6.4.5 verlangt Byte-Gleichheit
    // mit der Datei.
    credential_store
        .set(&key_ref, content.key)
        .map_err(|err| keychain_aware_credential_error(err, keychain))?;

    // Schritt 3 (C-3): die Passphrase-Referenz wandert unverändert mit.
    server.auth = AuthMethod::PrivateKey {
        credential_ref: key_ref.clone(),
        passphrase_ref,
    };

    // Schritt 4 (C-6, zweite Richtung).
    if let Err(err) = store.update_server(&server).await {
        let command_error = CommandError::from(err);
        // spec-reviewer-Fund: Scheitert der Rollback selbst, bleibt ein
        // privater Schlüssel im Schlüsselbund liegen, auf den kein Server
        // zeigt — genau der Zustand, den C-6 ausschließt. Das nur ins Log
        // zu schreiben wäre dasselbe falsche Erfolgssignal, das Spec 0071
        // A17 an anderer Stelle bereits abgeräumt hat: Der Nutzer bekäme
        // „Speichern fehlgeschlagen" und wüsste nicht, dass etwas
        // zurückblieb. Also sagen wir es ihm.
        if roll_back_key_slot(credential_store, &key_ref, previous) == Rollback::LeftBehind {
            return Err(CommandError::with_code(
                format!(
                    "{}. Außerdem konnte der bereits in den Schlüsselbund geschriebene \
                     Schlüssel nicht wieder entfernt werden — der Eintrag „{}“ ist dort \
                     liegen geblieben und kann von Hand gelöscht werden.",
                    command_error.message.trim_end_matches(['.', ' ']),
                    key_ref.as_str()
                ),
                // spec-reviewer-Fund (Runde 2): **eigener** Code, nicht
                // `KEYCHAIN_UNAVAILABLE`. Das Frontend ersetzt bei einem
                // bekannten Code die Meldung durch seinen eigenen Text —
                // mit dem Schlüsselbund-Code hätte der Nutzer „Der
                // Systemschlüsselbund ist nicht verfügbar" gelesen und
                // ausgerechnet nicht erfahren, was liegen geblieben ist.
                crate::error::IDENTITY_FILE_ROLLBACK_LEFT_KEY_BEHIND,
            ));
        }
        return Err(command_error);
    }

    Ok(ServerDto::from_server(&server, credential_store))
}

/// Ob der Rollback den Schlüsselbund wieder in den Ausgangszustand
/// gebracht hat — oder ob dort etwas liegen geblieben ist, das der Nutzer
/// erfahren muss (C-6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rollback {
    Clean,
    LeftBehind,
}

/// Nimmt Schritt 2 zurück: den vorherigen Wert wiederherstellen, oder — wenn
/// es keinen gab — den Slot leeren.
///
/// Gibt zurück, ob das gelungen ist. Der Aufrufer sagt es weiter, statt es
/// nur ins Log zu schreiben: Ein liegen gebliebener privater Schlüssel, auf
/// den kein Server zeigt, ist genau der halbe Zustand, den C-6 ausschließt
/// — und „Speichern fehlgeschlagen" allein verschweigt ihn.
#[must_use]
fn roll_back_key_slot(
    credential_store: &(dyn CredentialStore + Send + Sync),
    key_ref: &CredentialRef,
    previous: Option<SecretString>,
) -> Rollback {
    let outcome = match previous {
        Some(value) => credential_store.set(key_ref, value),
        None => match credential_store.delete(key_ref) {
            Err(CredentialError::NotFound(_)) => Ok(()),
            other => other,
        },
    };
    match outcome {
        Ok(()) => Rollback::Clean,
        Err(err) => {
            tracing::warn!(
                credential_ref = %key_ref.as_str(),
                error = %err,
                "Rollback der Schlüsselbund-Überführung fehlgeschlagen: verwaister Eintrag"
            );
            Rollback::LeftBehind
        }
    }
}

/// A-6/A-7: Die Meldung nennt Pfad und Grund — und nie einen Dateiinhalt.
/// Der stabile Code kommt aus [`KeyFileError::code`], damit das Frontend
/// übersetzen kann, ohne den Text zu lesen (Spec 0024, Abschnitt 5).
fn identity_file_error_to_command_error(err: KeyFileError) -> CommandError {
    CommandError::with_code(err.to_string(), err.code())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::AuthMethodKind;
    use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore};
    use chrono::Utc;
    use secrecy::ExposeSecret;
    use ssh_manager_core::profiles::{PostIngestPolicy, Server};
    use ssh_manager_core::ssh::mock::{MockKeyFile, MockKeyFileReader};

    const AVAILABLE: KeychainAvailability = KeychainAvailability::Available;
    const PATH: &str = "/home/deploy/.ssh/id_ed25519";

    /// Steht stellvertretend für einen verschlüsselten Schlüssel. Was hier
    /// zählt, ist **Byte-Gleichheit** — ob `russh` ihn parsen könnte, ist
    /// eine andere Frage und gehört zur Leseumsetzung, nicht hierher: Die
    /// Überführung entschlüsselt bewusst nichts (C-4).
    const ENCRYPTED_KEY_TEXT: &str =
        "-----BEGIN OPENSSH PRIVATE KEY-----\nZW5jcnlwdGVkCg==\n-----END OPENSSH PRIVATE KEY-----\n";

    fn server_with(id: ServerId, auth: AuthMethod) -> Server {
        let now = Utc::now();
        Server {
            id,
            name: "web-01".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth,
            notes: String::new(),
            jump_host: None,
            post_ingest_policy: PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn identity_server(id: ServerId, passphrase_ref: Option<CredentialRef>) -> Server {
        server_with(
            id,
            AuthMethod::IdentityFile {
                path: PATH.to_string(),
                passphrase_ref,
            },
        )
    }

    fn key_slot(id: ServerId) -> CredentialRef {
        CredentialRef::new(format!("server:{}:private_key", id.0))
    }

    fn stored(store: &InMemoryCredentialStore, r: &CredentialRef) -> Option<String> {
        store.get(r).ok().map(|s| s.expose_secret().to_string())
    }

    /// **§6.3.3: Überführung eines unverschlüsselten Schlüssels.** Danach
    /// `PrivateKey`, der Inhalt liegt im Schlüsselbund, der Server zeigt
    /// darauf — und an keiner Datei mehr.
    #[tokio::test]
    async fn test_t6_3_3_converting_an_unencrypted_key_switches_to_private_key() {
        let id = ServerId::new();
        let profile_store = InMemoryProfileStore::new().with_server(identity_server(id, None));
        let credential_store = InMemoryCredentialStore::new();
        let key_files = MockKeyFileReader::new().with_key(PATH, "the-key-bytes");

        let dto = convert_identity_file_to_keychain(
            &profile_store,
            &credential_store,
            AVAILABLE,
            &key_files,
            id,
        )
        .await
        .expect("eine lesbare, gültige Schlüsseldatei muss sich übernehmen lassen");

        assert_eq!(dto.auth_kind, AuthMethodKind::PrivateKey);
        assert_eq!(
            dto.identity_file_path, None,
            "nach der Überführung hängt nichts mehr an einer Datei"
        );

        let reloaded = profile_store.get_server(&id).await.unwrap();
        let AuthMethod::PrivateKey {
            credential_ref,
            passphrase_ref,
        } = &reloaded.auth
        else {
            panic!(
                "erwartete AuthMethod::PrivateKey, bekam {:?}",
                reloaded.auth
            );
        };
        assert_eq!(credential_ref, &key_slot(id));
        assert_eq!(passphrase_ref, &None);
        assert_eq!(
            stored(&credential_store, credential_ref).as_deref(),
            Some("the-key-bytes")
        );
    }

    /// **§6.3.4/§6.4.5/C-4: Ein verschlüsselter Schlüssel wird
    /// verschlüsselt übernommen.**
    ///
    /// Der abgelegte Wert ist **byte-gleich** mit der Datei — nicht die
    /// entschlüsselte Form und auch nicht die getrimmte: Die Datei endet
    /// auf einen Zeilenumbruch, und der gehört zum Inhalt.
    ///
    /// Der Test scheitert, sobald jemand den Wert über
    /// `write_or_reuse_secret` schreibt (die trimmt) oder ihn unterwegs
    /// entschlüsselt.
    #[tokio::test]
    async fn test_t6_3_4_an_encrypted_key_is_stored_byte_identical_with_its_passphrase_ref() {
        let id = ServerId::new();
        let passphrase_ref = CredentialRef::new(format!("server:{}:passphrase", id.0));
        let profile_store = InMemoryProfileStore::new()
            .with_server(identity_server(id, Some(passphrase_ref.clone())));
        let credential_store =
            InMemoryCredentialStore::new().with_secret(&passphrase_ref, "correct horse");
        let key_files = MockKeyFileReader::new().with_file(
            PATH,
            MockKeyFile {
                content: ENCRYPTED_KEY_TEXT.to_string(),
                encrypted: true,
                permissions_too_open: false,
            },
        );

        convert_identity_file_to_keychain(
            &profile_store,
            &credential_store,
            AVAILABLE,
            &key_files,
            id,
        )
        .await
        .unwrap();

        let reloaded = profile_store.get_server(&id).await.unwrap();
        let AuthMethod::PrivateKey {
            credential_ref,
            passphrase_ref: moved,
        } = &reloaded.auth
        else {
            panic!("erwartete AuthMethod::PrivateKey");
        };
        assert_eq!(
            moved.as_ref(),
            Some(&passphrase_ref),
            "C-3: eine vorhandene passphrase_ref wandert unverändert mit"
        );
        assert_eq!(
            stored(&credential_store, &passphrase_ref).as_deref(),
            Some("correct horse"),
            "die Passphrase selbst bleibt unangetastet"
        );

        let value = stored(&credential_store, credential_ref).expect("Schlüssel wurde abgelegt");
        assert_eq!(value, ENCRYPTED_KEY_TEXT, "C-4/§6.4.5: byte-gleich");
        assert!(
            value.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"),
            "nichts Entschlüsseltes im Schlüsselbund: {value}"
        );
        assert!(
            value.ends_with('\n'),
            "auch der abschließende Zeilenumbruch gehört zum Inhalt — nicht trimmen"
        );
    }

    /// **§6.3.2b/A-4, letzter Absatz: Eine Datei mit Rechten `0644` lässt
    /// sich übernehmen.**
    ///
    /// Das ist der Fall, für den es den Knopf gibt: A-4 würde die
    /// *Anmeldung* damit ablehnen, die Überführung behebt den Mangel. Der
    /// Test scheitert, sobald jemand hier `enforce_permissions: true`
    /// setzt — dann wäre der Rettungsknopf genau dann gesperrt, wenn er
    /// gebraucht wird.
    #[tokio::test]
    async fn test_t6_3_2b_a_world_readable_key_file_can_still_be_converted() {
        let id = ServerId::new();
        let profile_store = InMemoryProfileStore::new().with_server(identity_server(id, None));
        let credential_store = InMemoryCredentialStore::new();
        let key_files = MockKeyFileReader::new().with_file(
            PATH,
            MockKeyFile {
                content: "the-key-bytes".to_string(),
                encrypted: false,
                permissions_too_open: true,
            },
        );

        convert_identity_file_to_keychain(
            &profile_store,
            &credential_store,
            AVAILABLE,
            &key_files,
            id,
        )
        .await
        .expect("C-1: der Knopf behebt den Rechte-Mangel, er darf nicht an ihm scheitern");

        assert_eq!(
            key_files.calls(),
            vec![format!("read {PATH} false")],
            "die Überführung liest genau einmal (C-3), und ohne Rechte-Sperre"
        );
    }

    /// **§6.3.5, erste Richtung (C-6):** Schreiben in den Schlüsselbund
    /// schlägt fehl → der Server bleibt unverändert auf `IdentityFile`.
    #[tokio::test]
    async fn test_t6_3_5_a_failing_keychain_write_leaves_the_server_on_identity_file() {
        let id = ServerId::new();
        let profile_store = InMemoryProfileStore::new().with_server(identity_server(id, None));
        let credential_store =
            InMemoryCredentialStore::new().with_failing_set_for_slot("private_key");
        let key_files = MockKeyFileReader::new().with_key(PATH, "the-key-bytes");

        convert_identity_file_to_keychain(
            &profile_store,
            &credential_store,
            AVAILABLE,
            &key_files,
            id,
        )
        .await
        .expect_err("ein fehlgeschlagener Schreibzugriff darf nicht als Erfolg gelten");

        let reloaded = profile_store.get_server(&id).await.unwrap();
        assert!(
            matches!(reloaded.auth, AuthMethod::IdentityFile { .. }),
            "C-6: der Server bleibt unverändert, bekam {:?}",
            reloaded.auth
        );
        assert_eq!(stored(&credential_store, &key_slot(id)), None);
    }

    /// **§6.3.5, zweite Richtung (C-6):** Schlüsselbund-Schreiben gelingt,
    /// das Speichern des Servers scheitert → der eben geschriebene Eintrag
    /// ist wieder weg.
    ///
    /// Ohne diesen Rückweg bliebe ein privater Schlüssel im Schlüsselbund
    /// liegen, auf den kein Server zeigt — genau der verwaiste Zustand,
    /// den C-6 ausschließen soll.
    #[tokio::test]
    async fn test_t6_3_5_a_failing_server_save_removes_the_freshly_written_key() {
        let id = ServerId::new();
        let profile_store = InMemoryProfileStore::new()
            .with_server(identity_server(id, None))
            .with_failing_update_server();
        let credential_store = InMemoryCredentialStore::new();
        let key_files = MockKeyFileReader::new().with_key(PATH, "the-key-bytes");

        convert_identity_file_to_keychain(
            &profile_store,
            &credential_store,
            AVAILABLE,
            &key_files,
            id,
        )
        .await
        .expect_err("ein fehlgeschlagenes Speichern darf nicht als Erfolg gelten");

        assert_eq!(
            stored(&credential_store, &key_slot(id)),
            None,
            "C-6: kein verwaister Schlüssel im Schlüsselbund"
        );
        let reloaded = profile_store.get_server(&id).await.unwrap();
        assert!(matches!(reloaded.auth, AuthMethod::IdentityFile { .. }));
    }

    /// Ein Rollback, der fremde Daten entfernt, wäre kein Rollback: Stand
    /// unter dem Slot schon etwas, wird es wiederhergestellt statt
    /// gelöscht.
    #[tokio::test]
    async fn test_a_rollback_restores_a_pre_existing_slot_value_instead_of_deleting_it() {
        let id = ServerId::new();
        let profile_store = InMemoryProfileStore::new()
            .with_server(identity_server(id, None))
            .with_failing_update_server();
        let credential_store =
            InMemoryCredentialStore::new().with_secret(&key_slot(id), "etwas-das-schon-da-war");
        let key_files = MockKeyFileReader::new().with_key(PATH, "the-key-bytes");

        let _ = convert_identity_file_to_keychain(
            &profile_store,
            &credential_store,
            AVAILABLE,
            &key_files,
            id,
        )
        .await;

        assert_eq!(
            stored(&credential_store, &key_slot(id)).as_deref(),
            Some("etwas-das-schon-da-war")
        );
    }

    /// **spec-reviewer-Fund:** Kann der Schlüsselbund beim Lesen des
    /// Ziel-Slots nicht antworten, wird **vor** dem Schreiben abgebrochen.
    ///
    /// Vorher stand hier `…get(&key_ref).ok()`, was „kann nicht antworten"
    /// mit „ist leer" gleichsetzte — ein späterer Rollback hätte dann
    /// gelöscht statt wiederhergestellt. Wer nicht weiß, was er
    /// überschreibt, überschreibt nichts.
    #[tokio::test]
    async fn test_an_unreadable_target_slot_aborts_before_anything_is_written() {
        let id = ServerId::new();
        let profile_store = InMemoryProfileStore::new().with_server(identity_server(id, None));
        let credential_store = InMemoryCredentialStore::new()
            .with_secret(&key_slot(id), "etwas-das-schon-da-war")
            .with_failing_get();
        let key_files = MockKeyFileReader::new().with_key(PATH, "the-key-bytes");

        convert_identity_file_to_keychain(
            &profile_store,
            &credential_store,
            AVAILABLE,
            &key_files,
            id,
        )
        .await
        .expect_err("ein nicht antwortender Schlüsselbund ist kein „Slot ist leer“");

        assert!(
            matches!(
                profile_store.get_server(&id).await.unwrap().auth,
                AuthMethod::IdentityFile { .. }
            ),
            "der Server bleibt unverändert"
        );
    }

    /// **spec-reviewer-Fund (C-6):** Scheitert der Rollback selbst, bleibt
    /// ein privater Schlüssel im Schlüsselbund liegen, auf den kein Server
    /// zeigt. Das ist der halbe Zustand, den C-6 ausschließt — und der
    /// Nutzer muss davon erfahren, statt nur „Speichern fehlgeschlagen" zu
    /// lesen (dieselbe Linie wie Spec 0071, A17).
    ///
    /// Der Test scheitert, sobald der Rückstand wieder nur ins Log wandert.
    #[tokio::test]
    async fn test_a_failed_rollback_tells_the_user_what_stayed_behind() {
        let id = ServerId::new();
        let profile_store = InMemoryProfileStore::new()
            .with_server(identity_server(id, None))
            .with_failing_update_server();
        let credential_store = InMemoryCredentialStore::new().with_failing_delete();
        let key_files = MockKeyFileReader::new().with_key(PATH, "the-key-bytes");

        let err = convert_identity_file_to_keychain(
            &profile_store,
            &credential_store,
            AVAILABLE,
            &key_files,
            id,
        )
        .await
        .expect_err("das Speichern schlägt fehl");

        assert!(
            err.message.contains(key_slot(id).as_str()),
            "die Meldung muss den liegen gebliebenen Eintrag benennen: {}",
            err.message
        );
        // Eigener Code — **nicht** `KEYCHAIN_UNAVAILABLE`. Das Frontend
        // ersetzt bei einem bekannten Code die Meldung durch seinen
        // eigenen Text; mit dem Schlüsselbund-Code ginge genau die
        // Auskunft verloren, die dieser Pfad transportieren soll.
        assert_eq!(
            err.code,
            Some(crate::error::IDENTITY_FILE_ROLLBACK_LEFT_KEY_BEHIND)
        );
        assert_ne!(err.code, Some(crate::error::KEYCHAIN_UNAVAILABLE));
        // Die ursprüngliche Ursache bleibt erhalten, statt vom
        // Rollback-Problem verdrängt zu werden.
        assert!(
            err.message.contains("simulierter DB-Fehler"),
            "der eigentliche Grund darf nicht verloren gehen: {}",
            err.message
        );
        // Der Test taugt nur, wenn der Eintrag tatsächlich stehen bleibt.
        assert_eq!(
            stored(&credential_store, &key_slot(id)).as_deref(),
            Some("the-key-bytes")
        );
    }

    /// C-7/A-6: Ist die Datei nicht lesbar, sagt der Fehler warum — mit
    /// Pfad und stabilem Code, und ohne dass irgendetwas geschrieben wurde.
    #[tokio::test]
    async fn test_an_unreadable_file_reports_the_reason_and_writes_nothing() {
        let id = ServerId::new();
        let profile_store = InMemoryProfileStore::new().with_server(identity_server(id, None));
        let credential_store = InMemoryCredentialStore::new();
        // Die Attrappe kennt den Pfad nicht — also „nicht gefunden“.
        let key_files = MockKeyFileReader::new();

        let err = convert_identity_file_to_keychain(
            &profile_store,
            &credential_store,
            AVAILABLE,
            &key_files,
            id,
        )
        .await
        .expect_err("eine fehlende Datei lässt sich nicht übernehmen");

        assert_eq!(err.code, Some("KEY_FILE_NOT_FOUND"));
        assert!(err.message.contains(PATH), "{}", err.message);
        assert_eq!(stored(&credential_store, &key_slot(id)), None);
        assert!(matches!(
            profile_store.get_server(&id).await.unwrap().auth,
            AuthMethod::IdentityFile { .. }
        ));
    }

    /// Ein Server, der sich gar nicht mit einer Schlüsseldatei anmeldet,
    /// wird abgewiesen — das Kommando verlässt sich nicht darauf, dass die
    /// Oberfläche den Knopf versteckt.
    #[tokio::test]
    async fn test_a_server_without_an_identity_file_is_rejected() {
        let id = ServerId::new();
        let profile_store =
            InMemoryProfileStore::new().with_server(server_with(id, AuthMethod::Agent));
        let credential_store = InMemoryCredentialStore::new();
        let key_files = MockKeyFileReader::new().with_key(PATH, "the-key-bytes");

        let err = convert_identity_file_to_keychain(
            &profile_store,
            &credential_store,
            AVAILABLE,
            &key_files,
            id,
        )
        .await
        .expect_err("ohne Schlüsseldatei gibt es nichts zu übernehmen");

        assert_eq!(err.code, Some(NOT_AN_IDENTITY_FILE));
        assert!(
            key_files.calls().is_empty(),
            "ohne Schlüsseldatei wird auch keine gelesen"
        );
    }

    /// **§6.3.2/C-7: Der Vorab-Befund benutzt `inspect`, nicht `read`.**
    ///
    /// Der Test scheitert, sobald jemand ihn auf `read` umstellt — dann
    /// ginge Schlüsselmaterial durch eine Stelle, die es nicht braucht.
    #[test]
    fn test_the_preflight_check_uses_inspect_and_never_read() {
        let key_files = MockKeyFileReader::new().with_file(
            PATH,
            MockKeyFile {
                content: ENCRYPTED_KEY_TEXT.to_string(),
                encrypted: true,
                permissions_too_open: true,
            },
        );

        let facts = inspect_key_file(&key_files, PATH);

        assert_eq!(key_files.calls(), vec![format!("inspect {PATH}")]);
        assert_eq!(key_files.read_calls(), 0);
        assert!(facts.exists);
        assert!(facts.permissions_too_open, "B-3: der Mangel wird gemeldet");
        assert!(facts.valid_key);
        assert!(facts.encrypted);
        assert_eq!(facts.problem, None, "B-3: ein Mangel hindert nichts");

        // Und das Ergebnis geht als JSON ans Frontend — ohne Dateiinhalt.
        let json = serde_json::to_string(&facts).unwrap();
        assert!(!json.contains("OPENSSH"), "5.2: kein Dateiinhalt — {json}");
    }

    /// **§6.4.4/C-5: Die Überführung lässt die Ursprungsdatei in Ruhe.**
    ///
    /// Läuft gegen eine **echte** Datei und den echten
    /// [`crate::key_files::OsKeyFileReader`], nicht gegen die Attrappe —
    /// eine Attrappe kann per Konstruktion keine Datei verändern und würde
    /// hier also nichts beweisen.
    ///
    /// Geprüft werden Inhalt, Rechte und Änderungszeitpunkt vorher und
    /// nachher. Die Datei bleibt liegen, wie sie war; ein Angebot, sie zu
    /// löschen, ist ausdrücklich nicht Teil dieser Spec (E-4).
    #[tokio::test]
    async fn test_t6_4_4_converting_leaves_the_source_file_untouched() {
        use ssh_transport::test_keys;

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("id_ed25519");
        let key_text = test_keys::encrypted_private_key("correct horse");
        std::fs::write(&file, &key_text).unwrap();
        set_mode(&file, 0o644);

        let before = std::fs::metadata(&file).unwrap();
        let before_modified = before.modified().unwrap();

        let id = ServerId::new();
        let profile_store = InMemoryProfileStore::new().with_server(server_with(
            id,
            AuthMethod::IdentityFile {
                path: file.to_str().unwrap().to_string(),
                passphrase_ref: None,
            },
        ));
        let credential_store = InMemoryCredentialStore::new();

        convert_identity_file_to_keychain(
            &profile_store,
            &credential_store,
            AVAILABLE,
            &crate::key_files::OsKeyFileReader::new(),
            id,
        )
        .await
        .expect("§6.3.2b: auch eine 0644-Datei lässt sich übernehmen");

        let after = std::fs::metadata(&file).unwrap();
        assert_eq!(
            std::fs::read(&file).unwrap(),
            key_text.as_bytes(),
            "C-5: der Inhalt der Ursprungsdatei bleibt unverändert"
        );
        assert_eq!(
            mode_of(&before),
            mode_of(&after),
            "C-5: auch die Rechte werden nicht angefasst"
        );
        assert_eq!(
            before_modified,
            after.modified().unwrap(),
            "C-5: die Datei wird gelesen, nicht geschrieben"
        );

        // Und der Wert im Schlüsselbund ist byte-gleich mit ihr (§6.4.5).
        assert_eq!(
            stored(&credential_store, &key_slot(id)).as_deref(),
            Some(key_text.as_str())
        );
    }

    #[cfg(unix)]
    fn set_mode(path: &std::path::Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[cfg(not(unix))]
    fn set_mode(_path: &std::path::Path, _mode: u32) {}

    #[cfg(unix)]
    fn mode_of(meta: &std::fs::Metadata) -> u32 {
        use std::os::unix::fs::MetadataExt;
        meta.mode()
    }

    #[cfg(not(unix))]
    fn mode_of(meta: &std::fs::Metadata) -> u32 {
        let _ = meta;
        0
    }

    /// Gegenstück: Bei einem Problem trägt der Befund Code und Text — und
    /// weiterhin keinen Inhalt.
    #[test]
    fn test_a_problem_is_reported_with_a_stable_code() {
        let key_files = MockKeyFileReader::new().with_error(
            PATH,
            KeyFileError::PathNotAbsolute {
                path: PATH.to_string(),
            },
        );

        let facts = inspect_key_file(&key_files, PATH);

        let problem = facts.problem.expect("ein Problem war gestellt");
        assert_eq!(problem.code, "KEY_FILE_PATH_NOT_ABSOLUTE");
        assert!(problem.message.contains(PATH));
        assert!(!facts.valid_key);
        assert!(!facts.exists);
    }
}
