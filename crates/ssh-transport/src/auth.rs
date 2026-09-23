use std::sync::Arc;

use russh::client;
use russh::keys::{Certificate, PrivateKey, PrivateKeyWithHashAlg};
use secrecy::{ExposeSecret, SecretString};
use ssh_manager_core::profiles::CredentialStore;
use ssh_manager_core::ssh::{resolve_auth, Hop, KeyFileReader, ResolvedAuth, SshError};

use crate::error::map_russh_error;
use crate::handler::ClientHandler;

/// Authentifiziert `handle` für den gegebenen `hop` (Spec 0005, Teil 2 Punkt
/// 3 der Aufgabenstellung): löst `hop.auth` über `credentials` auf (reine
/// `core`-Logik, s. `ssh_manager_core::ssh::resolve_auth`) und übersetzt das
/// Ergebnis in den passenden `russh`-Auth-Aufruf.
///
/// Spec 0076, §4.2: `key_files` reist neben `credentials` mit — die
/// Anmeldelogik selbst bleibt dabei inhaltlich unverändert (§2, Nicht-Ziel
/// 5: keine neue Verzweigung auf die Anmeldeart im Transport).
pub(crate) async fn authenticate(
    handle: &mut client::Handle<ClientHandler>,
    hop: &Hop,
    credentials: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
) -> Result<(), SshError> {
    let resolved = resolve_auth(&hop.auth, credentials, key_files).map_err(|e| name_hop(e, hop))?;

    let result = match resolved {
        ResolvedAuth::Password(secret) => handle
            .authenticate_password(hop.username.clone(), secret.expose_secret().to_string())
            .await
            .map_err(map_russh_error)?,
        ResolvedAuth::PrivateKey { key, passphrase } => {
            let private_key =
                load_private_key(&key, passphrase.as_ref()).map_err(|e| name_hop(e, hop))?;
            let hash_alg = handle
                .best_supported_rsa_hash()
                .await
                .unwrap_or(None)
                .flatten();
            let key_with_hash = PrivateKeyWithHashAlg::new(Arc::new(private_key), hash_alg);
            handle
                .authenticate_publickey(hop.username.clone(), key_with_hash)
                .await
                .map_err(map_russh_error)?
        }
        ResolvedAuth::Agent => authenticate_via_agent(handle, &hop.username).await?,
        ResolvedAuth::Certificate { cert, key } => {
            let private_key = load_private_key(&key, None).map_err(|e| name_hop(e, hop))?;
            let certificate = Certificate::from_openssh(cert.expose_secret()).map_err(|e| {
                name_hop(
                    SshError::CredentialResolutionFailed(format!("Zertifikat ungültig: {e}")),
                    hop,
                )
            })?;
            handle
                .authenticate_openssh_cert(hop.username.clone(), Arc::new(private_key), certificate)
                .await
                .map_err(map_russh_error)?
        }
    };

    if result.success() {
        Ok(())
    } else {
        Err(SshError::AuthenticationFailed)
    }
}

/// Spec 0076, A-8: **Die Meldung muss sagen, welcher Hop gescheitert ist.**
///
/// `authenticate` läuft je Hop der Verbindungskette
/// (`connect::connect`), und ein Jump-Host mit Schlüsseldatei ist damit
/// ohne Zusatzarbeit möglich — aber eine Meldung wie „Die Schlüsseldatei
/// … wurde nicht gefunden" ist wertlos, wenn die Kette drei Rechner lang
/// ist und offenbleibt, welcher gemeint war.
///
/// Angereichert wird **nur** [`SshError::CredentialResolutionFailed`]:
/// Jede andere Variante entsteht entweder gar nicht je Hop oder trägt
/// keinen Text, in den etwas passte. Der stabile `code()` bleibt dabei
/// unverändert — das Frontend-Mapping hängt nicht am Text (Spec 0024,
/// Abschnitt 5).
///
/// Benutzername, Host und Port sind keine Geheimnisse; Dateiinhalt oder
/// Schlüsselmaterial kommen hier ohnehin nicht vorbei (5.2).
fn name_hop(error: SshError, hop: &Hop) -> SshError {
    match error {
        SshError::CredentialResolutionFailed(message) => SshError::CredentialResolutionFailed(
            format!("{}@{}:{}: {message}", hop.username, hop.host, hop.port),
        ),
        other => other,
    }
}

/// Was die Prüfung einer Schlüsseldatei auf Gültigkeit ergibt (Spec 0076,
/// §4.2). Bewusst **ohne** den Fehlertext der Parse-Bibliothek: Ob
/// `ssh_key`s `Display` je Eingabebytes wiedergibt, wissen wir nicht, und
/// eine Sicherheits-Invariante darf nicht am Anzeigeverhalten einer fremden
/// Kiste ruhen (5.2). Wer diesen Typ bekommt, formuliert seine eigene
/// Meldung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyClassification {
    /// Ein gültiger OpenSSH-Schlüssel. `encrypted` ist eine
    /// **Feststellung**, kein Urteil: ob eine fehlende Passphrase ein
    /// Fehler ist, entscheidet `resolve_auth` (Spec 0076, A-5).
    Valid {
        encrypted: bool,
    },
    Invalid,
}

