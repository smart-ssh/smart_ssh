//! Übersetzung zwischen [`AuthMethodInput`] (Formulardaten) und
//! [`AuthMethod`] (persistierte Form mit `CredentialRef`s) — Spec 0008,
//! Abschnitt 4: "Backend schreibt Secret-Inhalte über
//! `CredentialStore::set()` **vor** dem Schreiben der restlichen Felder in
//! die DB", "`None`/leer bei `update_server` bedeutet unverändert lassen".
//! Dieselbe Konvention wie beim AI-Provider (Spec 0007, Abschnitt 8.2),
//! hier aber über bis zu zwei Secret-Slots pro Methode (Key+Passphrase,
//! Zertifikat+Key) statt nur einem.

use secrecy::SecretString;

use ssh_manager_core::profiles::{AuthMethod, CredentialError, CredentialRef, CredentialStore};
use ssh_manager_core::shared::ServerId;

use credentials_keyring::KeychainAvailability;

use crate::dto::AuthMethodInput;
use crate::error::{keychain_aware_credential_error, CommandError};

/// Deterministischer `CredentialRef` pro `(server_id, slot)` — kein
/// zusätzlicher Zustand nötig, um sich "den Ref von vorhin" zu merken; bei
/// `update_server` wird derselbe String einfach erneut berechnet.
fn credential_ref(server_id: ServerId, slot: &str) -> CredentialRef {
    CredentialRef::new(format!("server:{}:{slot}", server_id.0))
}

/// Spec 0018, Abschnitt 4: eigener, vom Login-Auth-Secret unabhängiger Slot
/// — ein Server hat unabhängig von seiner `AuthMethod` (Passwort/Key/Agent/
/// Zertifikat) höchstens **ein** optionales Sudo-Passwort. Deterministisch
/// wie die übrigen Slots: kein eigenes DB-Feld nötig, "ist eines
/// hinterlegt" wird per `CredentialStore::get(...).is_ok()` ermittelt (s.
/// `crate::dto::ServerDto::has_sudo_password`).
pub fn sudo_password_credential_ref(server_id: ServerId) -> CredentialRef {
    credential_ref(server_id, "sudo_password")
}

/// Spec 0018, Abschnitt 4: "leer = unverändert" wie bei den Login-Auth-
/// Secrets (Abschnitt 3 dieses Moduls) — anders als dort ist ein fehlender
/// Wert aber ein **gültiger** Endzustand (kein Sudo-Passwort hinterlegt),
/// kein Fehler wie bei `write_or_reuse_secret` (die verlangt zwingend
/// *irgendeinen* Wert für ein Pflichtfeld). `provided: Some("")` wird wie
/// `None` behandelt (Formularfelder liefern bei "nichts eingegeben" einen
/// leeren String, keinen fehlenden Wert).
///
/// Spec 0049, Fund 1: `provided` wird zuerst rand-getrimmt (führende/
/// nachfolgende `\r`/`\n`/Tabs/Leerzeichen — z. B. von einem Windows-
/// Copy-Paste), **bevor** die Leer-Prüfung läuft. Ohne diese Reihenfolge
/// würde ein rein aus Whitespace bestehender Paste die `!value.is_empty()`-
/// Prüfung fälschlich bestehen und ein leeres/Whitespace-Secret
/// überschreiben, statt (wie ein echtes Leerfeld) als "unverändert" zu
/// gelten.
///
/// Spec 0071, A13: `keychain` wird nur durchgereicht, um einem
/// Schreibfehler bei nicht verfügbarem Schlüsselbund den stabilen Code
/// `KEYCHAIN_UNAVAILABLE` zu geben (s. `error::keychain_aware_credential_
/// error`). Der Zustand kommt aus dem `AppState` (A16) — hier wird nichts
/// zusätzlich abgefragt, und am Schreibverhalten selbst ändert sich nichts:
/// Der Fehler wird unverändert weitergereicht, nie verschluckt (X6).
pub fn resolve_sudo_password(
    credential_store: &dyn CredentialStore,
    keychain: KeychainAvailability,
    server_id: ServerId,
    provided: Option<String>,
) -> Result<(), CommandError> {
    match provided.map(|value| value.trim().to_string()) {
        Some(value) if !value.is_empty() => {
            credential_store
                .set(
                    &sudo_password_credential_ref(server_id),
                    SecretString::from(value),
                )
                .map_err(|err| keychain_aware_credential_error(err, keychain))?;
        }
        _ => {}
    }
    Ok(())
}

