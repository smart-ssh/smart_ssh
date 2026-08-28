use std::path::Path;

use async_trait::async_trait;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::Row;

use ssh_manager_core::profiles::{
    Group, GroupId, NoteEditor, NoteRevision, NoteTarget, ProfileError, ProfileResult,
    ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;

use crate::error::PersistenceResult;
use crate::mapping::{auth_method_from_json, auth_method_to_json, parse_timestamp, parse_uuid};

/// SQLite-gestützte Implementierung von [`ProfileStore`] (Spec 0004,
/// Abschnitt 5).
///
/// Nutzt durchgehend die Runtime-API von `sqlx` (`sqlx::query`/
/// `sqlx::query_scalar`, manuelles Zeilen-Mapping), nicht die
/// compile-time-geprüften `query!`/`query_as!`-Makros — Letztere würden
/// beim Bauen entweder eine laufende DB unter `DATABASE_URL` oder einen
/// eingecheckten Offline-Cache (`cargo sqlx prepare`) voraussetzen, was
/// hier bewusst vermieden wurde. Siehe
/// `docs/adr/0006-sqlx-runtime-checked-queries.md`.
pub struct SqliteProfileStore {
    // `pub(crate)` statt privat: die Testsuite (`crate::tests`, ein
    // Geschwister-, kein Kind-Modul von `store`) muss gelegentlich Zustand
    // direkt über den Pool verifizieren, den der `ProfileStore`-Trait nicht
    // exponiert (z. B. Zeilenzahl in `server_tags`/`note_revisions`) — kein
    // Teil der öffentlichen Crate-API.
    pub(crate) pool: SqlitePool,
}

impl SqliteProfileStore {
    /// Baut den Connection-Pool zu `db_path` auf, führt die eingebetteten
    /// Migrationen aus (`sqlx::migrate!()` — zur Compile-Zeit eingebettet,
    /// nicht zur Laufzeit von Disk gelesen) und aktiviert
    /// `PRAGMA foreign_keys`. Legt fehlende Elternverzeichnisse von
    /// `db_path` an, damit ein frischer App-Datenordner (erster Start) nicht
    /// manuell vorbereitet werden muss.
    pub async fn connect(db_path: &Path) -> PersistenceResult<Self> {
        if let Some(parent) = db_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(sqlx::Error::Io)?;
            }
        }

        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true);
        Self::connect_with(options).await
    }

    /// Interner Einstiegspunkt für Tests: baut einen Store direkt aus
    /// [`SqliteConnectOptions`] auf, z. B. gegen eine In-Memory-DB
    /// (`SqliteConnectOptions::new().filename(":memory:")`), ohne den Umweg
    /// über einen echten Dateipfad zu gehen. Bewusst `pub(crate)`, nicht
    /// Teil der öffentlichen API — die Spec sieht als öffentlichen
    /// Einstiegspunkt nur `connect(db_path: &Path)` vor (Abschnitt 5).
    ///
    /// `max_connections(1)`: SQLite profitiert bei einem einzelnen lokalen
    /// Nutzer/Prozess kaum von mehreren gleichzeitigen Verbindungen (sein
    /// Locking-Modell serialisiert Schreibzugriffe ohnehin), und für
    /// `:memory:`-Datenbanken ist eine einzelne Verbindung sogar
    /// **notwendig**: jede Verbindung zu `:memory:` bekommt sonst ihre
    /// eigene, isolierte Datenbank, sodass ein Pool mit mehr als einer
    /// Verbindung Schreib-/Lesezugriffe zwischen verschiedenen Connections
    /// unsichtbar zueinander machen würde.
    pub(crate) async fn connect_with(options: SqliteConnectOptions) -> PersistenceResult<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;

        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;
        sqlx::migrate!().run(&pool).await?;

        Ok(Self { pool })
    }

    async fn fetch_tags(&self, server_id: &str) -> ProfileResult<Vec<String>> {
        let rows = sqlx::query("SELECT tag FROM server_tags WHERE server_id = ? ORDER BY tag")
            .bind(server_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| ProfileError::Backend(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|row| row.get::<String, _>("tag"))
            .collect())
    }
}

fn row_to_group(row: &sqlx::sqlite::SqliteRow) -> ProfileResult<Group> {
    let id: String = row.get("id");
    let parent_id: Option<String> = row.get("parent_id");
    let created_at: String = row.get("created_at");
    let updated_at: String = row.get("updated_at");

    Ok(Group {
        id: GroupId(parse_uuid(&id, "groups.id")?),
        name: row.get("name"),
        parent_id: parent_id
            .map(|raw| parse_uuid(&raw, "groups.parent_id"))
            .transpose()?
            .map(GroupId),
        notes: row.get("notes"),
        created_at: parse_timestamp(&created_at, "groups.created_at")?,
        updated_at: parse_timestamp(&updated_at, "groups.updated_at")?,
    })
}

