//! Probe commands of the elevation check (Spec 0067), split out of
//! `elevated_sftp.rs`.

use app_logic::session::Session;

/// Obergrenze je Probe-Kommando (Pfad-Erkennung, `sudo -n -l`) — `-n` lässt
/// sudo nie auf ein Passwort warten, das hier ist nur das Sicherheitsnetz.
pub(crate) const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Runs one probe command. Issue #923: on a transport with split start
/// (issue #328) the transport lock is held only until the exec channel is
/// open, so a disconnect is not blocked for up to [`PROBE_TIMEOUT`]; a
/// transport without it keeps the lock for the whole probe, as before.
/// Lock hierarchy: docs/architecture.md, "Lock hierarchy of a session".
pub(crate) async fn run_probe(
    session: &Session,
    command: &str,
) -> Result<ssh_manager_core::ssh::CommandOutput, String> {
    // The sender must outlive the run: a dropped sender would cancel it.
    let (_cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let (sink, _sink_rx) = ssh_manager_core::ssh::exec_output_channel();
    let mut transport = session.transport.lock().await;
    let started = tokio::time::timeout(PROBE_TIMEOUT, transport.start_exec(command, None)).await;
    match started {
        Ok(Ok(Some(running))) => {
            drop(transport);
            match tokio::time::timeout(PROBE_TIMEOUT, running.run(cancel_rx, sink)).await {
                Ok(Ok(outcome)) => Ok(outcome.output),
                Ok(Err(err)) => Err(err.to_string()),
                Err(_) => Err("Zeitüberschreitung".to_string()),
            }
        }
        Ok(Ok(None)) => match tokio::time::timeout(PROBE_TIMEOUT, transport.execute(command)).await
        {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(err)) => Err(err.to_string()),
            Err(_) => Err("Zeitüberschreitung".to_string()),
        },
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err("Zeitüberschreitung".to_string()),
    }
}