/// Ein einzelner `delete` auf einem **vom Nutzer ausgelösten**
/// Entfernen-Pfad (Spec 0071, A17 und die X6-Korrektur vom 2026-09-22).
///
/// Liefert den `CredentialRef` zurück, wenn das Secret **nicht** entfernt
/// werden konnte und damit im Schlüsselbund stehen bleibt. `NotFound` gilt
/// als Erfolg (es gab nichts zu löschen) — dieselbe Idempotenz wie in
/// `KeyringCredentialStore::delete`.
///
/// In jedem Fehlerfall zusätzlich eine Logzeile: Der Zustand darf nicht
/// spurlos verschwinden, auch wenn der Aufrufer ihn (bei `delete_server`,
/// s. A17) bewusst nicht zum Abbruch nimmt.
fn delete_user_requested_secret(
    credential_store: &dyn CredentialStore,
    r: &CredentialRef,
) -> Option<CredentialRef> {
    match credential_store.delete(r) {
        Ok(()) | Err(CredentialError::NotFound(_)) => None,
        Err(err) => {
            tracing::warn!(
                credential_ref = %r.as_str(),
                error = %err,
                "vom Nutzer angefordertes Entfernen eines Secrets ist fehlgeschlagen — \
                 der Eintrag bleibt im Schlüsselbund"
            );
            Some(r.clone())
        }
    }
}

/// Explizites Entfernen (Spec 0018, Abschnitt 4) — "Feld leer lassen"
/// bedeutet bereits "unverändert" (s. [`resolve_sudo_password`]), ein
/// einmal gesetztes Sudo-Passwort braucht daher einen eigenen Weg, um es
/// wieder zu löschen. Ein bereits fehlender Eintrag ist kein Fehler.
///
/// Spec 0071, A17: **schlägt sichtbar fehl**, wenn das Sudo-Passwort nicht
/// entfernt werden konnte.
///
/// Begründung (A17, erster Punkt): Anders als beim Löschen eines Servers
/// ist hier sonst *nichts* geschehen — es gibt keinen Teilerfolg, den man
/// melden könnte. Ein `Ok(())` wäre schlicht unwahr: Der Nutzer sähe „kein
/// Sudo-Passwort hinterlegt", während das Passwort weiter im
/// Schlüsselbund liegt und beim nächsten `sudo` wieder eingespeist würde.
///
/// Der Fehlerweg ist derselbe wie überall sonst (A13): bei nicht
/// verfügbarem Schlüsselbund der stabile Code `KEYCHAIN_UNAVAILABLE`,
/// vom Frontend übersetzt.
pub fn clear_sudo_password(
    credential_store: &dyn CredentialStore,
    server_id: ServerId,
) -> Result<(), CommandError> {
    let r = sudo_password_credential_ref(server_id);
    match credential_store.delete(&r) {
        Ok(()) | Err(CredentialError::NotFound(_)) => Ok(()),
        Err(err) => {
            tracing::warn!(
                server_id = %server_id.0,
                slot = "sudo_password",
                error = %err,
                "Sudo-Passwort konnte nicht entfernt werden — der Eintrag bleibt im Schlüsselbund"
            );
            // A17: „Der Fehler nutzt denselben Weg wie A13
            // (`KEYCHAIN_UNAVAILABLE`, übersetzt)" — hier **unbedingt**,
            // nicht abhängig vom Startzustand aus dem `AppState`.
            //
            // spec-reviewer-Fund: `keychain_aware_credential_error` hängt
            // den Code nur an, wenn der Schlüsselbund schon beim Start als
            // nicht verfügbar erkannt wurde. Klemmt er erst danach (der
            // Anbieter sperrt sich während der Sitzung, ein Entsperr-Prompt
            // wird abgebrochen), stünde auf diesem — durch A17 überhaupt
            // erst entstandenen — Fehlerpfad der rohe englische
            // Bibliothekstext im UI. Genau das schließt §2 aus.
            //
            // Hier ist die unbedingte Zuordnung auch sachlich richtig: Ein
            // `Backend`-Fehler auf einem `delete` hat keine andere Ursache
            // als einen Schlüsselbund, der nicht tut, was er soll. Der
            // Code ersetzt den Text, er ergänzt ihn nicht (X2).
            Err(CommandError::with_code(
                "Der Systemschlüsselbund ist nicht verfügbar.",
                crate::error::KEYCHAIN_UNAVAILABLE,
            ))
        }
    }
}

/// Schreibt `provided` unter `ref_`, falls gesetzt; ist `provided` leer
/// und der Slot existierte bereits (Update, unverändert), passiert nichts
/// — der alte Wert bleibt unter demselben `ref_` stehen. Existierte der
/// Slot nicht (Neuanlage, oder Methodenwechsel bei Update) und `provided`
/// ist leer, ist das ein Fehler: es gibt keinen "alten Wert", der
/// übernommen werden könnte.
///
/// Spec 0049, Fund 1: `provided` wird zuerst rand-getrimmt. Das deckt
/// zwei Fälle: (a) der eigentliche Fund — ein Windows-Copy-Paste mit
/// angehängtem `\r\n` wird nicht ungetrimmt gespeichert; (b) macht diesen
/// Doc-Kommentar erst wahr — bislang prüfte der Code unten `Some(value)`
/// unabhängig vom Inhalt, ein rein aus Whitespace bestehender Paste (oder
/// schlicht ein leerer String bei Neuanlage) wurde also als "gültiger
/// Wert" durchgereicht und gespeichert, statt wie ein echtes Leerfeld
/// "unverändert lassen"/"Pflichtfeld fehlt" auszulösen.
fn write_or_reuse_secret(
    credential_store: &dyn CredentialStore,
    keychain: KeychainAvailability,
    ref_: &CredentialRef,
    provided: Option<String>,
    previously_existed: bool,
    label: &str,
    code: &'static str,
) -> Result<(), CommandError> {
    match provided.map(|value| value.trim().to_string()) {
        Some(value) if !value.is_empty() => {
            credential_store
                .set(ref_, SecretString::from(value))
                .map_err(|err| keychain_aware_credential_error(err, keychain))?;
            Ok(())
        }
        _ if previously_existed => Ok(()),
        _ => Err(CommandError::with_code(
            format!("{label} ist erforderlich"),
            code,
        )),
    }
}

