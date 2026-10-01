use std::fmt;

use crate::profiles::KEYCHAIN_ACCESS_FAILED;

/// Welches Secret ein [`SshError::CredentialStoreFailed`] betrifft (Spec
/// 0098, A5). Eine Variante je `credentials.get(...)`-Aufruf in
/// [`super::resolve_auth`].
///
/// **Ein Enum und kein `&str`**, damit die erlaubten Angaben aus A5 („die
/// Art des Secrets, feste Texte und die Hop-Angabe") an der Signatur
/// ablesbar sind: In diese Variante lässt sich kein freier Text und damit
/// auch nicht die Nutzlast der Bibliothek einsetzen. Eine Umgehung wäre kein
/// Versehen mehr, sondern eine neue Variante.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    Password,
    PrivateKey,
    Passphrase,
    Certificate,
    CertificateKey,
}

impl SecretKind {
    /// Die Benennung, die schon vor Spec 0098 in der Meldung stand — wörtlich
    /// dieselben Wörter wie in den `format!("Passwort: {e}")`-Zeilen von
    /// [`super::resolve_auth`], damit die `NotFound`-Meldungen (die
    /// unverändert bleiben, A4 letzter Satz) Zeichen für Zeichen gleich
    /// aussehen.
    pub fn label(&self) -> &'static str {
        match self {
            SecretKind::Password => "Passwort",
            SecretKind::PrivateKey => "Private Key",
            SecretKind::Passphrase => "Passphrase",
            SecretKind::Certificate => "Zertifikat",
            SecretKind::CertificateKey => "Key",
        }
    }
}

/// Welcher Hop einer Verbindungskette gemeint ist (Spec 0076, A-8).
///
/// Strukturiert statt als vorangestellter Text, weil
/// [`SshError::CredentialStoreFailed`] keinen freien Meldungstext hat, in den
/// sich ein Präfix schreiben ließe (A5). Benutzername, Host und Port sind
/// keine Geheimnisse; Schlüsselmaterial kommt hier nicht vorbei (Spec 0076,
/// 5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HopLabel {
    pub username: String,
    pub host: String,
    pub port: u16,
}

