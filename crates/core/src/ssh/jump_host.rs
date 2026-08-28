use std::collections::HashSet;

use crate::profiles::{ProfileStore, Server};
use crate::shared::ServerId;

use super::error::SshError;
use super::types::{ConnectionTarget, Hop};

/// Löst die `jump_host`-Kette eines [`Server`]-Profils rekursiv über einen
/// [`ProfileStore`] zu einem [`ConnectionTarget`] auf (Spec 0005, Abschnitt
/// 5): erster Hop = äußerster Jump-Host, letzter Hop = `server` selbst.
///
/// Zirkelsicher, analog zu `ProfileStore::group_chain` (Spec 0003) — eine
/// zyklische `jump_host`-Kette (Store-Fehler/korrupte Daten) ergibt
/// [`SshError::JumpHostCycle`] statt einer Endlosschleife oder eines
/// Panics.
pub async fn resolve_connection_target(
    server: &Server,
    store: &dyn ProfileStore,
) -> Result<ConnectionTarget, SshError> {
    let mut chain = vec![server.clone()];
    let mut visited: HashSet<ServerId> = HashSet::new();
    visited.insert(server.id);

    let mut next = server.jump_host;
    while let Some(jump_id) = next {
        if !visited.insert(jump_id) {
            return Err(SshError::JumpHostCycle);
        }
        let jump_server = store.get_server(&jump_id).await.map_err(|e| {
            SshError::ConnectionFailed(format!("Jump-Host {jump_id:?} nicht auflösbar: {e}"))
        })?;
        next = jump_server.jump_host;
        chain.push(jump_server);
    }

    // `chain` ist aktuell [Ziel, nächster Jump-Host, ..., äußerster
    // Jump-Host] (wir sind vom Ziel ausgehend rückwärts durch die Kette
    // gelaufen) — für "erster Hop zuerst, Ziel zuletzt" umdrehen.
    chain.reverse();

    let hops = chain.into_iter().map(server_to_hop).collect();
    Ok(ConnectionTarget { hops })
}

fn server_to_hop(server: Server) -> Hop {
    Hop {
        host: server.host,
        port: server.port,
        username: server.username,
        auth: server.auth,
    }
}