/// Räumt Secret-Slots einer **anderen** Auth-Methode auf, wenn `input`
/// eine andere Art als `existing` wählt — sonst blieben z. B. beim
/// Wechsel von `PrivateKey` zu `Agent` ein verwaistes
/// `server:{id}:private_key`/`server:{id}:passphrase` im Keychain zurück,
/// auf das kein `AuthMethod` mehr verweist. Best-effort (Fehler beim
/// Aufräumen sind nicht kritisch genug, den ganzen `update_server`-Aufruf
/// scheitern zu lassen).
fn cleanup_abandoned_slots(
    credential_store: &dyn CredentialStore,
    existing: Option<&AuthMethod>,
    input: &AuthMethodInput,
) {
    let Some(existing) = existing else { return };
    let same_kind = matches!(
        (existing, input),
        (
            AuthMethod::Password { .. },
            AuthMethodInput::Password { .. }
        ) | (
            AuthMethod::PrivateKey { .. },
            AuthMethodInput::PrivateKey { .. }
        ) | (AuthMethod::Agent, AuthMethodInput::Agent)
            | (
                AuthMethod::Certificate { .. },
                AuthMethodInput::Certificate { .. }
            )
    );
    if same_kind {
        return;
    }
    let abandoned: Vec<&CredentialRef> = match existing {
        AuthMethod::Password { credential_ref } => vec![credential_ref],
        AuthMethod::PrivateKey {
            credential_ref,
            passphrase_ref,
        } => {
            let mut refs = vec![credential_ref];
            refs.extend(passphrase_ref.iter());
            refs
        }
        AuthMethod::Agent => Vec::new(),
        AuthMethod::Certificate { cert_ref, key_ref } => vec![cert_ref, key_ref],
    };
    for r in abandoned {
        let _ = credential_store.delete(r);
    }
}

/// Baut ein [`AuthMethod`] aus `input`, schreibt dabei benötigte Secrets
/// in `credential_store`. `existing` ist `Some(&AuthMethod)` bei
/// `update_server` (für "leer = unverändert" + Aufräumen bei
/// Methodenwechsel), `None` bei `create_server` (dort ist jeder
/// benötigte Slot zwingend, s. [`write_or_reuse_secret`]).
///
/// Spec 0071, A13: `keychain` wird nur für die Fehlerkennzeichnung
/// durchgereicht (s. [`resolve_sudo_password`]).
pub fn resolve_auth_method(
    credential_store: &dyn CredentialStore,
    keychain: KeychainAvailability,
    server_id: ServerId,
    input: AuthMethodInput,
    existing: Option<&AuthMethod>,
) -> Result<AuthMethod, CommandError> {
    cleanup_abandoned_slots(credential_store, existing, &input);

    match input {
        AuthMethodInput::Password { value } => {
            let ref_ = credential_ref(server_id, "password");
            let existed = matches!(existing, Some(AuthMethod::Password { .. }));
            write_or_reuse_secret(
                credential_store,
                keychain,
                &ref_,
                value,
                existed,
                "Passwort",
                "SERVER_PASSWORD_REQUIRED",
            )?;
            Ok(AuthMethod::Password {
                credential_ref: ref_,
            })
        }
        AuthMethodInput::PrivateKey {
            key_content,
            passphrase,
        } => {
            let key_ref = credential_ref(server_id, "private_key");
            let existed_key = matches!(existing, Some(AuthMethod::PrivateKey { .. }));
            write_or_reuse_secret(
                credential_store,
                keychain,
                &key_ref,
                key_content,
                existed_key,
                "Private Key",
                "SERVER_PRIVATE_KEY_REQUIRED",
            )?;

            let existing_passphrase_ref = match existing {
                Some(AuthMethod::PrivateKey {
                    passphrase_ref: Some(r),
                    ..
                }) => Some(r.clone()),
                _ => None,
            };
            // Spec 0049, Fund 1: rand-trimmen, bevor entschieden wird, ob
            // überhaupt eine neue Passphrase vorliegt — sonst würde ein
            // rein aus Whitespace bestehender Paste (kommt von der
            // Frontend-Leer-Prüfung `passphrase === ""` nicht ab, s.
            // `ServerForm.tsx`s `toAuthMethodInput`) fälschlich als "neue
            // Passphrase gesetzt" gewertet, statt wie ein echtes Leerfeld
            // die bestehende Passphrase unverändert zu lassen.
            let passphrase_ref = match passphrase.map(|p| p.trim().to_string()) {
                Some(p) if !p.is_empty() => {
                    let r = credential_ref(server_id, "passphrase");
                    credential_store
                        .set(&r, SecretString::from(p))
                        .map_err(|err| keychain_aware_credential_error(err, keychain))?;
                    Some(r)
                }
                _ => existing_passphrase_ref,
            };
            Ok(AuthMethod::PrivateKey {
                credential_ref: key_ref,
                passphrase_ref,
            })
        }
        AuthMethodInput::Agent => Ok(AuthMethod::Agent),
        AuthMethodInput::Certificate {
            cert_content,
            key_content,
        } => {
            let cert_ref = credential_ref(server_id, "certificate");
            let key_ref = credential_ref(server_id, "certificate_key");
            let existed = matches!(existing, Some(AuthMethod::Certificate { .. }));
            write_or_reuse_secret(
                credential_store,
                keychain,
                &cert_ref,
                cert_content,
                existed,
                "Zertifikat",
                "SERVER_CERTIFICATE_REQUIRED",
            )?;
            write_or_reuse_secret(
                credential_store,
                keychain,
                &key_ref,
                key_content,
                existed,
                "Zertifikats-Key",
                "SERVER_CERTIFICATE_KEY_REQUIRED",
            )?;
            Ok(AuthMethod::Certificate { cert_ref, key_ref })
        }
    }
}

