//! `test_connection` (Spec 0008, Abschnitt 7) — prüft Erreichbarkeit/
//! Zugangsdaten eines (ggf. noch nicht gespeicherten) Servers, ohne
//! irgendetwas zu persistieren.

use std::sync::Arc;

use async_trait::async_trait;
use secrecy::SecretString;

use ssh_manager_core::profiles::{
    trim_credential_value, AuthMethod, CredentialRef, CredentialStore, ProfileStore,
};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{
    resolve_connection_target, ConnectLog, ConnectionTarget, Hop, HostKeyDecision, HostKeyStore,
    KeyFileReader, SshError,
};
use ssh_transport::{ConnectLimits, ConnectOutcome};

use crate::dto::{AuthMethodInput, ServerInput, TestConnectionReport, TestConnectionResult};
use crate::ephemeral_credentials::EphemeralCredentialStore;

use crate::connect_attempts::ConnectAttemptGuard;
use crate::error::{secret_store_error, CommandError, CommandResult};
use crate::events::{emit_connect_progress, EventEmitter};

/// Spec 0084, §4: liegt hier, weil `test_connection` (Tauri-frei, in
/// `app-logic`) und `commands::connect` (Tauri-gebunden, in `app-shell`)
/// dieselben Grenzen brauchen — die umgekehrte Abhängigkeitsrichtung wäre
/// nicht erfüllbar.
///
/// Spec 0069, Teil A3 (Issue #97): die Zeitgrenzen je Phase und je Hop —
/// Verbindung und Handshake kurz, Anmeldung großzügig. Beide Pfade,
/// Verbinden und „Verbindung testen", nutzen **diese eine** Konstante; die
/// Werte stehen in der Spec. Den ganzen Versuch umschließt zusätzlich
/// `SSH_CONNECT_LIMITS.overall(hop_count)` als Sicherheitsnetz. Keine der
/// Grenzen umfasst das Warten auf eine Host-Key-Entscheidung (das bleibt
/// bei `crate::orchestration::PENDING_ACTION_CONFIRM_TIMEOUT`, Spec 0068
/// Teil 5b) — s. `ssh_transport::connect_with_timeout`s Doc-Kommentar zur
/// Sicherheits-Invariante ("liefert immer einen Fehler, nie `Connected`,
/// nie `trust()`").
pub const SSH_CONNECT_LIMITS: ConnectLimits = ConnectLimits::DEFAULT;

/// Kapselt `ssh_transport::connect()` hinter einem Trait, rein damit
/// `test_connection`s Logik (Ephemeral-Credential-Aufbau, Hop-Kette,
/// Timeout, `SshError`/`ConnectOutcome` → `TestConnectionResult`-Mapping)
/// gegen einen `MockConnector` testbar ist, ohne echtes Netzwerk zu
/// brauchen (Aufgabenstellung Teil 1, Punkt 6: "gegen MockSshTransport ...
/// für alle TestConnectionResult-Varianten"). `ssh_transport::connect` ist
/// eine freie Funktion, kein Trait-Objekt (anders als `SshTransport`
/// selbst, das erst NACH einem erfolgreichen Verbindungsaufbau greift) —
/// ohne diese Abstraktion ließe sich der Verbindungsversuch selbst gar
/// nicht mocken.
#[async_trait]
pub trait Connector: Send + Sync {
    async fn connect(
        &self,
        target: &ConnectionTarget,
        credentials: &(dyn CredentialStore + Send + Sync),
        key_files: &(dyn KeyFileReader + Send + Sync),
        host_keys: Arc<dyn HostKeyStore>,
        // Issue #51: Schritt-Protokoll des Versuchs, nur für die Anzeige.
        log: &ConnectLog,
        // Issue #97: Zeitgrenzen je Phase und Hop — dieselben wie beim
        // Verbinden.
        limits: ConnectLimits,
    ) -> Result<ConnectOutcome, SshError>;
}

pub struct RealConnector;

#[async_trait]
impl Connector for RealConnector {
    async fn connect(
        &self,
        target: &ConnectionTarget,
        credentials: &(dyn CredentialStore + Send + Sync),
        key_files: &(dyn KeyFileReader + Send + Sync),
        host_keys: Arc<dyn HostKeyStore>,
        log: &ConnectLog,
        limits: ConnectLimits,
    ) -> Result<ConnectOutcome, SshError> {
        ssh_transport::connect_with_limits(target, credentials, key_files, host_keys, log, limits)
            .await
    }
}