impl fmt::Display for HopLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}:{}", self.username, self.host, self.port)
    }
}

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
    /// Spec 0098, A4: Das Lesen eines Secrets aus dem [`CredentialStore`]
    /// ist mit [`crate::profiles::CredentialError::Backend`] gescheitert —
    /// der Schlüsselbund hat nicht geantwortet, gesperrt, abgelehnt. Eigene
    /// Variante statt in [`Self::CredentialResolutionFailed`] verpackt, aus
    /// zwei Gründen:
    ///
    /// 1. **Der Aufrufer muss unterscheiden können** (A4), ohne den
    ///    Fehlertext zu lesen — dasselbe Argument wie bei
    ///    [`Self::SftpPermissionDenied`]. Nur die Schicht, die
    ///    `AppState.keychain` kennt, kann entscheiden, ob daraus
    ///    `KEYCHAIN_ACCESS_FAILED` oder `KEYCHAIN_UNAVAILABLE` wird; `core`
    ///    bleibt ohne Abhängigkeit vom `AppState` (§5).
    /// 2. **Die Variante trägt keinen freien Text** (A5). Die Nutzlast der
    ///    Bibliothek bleibt dort liegen, wo sie entsteht
    ///    ([`super::resolve_auth`]); hierher kommt nur, welches Secret es war
    ///    und — sobald `ssh-transport` sie setzt — welcher Hop.
    ///
    /// Ein fehlender Eintrag (`NotFound`) führt **nicht** hierher: Er bleibt
    /// [`Self::CredentialResolutionFailed`], weil „kein Eintrag" eine
    /// fachliche Aussage ist und keine Störung des Schlüsselbunds (Spec 0071
    /// A14/I4).
    CredentialStoreFailed {
        secret: SecretKind,
        /// `None`, solange niemand den Hop benannt hat — gesetzt von
        /// `ssh_transport::auth::name_hop` (Spec 0076, A-8).
        hop: Option<HopLabel>,
    },
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
            // Spec 0098, A5: feste Texte, die Art des Secrets und die
            // Hop-Angabe — nichts sonst. Der Hop steht **vorn**, wie bei
            // jeder anderen benannten Meldung (Spec 0076, A-8), damit in
            // einer dreigliedrigen Kette als Erstes sichtbar ist, welcher
            // Rechner gemeint war.
            SshError::CredentialStoreFailed { secret, hop } => {
                if let Some(hop) = hop {
                    write!(f, "{hop}: ")?;
                }
                write!(
                    f,
                    "Zugriff auf den Schlüsselbund fehlgeschlagen ({})",
                    secret.label()
                )
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
            // Spec 0098, A4: **nicht** `SSH_`-präfigiert, weil es kein
            // SSH-Problem ist — der Nutzer soll den Schlüsselbund als
            // Ursache sehen, nicht „Netzwerkfehler" und nicht „Zugangsdaten
            // konnten nicht aufgelöst werden".
            //
            // Dies ist die Fassung für „Schlüsselbund verfügbar". `core`
            // kennt den Startzustand nicht (§5) und vergibt deshalb bewusst
            // die Fassung, die **nie** fälschlich „nicht verfügbar"
            // behauptet (A3). Wo der Zustand bekannt ist, wird daraus
            // `KEYCHAIN_UNAVAILABLE` — s.
            // `app_logic::error::keychain_aware_ssh_error_code` (A2). Vergisst
            // ein Aufrufer diesen Schritt, ist das Ergebnis also zu
            // vorsichtig, nicht zu dreist.
            SshError::CredentialStoreFailed { .. } => KEYCHAIN_ACCESS_FAILED,
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
            // Spec 0098, A4.
            SshError::CredentialStoreFailed {
                secret: SecretKind::Password,
                hop: None,
            },
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

    /// Spec 0098, A5: Der `Display`-Text dieser Variante besteht
    /// ausschließlich aus festem Text, der Secret-Art und der Hop-Angabe.
    /// Hier festgehalten, weil die Variante die einzige ist, deren Meldung
    /// eine Sicherheits-Invariante trägt: Hätte sie je ein freies
    /// Textfeld, stünde dort der Text der `keyring`-Bibliothek.
    #[test]
    fn test_spec_0098_a5_credential_store_failure_names_only_secret_kind_and_hop() {
        let named = SshError::CredentialStoreFailed {
            secret: SecretKind::Passphrase,
            hop: Some(HopLabel {
                username: "deploy".to_string(),
                host: "127.0.0.1".to_string(),
                port: 2222,
            }),
        };

        let message = named.to_string();
        assert!(
            message.starts_with("deploy@127.0.0.1:2222: "),
            "die Hop-Angabe gehört nach vorn (Spec 0076, A-8): {message}"
        );
        assert!(
            message.contains("Passphrase"),
            "die Art des Secrets gehört in die Meldung: {message}"
        );
        assert!(
            !message.contains("Netzwerk"),
            "es ist kein Netzwerkfehler (A4): {message}"
        );

        // Ohne Hop bleibt derselbe Satz, nur ohne Präfix — kein leeres
        // „: " und kein Platzhalter.
        let unnamed = SshError::CredentialStoreFailed {
            secret: SecretKind::Passphrase,
            hop: None,
        };
        assert_eq!(
            unnamed.to_string(),
            "Zugriff auf den Schlüsselbund fehlgeschlagen (Passphrase)"
        );
    }

    /// Spec 0098, A4: Der Code hängt an der Variante, nicht an Secret-Art
    /// oder Hop — sonst müsste das Frontend für jede Secret-Art eine eigene
    /// Übersetzung führen.
    #[test]
    fn test_spec_0098_credential_store_failure_code_is_the_same_for_every_secret_kind() {
        for secret in [
            SecretKind::Password,
            SecretKind::PrivateKey,
            SecretKind::Passphrase,
            SecretKind::Certificate,
            SecretKind::CertificateKey,
        ] {
            assert_eq!(
                SshError::CredentialStoreFailed { secret, hop: None }.code(),
                crate::profiles::KEYCHAIN_ACCESS_FAILED,
            );
        }
    }
}
