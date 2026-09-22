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
    /// Spec 0020, Abschnitt 4.3: fehlende Rechte bei einem SFTP-Datei-
    /// zugriff — eigene Variante statt in `ChannelError` verpackt, damit die
    /// App-Ebene zuverlässig danach unterscheiden kann (Sudo-Rechte-
    /// Fallback für `WriteRemoteFile`), ohne den Fehlertext parsen zu
    /// müssen.
    SftpPermissionDenied(String),
    /// Spec 0069, Teil A3: der Server hat die TCP-Verbindung aktiv
    /// abgelehnt (`io::ErrorKind::ConnectionRefused`) — Port zu oder kein
    /// SSH-Dienst dort. Bisher in `ConnectionFailed` verschmolzen.
    ConnectionRefused(String),
    /// Spec 0069, Teil A3: der Hostname ließ sich nicht auflösen —
    /// nachträglich per `lookup_host` diagnostiziert, s.
    /// `ssh_transport::connect`s Doc-Kommentar zur DNS-Diagnose (nur beim
    /// ersten Hop, nie eine Vorab-Auflösung).
    HostNotFound(String),
    /// Spec 0069, Teil A3: `io::ErrorKind::HostUnreachable`/
    /// `NetworkUnreachable` — keine Route zum Ziel (z. B. fehlendes VPN).
    HostUnreachable(String),
    /// Spec 0069, Teil A3: die Verbindung wurde während des Aufbaus beendet
    /// (`ConnectionReset`/`ConnectionAborted`/`UnexpectedEof`, oder
    /// `russh::Error::Disconnect`) — z. B. ein Nicht-SSH-Dienst auf dem
    /// Port, oder ein Server, der zu viele Versuche abwehrt.
    ConnectionClosed(String),
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
            SshError::SftpPermissionDenied(msg) => write!(f, "Zugriff verweigert: {msg}"),
            SshError::ConnectionRefused(msg) => write!(f, "Verbindung abgelehnt: {msg}"),
            SshError::HostNotFound(msg) => write!(f, "Host nicht gefunden: {msg}"),
            SshError::HostUnreachable(msg) => write!(f, "Host nicht erreichbar: {msg}"),
            SshError::ConnectionClosed(msg) => {
                write!(f, "Verbindung während des Aufbaus beendet: {msg}")
            }
        }
    }
}

impl SshError {
    /// Stabiler, sprachunabhängiger Bezeichner je Fehlerart (Spec 0024,
    /// Abschnitt 5) — fürs Frontend-Mapping auf Übersetzungs-Keys. Bleibt
    /// über Code-Änderungen hinweg stabil, anders als der `Display`-Text
    /// oben (der unverändert bleibt und weiterhin als Fallback dient, falls
    /// das Frontend einen Code nicht kennt).
    pub fn code(&self) -> &'static str {
        match self {
            SshError::ConnectionFailed(_) => "SSH_CONNECTION_FAILED",
            SshError::AuthenticationFailed => "SSH_AUTH_FAILED",
            SshError::HostKeyRejected => "SSH_HOST_KEY_REJECTED",
            SshError::ChannelError(_) => "SSH_CHANNEL_ERROR",
            SshError::Timeout => "SSH_TIMEOUT",
            SshError::JumpHostCycle => "SSH_JUMP_HOST_CYCLE",
            SshError::CredentialResolutionFailed(_) => "SSH_CREDENTIAL_RESOLUTION_FAILED",
            SshError::SftpPermissionDenied(_) => "SSH_SFTP_PERMISSION_DENIED",
            SshError::ConnectionRefused(_) => "SSH_CONNECTION_REFUSED",
            SshError::HostNotFound(_) => "SSH_HOST_NOT_FOUND",
            SshError::HostUnreachable(_) => "SSH_HOST_UNREACHABLE",
            SshError::ConnectionClosed(_) => "SSH_CONNECTION_CLOSED",
        }
    }
}

impl std::error::Error for SshError {}

#[cfg(test)]
mod code_tests {
    use super::*;

    /// Spec 0024, Abschnitt 5: Codes müssen stabil und eindeutig sein — kein
    /// Code darf für zwei unterschiedliche Fehlerarten doppelt vergeben sein.
    #[test]
    fn test_ssh_error_codes_are_unique() {
        let samples = [
            SshError::ConnectionFailed("x".to_string()),
            SshError::AuthenticationFailed,
            SshError::HostKeyRejected,
            SshError::ChannelError("x".to_string()),
            SshError::Timeout,
            SshError::JumpHostCycle,
            SshError::CredentialResolutionFailed("x".to_string()),
            SshError::SftpPermissionDenied("x".to_string()),
            SshError::ConnectionRefused("x".to_string()),
            SshError::HostNotFound("x".to_string()),
            SshError::HostUnreachable("x".to_string()),
            SshError::ConnectionClosed("x".to_string()),
        ];
        let codes: Vec<&'static str> = samples.iter().map(SshError::code).collect();
        let mut unique = codes.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            codes.len(),
            unique.len(),
            "doppelt vergebener SshError-Code: {codes:?}"
        );
    }

    #[test]
    fn test_ssh_error_code_stable_across_payload_variation() {
        assert_eq!(
            SshError::ConnectionFailed("a".to_string()).code(),
            SshError::ConnectionFailed("b".to_string()).code(),
        );
    }
}