fn row_to_server(row: &sqlx::sqlite::SqliteRow, tags: Vec<String>) -> ProfileResult<Server> {
    let id: String = row.get("id");
    let group_id: Option<String> = row.get("group_id");
    let jump_host_id: Option<String> = row.get("jump_host_id");
    let auth_json: String = row.get("auth_method");
    let port_raw: i64 = row.get("port");
    let created_at: String = row.get("created_at");
    let updated_at: String = row.get("updated_at");

    Ok(Server {
        id: ServerId(parse_uuid(&id, "servers.id")?),
        name: row.get("name"),
        host: row.get("host"),
        port: u16::try_from(port_raw)
            .map_err(|_| ProfileError::Backend(format!("port {port_raw} passt nicht in u16")))?,
        username: row.get("username"),
        group_id: group_id
            .map(|raw| parse_uuid(&raw, "servers.group_id"))
            .transpose()?
            .map(GroupId),
        tags,
        auth: auth_method_from_json(&auth_json)?,
        notes: row.get("notes"),
        jump_host: jump_host_id
            .map(|raw| parse_uuid(&raw, "servers.jump_host_id"))
            .transpose()?
            .map(ServerId),
        created_at: parse_timestamp(&created_at, "servers.created_at")?,
        updated_at: parse_timestamp(&updated_at, "servers.updated_at")?,
    })
}

fn backend_err(e: sqlx::Error) -> ProfileError {
    ProfileError::Backend(e.to_string())
}

#[async_trait]
impl ProfileStore for SqliteProfileStore {
    async fn get_group(&self, id: &GroupId) -> ProfileResult<Group> {
        let row = sqlx::query(
            "SELECT id, name, parent_id, notes, created_at, updated_at FROM groups WHERE id = ?",
        )
        .bind(id.0.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend_err)?
        .ok_or(ProfileError::GroupNotFound(*id))?;

        row_to_group(&row)
    }

    async fn get_server(&self, id: &ServerId) -> ProfileResult<Server> {
        let id_str = id.0.to_string();
        let row = sqlx::query(
            "SELECT id, name, host, port, username, group_id, auth_method, notes, \
             jump_host_id, created_at, updated_at FROM servers WHERE id = ?",
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend_err)?
        .ok_or(ProfileError::ServerNotFound(*id))?;

        let tags = self.fetch_tags(&id_str).await?;
        row_to_server(&row, tags)
    }

    // `group_chain` bewusst **nicht** überschrieben: die Default-
    // Implementierung des Traits (wiederholte `get_group`-Aufrufe +
    // Zyklenerkennung) ist für die in einer lokalen Desktop-App realistische
    // Gruppentiefe (wenige Ebenen) völlig ausreichend performant. Eine
    // rekursive SQL-CTE wäre eine verfrühte Optimierung ohne aktuellen
    // Bedarf und würde die Zyklenerkennungs-Logik duplizieren, statt sie
    // wiederzuverwenden.

