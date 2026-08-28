use std::fmt;

/// Fehler rund um Aufbau und Nutzung einer SSH-Verbindung (Spec 0005,
/// Abschnitt 7).
#[derive(Debug, Clone, PartialEq)]
pub enum SshError {
    ConnectionFailed(String),
    AuthenticationFailed,
    HostKeyRejected,
    ChannelError(String),
    Timeout,
    JumpHostCycle,
    CredentialResolutionFailed(String),
}

impl fmt::Display for SshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SshError::ConnectionFailed(msg) => write!(f, "Verbindung fehlgeschlagen: {msg}"),
            SshError::AuthenticationFailed => write!(f, "Authentifizierung fehlgeschlagen"),
            SshError::HostKeyRejected => write!(f, "Host-Key abgelehnt"),
            SshError::ChannelError(msg) => write!(f, "Channel-Fehler: {msg}"),
            SshError::Timeout => write!(f, "Zeitüberschreitung"),
            SshError::JumpHostCycle => write!(f, "zyklische Jump-Host-Kette erkannt"),
            SshError::CredentialResolutionFailed(msg) => {
                write!(f, "Credential-Auflösung fehlgeschlagen: {msg}")
            }
        }
    }
}

impl std::error::Error for SshError {}