/// Spec 0008, Abschnitt 7. `existing_server_id` ist die in der Spec-Skizze
/// fehlende, aber notwendige Ergänzung: `test_connection(input:
/// ServerInput)` allein kann nicht wissen, **welcher** gespeicherte Server
/// gemeint ist, wenn ein Secret-Feld leer ("unverändert lassen") ist — die
/// Spec selbst verlangt aber genau dieses Verhalten ("wird für den Test
/// das bereits gespeicherte Credential des existierenden Servers
/// herangezogen"). Siehe ADR-Vorschlag am Ende der Aufgabe.
pub async fn test_connection(
    profile_store: &dyn ProfileStore,
    real_credential_store: &(dyn CredentialStore + Send + Sync),
    // Spec 0076, §4.2: reist neben `real_credential_store` mit — ein
    // Verbindungstest gegen einen Server mit Schlüsseldatei muss dieselbe
    // Datei lesen wie der echte Verbindungsaufbau.
    key_files: &(dyn KeyFileReader + Send + Sync),
    host_key_store: Arc<dyn HostKeyStore>,
    connector: &dyn Connector,
    input: ServerInput,
    existing_server_id: Option<ServerId>,
) -> CommandResult<TestConnectionReport> {
    test_connection_with_limits(
        profile_store,
        real_credential_store,
        key_files,
        host_key_store,
        connector,
        input,
        existing_server_id,
        SSH_CONNECT_LIMITS,
    )
    .await
}

/// Testbare Variante mit injizierbaren Grenzen — die echten Werte aus der
/// Spec wären in einem Unit-Test schlicht zu langsam.
///
/// Issue #51: liefert neben dem Ergebnis das Schritt-Protokoll des
/// Versuchs — bei Erfolg wie bei Fehlschlag. Bricht der Timeout den Versuch
/// ab, wird der gerade laufende Schritt mit `SSH_TIMEOUT` geschlossen.
/// Das Protokoll geht nur an die Oberfläche, nie ins Log.
#[allow(clippy::too_many_arguments)]
async fn test_connection_with_limits(
    profile_store: &dyn ProfileStore,
    real_credential_store: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
    host_key_store: Arc<dyn HostKeyStore>,
    connector: &dyn Connector,
    input: ServerInput,
    existing_server_id: Option<ServerId>,
    limits: ConnectLimits,
) -> CommandResult<TestConnectionReport> {
    test_connection_with_progress(
        profile_store,
        real_credential_store,
        key_files,
        host_key_store,
        connector,
        input,
        existing_server_id,
        limits,
        None,
    )
    .await
}

/// Issue #633: live progress and cancel for a connection test. Carries the
/// attempt id the frontend generated, the event emitter for
/// `connect-progress` events and the cancel guard registered under that id.
pub struct TestProgress {
    pub attempt_id: String,
    pub emitter: Arc<dyn EventEmitter>,
    pub guard: ConnectAttemptGuard,
}