/// Prüft rohe Bytes auf einen gültigen OpenSSH-Privatschlüssel und meldet,
/// ob er verschlüsselt ist (Spec 0076, §4.2).
///
/// Liegt hier und nicht in `app-shell`, obwohl die `KeyFileReader`-
/// Umsetzung dort wohnt: Das Parsen von Schlüsseln ist bereits Sache dieser
/// Crate (s. [`load_private_key`] direkt darunter, dieselbe
/// `PrivateKey::from_openssh`-Zeile), und `app-shell` hat keine eigene
/// Schlüssel-Bibliothek — sie dafür aufzunehmen wäre eine neue
/// Abhängigkeit für eine Zeile, die es hier schon gibt. `core` scheidet
/// ohnehin aus (keine I/O- und keine `russh`-Abhängigkeit, §1.3).
pub fn classify_openssh_key(bytes: &[u8]) -> KeyClassification {
    match PrivateKey::from_openssh(bytes) {
        Ok(parsed) => KeyClassification::Valid {
            encrypted: parsed.is_encrypted(),
        },
        // Der Fehler wird bewusst fallen gelassen, nicht durchgereicht
        // (5.2) — der Aufrufer formuliert eine eigene, pfadtragende
        // Meldung ohne Dateiinhalt.
        Err(_) => KeyClassification::Invalid,
    }
}

/// Parst einen Private Key im OpenSSH-Format aus `key` (Rohtext, wie ihn
/// `CredentialStore` liefert) und entschlüsselt ihn ggf. mit `passphrase`.
/// Fehlende Passphrase bei verschlüsseltem Key ist ein Credential-
/// Auflösungsfehler, kein Panic.
fn load_private_key(
    key: &SecretString,
    passphrase: Option<&SecretString>,
) -> Result<PrivateKey, SshError> {
    let parsed = PrivateKey::from_openssh(key.expose_secret().as_bytes())
        .map_err(|e| SshError::CredentialResolutionFailed(format!("Private Key ungültig: {e}")))?;

    if !parsed.is_encrypted() {
        return Ok(parsed);
    }

    let Some(passphrase) = passphrase else {
        return Err(SshError::CredentialResolutionFailed(
            "Private Key ist verschlüsselt, aber keine Passphrase hinterlegt".to_string(),
        ));
    };

    parsed
        .decrypt(passphrase.expose_secret().as_bytes())
        .map_err(|e| {
            SshError::CredentialResolutionFailed(format!(
                "Passphrase falsch oder Key beschädigt: {e}"
            ))
        })
}

#[cfg(unix)]
async fn authenticate_via_agent(
    handle: &mut client::Handle<ClientHandler>,
    username: &str,
) -> Result<russh::client::AuthResult, SshError> {
    use russh::keys::agent::client::AgentClient;

    let mut agent = AgentClient::connect_env().await.map_err(|e| {
        SshError::CredentialResolutionFailed(format!("SSH-Agent nicht erreichbar: {e}"))
    })?;
    let identities = agent.request_identities().await.map_err(|e| {
        SshError::CredentialResolutionFailed(format!("Agent-Identitäten nicht abrufbar: {e}"))
    })?;
    let identity = identities.first().cloned().ok_or_else(|| {
        SshError::CredentialResolutionFailed("SSH-Agent hat keine Identitäten geladen".to_string())
    })?;

    let public_key = match &identity {
        russh::keys::agent::AgentIdentity::PublicKey { key, .. } => key.clone(),
        russh::keys::agent::AgentIdentity::Certificate { certificate, .. } => {
            russh::keys::PublicKey::new(certificate.public_key().clone(), "")
        }
    };

    let mut signer = AgentSigner {
        client: agent,
        identity,
    };
    handle
        .authenticate_publickey_with(username.to_string(), public_key, None, &mut signer)
        .await
        .map_err(crate::error::map_transport_error)
}

#[cfg(not(unix))]
async fn authenticate_via_agent(
    _handle: &mut client::Handle<ClientHandler>,
    _username: &str,
) -> Result<russh::client::AuthResult, SshError> {
    // `russh::keys::agent::client::AgentClient::connect_env()` ist
    // `#[cfg(unix)]` (Unix-Domain-Socket über `SSH_AUTH_SOCK`) — ein
    // Windows-Pendant existiert in `russh` zwar (`connect_pageant()`,
    // `connect_named_pipe()`), wird hier aber bewusst noch nicht angebunden.
    // Kein Panic/TODO, sondern ein ehrlicher, klar benannter Fehler. Siehe
    // ADR-Vorschlag in der Abschluss-Nachricht.
    Err(SshError::CredentialResolutionFailed(
        "SSH-Agent-Authentifizierung ist auf dieser Plattform noch nicht unterstützt".to_string(),
    ))
}

/// Dünner Wrapper, der `russh::keys::agent::client::AgentClient` als
/// `auth::Signer` nutzbar macht.
///
/// `russh` 0.63.1 implementiert `Signer` für `AgentClient` entgegen seiner
/// eigenen Doc-Kommentare ("this crate only provides an implementation for
/// an SSH agent") tatsächlich **nicht** — im Zuge dieser Implementierung im
/// Quellcode verifiziert (`grep` nach `impl.*Signer.*for AgentClient` liefert
/// keinen Treffer). Dieser Wrapper schließt die Lücke selbst: `auth_sign`
/// leitet direkt an `AgentClient::sign_request` weiter, dessen Signatur
/// bereits nahezu exakt zu `Signer::auth_sign` passt. Siehe ADR-Vorschlag in
/// der Abschluss-Nachricht.
#[cfg(unix)]
struct AgentSigner {
    client: russh::keys::agent::client::AgentClient<tokio::net::UnixStream>,
    identity: russh::keys::agent::AgentIdentity,
}

#[cfg(unix)]
impl russh::Signer for AgentSigner {
    type Error = crate::error::TransportError;

    async fn auth_sign(
        &mut self,
        _key: &russh::keys::agent::AgentIdentity,
        hash_alg: Option<russh::keys::HashAlg>,
        to_sign: Vec<u8>,
    ) -> Result<Vec<u8>, Self::Error> {
        self.client
            .sign_request(&self.identity, hash_alg, to_sign)
            .await
            .map_err(crate::error::TransportError::Keys)
    }
}
