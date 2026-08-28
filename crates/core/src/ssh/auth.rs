use secrecy::SecretString;

use crate::profiles::{AuthMethod, CredentialStore};

use super::error::SshError;

/// Aufgelöstes Auth-Material für einen [`super::Hop`].
///
/// Kein `russh`-spezifischer Typ — die Übersetzung in konkrete `russh`-
/// Auth-Aufrufe ist Sache von `crates/ssh-transport` (Spec 0005 Abschnitt
/// 2). Nicht Teil der in Abschnitt 4-7 der Spec explizit vorgegebenen
/// Typen, aber eine direkte, naheliegende Konsequenz aus Abschnitt 8
/// ("Fehler-Mapping ... fehlende Credentials führen zu
/// `CredentialResolutionFailed`") und Teil 2 Punkt 3 der Aufgabenstellung:
/// Die *Auflösung* von `AuthMethod` + `CredentialStore` zu tatsächlichem
/// Auth-Material ist reine Logik ohne Netz-I/O (nur `CredentialStore`-
/// Lookups, selbst synchron) — gehört also nach `core`, nicht in die
/// `russh`-spezifische Crate, die nur noch das bereits aufgelöste Material
/// gegen `russh`s Auth-API verwenden soll.
#[derive(Debug, Clone)]
pub enum ResolvedAuth {
    Password(SecretString),
    PrivateKey {
        key: SecretString,
        passphrase: Option<SecretString>,
    },
    Agent,
    Certificate {
        cert: SecretString,
        key: SecretString,
    },
}

/// Löst ein [`AuthMethod`] über einen [`CredentialStore`] zu tatsächlichem
/// Auth-Material auf. Fehlende/ungültige Credentials ergeben
/// [`SshError::CredentialResolutionFailed`] mit einer verständlichen
/// Meldung, nie einen Panic (Spec 0005 Abschnitt 8; Teil 2, Punkt 3 der
/// Aufgabenstellung).
pub fn resolve_auth(
    auth: &AuthMethod,
    credentials: &dyn CredentialStore,
) -> Result<ResolvedAuth, SshError> {
    match auth {
        AuthMethod::Password { credential_ref } => {
            let secret = credentials
                .get(credential_ref)
                .map_err(|e| SshError::CredentialResolutionFailed(format!("Passwort: {e}")))?;
            Ok(ResolvedAuth::Password(secret))
        }
        AuthMethod::PrivateKey {
            credential_ref,
            passphrase_ref,
        } => {
            let key = credentials
                .get(credential_ref)
                .map_err(|e| SshError::CredentialResolutionFailed(format!("Private Key: {e}")))?;
            let passphrase = passphrase_ref
                .as_ref()
                .map(|r| credentials.get(r))
                .transpose()
                .map_err(|e| SshError::CredentialResolutionFailed(format!("Passphrase: {e}")))?;
            Ok(ResolvedAuth::PrivateKey { key, passphrase })
        }
        AuthMethod::Agent => Ok(ResolvedAuth::Agent),
        AuthMethod::Certificate { cert_ref, key_ref } => {
            let cert = credentials
                .get(cert_ref)
                .map_err(|e| SshError::CredentialResolutionFailed(format!("Zertifikat: {e}")))?;
            let key = credentials
                .get(key_ref)
                .map_err(|e| SshError::CredentialResolutionFailed(format!("Key: {e}")))?;
            Ok(ResolvedAuth::Certificate { cert, key })
        }
    }
}
