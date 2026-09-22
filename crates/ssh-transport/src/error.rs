use ssh_manager_core::ssh::{HostKeyDecision, SshError};

/// Interner Fehlertyp für `russh::client::Handler::Error`.
///
/// `russh::client::Handler` verlangt `type Error: From<russh::Error> + Send
/// + Debug` — `ssh_manager_core::ssh::SshError` kann dieses `From` nicht
/// implementieren, ohne dass `core` eine Abhängigkeit auf `russh` bekäme,
/// was dem in Spec 0004/0005 festgelegten Architektur-Prinzip
/// widerspräche ("core bleibt frei von I/O-Abhängigkeiten"). Deshalb dieser
/// lokale Fehlertyp als `Handler::Error`, umgewandelt in [`SshError`] direkt
/// an der Stelle, wo `client::connect`/`connect_stream` aufgerufen wird
/// (s. [`crate::connect`]).
///
/// Trägt zusätzlich [`TransportError::HostKey`]: `check_server_key` (s.
/// `crate::handler`) kann keinen eigenen Rückgabekanal für die
/// `HostKeyDecision` nutzen (die Handler-Callback-API von `russh` erlaubt
/// nur `Result<bool, Self::Error>`) — ein `Unknown`/`Mismatch`-Ergebnis wird
/// deshalb als `Err(TransportError::HostKey{..})` aus dem Handshake
/// zurückgegeben und von `connect()` in [`crate::ConnectOutcome::PendingHostKeyConfirmation`]
/// übersetzt, statt als generischer Verbindungsfehler durchzureichen.
#[derive(Debug)]
pub enum TransportError {
    Russh(russh::Error),
    Keys(russh::keys::Error),
    /// Fehler beim Parsen/Enkodieren eines Schlüssels (`ssh_key`-Crate,
    /// re-exportiert über `russh::keys::ssh_key`) — eine eigene Variante,
    /// da `ssh_key::Error` ein anderer Typ als `russh::keys::Error` ist
    /// (Letzteres deckt Agent-/`known_hosts`-Operationen ab, nicht das
    /// reine Schlüssel-Parsing).
    KeyParse(russh::keys::ssh_key::Error),
    Send(russh::SendError),
    HostKey {
        raw_key: Vec<u8>,
        decision: HostKeyDecision,
    },
    Ssh(SshError),
}

impl From<russh::Error> for TransportError {
    fn from(e: russh::Error) -> Self {
        TransportError::Russh(e)
    }
}

impl From<russh::keys::Error> for TransportError {
    fn from(e: russh::keys::Error) -> Self {
        TransportError::Keys(e)
    }
}

impl From<russh::keys::ssh_key::Error> for TransportError {
    fn from(e: russh::keys::ssh_key::Error) -> Self {
        TransportError::KeyParse(e)
    }
}

impl From<russh::SendError> for TransportError {
    fn from(e: russh::SendError) -> Self {
        TransportError::Send(e)
    }
}

impl From<SshError> for TransportError {
    fn from(e: SshError) -> Self {
        TransportError::Ssh(e)
    }
}

/// Wandelt einen [`TransportError`] in ein [`SshError`] — mit Ausnahme von
/// `HostKey`, das eine eigene Behandlung durch den Aufrufer braucht (s.
/// [`crate::connect`]) und deshalb bewusst *nicht* hier mit abgedeckt wird
/// (Aufrufer muss den `HostKey`-Fall vorher separat abfangen).
pub(crate) fn map_transport_error(e: TransportError) -> SshError {
    match e {
        TransportError::Russh(err) => map_russh_error(err),
        TransportError::Keys(err) => SshError::ConnectionFailed(format!("Key-Fehler: {err}")),
        TransportError::KeyParse(err) => {
            SshError::ConnectionFailed(format!("Key-Parse-Fehler: {err}"))
        }
        TransportError::Send(_) => SshError::ChannelError(
            "Nachricht konnte nicht gesendet werden (Verbindung bereits geschlossen?)".to_string(),
        ),
        TransportError::HostKey { decision, .. } => {
            // Sollte hier nie ankommen (s. Doc-Kommentar), aber falls doch:
            // kein Panic, sondern ein generischer, ehrlicher Fehler.
            SshError::ConnectionFailed(format!(
                "unerwarteter Host-Key-Fehler außerhalb des Connect-Flows: {decision:?}"
            ))
        }
        TransportError::Ssh(err) => err,
    }
}

