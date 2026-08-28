use std::collections::HashSet;
use std::fmt;

use async_trait::async_trait;

use crate::shared::ServerId;

use super::types::{Group, GroupId, NoteRevision, Server};

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
    /// Backend-spezifischer Fehler (z. B. eine echte DB-Anbindung wie
    /// `persistence-sqlite`, Spec 0004), der sich keiner der obigen
    /// fachlichen Varianten zuordnen lässt — nur die Fehlermeldung, kein
    /// strukturierter Zugriff auf den Original-Fehlertyp. `core` selbst darf
    /// keine I/O-/DB-Abhängigkeit bekommen (Spec 0004 Abschnitt 1), daher
    /// kann hier kein `From<sqlx::Error>` o. ä. implementiert werden — jede
    /// konkrete Store-Implementierung wandelt ihre eigenen Fehler über
    /// `.to_string()` in diese Variante um.
    Backend(String),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::ServerNotFound(id) => write!(f, "Server {id:?} nicht gefunden"),
            ProfileError::GroupNotFound(id) => write!(f, "Gruppe {id:?} nicht gefunden"),
            ProfileError::CycleDetected => {
                write!(f, "zyklische Gruppen-Elternkette erkannt")
            }
            ProfileError::Backend(msg) => write!(f, "Profile-Store-Backend-Fehler: {msg}"),
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

    /// Alle gespeicherten Server, für die einfache Serverliste (Spec 0007,
    /// Abschnitt 7 — "keine Anlege-/Bearbeiten-UI", nur Anzeige). War in
    /// Spec 0003/0004 nicht vorgesehen (dort nur gezielte
    /// `get_server(id)`-Lookups), wird aber von Spec 0007s
    /// `list_servers`-Tauri-Befehl vorausgesetzt — daher hier als
    /// zusätzliche Trait-Methode ergänzt statt in `app-tauri` mit
    /// Store-internen Interna zu umgehen.
    async fn list_servers(&self) -> ProfileResult<Vec<Server>>;

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

    // --- Schreibende Operationen ------------------------------------------
    //
    // Ursprünglich (Spec 0003, Abschnitt 5) hatte `ProfileStore` nur
    // Lese-Methoden (`get_server`/`get_group`/`group_chain`) — die
    // SQLite-Anbindung (Spec 0004) braucht aber zwingend auch schreibende
    // Operationen, um Server/Gruppen anzulegen und Notiz-Änderungen zu
    // persistieren. Bewusst hier am Trait ergänzt statt nur als Inherent-
    // Methoden auf `SqliteProfileStore`: sonst könnte `InMemoryProfileStore`
    // in Tests nicht auf demselben Weg befüllt werden wie eine echte
    // DB-Implementierung, und ein künftiger zweiter Store (z. B. für Sync)
    // müsste dieselben Methoden separat neu erfinden statt sie über den
    // Trait erzwungen zu bekommen.

    async fn create_group(&self, group: &Group) -> ProfileResult<()>;
    async fn update_group(&self, group: &Group) -> ProfileResult<()>;
    async fn delete_group(&self, id: &GroupId) -> ProfileResult<()>;

    async fn create_server(&self, server: &Server) -> ProfileResult<()>;
    async fn update_server(&self, server: &Server) -> ProfileResult<()>;
    async fn delete_server(&self, id: &ServerId) -> ProfileResult<()>;

    /// Schreibt eine neue [`NoteRevision`] in die Änderungs-Historie
    /// (Spec 0003 Abschnitt 5.3) **und** aktualisiert atomar das aktuelle
    /// `notes`-Feld des in `revision.target` referenzierten Servers/Gruppe
    /// auf `revision.content` — beides muss zusammen gelingen oder
    /// zusammen fehlschlagen (konkret umgesetzt als DB-Transaktion in
    /// `persistence-sqlite`), damit `notes`-Feld und Historie nie
    /// auseinanderlaufen.
    async fn record_note_revision(&self, revision: &NoteRevision) -> ProfileResult<()>;
}