/// `test_connection` with live `connect-progress` events and cancellation
/// (Issue #633). A cancelled test ends with `SSH_CONNECT_CANCELLED`.
#[allow(clippy::too_many_arguments)]
pub async fn test_connection_tracked(
    profile_store: &dyn ProfileStore,
    real_credential_store: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
    host_key_store: Arc<dyn HostKeyStore>,
    connector: &dyn Connector,
    input: ServerInput,
    existing_server_id: Option<ServerId>,
    progress: &TestProgress,
) -> CommandResult<TestConnectionReport> {
    test_connection_with_progress(
        profile_store,
        real_credential_store,
        key_files,
        host_key_store,
        connector,
        input,
        existing_server_id,
        SSH_CONNECT_LIMITS,
        Some(progress),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn test_connection_with_progress(
    profile_store: &dyn ProfileStore,
    real_credential_store: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
    host_key_store: Arc<dyn HostKeyStore>,
    connector: &dyn Connector,
    input: ServerInput,
    existing_server_id: Option<ServerId>,
    limits: ConnectLimits,
    progress: Option<&TestProgress>,
) -> CommandResult<TestConnectionReport> {
    let log = match progress {
        Some(p) => {
            let emitter = p.emitter.clone();
            let attempt_id = p.attempt_id.clone();
            let server_id = existing_server_id.unwrap_or_default();
            ConnectLog::with_sink(Arc::new(move |steps| {
                emit_connect_progress(emitter.as_ref(), &attempt_id, server_id, 1, steps);
            }))
        }
        None => ConnectLog::new(),
    };
    let result = run_test_connection(
        profile_store,
        real_credential_store,
        key_files,
        host_key_store,
        connector,
        input,
        existing_server_id,
        limits,
        &log,
        progress,
    )
    .await?;
    Ok(TestConnectionReport {
        result,
        steps: log.snapshot(),
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_test_connection(
    profile_store: &dyn ProfileStore,
    real_credential_store: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
    host_key_store: Arc<dyn HostKeyStore>,
    connector: &dyn Connector,
    input: ServerInput,
    existing_server_id: Option<ServerId>,
    limits: ConnectLimits,
    log: &ConnectLog,
    progress: Option<&TestProgress>,
) -> CommandResult<TestConnectionResult> {
    let existing_auth = match existing_server_id {
        Some(id) => Some(profile_store.get_server(&id).await?.auth),
        None => None,
    };

    let ephemeral = EphemeralCredentialStore::new();
    let final_auth = resolve_final_hop_auth(
        &ephemeral,
        real_credential_store,
        input.auth,
        existing_auth.as_ref(),
    )?;

    let mut hops = Vec::new();
    if let Some(jump_id) = input.jump_host {
        // Spec 0008 Abschnitt 7: "bereits gespeicherte Zwischen-Hops werden
        // regulär über ProfileStore aufgelöst" — derselbe Weg wie beim
        // echten `connect()` (Spec 0007), nur dass danach noch der frische
        // letzte Hop angehängt wird statt die Kette dort enden zu lassen.
        let jump_server = profile_store.get_server(&jump_id).await?;
        let jump_target = resolve_connection_target(&jump_server, profile_store).await?;
        hops.extend(jump_target.hops);
    }
    hops.push(Hop {
        host: input.host,
        port: input.port,
        username: input.username,
        auth: final_auth,
    });
    let target = ConnectionTarget { hops };

    let tiered = TieredCredentialStore {
        ephemeral: &ephemeral,
        real: real_credential_store,
    };

    // Issue #97: die Phasengrenzen wirken im Verbindungsaufbau selbst; der
    // äußere Timeout ist nur das Sicherheitsnetz, das mit der Hop-Zahl
    // wächst — wie in `connect_session`.
    let overall = limits.overall(target.hops.len());
    let attempt = connector.connect(&target, &tiered, key_files, host_key_store, log, limits);
    let timed = tokio::time::timeout(overall, attempt);
    let timed = match progress {
        Some(p) => tokio::select! {
            out = timed => Some(out),
            () = p.guard.cancelled() => None,
        },
        None => Some(timed.await),
    };
    let Some(timed) = timed else {
        // Issue #633: cancelled by the user; the dropped attempt releases its
        // sockets. Not a test result — the dialog just ends.
        log.fail_running("SSH_CONNECT_CANCELLED");
        return Err(CommandError::with_code(
            "Verbindungsaufbau abgebrochen",
            "SSH_CONNECT_CANCELLED",
        ));
    };
    let outcome = match timed {
        Err(_elapsed) => {
            log.fail_running_timeout();
            return Ok(TestConnectionResult::Timeout);
        }
        Ok(result) => result,
    };

    Ok(match outcome {
        Ok(ConnectOutcome::Connected(mut transport)) => {
            // Nur der Auth-Handshake wird geprüft — kein `execute()`, kein
            // Session-Eintrag (Spec Abschnitt 7). Verbindung sofort wieder
            // schließen.
            let _ = transport.disconnect().await;
            TestConnectionResult::Success
        }
        Ok(ConnectOutcome::PendingHostKeyConfirmation {
            host,
            port,
            raw_key,
            decision,
        }) => {
            let key_type = crate::host_key_store::offered_key_type(&raw_key);
            match decision {
                HostKeyDecision::Unknown { fingerprint } => TestConnectionResult::HostKeyUnknown {
                    host,
                    port,
                    raw_key,
                    fingerprint,
                    key_type,
                },
                HostKeyDecision::Mismatch {
                    expected_fingerprint,
                    actual_fingerprint,
                } => TestConnectionResult::HostKeyMismatch {
                    host,
                    port,
                    raw_key,
                    expected_fingerprint,
                    actual_fingerprint,
                    key_type,
                },
                HostKeyDecision::Trusted => {
                    unreachable!("PendingHostKeyConfirmation wird nur für Unknown/Mismatch gebaut")
                }
            }
        }
        Err(SshError::AuthenticationFailed) => TestConnectionResult::AuthenticationFailed,
        Err(SshError::Timeout) => TestConnectionResult::Timeout,
        // Spec 0098, A4: Die Variante des Ergebnisses bleibt `NetworkError`
        // (§5: die Form von `TestConnectionResult` ändert sich nicht, es
        // kommt nur ein Code-Wert hinzu) — der **Code** sagt jetzt aber den
        // Secret-Speicher als Ursache, und das Frontend zeigt dafür weder
        // „Netzwerkfehler" noch „Zugangsdaten konnten nicht aufgelöst
        // werden".
        //
        // `message` kommt unverändert aus `SshError`s `Display`; für
        // `CredentialStoreFailed` ist das per Konstruktion frei von der
        // Nutzlast der Bibliothek (A5).
        Err(other) => TestConnectionResult::NetworkError {
            message: other.to_string(),
            code: Some(other.code()),
        },
    })
}

/// Baut das `AuthMethod` für den frischen letzten Hop, befüllt dabei
/// `ephemeral` mit den benötigten Secrets — entweder aus `input` selbst
/// oder (bei leerem Feld) aus dem bereits gespeicherten Credential von
/// `existing` (s. Modul-Doc).
fn resolve_final_hop_auth(
    ephemeral: &EphemeralCredentialStore,
    real_credential_store: &(dyn CredentialStore + Send + Sync),
    input: AuthMethodInput,
    existing: Option<&AuthMethod>,
) -> CommandResult<AuthMethod> {
    match input {
        AuthMethodInput::NoPassword => Ok(AuthMethod::NoPassword),
        // Issue #632: gleiche Regel wie beim Speichern
        // (`server_credentials::resolve_auth_method`) — ein leeres Feld
        // nimmt das gespeicherte Passwort, und gibt es keines, gilt "kein
        // Passwort" (erst `none`, dann leeres `password`).
        AuthMethodInput::Password { value }
            if value
                .as_deref()
                .is_none_or(|v| trim_credential_value(v).is_empty())
                && !matches!(existing, Some(AuthMethod::Password { .. })) =>
        {
            Ok(AuthMethod::NoPassword)
        }
        AuthMethodInput::Password { value } => {
            let secret = resolve_secret(
                value,
                existing,
                real_credential_store,
                "SERVER_PASSWORD_REQUIRED",
                |a| match a {
                    AuthMethod::Password { credential_ref } => Some(credential_ref),
                    _ => None,
                },
            )?;
            let r = CredentialRef::new("test:password");
            ephemeral.insert(&r, secret);
            Ok(AuthMethod::Password { credential_ref: r })
        }
        AuthMethodInput::PrivateKey {
            key_content,
            passphrase,
        } => {
            let key_secret = resolve_secret(
                key_content,
                existing,
                real_credential_store,
                "SERVER_PRIVATE_KEY_REQUIRED",
                |a| match a {
                    AuthMethod::PrivateKey { credential_ref, .. } => Some(credential_ref),
                    _ => None,
                },
            )?;
            let key_ref = CredentialRef::new("test:private_key");
            ephemeral.insert(&key_ref, key_secret);

            // Spec 0073, A6–A8 (K1, BL-0243): dieselbe Trim-Semantik und
            // dieselbe „leer = das Gespeicherte nehmen"-Regel wie beim
            // Speichern (`server_credentials::resolve_auth_method`).
            // Vorher trat der Verbindungstest mit der Passphrase an, wie
            // sie im Feld stand — derselbe Paste konnte den Test scheitern
            // lassen und danach trotzdem richtig gespeichert werden.
            let passphrase_ref = match passphrase.map(|p| trim_credential_value(&p)) {
                Some(p) if !p.is_empty() => {
                    let r = CredentialRef::new("test:passphrase");
                    ephemeral.insert(&r, SecretString::from(p));
                    Some(r)
                }
                _ => match existing {
                    Some(AuthMethod::PrivateKey {
                        passphrase_ref: Some(existing_ref),
                        ..
                    }) => {
                        let secret = real_credential_store
                            .get(existing_ref)
                            .map_err(secret_store_error)?;
                        let r = CredentialRef::new("test:passphrase");
                        ephemeral.insert(&r, secret);
                        Some(r)
                    }
                    _ => None,
                },
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
            let cert_secret = resolve_secret(
                cert_content,
                existing,
                real_credential_store,
                "SERVER_CERTIFICATE_REQUIRED",
                |a| match a {
                    AuthMethod::Certificate { cert_ref, .. } => Some(cert_ref),
                    _ => None,
                },
            )?;
            let cert_ref = CredentialRef::new("test:certificate");
            ephemeral.insert(&cert_ref, cert_secret);

            let key_secret = resolve_secret(
                key_content,
                existing,
                real_credential_store,
                "SERVER_CERTIFICATE_KEY_REQUIRED",
                |a| match a {
                    AuthMethod::Certificate { key_ref, .. } => Some(key_ref),
                    _ => None,
                },
            )?;
            let key_ref = CredentialRef::new("test:certificate_key");
            ephemeral.insert(&key_ref, key_secret);

            Ok(AuthMethod::Certificate { cert_ref, key_ref })
        }
        // Spec 0076: Der Pfad wandert unverändert in den Test-Hop — er ist
        // kein Secret und braucht keinen Ephemeral-Slot. Gelesen wird die
        // Datei erst im Verbindungsversuch selbst, über denselben
        // `KeyFileReader` wie beim echten Verbinden. Nur die Passphrase
        // folgt der gewohnten „leer = das Gespeicherte nehmen"-Regel.
        AuthMethodInput::IdentityFile { path, passphrase } => {
            // Dieselbe Leer-Prüfung wie beim Speichern — vor jedem
            // Verbindungsversuch und jedem Lesen der Datei.
            crate::server_credentials::require_identity_file_path(&path)?;
            // Spec 0073, A6–A8 (K1, BL-0243): wie im `PrivateKey`-Zweig über
            // den geteilten `trim_credential_value` statt über
            // `str::trim` — und der getrimmte Wert ist auch der, der in
            // den Ephemeral-Store geht, nicht der rohe.
            let passphrase_ref = match passphrase.map(|p| trim_credential_value(&p)) {
                Some(p) if !p.is_empty() => {
                    let r = CredentialRef::new("test:passphrase");
                    ephemeral.insert(&r, SecretString::from(p));
                    Some(r)
                }
                _ => match existing {
                    Some(AuthMethod::IdentityFile {
                        passphrase_ref: Some(existing_ref),
                        ..
                    }) => {
                        let secret = real_credential_store
                            .get(existing_ref)
                            .map_err(secret_store_error)?;
                        let r = CredentialRef::new("test:passphrase");
                        ephemeral.insert(&r, secret);
                        Some(r)
                    }
                    _ => None,
                },
            };
            Ok(AuthMethod::IdentityFile {
                path,
                passphrase_ref,
            })
        }
    }
}

struct TieredCredentialStore<'a> {
    ephemeral: &'a (dyn CredentialStore + Send + Sync),
    real: &'a (dyn CredentialStore + Send + Sync),
}

impl<'a> CredentialStore for TieredCredentialStore<'a> {
    fn get(
        &self,
        reference: &CredentialRef,
    ) -> Result<SecretString, ssh_manager_core::profiles::CredentialError> {
        match self.ephemeral.get(reference) {
            Ok(secret) => Ok(secret),
            // **Nur „kein Eintrag" führt zum nächsten Store** (Spec 0098,
            // spec-reviewer Runde 1): Ein `Backend`-Fehler ist die Störung
            // eines Stores, keine Aussage über den Inhalt. Ihn weiterzugeben
            // hieße, aus einer Störung ein „nicht vorhanden" zu machen — die
            // Rückrichtung von T7, und damit genau die Verwechslung, die
            // Spec 0071 A14/I4 verbietet. Der `EphemeralCredentialStore`
            // liefert heute nur `NotFound`, der Zweig ändert also nichts am
            // Verhalten; er hält den Weg geschlossen.
            Err(ssh_manager_core::profiles::CredentialError::NotFound(_)) => {
                self.real.get(reference)
            }
            Err(backend) => Err(backend),
        }
    }

    fn set(
        &self,
        reference: &CredentialRef,
        secret: SecretString,
    ) -> Result<(), ssh_manager_core::profiles::CredentialError> {
        self.ephemeral.set(reference, secret)
    }

    fn delete(
        &self,
        reference: &CredentialRef,
    ) -> Result<(), ssh_manager_core::profiles::CredentialError> {
        self.ephemeral.delete(reference)
    }
}

/// Das Secret, mit dem der Verbindungstest antritt — aus dem Formularfeld
/// oder, wenn das leer ist, aus dem gespeicherten Credential des bestehenden
/// Servers.
///
/// Spec 0073, A6–A8 (Q-BL-0149-02): dieselbe Form wie
/// `server_credentials::write_or_reuse_secret` beim Speichern. Der
/// eingegebene Wert läuft über den geteilten [`trim_credential_value`], und
/// **danach** entscheidet dieselbe Leer-Regel: nicht leer → dieser Wert;
/// leer oder nicht angegeben → das gespeicherte Credential; gibt es keins,
/// der Fehler. Vorher nahm dieser Weg jeden `Some`-Wert, auch `""` — das
/// Formular schickt bei Neuanlage für ein leeres Pflichtfeld aber genau
/// `""` (`ServerForm.tsx`, `orNullIfUpdate` greift nur beim Bearbeiten).
/// „Neuer Server, Passwortfeld leer, Verbindung testen" endete damit auf
/// einem Server mit `PermitEmptyPasswords yes` im Erfolg für ein Profil,
/// das sich anschließend nicht speichern ließ.
///
/// Reihenfolge „erst trimmen, dann Leer-Prüfung" wie in Spec 0049, Fund 1:
/// Andernfalls bestünde ein reiner Leerraum-Paste die Leer-Prüfung und
/// würde zu einem Anmeldeversuch mit Leerraum als Secret.
fn resolve_secret(
    provided: Option<String>,
    existing: Option<&AuthMethod>,
    real_store: &(dyn CredentialStore + Send + Sync),
    code: &'static str,
    extract_ref: impl Fn(&AuthMethod) -> Option<&CredentialRef>,
) -> CommandResult<SecretString> {
    // Bewusst dieselbe Gestalt wie `write_or_reuse_secret`: derselbe Trim,
    // dieselbe Bedingung, dieselbe Reihenfolge der Zweige. Weicht eine der
    // beiden Stellen künftig ab, fällt es beim Vergleich auf.
    //
    // Gleiche Regel, nicht gleiches Ergebnis in jedem Fall: Beim Speichern
    // genügt „der Slot existierte vorher", hier wird das Secret zusätzlich
    // gelesen — ist der Schlüsselbund-Eintrag von außen verschwunden, ist
    // Speichern `Ok` und der Test `Err` (ADR 0067 §1).
    match provided.map(|value| trim_credential_value(&value)) {
        Some(value) if !value.is_empty() => Ok(SecretString::from(value)),
        _ => {
            // Spec 0073, A6–A8 (Q-BL-0149-03): derselbe Code wie beim Speichern
            // (`write_or_reuse_secret`), damit beide Knöpfe bei derselben
            // Eingabe denselben übersetzten Satz zeigen. Der Text (nur
            // Rückfall ohne Übersetzung) behauptet nicht „kein Server
            // gefunden": Der Zweig gilt auch für einen bestehenden Server
            // mit anderer Anmeldeart, der für diese Art keinen Slot hat.
            let existing_ref = existing.and_then(extract_ref).ok_or_else(|| {
                CommandError::with_code(
                    "Secret erforderlich (Feld leer, kein hinterlegtes Credential dieser Anmeldeart)",
                    code,
                )
            })?;
            real_store.get(existing_ref).map_err(secret_store_error)
        }
    }
}

#[cfg(test)]
mod tests;