/// Wandelt einen `russh::Error` in ein [`SshError`] (Spec 0005, Abschnitt
/// 7). Bildet nur die Fälle explizit ab, die sich sinnvoll einer der
/// spezifischen `SshError`-Varianten zuordnen lassen; alles andere landet in
/// `ChannelError` mit der Original-Fehlermeldung, statt jede der zahlreichen
/// `russh::Error`-Varianten einzeln (und damit bei jedem `russh`-Upgrade
/// erneut brüchig) nachzubilden.
///
/// Spec 0069, Teil A3: der `IO`-Zweig unterscheidet seit hier zusätzlich
/// nach `io_err.kind()` — vorher landete jeder `io::Error` unterschiedslos
/// in `ConnectionFailed` (abgelehnt, DNS, keine Route: alles gleich, s.
/// Spec 0047 Fund D2, "Lücke"-Spalte). Nur die Kinds, die sich einem der
/// neuen, spezifischeren Fälle eindeutig zuordnen lassen, bekommen eine
/// eigene Variante; alles andere bleibt bewusst `ConnectionFailed` (der
/// bisherige, generische Auffangfall — **keine Verengung**, nur zusätzliche
/// Präzision für die Kinds, die die Spec explizit nennt). `Disconnect`
/// bekam vorher immer den festen Text "Verbindung getrennt" — das war
/// bereits eine Abbruch-während-des-Aufbaus-Situation und wandert deshalb
/// nach `ConnectionClosed`, nicht `ConnectionFailed`.
pub(crate) fn map_russh_error(e: russh::Error) -> SshError {
    match e {
        russh::Error::NotAuthenticated => SshError::AuthenticationFailed,
        russh::Error::Disconnect => SshError::ConnectionClosed("Verbindung getrennt".to_string()),
        russh::Error::IO(io_err) => map_io_error(io_err),
        other => SshError::ChannelError(other.to_string()),
    }
}

fn map_io_error(io_err: std::io::Error) -> SshError {
    use std::io::ErrorKind;

    match io_err.kind() {
        ErrorKind::ConnectionRefused => SshError::ConnectionRefused(io_err.to_string()),
        ErrorKind::TimedOut => SshError::Timeout,
        ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable => {
            SshError::HostUnreachable(io_err.to_string())
        }
        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::UnexpectedEof => {
            SshError::ConnectionClosed(io_err.to_string())
        }
        _ => SshError::ConnectionFailed(io_err.to_string()),
    }
}

#[cfg(test)]
mod map_russh_error_tests {
    //! Spec 0069, Teil A3, Test 6: jede `io::ErrorKind` der Tabelle §4.2 und
    //! `Disconnect` müssen auf die erwartete `SshError`-Variante
    //! abgebildet werden; eine unbekannte Kind bleibt `ConnectionFailed`
    //! (der bisherige Auffangfall — **Gegenbeweis**: vor diesem Commit
    //! bildeten alle diese Kinds unterschiedslos auf `ConnectionFailed` ab,
    //! s. Bericht).
    use std::io::{Error as IoError, ErrorKind};

    use ssh_manager_core::ssh::SshError;

    use super::map_russh_error;

    fn io(kind: ErrorKind) -> russh::Error {
        russh::Error::IO(IoError::new(kind, "probe"))
    }

    #[test]
    fn test_connection_refused_maps_to_connection_refused() {
        assert!(matches!(
            map_russh_error(io(ErrorKind::ConnectionRefused)),
            SshError::ConnectionRefused(_)
        ));
    }

    #[test]
    fn test_timed_out_maps_to_timeout() {
        assert!(matches!(
            map_russh_error(io(ErrorKind::TimedOut)),
            SshError::Timeout
        ));
    }

    #[test]
    fn test_host_unreachable_maps_to_host_unreachable() {
        assert!(matches!(
            map_russh_error(io(ErrorKind::HostUnreachable)),
            SshError::HostUnreachable(_)
        ));
    }

    #[test]
    fn test_network_unreachable_maps_to_host_unreachable() {
        assert!(matches!(
            map_russh_error(io(ErrorKind::NetworkUnreachable)),
            SshError::HostUnreachable(_)
        ));
    }

    #[test]
    fn test_connection_reset_maps_to_connection_closed() {
        assert!(matches!(
            map_russh_error(io(ErrorKind::ConnectionReset)),
            SshError::ConnectionClosed(_)
        ));
    }

    #[test]
    fn test_connection_aborted_maps_to_connection_closed() {
        assert!(matches!(
            map_russh_error(io(ErrorKind::ConnectionAborted)),
            SshError::ConnectionClosed(_)
        ));
    }

    #[test]
    fn test_unexpected_eof_maps_to_connection_closed() {
        assert!(matches!(
            map_russh_error(io(ErrorKind::UnexpectedEof)),
            SshError::ConnectionClosed(_)
        ));
    }

    #[test]
    fn test_disconnect_maps_to_connection_closed() {
        assert!(matches!(
            map_russh_error(russh::Error::Disconnect),
            SshError::ConnectionClosed(_)
        ));
    }

    /// Gegenbeweis-Fall für "keine Verengung": eine `io::ErrorKind`, die
    /// keiner der neuen spezifischen Varianten zugeordnet ist, bleibt der
    /// bisherige generische Fall — nicht `ChannelError`, nicht verloren.
    #[test]
    fn test_unmapped_io_kind_stays_connection_failed() {
        assert!(matches!(
            map_russh_error(io(ErrorKind::PermissionDenied)),
            SshError::ConnectionFailed(_)
        ));
    }
}
