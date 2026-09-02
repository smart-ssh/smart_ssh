//! Übersetzung zwischen [`AuthMethodInput`] (Formulardaten) und
//! [`AuthMethod`] (persistierte Form mit `CredentialRef`s) — Spec 0008,
//! Abschnitt 4: "Backend schreibt Secret-Inhalte über
//! `CredentialStore::set()` **vor** dem Schreiben der restlichen Felder in
//! die DB", "`None`/leer bei `update_server` bedeutet unverändert lassen".
//! Dieselbe Konvention wie beim AI-Provider (Spec 0007, Abschnitt 8.2),
//! hier aber über bis zu zwei Secret-Slots pro Methode (Key+Passphrase,
//! Zertifikat+Key) statt nur einem.

use secrecy::SecretString;

use ssh_manager_core::profiles::{AuthMethod, CredentialRef, CredentialStore};
use ssh_manager_core::shared::ServerId;

use crate::dto::AuthMethodInput;
use crate::error::CommandError;

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
pub fn resolve_sudo_password(
    credential_store: &dyn CredentialStore,
    server_id: ServerId,
    provided: Option<String>,
) -> Result<(), CommandError> {
    match provided {
        Some(value) if !value.is_empty() => {
            credential_store.set(&sudo_password_credential_ref(server_id), SecretString::from(value))?;
        }
        _ => {}
    }
    Ok(())
}

/// Explizites Entfernen (Spec 0018, Abschnitt 4) — "Feld leer lassen"
/// bedeutet bereits "unverändert" (s. [`resolve_sudo_password`]), ein
/// einmal gesetztes Sudo-Passwort braucht daher einen eigenen Weg, um es
/// wieder zu löschen. Best-effort: ein bereits fehlender Eintrag ist kein
/// Fehler.
pub fn clear_sudo_password(credential_store: &dyn CredentialStore, server_id: ServerId) {
    let _ = credential_store.delete(&sudo_password_credential_ref(server_id));
}

/// Schreibt `provided` unter `ref_`, falls gesetzt; ist `provided` leer
/// und der Slot existierte bereits (Update, unverändert), passiert nichts
/// — der alte Wert bleibt unter demselben `ref_` stehen. Existierte der
/// Slot nicht (Neuanlage, oder Methodenwechsel bei Update) und `provided`
/// ist leer, ist das ein Fehler: es gibt keinen "alten Wert", der
/// übernommen werden könnte.
fn write_or_reuse_secret(
    credential_store: &dyn CredentialStore,
    ref_: &CredentialRef,
    provided: Option<String>,
    previously_existed: bool,
    label: &str,
    code: &'static str,
) -> Result<(), CommandError> {
    match provided {
        Some(value) => {
            credential_store.set(ref_, SecretString::from(value))?;
            Ok(())
        }
        None if previously_existed => Ok(()),
        None => Err(CommandError::with_code(format!("{label} ist erforderlich"), code)),
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
pub fn resolve_auth_method(
    credential_store: &dyn CredentialStore,
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
            let passphrase_ref = match passphrase {
                Some(p) => {
                    let r = credential_ref(server_id, "passphrase");
                    credential_store.set(&r, SecretString::from(p))?;
                    Some(r)
                }
                None => existing_passphrase_ref,
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
                &cert_ref,
                cert_content,
                existed,
                "Zertifikat",
                "SERVER_CERTIFICATE_REQUIRED",
            )?;
            write_or_reuse_secret(
                credential_store,
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
pub fn delete_auth_method_secrets(credential_store: &dyn CredentialStore, auth: &AuthMethod) {
    match auth {
        AuthMethod::Password { credential_ref } => {
            let _ = credential_store.delete(credential_ref);
        }
        AuthMethod::PrivateKey {
            credential_ref,
            passphrase_ref,
        } => {
            let _ = credential_store.delete(credential_ref);
            if let Some(r) = passphrase_ref {
                let _ = credential_store.delete(r);
            }
        }
        AuthMethod::Agent => {}
        AuthMethod::Certificate { cert_ref, key_ref } => {
            let _ = credential_store.delete(cert_ref);
            let _ = credential_store.delete(key_ref);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::InMemoryCredentialStore;

    fn secret_value(store: &InMemoryCredentialStore, r: &CredentialRef) -> Option<String> {
        use secrecy::ExposeSecret;
        store.get(r).ok().map(|s| s.expose_secret().to_string())
    }

    #[test]
    fn test_create_password_requires_value() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        let result =
            resolve_auth_method(&store, id, AuthMethodInput::Password { value: None }, None);

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

        let auth = resolve_auth_method(&store, id, AuthMethodInput::Agent, None).unwrap();

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

        let auth =
            resolve_auth_method(&store, id, AuthMethodInput::Agent, Some(&existing)).unwrap();

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

        delete_auth_method_secrets(&store, &auth);

        assert!(secret_value(&store, &key_ref).is_none());
        assert!(secret_value(&store, &passphrase_ref).is_none());
    }

    // --- Spec 0018: Sudo-Passwort ------------------------------------------

    #[test]
    fn test_resolve_sudo_password_stores_provided_value() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        resolve_sudo_password(&store, id, Some("hunter2".to_string())).unwrap();

        assert_eq!(
            secret_value(&store, &sudo_password_credential_ref(id)).as_deref(),
            Some("hunter2")
        );
    }

    #[test]
    fn test_resolve_sudo_password_none_or_empty_leaves_nothing_stored() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        resolve_sudo_password(&store, id, None).unwrap();
        resolve_sudo_password(&store, id, Some(String::new())).unwrap();

        assert!(secret_value(&store, &sudo_password_credential_ref(id)).is_none());
    }

    #[test]
    fn test_resolve_sudo_password_empty_value_on_update_leaves_existing_unchanged() {
        let id = ServerId::new();
        let store = InMemoryCredentialStore::new()
            .with_secret(&sudo_password_credential_ref(id), "old-password");

        // Leeres Feld bei "update" bedeutet unverändert (Spec 0018,
        // Abschnitt 4) — kein Löschen, kein Überschreiben.
        resolve_sudo_password(&store, id, Some(String::new())).unwrap();

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

        clear_sudo_password(&store, id);

        assert!(secret_value(&store, &sudo_password_credential_ref(id)).is_none());
    }

    #[test]
    fn test_clear_sudo_password_on_already_missing_entry_does_not_panic() {
        let store = InMemoryCredentialStore::new();
        let id = ServerId::new();

        clear_sudo_password(&store, id);
    }
}
