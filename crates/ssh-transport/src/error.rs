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
pub(crate) fn map_russh_error(e: russh::Error) -> SshError {
    match e {
        russh::Error::NotAuthenticated => SshError::AuthenticationFailed,
        russh::Error::Disconnect => SshError::ConnectionFailed("Verbindung getrennt".to_string()),
        russh::Error::IO(io_err) => SshError::ConnectionFailed(io_err.to_string()),
        other => SshError::ChannelError(other.to_string()),
    }
}