/// Löscht alle Secret-Slots einer [`AuthMethod`] — für `delete_server`
/// (Spec 0008, Abschnitt 4 folgt derselben "CredentialStore zuerst"-
/// Konvention wie `delete_ai_provider` in Spec 0007). Best-effort: ein
/// fehlender/schon gelöschter Eintrag soll `delete_server` nicht
/// scheitern lassen.
///
/// Spec 0071, A17 (zweiter Punkt): Das Löschen **läuft durch**, auch wenn
/// ein Secret nicht entfernt werden konnte — niemand soll auf einem
/// unlöschbaren Server sitzen bleiben, nur weil der Schlüsselbund klemmt.
/// Anders als bei [`clear_sudo_password`] gibt es hier einen echten
/// Teilerfolg (das Profil ist weg), der sich melden lässt.
///
/// Gibt deshalb die Refs zurück, die **stehen geblieben** sind. Der
/// Aufrufer reicht sie im Ergebnis ans Frontend weiter, damit der Nutzer
/// erfährt, dass die Einträge im Schlüsselbund verwaist sind (die
/// Server-ID existiert nicht mehr) und von Hand gelöscht werden können.
/// Sie stillschweigend zu verschlucken wäre das falsche Erfolgssignal aus
/// der X6-Korrektur.
#[must_use]
pub fn delete_auth_method_secrets(
    credential_store: &dyn CredentialStore,
    auth: &AuthMethod,
) -> Vec<CredentialRef> {
    let refs: Vec<&CredentialRef> = match auth {
        AuthMethod::Password { credential_ref } => vec![credential_ref],
        AuthMethod::PrivateKey {
            credential_ref,
            passphrase_ref,
        } => {
            let mut refs = vec![credential_ref];
            refs.extend(passphrase_ref.iter());
            refs
        }
        AuthMethod::Agent => Vec::new(),
        AuthMethod::Certificate { cert_ref, key_ref } => vec![cert_ref, key_ref],
    };
    refs.into_iter()
        .filter_map(|r| delete_user_requested_secret(credential_store, r))
        .collect()
}

/// Spec 0071, A17: das Sudo-Passwort auf dem **Server-Löschen**-Pfad —
/// dort gilt „durchlaufen und melden", nicht „sichtbar scheitern" (s.
/// [`delete_auth_method_secrets`]). Bewusst getrennt von
/// [`clear_sudo_password`], damit die beiden Entscheidungen aus A17 nicht
/// über einen Parameter vermischt werden und ein künftiger Aufrufer nicht
/// versehentlich die falsche Hälfte bekommt.
#[must_use]
pub fn delete_sudo_password_on_server_delete(
    credential_store: &dyn CredentialStore,
    server_id: ServerId,
) -> Vec<CredentialRef> {
    delete_user_requested_secret(credential_store, &sudo_password_credential_ref(server_id))
        .into_iter()
        .collect()
}

