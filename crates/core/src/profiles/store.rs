use std::collections::HashSet;
use std::fmt;

use async_trait::async_trait;

use crate::shared::ServerId;

use super::types::{Group, GroupId, Server};

/// Fehler eines [`ProfileStore`]-Zugriffs bzw. einer darauf aufbauenden
/// Operation wie `group_chain`/`effective_notes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    ServerNotFound(ServerId),
    GroupNotFound(GroupId),
    /// Die Eltern-Kette einer Gruppe verweist zyklisch auf sich selbst
    /// (Store-Fehler/korrupte Daten) — kein Panic, sondern ein regulärer
    /// Fehler, s. `group_chain`.
    CycleDetected,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::ServerNotFound(id) => write!(f, "Server {id:?} nicht gefunden"),
            ProfileError::GroupNotFound(id) => write!(f, "Gruppe {id:?} nicht gefunden"),
            ProfileError::CycleDetected => {
                write!(f, "zyklische Gruppen-Elternkette erkannt")
            }
        }
    }
}

impl std::error::Error for ProfileError {}

pub type ProfileResult<T> = Result<T, ProfileError>;

/// Quelle für Server- und Gruppen-Daten (Spec 0003, Abschnitt 5.1), analog
/// zum `PolicyStore`-Muster aus Spec 0002: als Trait modelliert, damit Tests
/// (und die reine `effective_notes`-Logik) eine In-Memory-Implementierung
/// nutzen können, ohne von der späteren DB-Anbindung abzuhängen.
///
/// `async fn` über die `async-trait`-Crate, damit eine echte DB-Anbindung
/// (SQLite über `sqlx`, siehe Spec 0004) Netz-/Datei-I/O nicht blockierend
/// ausführen kann. `async-trait` boxt die zurückgegebenen Futures, damit der
/// Trait weiterhin als `dyn ProfileStore` nutzbar bleibt — native `async fn`
/// in Traits ist (Stand der in diesem Workspace verwendeten Rust-Version)
/// nicht dyn-kompatibel.
#[async_trait]
pub trait ProfileStore: Send + Sync {
    async fn get_server(&self, id: &ServerId) -> ProfileResult<Server>;
    async fn get_group(&self, id: &GroupId) -> ProfileResult<Group>;

    /// Gruppenkette von der Wurzel bis **einschließlich** `id`, root-first
    /// geordnet (passend für `effective_notes`, Spec Abschnitt 5.1).
    ///
    /// Default-Implementierung auf Basis von [`ProfileStore::get_group`] —
    /// Implementierer müssen nur die beiden Lookup-Methoden bereitstellen,
    /// bekommen eine korrekte, zyklensichere Traversierung geschenkt statt
    /// sie selbst (fehleranfällig) nachbauen zu müssen. Bricht mit
    /// [`ProfileError::CycleDetected`] ab, statt endlos zu laufen, falls die
    /// `parent_id`-Kette (durch einen Store-Fehler) zyklisch würde.
    async fn group_chain(&self, id: &GroupId) -> ProfileResult<Vec<Group>> {
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut current = Some(*id);

        while let Some(group_id) = current {
            if !visited.insert(group_id) {
                return Err(ProfileError::CycleDetected);
            }
            let group = self.get_group(&group_id).await?;
            current = group.parent_id;
            chain.push(group);
        }

        chain.reverse();
        Ok(chain)
    }
}