    async fn create_group(&self, group: &Group) -> ProfileResult<()> {
        sqlx::query(
            "INSERT INTO groups (id, name, parent_id, notes, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(group.id.0.to_string())
        .bind(&group.name)
        .bind(group.parent_id.map(|g| g.0.to_string()))
        .bind(&group.notes)
        .bind(group.created_at.to_rfc3339())
        .bind(group.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(backend_err)?;
        Ok(())
    }

    async fn update_group(&self, group: &Group) -> ProfileResult<()> {
        let result = sqlx::query(
            "UPDATE groups SET name = ?, parent_id = ?, notes = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&group.name)
        .bind(group.parent_id.map(|g| g.0.to_string()))
        .bind(&group.notes)
        .bind(group.updated_at.to_rfc3339())
        .bind(group.id.0.to_string())
        .execute(&self.pool)
        .await
        .map_err(backend_err)?;

        if result.rows_affected() == 0 {
            return Err(ProfileError::GroupNotFound(group.id));
        }
        Ok(())
    }

    async fn delete_group(&self, id: &GroupId) -> ProfileResult<()> {
        // `ON DELETE CASCADE` (Kind-Gruppen) und `ON DELETE SET NULL`
        // (Server.group_id) übernimmt SQLite selbst, s. Migration.
        let result = sqlx::query("DELETE FROM groups WHERE id = ?")
            .bind(id.0.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend_err)?;

        if result.rows_affected() == 0 {
            return Err(ProfileError::GroupNotFound(*id));
        }
        Ok(())
    }

    async fn create_server(&self, server: &Server) -> ProfileResult<()> {
        let mut tx = self.pool.begin().await.map_err(backend_err)?;

        sqlx::query(
            "INSERT INTO servers \
             (id, name, host, port, username, group_id, auth_method, notes, jump_host_id, \
              created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(server.id.0.to_string())
        .bind(&server.name)
        .bind(&server.host)
        .bind(i64::from(server.port))
        .bind(&server.username)
        .bind(server.group_id.map(|g| g.0.to_string()))
        .bind(auth_method_to_json(&server.auth)?)
        .bind(&server.notes)
        .bind(server.jump_host.map(|s| s.0.to_string()))
        .bind(server.created_at.to_rfc3339())
        .bind(server.updated_at.to_rfc3339())
        .execute(&mut *tx)
        .await
        .map_err(backend_err)?;

        for tag in &server.tags {
            sqlx::query("INSERT INTO server_tags (server_id, tag) VALUES (?, ?)")
                .bind(server.id.0.to_string())
                .bind(tag)
                .execute(&mut *tx)
                .await
                .map_err(backend_err)?;
        }

        tx.commit().await.map_err(backend_err)?;
        Ok(())
    }

    async fn update_server(&self, server: &Server) -> ProfileResult<()> {
        let mut tx = self.pool.begin().await.map_err(backend_err)?;

        let result = sqlx::query(
            "UPDATE servers SET name = ?, host = ?, port = ?, username = ?, group_id = ?, \
             auth_method = ?, notes = ?, jump_host_id = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&server.name)
        .bind(&server.host)
        .bind(i64::from(server.port))
        .bind(&server.username)
        .bind(server.group_id.map(|g| g.0.to_string()))
        .bind(auth_method_to_json(&server.auth)?)
        .bind(&server.notes)
        .bind(server.jump_host.map(|s| s.0.to_string()))
        .bind(server.updated_at.to_rfc3339())
        .bind(server.id.0.to_string())
        .execute(&mut *tx)
        .await
        .map_err(backend_err)?;

        if result.rows_affected() == 0 {
            // `tx` wird beim Verlassen des Scopes gedroppt statt committet
            // -> automatischer Rollback, keine halbfertige Änderung sichtbar.
            return Err(ProfileError::ServerNotFound(server.id));
        }

        sqlx::query("DELETE FROM server_tags WHERE server_id = ?")
            .bind(server.id.0.to_string())
            .execute(&mut *tx)
            .await
            .map_err(backend_err)?;

        for tag in &server.tags {
            sqlx::query("INSERT INTO server_tags (server_id, tag) VALUES (?, ?)")
                .bind(server.id.0.to_string())
                .bind(tag)
                .execute(&mut *tx)
                .await
                .map_err(backend_err)?;
        }

        tx.commit().await.map_err(backend_err)?;
        Ok(())
    }

    async fn delete_server(&self, id: &ServerId) -> ProfileResult<()> {
        // `server_tags` wird per `ON DELETE CASCADE` automatisch entfernt.
        let result = sqlx::query("DELETE FROM servers WHERE id = ?")
            .bind(id.0.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend_err)?;

        if result.rows_affected() == 0 {
            return Err(ProfileError::ServerNotFound(*id));
        }
        Ok(())
    }

    async fn record_note_revision(&self, revision: &NoteRevision) -> ProfileResult<()> {
        let mut tx = self.pool.begin().await.map_err(backend_err)?;

        let (target_type, target_id): (&str, String) = match revision.target {
            NoteTarget::Server(id) => ("server", id.0.to_string()),
            NoteTarget::Group(id) => ("group", id.0.to_string()),
        };
        let (editor_type, ai_provider, ai_model): (&str, Option<&str>, Option<&str>) =
            match &revision.edited_by {
                NoteEditor::User => ("user", None, None),
                NoteEditor::Ai { provider, model } => {
                    ("ai", Some(provider.as_str()), Some(model.as_str()))
                }
            };

        sqlx::query(
            "INSERT INTO note_revisions \
             (id, target_type, target_id, content, editor_type, ai_provider, ai_model, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(revision.id.to_string())
        .bind(target_type)
        .bind(&target_id)
        .bind(&revision.content)
        .bind(editor_type)
        .bind(ai_provider)
        .bind(ai_model)
        .bind(revision.created_at.to_rfc3339())
        .execute(&mut *tx)
        .await
        .map_err(backend_err)?;

        // Dieselbe Transaktion aktualisiert das `notes`-Feld des Ziels —
        // Insert in `note_revisions` und Update auf `servers`/`groups`
        // gelingen oder scheitern gemeinsam (Spec 0004, Abschnitt 6:
        // "note_revisions werden ... zusätzlich zum aktuellen notes-Feld
        // persistiert, nicht ersetzt").
        let update_result = match revision.target {
            NoteTarget::Server(_) => {
                sqlx::query("UPDATE servers SET notes = ?, updated_at = ? WHERE id = ?")
                    .bind(&revision.content)
                    .bind(revision.created_at.to_rfc3339())
                    .bind(&target_id)
                    .execute(&mut *tx)
                    .await
            }
            NoteTarget::Group(_) => {
                sqlx::query("UPDATE groups SET notes = ?, updated_at = ? WHERE id = ?")
                    .bind(&revision.content)
                    .bind(revision.created_at.to_rfc3339())
                    .bind(&target_id)
                    .execute(&mut *tx)
                    .await
            }
        }
        .map_err(backend_err)?;

        if update_result.rows_affected() == 0 {
            return Err(match revision.target {
                NoteTarget::Server(id) => ProfileError::ServerNotFound(id),
                NoteTarget::Group(id) => ProfileError::GroupNotFound(id),
            });
        }

        tx.commit().await.map_err(backend_err)?;
        Ok(())
    }
}