/// Spec 0047, Fund A2: räumt bei einem fehlgeschlagenen `create_server`
/// **alle** Keychain-Slots ab, die dieser Aufruf potenziell beschrieben
/// haben könnte — unabhängig davon, an welcher Stelle genau der Fehler
/// auftrat (`resolve_auth_method` selbst kann bei `PrivateKey`/
/// `Certificate` bereits den ersten Slot geschrieben haben, bevor der
/// zweite fehlschlägt, s. dortiger Kommentar; ebenso kann das
/// Sudo-Passwort vor einem späteren DB-Fehler bereits gestanden haben).
/// Sicher, weil `server_id` bei `create_server` immer frisch erzeugt wird
/// (`ServerId::new()`) — unter dieser ID kann nichts Legitimes stehen
/// außer dem, was genau dieser (fehlgeschlagene) Aufruf selbst geschrieben
/// hat. Best-effort wie `delete_auth_method_secrets`: ein bereits
/// fehlender Slot ist kein Fehler.
pub fn delete_all_possible_server_secrets(
    credential_store: &dyn CredentialStore,
    server_id: ServerId,
) {
    for slot in [
        "password",
        "private_key",
        "passphrase",
        "certificate",
        "certificate_key",
        "sudo_password",
    ] {
        // Spec-Reviewer-Fund (Spec 0047, Fund A2): ein fehlendes `NotFound`
        // ist erwartet (der Slot wurde nie geschrieben) und bleibt still.
        // Ein `Backend`-Fehler (z. B. gesperrte/verweigerte macOS-Keychain
        // während des Rollbacks selbst) ließ einen orphaned Eintrag bisher
        // komplett spurlos zurück — das kollidiert mit der B1/B2-Invariante
        // dieser selben Spec ("nie spurlos"). Kein Fehler-Return hier (das
        // Rollback bleibt best-effort, s. Doc-Kommentar oben), aber
        // mindestens eine Logzeile.
        if let Err(CredentialError::Backend(msg)) =
            credential_store.delete(&credential_ref(server_id, slot))
        {
            tracing::warn!(
                server_id = %server_id.0,
                slot,
                error = %msg,
                "Keychain-Rollback fehlgeschlagen: möglicherweise verwaister Eintrag"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec 0071: In diesen Tests geht es nicht um die Verfügbarkeit des
    /// Schlüsselbunds, sondern um das Schreibverhalten — der In-Memory-Store
    /// ist per Definition verfügbar.
    const AVAILABLE: KeychainAvailability = KeychainAvailability::Available;
    use crate::test_support::InMemoryCredentialStore;

    fn secret_value(store: &InMemoryCredentialStore, r: &CredentialRef) -> Option<String> {
        use secrecy::ExposeSecret;
        store.get(r).ok().map(|s| s.expose_secret().to_string())
    }

    #[test]
    fn test_create_password_requires_value() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        let result = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::Password { value: None },
            None,
        );

        let err = result.expect_err("erwartet: Fehler bei fehlendem Passwort");
        // Spec 0024, Abschnitt 5: stabiler Code fürs Frontend-Mapping.
        assert_eq!(err.code, Some("SERVER_PASSWORD_REQUIRED"));
    }

    #[test]
    fn test_create_private_key_requires_key_content() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        let result = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::PrivateKey {
                key_content: None,
                passphrase: None,
            },
            None,
        );

        let err = result.expect_err("erwartet: Fehler bei fehlendem Private Key");
        assert_eq!(err.code, Some("SERVER_PRIVATE_KEY_REQUIRED"));
    }

    #[test]
    fn test_create_certificate_requires_cert_content() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        let result = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::Certificate {
                cert_content: None,
                key_content: Some("key".to_string()),
            },
            None,
        );

        let err = result.expect_err("erwartet: Fehler bei fehlendem Zertifikat");
        assert_eq!(err.code, Some("SERVER_CERTIFICATE_REQUIRED"));
    }

    #[test]
    fn test_create_password_with_value_succeeds_and_stores_secret() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        let auth = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::Password {
                value: Some("hunter2".to_string()),
            },
            None,
        )
        .unwrap();

        let AuthMethod::Password { credential_ref } = &auth else {
            panic!("erwartete AuthMethod::Password");
        };
        assert_eq!(
            secret_value(&store, credential_ref).as_deref(),
            Some("hunter2")
        );
    }

    #[test]
    fn test_create_private_key_without_passphrase_leaves_passphrase_ref_none() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        let auth = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::PrivateKey {
                key_content: Some("-----BEGIN KEY-----".to_string()),
                passphrase: None,
            },
            None,
        )
        .unwrap();

        let AuthMethod::PrivateKey { passphrase_ref, .. } = &auth else {
            panic!("erwartete AuthMethod::PrivateKey");
        };
        assert!(passphrase_ref.is_none());
    }

    #[test]
    fn test_create_certificate_requires_both_fields() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        let result = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::Certificate {
                cert_content: Some("cert".to_string()),
                key_content: None,
            },
            None,
        );

        let err = result.expect_err("erwartet: Fehler bei fehlendem Zertifikats-Key");
        assert_eq!(err.code, Some("SERVER_CERTIFICATE_KEY_REQUIRED"));
    }

    #[test]
    fn test_create_agent_needs_no_secret() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        let auth =
            resolve_auth_method(&store, AVAILABLE, id, AuthMethodInput::Agent, None).unwrap();

        assert!(matches!(auth, AuthMethod::Agent));
    }

    #[test]
    fn test_update_with_empty_value_reuses_existing_secret_unchanged() {
        let id = ServerId::new();
        let existing_ref = credential_ref(id, "password");
        let store = InMemoryCredentialStore::new().with_secret(&existing_ref, "old-password");
        let existing = AuthMethod::Password {
            credential_ref: existing_ref.clone(),
        };

        let auth = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::Password { value: None },
            Some(&existing),
        )
        .unwrap();

        let AuthMethod::Password { credential_ref } = &auth else {
            panic!("erwartete AuthMethod::Password");
        };
        assert_eq!(credential_ref, &existing_ref);
        assert_eq!(
            secret_value(&store, credential_ref).as_deref(),
            Some("old-password")
        );
    }

    #[test]
    fn test_update_kind_change_cleans_up_abandoned_slot() {
        let id = ServerId::new();
        let old_ref = credential_ref(id, "password");
        let store = InMemoryCredentialStore::new().with_secret(&old_ref, "old-password");
        let existing = AuthMethod::Password {
            credential_ref: old_ref.clone(),
        };

        let auth = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::Agent,
            Some(&existing),
        )
        .unwrap();

        assert!(matches!(auth, AuthMethod::Agent));
        assert!(
            secret_value(&store, &old_ref).is_none(),
            "verwaister Password-Slot muss aufgeräumt werden"
        );
    }

    #[test]
    fn test_delete_auth_method_secrets_removes_private_key_and_passphrase() {
        let id = ServerId::new();
        let key_ref = credential_ref(id, "private_key");
        let passphrase_ref = credential_ref(id, "passphrase");
        let store = InMemoryCredentialStore::new()
            .with_secret(&key_ref, "key")
            .with_secret(&passphrase_ref, "phrase");
        let auth = AuthMethod::PrivateKey {
            credential_ref: key_ref.clone(),
            passphrase_ref: Some(passphrase_ref.clone()),
        };

        let left_behind = delete_auth_method_secrets(&store, &auth);
        assert!(
            left_behind.is_empty(),
            "ein funktionierender Store lässt nichts zurück"
        );

        assert!(secret_value(&store, &key_ref).is_none());
        assert!(secret_value(&store, &passphrase_ref).is_none());
    }

    // --- Spec 0018: Sudo-Passwort ------------------------------------------

    #[test]
    fn test_resolve_sudo_password_stores_provided_value() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        resolve_sudo_password(&store, AVAILABLE, id, Some("hunter2".to_string())).unwrap();

        assert_eq!(
            secret_value(&store, &sudo_password_credential_ref(id)).as_deref(),
            Some("hunter2")
        );
    }

    #[test]
    fn test_resolve_sudo_password_none_or_empty_leaves_nothing_stored() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        resolve_sudo_password(&store, AVAILABLE, id, None).unwrap();
        resolve_sudo_password(&store, AVAILABLE, id, Some(String::new())).unwrap();

        assert!(secret_value(&store, &sudo_password_credential_ref(id)).is_none());
    }

    #[test]
    fn test_resolve_sudo_password_empty_value_on_update_leaves_existing_unchanged() {
        let id = ServerId::new();
        let store = InMemoryCredentialStore::new()
            .with_secret(&sudo_password_credential_ref(id), "old-password");

        // Leeres Feld bei "update" bedeutet unverändert (Spec 0018,
        // Abschnitt 4) — kein Löschen, kein Überschreiben.
        resolve_sudo_password(&store, AVAILABLE, id, Some(String::new())).unwrap();

        assert_eq!(
            secret_value(&store, &sudo_password_credential_ref(id)).as_deref(),
            Some("old-password")
        );
    }

    #[test]
    fn test_clear_sudo_password_removes_stored_value() {
        let id = ServerId::new();
        let store = InMemoryCredentialStore::new()
            .with_secret(&sudo_password_credential_ref(id), "hunter2");

        clear_sudo_password(&store, id).unwrap();

        assert!(secret_value(&store, &sudo_password_credential_ref(id)).is_none());
    }

    #[test]
    fn test_clear_sudo_password_on_already_missing_entry_does_not_panic() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        clear_sudo_password(&store, id).unwrap();
    }

    // --- Spec 0049, Fund 1: Rand-Trimmen (Windows-Copy-Paste-`\r\n`) -------

    #[test]
    fn test_password_with_trailing_crlf_is_stored_trimmed() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        let auth = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::Password {
                value: Some("hunter2\r\n".to_string()),
            },
            None,
        )
        .unwrap();

        let AuthMethod::Password { credential_ref } = &auth else {
            panic!("erwartete AuthMethod::Password");
        };
        assert_eq!(
            secret_value(&store, credential_ref).as_deref(),
            Some("hunter2"),
            "angehängtes \\r\\n muss beim Speichern getrimmt werden"
        );
    }

    #[test]
    fn test_password_with_leading_and_trailing_whitespace_is_trimmed_but_interior_kept() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        let auth = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::Password {
                value: Some("  hunter two \t".to_string()),
            },
            None,
        )
        .unwrap();

        let AuthMethod::Password { credential_ref } = &auth else {
            panic!("erwartete AuthMethod::Password");
        };
        assert_eq!(
            secret_value(&store, credential_ref).as_deref(),
            Some("hunter two"),
            "nur der Rand wird getrimmt, das innenliegende Leerzeichen bleibt erhalten"
        );
    }

    #[test]
    fn test_password_of_only_whitespace_on_create_is_rejected_as_missing() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        let result = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::Password {
                value: Some("  \r\n\t ".to_string()),
            },
            None,
        );

        let err = result.expect_err("ein rein aus Whitespace bestehender Paste ist kein Passwort");
        assert_eq!(err.code, Some("SERVER_PASSWORD_REQUIRED"));
    }

    #[test]
    fn test_password_of_only_whitespace_on_update_leaves_existing_secret_unchanged() {
        let id = ServerId::new();
        let existing_ref = credential_ref(id, "password");
        let store = InMemoryCredentialStore::new().with_secret(&existing_ref, "old-password");
        let existing = AuthMethod::Password {
            credential_ref: existing_ref.clone(),
        };

        resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::Password {
                value: Some(" \r\n ".to_string()),
            },
            Some(&existing),
        )
        .unwrap();

        assert_eq!(
            secret_value(&store, &existing_ref).as_deref(),
            Some("old-password"),
            "ein Whitespace-Paste bei einem Update darf das bestehende Passwort nicht überschreiben"
        );
    }

    #[test]
    fn test_sudo_password_with_trailing_crlf_is_stored_trimmed() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        resolve_sudo_password(&store, AVAILABLE, id, Some("sudo-secret\r\n".to_string())).unwrap();

        assert_eq!(
            secret_value(&store, &sudo_password_credential_ref(id)).as_deref(),
            Some("sudo-secret")
        );
    }

    #[test]
    fn test_sudo_password_of_only_whitespace_on_update_leaves_existing_unchanged() {
        let id = ServerId::new();
        let store = InMemoryCredentialStore::new()
            .with_secret(&sudo_password_credential_ref(id), "old-sudo-password");

        resolve_sudo_password(&store, AVAILABLE, id, Some("\r\n".to_string())).unwrap();

        assert_eq!(
            secret_value(&store, &sudo_password_credential_ref(id)).as_deref(),
            Some("old-sudo-password")
        );
    }

    /// Spec 0071, A17 (erster Punkt): „Hinterlegtes Sudo-Passwort
    /// entfernen" schlägt **sichtbar** fehl, wenn nichts entfernt werden
    /// konnte. Am Stand vor A17 stand hier `let _ = delete(...)` und die
    /// Funktion gab `()` zurück — die Oberfläche meldete Erfolg und zeigte
    /// „kein Sudo-Passwort hinterlegt", während das Passwort weiter im
    /// Schlüsselbund lag und beim nächsten `sudo` wieder eingespeist
    /// worden wäre.
    ///
    /// Der Code hängt hier **unbedingt** am Fehler, nicht abhängig vom
    /// Schlüsselbund-Zustand beim Start (A17: „denselben Weg wie A13") —
    /// sonst stünde der rohe englische Bibliothekstext im Formular, sobald
    /// der Schlüsselbund erst während der Sitzung klemmt. Der Store in
    /// diesem Test bildet genau das nach: Er ist nicht als „nicht
    /// verfügbar" bekannt, sondern scheitert erst beim `delete`.
    #[test]
    fn test_clearing_a_sudo_password_fails_visibly_when_nothing_was_removed() {
        let id = ServerId::new();
        let store = InMemoryCredentialStore::new()
            .with_secret(&sudo_password_credential_ref(id), "sudo-secret")
            .with_failing_delete();

        let err = clear_sudo_password(&store, id)
            .expect_err("ein fehlgeschlagenes Entfernen darf nicht als Erfolg gelten");

        assert_eq!(err.code, Some(crate::error::KEYCHAIN_UNAVAILABLE));
        assert_eq!(
            secret_value(&store, &sudo_password_credential_ref(id)).as_deref(),
            Some("sudo-secret"),
            "der Test taugt nur, wenn das Secret tatsächlich stehen bleibt"
        );
    }

    /// Gegenprobe: Ohne hinterlegtes Passwort ist „entfernen" weiterhin
    /// erfolgreich — `NotFound` ist kein Fehler (Idempotenz, Spec 0018).
    #[test]
    fn test_clearing_an_absent_sudo_password_still_succeeds() {
        let store = InMemoryCredentialStore::new();
        clear_sudo_password(&store, ServerId::new())
            .expect("kein Eintrag vorhanden ist kein Fehler");
    }

    /// Spec 0071, A17 (zweiter Punkt): Das Löschen eines Servers läuft
    /// **durch**, meldet aber, was im Schlüsselbund zurückblieb. Am Stand
    /// vor A17 gab die Funktion `()` zurück, der Rückstand war nur im Log
    /// sichtbar — das Ergebnis behauptete implizit, alles sei entfernt.
    #[test]
    fn test_deleting_auth_secrets_reports_what_stayed_behind() {
        let id = ServerId::new();
        let key_ref = credential_ref(id, "private_key");
        let passphrase_ref = credential_ref(id, "passphrase");
        let store = InMemoryCredentialStore::new()
            .with_secret(&key_ref, "key")
            .with_secret(&passphrase_ref, "phrase")
            .with_failing_delete();
        let auth = AuthMethod::PrivateKey {
            credential_ref: key_ref.clone(),
            passphrase_ref: Some(passphrase_ref.clone()),
        };

        let left_behind = delete_auth_method_secrets(&store, &auth);

        assert_eq!(left_behind.len(), 2, "beide Slots blieben stehen");
        assert!(left_behind.contains(&key_ref));
        assert!(left_behind.contains(&passphrase_ref));
        assert_eq!(
            secret_value(&store, &key_ref).as_deref(),
            Some("key"),
            "der Test taugt nur, wenn die Secrets tatsächlich stehen bleiben"
        );
    }

    /// A17: dasselbe für das Sudo-Passwort auf dem Lösch-Pfad — hier
    /// **kein** Fehler, sondern ein Eintrag in der Rückstandsliste.
    #[test]
    fn test_deleting_the_sudo_password_on_server_delete_reports_instead_of_failing() {
        let id = ServerId::new();
        let store = InMemoryCredentialStore::new()
            .with_secret(&sudo_password_credential_ref(id), "sudo-secret")
            .with_failing_delete();

        let left_behind = delete_sudo_password_on_server_delete(&store, id);

        assert_eq!(left_behind, vec![sudo_password_credential_ref(id)]);
        assert_eq!(
            secret_value(&store, &sudo_password_credential_ref(id)).as_deref(),
            Some("sudo-secret"),
            "der Test taugt nur, wenn das Secret tatsächlich stehen bleibt"
        );
    }

    /// Spec 0071, A13/X6: Schlägt ein Schreibzugriff fehl, während der
    /// Schlüsselbund als nicht verfügbar bekannt ist, geht der Fehler
    /// weiter (nie ein stiller Erfolg) — und zwar mit dem stabilen Code,
    /// damit das Frontend den übersetzten Text zeigt statt des englischen
    /// Bibliothekstexts.
    #[test]
    fn test_failed_write_reports_the_keychain_code_and_never_silently_succeeds() {
        let unavailable = KeychainAvailability::Unavailable(
            credentials_keyring::KeychainUnavailableReason::NoSecretServiceProvider,
        );
        let store = InMemoryCredentialStore::new().with_failing_set_for_slot("sudo_password");
        let id = ServerId::new();

        let err = resolve_sudo_password(&store, unavailable, id, Some("sudo-secret".to_string()))
            .expect_err("ein fehlgeschlagener Schreibzugriff darf nicht als Erfolg gelten");

        assert_eq!(err.code, Some(crate::error::KEYCHAIN_UNAVAILABLE));
        assert!(
            !err.message.contains("simulierter Keychain-Fehler"),
            "der rohe Backend-Text darf nicht mitgereicht werden: {}",
            err.message
        );
        assert!(
            secret_value(&store, &sudo_password_credential_ref(id)).is_none(),
            "nichts darf gespeichert worden sein"
        );
    }

    /// Gegenprobe zu oben: Derselbe Schreibfehler bei **verfügbarem**
    /// Schlüsselbund ist ein gewöhnlicher Fehler ohne diesen Code — sonst
    /// schickte die Oberfläche den Nutzer wegen eines einzelnen
    /// verweigerten Eintrags zu einer Paketinstallation.
    #[test]
    fn test_failed_write_with_an_available_keychain_keeps_its_ordinary_error() {
        let store = InMemoryCredentialStore::new().with_failing_set_for_slot("sudo_password");
        let id = ServerId::new();

        let err = resolve_sudo_password(&store, AVAILABLE, id, Some("sudo-secret".to_string()))
            .expect_err("ein fehlgeschlagener Schreibzugriff darf nicht als Erfolg gelten");

        assert_eq!(err.code, None);
    }

    #[test]
    fn test_passphrase_with_trailing_crlf_is_stored_trimmed() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        let auth = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::PrivateKey {
                key_content: Some("-----BEGIN KEY-----".to_string()),
                passphrase: Some("passphrase-secret\r\n".to_string()),
            },
            None,
        )
        .unwrap();

        let AuthMethod::PrivateKey { passphrase_ref, .. } = &auth else {
            panic!("erwartete AuthMethod::PrivateKey");
        };
        let passphrase_ref = passphrase_ref.as_ref().expect("Passphrase wurde gesetzt");
        assert_eq!(
            secret_value(&store, passphrase_ref).as_deref(),
            Some("passphrase-secret")
        );
    }

    #[test]
    fn test_passphrase_of_only_whitespace_on_update_leaves_existing_passphrase_unchanged() {
        let id = ServerId::new();
        let key_ref = credential_ref(id, "private_key");
        let existing_passphrase_ref = credential_ref(id, "passphrase");
        let store = InMemoryCredentialStore::new()
            .with_secret(&key_ref, "old-key")
            .with_secret(&existing_passphrase_ref, "old-passphrase");
        let existing = AuthMethod::PrivateKey {
            credential_ref: key_ref.clone(),
            passphrase_ref: Some(existing_passphrase_ref.clone()),
        };

        let auth = resolve_auth_method(
            &store,
            AVAILABLE,
            id,
            AuthMethodInput::PrivateKey {
                key_content: None,
                passphrase: Some(" \t ".to_string()),
            },
            Some(&existing),
        )
        .unwrap();

        let AuthMethod::PrivateKey { passphrase_ref, .. } = &auth else {
            panic!("erwartete AuthMethod::PrivateKey");
        };
        assert_eq!(passphrase_ref.as_ref(), Some(&existing_passphrase_ref));
        assert_eq!(
            secret_value(&store, &existing_passphrase_ref).as_deref(),
            Some("old-passphrase")
        );
    }
}
