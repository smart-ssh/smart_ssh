//! SQLite-gestützte Verwaltung von `ai_provider_configs` (Spec 0007,
//! Abschnitt 8).
//!
//! **Eigener Store statt Erweiterung von [`crate::SqliteProfileStore`]**:
//! `SqliteProfileStore` implementiert den `ProfileStore`-Trait aus
//! `core::profiles` (Spec 0003/0004) — dessen Interface ist durch die
//! Trait-Definition in `core` festgelegt und hat mit AI-Provider-
//! Konfigurationen nichts zu tun. Spec 0007 definiert für
//! `ai_provider_configs` **keinen** eigenen `core`-Trait (anders als bei
//! `ProfileStore`/`SshTransport`/`AiProvider`), sondern beschreibt nur
//! konkretes CRUD-Verhalten (Abschnitt 8.2) — es gibt also weder einen
//! Trait, den `SqliteProfileStore` zusätzlich implementieren müsste, noch
//! einen Grund, zwei fachlich unabhängige Verantwortlichkeiten
//! (Server/Gruppen-Profile vs. AI-Provider-Konfiguration) in einem Typ zu
//! bündeln. `SqliteAiProviderStore` ist deshalb ein eigener, kleiner Typ,
//! der sich lediglich denselben `SqlitePool` teilt (s.
//! `SqliteAiProviderStore::new` und `crate::store::SqliteProfileStore::pool`
//! weiter unten) statt eine zweite, unabhängige Verbindung/Migration
//! aufzumachen.

use chrono::{DateTime, Utc};
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

use ssh_manager_core::ai::{ProviderId, ProviderType};
use ssh_manager_core::profiles::CredentialRef;

/// Vollständige gespeicherte Konfiguration inkl. `credential_ref` — die
/// reine Persistenzsicht. `crates/app-shell` bildet daraus die
/// nach-außen sichtbare `AiProviderConfigDto` (ohne `credential_ref`, s.
/// Spec 0007 Abschnitt 8.2: "bewusst KEIN api_key-Feld — der Key geht nie
/// zurück ans Frontend"; `credential_ref` selbst ist zwar kein Secret, aber
/// ebenfalls reines Backend-Implementierungsdetail).
#[derive(Debug, Clone, PartialEq)]
pub struct AiProviderConfig {
    pub id: ProviderId,
    pub provider_type: ProviderType,
    pub display_name: String,
    pub base_url: Option<String>,
    pub model: String,
    pub supports_native_tool_calling: bool,
    pub credential_ref: CredentialRef,
    pub is_active: bool,
    /// Spec 0025, Abschnitt 3 — als JSON-Array in der `extra_headers`-Spalte
    /// gespeichert (s. Migration `0005_...sql`), hier bereits geparst.
    pub extra_headers: Vec<(String, String)>,
    /// Spec 0025, Abschnitt 4.
    pub attestation_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Felder, die `update_ai_provider` (Spec 0007 Abschnitt 8.2) ändern darf.
/// Bewusst **kein** `is_active`/`credential_ref`/`created_at` — `is_active`
/// wird ausschließlich über [`SqliteAiProviderStore::set_active`] geändert
/// (eigener Befehl `set_active_ai_provider`, s. Spec Abschnitt 4),
/// `credential_ref` ändert sich nie nach dem Anlegen (derselbe Verweis wird
/// nur im `CredentialStore` überschrieben, s. Spec Abschnitt 8.2 zum
/// leeren `api_key`-Feld). Ein eigener Typ statt der vollen
/// `AiProviderConfig` verhindert, dass ein Aufrufer versehentlich erwartet,
/// über dieses Update auch `is_active` setzen zu können.
#[derive(Debug, Clone, PartialEq)]
pub struct AiProviderConfigUpdate {
    pub id: ProviderId,
    pub provider_type: ProviderType,
    pub display_name: String,
    pub base_url: Option<String>,
    pub model: String,
    pub supports_native_tool_calling: bool,
    pub extra_headers: Vec<(String, String)>,
    pub attestation_url: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiProviderStoreError {
    NotFound(ProviderId),
    /// Spec 0007, Abschnitt 9: Löschen eines aktiven Providers ist
    /// verboten, nicht automatisch aufgelöst (verbindliche Entscheidung,
    /// s. Aufgabenstellung).
    ActiveProviderDeletionForbidden(ProviderId),
    Backend(String),
}

impl std::fmt::Display for AiProviderStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiProviderStoreError::NotFound(id) => {
                write!(f, "AI-Provider-Konfiguration '{}' nicht gefunden", id.0)
            }
            AiProviderStoreError::ActiveProviderDeletionForbidden(id) => write!(
                f,
                "AI-Provider-Konfiguration '{}' ist aktiv und kann nicht gelöscht werden — \
                 zuerst einen anderen Provider aktiv setzen",
                id.0
            ),
            AiProviderStoreError::Backend(msg) => write!(f, "Datenbankfehler: {msg}"),
        }
    }
}

impl std::error::Error for AiProviderStoreError {}

fn backend_err(e: sqlx::Error) -> AiProviderStoreError {
    AiProviderStoreError::Backend(e.to_string())
}

fn parse_uuid(raw: &str) -> Result<uuid::Uuid, AiProviderStoreError> {
    uuid::Uuid::parse_str(raw)
        .map_err(|e| AiProviderStoreError::Backend(format!("ungültige UUID '{raw}': {e}")))
}

fn parse_timestamp(raw: &str) -> Result<DateTime<Utc>, AiProviderStoreError> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| AiProviderStoreError::Backend(format!("ungültiger Zeitstempel '{raw}': {e}")))
}

/// (De-)Serialisierung von `extra_headers` als JSON-Array-Spalte — s.
/// Doc-Kommentar der Migration `0005_...sql` zur Begründung, warum kein
/// eigener Tabellen-Join.
fn encode_extra_headers(headers: &[(String, String)]) -> String {
    serde_json::to_string(headers).expect("Vec<(String, String)> ist immer serialisierbar")
}

fn parse_extra_headers(raw: &str) -> Result<Vec<(String, String)>, AiProviderStoreError> {
    serde_json::from_str(raw)
        .map_err(|e| AiProviderStoreError::Backend(format!("ungültige extra_headers '{raw}': {e}")))
}

fn row_to_config(row: &sqlx::sqlite::SqliteRow) -> Result<AiProviderConfig, AiProviderStoreError> {
    let id: String = row.get("id");
    let provider_type_raw: String = row.get("provider_type");
    let created_at: String = row.get("created_at");
    let updated_at: String = row.get("updated_at");

    let provider_type = ProviderType::from_db_str(&provider_type_raw).ok_or_else(|| {
        AiProviderStoreError::Backend(format!(
            "unbekannter provider_type '{provider_type_raw}' in der Datenbank"
        ))
    })?;

    let extra_headers_raw: String = row.get("extra_headers");

    Ok(AiProviderConfig {
        id: ProviderId(parse_uuid(&id)?),
        provider_type,
        display_name: row.get("display_name"),
        base_url: row.get("base_url"),
        model: row.get("model"),
        supports_native_tool_calling: row.get("supports_native_tool_calling"),
        credential_ref: CredentialRef::new(row.get::<String, _>("credential_ref")),
        is_active: row.get("is_active"),
        extra_headers: parse_extra_headers(&extra_headers_raw)?,
        attestation_url: row.get("attestation_url"),
        created_at: parse_timestamp(&created_at)?,
        updated_at: parse_timestamp(&updated_at)?,
    })
}

pub struct SqliteAiProviderStore {
    pool: SqlitePool,
}

impl SqliteAiProviderStore {
    /// Teilt sich `pool` mit einem bereits verbundenen [`crate::SqliteProfileStore`]
    /// (dessen `connect()` die Migrationen — inkl. `0002_ai_provider_configs.sql`
    /// — bereits ausgeführt hat). Führt selbst **keine** Migration/Verbindung
    /// auf, um nicht versehentlich eine zweite, gegen `:memory:` isolierte
    /// Datenbank aufzumachen (s. Modul-Kommentar von
    /// `crate::store::SqliteProfileStore::connect_with`).
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list(&self) -> Result<Vec<AiProviderConfig>, AiProviderStoreError> {
        let rows = sqlx::query(
            "SELECT id, provider_type, display_name, base_url, model, \
             supports_native_tool_calling, credential_ref, is_active, extra_headers, \
             attestation_url, created_at, updated_at \
             FROM ai_provider_configs ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend_err)?;

        rows.iter().map(row_to_config).collect()
    }

    /// Einzelabfrage — genutzt von `app-shell`s `delete_ai_provider`-Befehl,
    /// um `is_active` **vor** dem (nicht rücknehmbaren) `CredentialStore::delete()`
    /// zu prüfen, ohne die komplette Liste zu laden (s. Spec 0007 Abschnitt
    /// 8.2 zur Lösch-Reihenfolge und Abschnitt 9 zum Aktiv-Löschverbot).
    pub async fn get(&self, id: &ProviderId) -> Result<AiProviderConfig, AiProviderStoreError> {
        let row = sqlx::query(
            "SELECT id, provider_type, display_name, base_url, model, \
             supports_native_tool_calling, credential_ref, is_active, extra_headers, \
             attestation_url, created_at, updated_at \
             FROM ai_provider_configs WHERE id = ?",
        )
        .bind(id.0.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend_err)?
        .ok_or(AiProviderStoreError::NotFound(*id))?;

        row_to_config(&row)
    }

    /// Legt `config` an. `is_active` wird bewusst **nicht** aus `config`
    /// übernommen, sondern hart auf `FALSE` gesetzt: `AiProviderConfigInput`
    /// (Spec 0007 Abschnitt 8.2) kennt gar kein `is_active`-Feld — ein neu
    /// angelegter Provider wird nie in derselben Operation aktiv geschaltet,
    /// das passiert ausschließlich über den separaten
    /// `set_active_ai_provider`-Befehl ([`Self::set_active`]). Damit kann
    /// `create` den Unique-Index (höchstens ein aktiver Provider) nie
    /// verletzen und braucht keine eigene Transaktion.
    pub async fn create(&self, config: &AiProviderConfig) -> Result<(), AiProviderStoreError> {
        sqlx::query(
            "INSERT INTO ai_provider_configs \
             (id, provider_type, display_name, base_url, model, supports_native_tool_calling, \
              credential_ref, is_active, extra_headers, attestation_url, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, FALSE, ?, ?, ?, ?)",
        )
        .bind(config.id.0.to_string())
        .bind(config.provider_type.as_db_str())
        .bind(&config.display_name)
        .bind(&config.base_url)
        .bind(&config.model)
        .bind(config.supports_native_tool_calling)
        .bind(config.credential_ref.as_str())
        .bind(encode_extra_headers(&config.extra_headers))
        .bind(&config.attestation_url)
        .bind(config.created_at.to_rfc3339())
        .bind(config.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(backend_err)?;
        Ok(())
    }

    /// Aktualisiert die in [`AiProviderConfigUpdate`] genannten Felder.
    /// Rührt weder `credential_ref` noch `is_active` an (s. Doc-Kommentar
    /// dort).
    pub async fn update_fields(
        &self,
        update: &AiProviderConfigUpdate,
    ) -> Result<(), AiProviderStoreError> {
        let result = sqlx::query(
            "UPDATE ai_provider_configs SET provider_type = ?, display_name = ?, base_url = ?, \
             model = ?, supports_native_tool_calling = ?, extra_headers = ?, \
             attestation_url = ?, updated_at = ? WHERE id = ?",
        )
        .bind(update.provider_type.as_db_str())
        .bind(&update.display_name)
        .bind(&update.base_url)
        .bind(&update.model)
        .bind(update.supports_native_tool_calling)
        .bind(encode_extra_headers(&update.extra_headers))
        .bind(&update.attestation_url)
        .bind(update.updated_at.to_rfc3339())
        .bind(update.id.0.to_string())
        .execute(&self.pool)
        .await
        .map_err(backend_err)?;

        if result.rows_affected() == 0 {
            return Err(AiProviderStoreError::NotFound(update.id));
        }
        Ok(())
    }

    /// Setzt `id` aktiv und alle anderen automatisch inaktiv — in einer
    /// Transaktion, wie von der Aufgabenstellung gefordert (kein manuelles
    /// Vorher-Deaktivieren durch den Aufrufer nötig). Erfüllt damit den
    /// `idx_ai_provider_single_active`-Unique-Index, ohne dass ein
    /// Zwischenzustand mit zwei aktiven Providern sichtbar werden könnte.
    pub async fn set_active(
        &self,
        id: &ProviderId,
        updated_at: DateTime<Utc>,
    ) -> Result<(), AiProviderStoreError> {
        let mut tx = self.pool.begin().await.map_err(backend_err)?;
        let id_str = id.0.to_string();
        let updated_at_str = updated_at.to_rfc3339();

        sqlx::query(
            "UPDATE ai_provider_configs SET is_active = FALSE, updated_at = ? \
             WHERE id != ? AND is_active = TRUE",
        )
        .bind(&updated_at_str)
        .bind(&id_str)
        .execute(&mut *tx)
        .await
        .map_err(backend_err)?;

        let result = sqlx::query(
            "UPDATE ai_provider_configs SET is_active = TRUE, updated_at = ? WHERE id = ?",
        )
        .bind(&updated_at_str)
        .bind(&id_str)
        .execute(&mut *tx)
        .await
        .map_err(backend_err)?;

        if result.rows_affected() == 0 {
            // Rollback durch Drop von `tx` statt commit — kein anderer
            // Provider bleibt versehentlich deaktiviert, wenn `id` gar
            // nicht existiert.
            return Err(AiProviderStoreError::NotFound(*id));
        }

        tx.commit().await.map_err(backend_err)?;
        Ok(())
    }

    /// Löscht `id` — schlägt mit
    /// [`AiProviderStoreError::ActiveProviderDeletionForbidden`] fehl, wenn
    /// der Provider gerade aktiv ist (Spec 0007, Abschnitt 9: als
    /// verbindliche Entscheidung "verbieten" statt automatisch
    /// aufzulösen). `SELECT` und `DELETE` laufen nicht in einer expliziten
    /// Transaktion: der Pool dieser Crate hat durchgehend
    /// `max_connections(1)` (s. `crate::store::SqliteProfileStore::connect_with`),
    /// wodurch alle Anfragen ohnehin seriell auf derselben Verbindung
    /// laufen — ein TOCTOU-Fenster zwischen den beiden Schritten kann in
    /// diesem Prozess nicht auftreten.
    pub async fn delete(&self, id: &ProviderId) -> Result<(), AiProviderStoreError> {
        let id_str = id.0.to_string();
        let is_active: Option<bool> =
            sqlx::query_scalar("SELECT is_active FROM ai_provider_configs WHERE id = ?")
                .bind(&id_str)
                .fetch_optional(&self.pool)
                .await
                .map_err(backend_err)?;

        match is_active {
            None => Err(AiProviderStoreError::NotFound(*id)),
            Some(true) => Err(AiProviderStoreError::ActiveProviderDeletionForbidden(*id)),
            Some(false) => {
                sqlx::query("DELETE FROM ai_provider_configs WHERE id = ?")
                    .bind(&id_str)
                    .execute(&self.pool)
                    .await
                    .map_err(backend_err)?;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqliteConnectOptions;

    use super::*;
    use crate::SqliteProfileStore;

    async fn in_memory_ai_provider_store() -> SqliteAiProviderStore {
        let options = SqliteConnectOptions::new().filename(":memory:");
        SqliteProfileStore::connect_with(options)
            .await
            .expect("In-Memory-Store mit angewendeten Migrationen sollte immer aufbaubar sein")
            .ai_provider_store()
    }

    fn make_config(display_name: &str) -> AiProviderConfig {
        let now = Utc::now();
        AiProviderConfig {
            id: ProviderId::new(),
            provider_type: ProviderType::Anthropic,
            display_name: display_name.to_string(),
            base_url: None,
            model: "claude-sonnet-5".to_string(),
            supports_native_tool_calling: true,
            credential_ref: CredentialRef::new(format!("ai-provider:{display_name}")),
            is_active: false,
            extra_headers: Vec::new(),
            attestation_url: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn test_create_and_list_roundtrip_starts_inactive_regardless_of_input() {
        let store = in_memory_ai_provider_store().await;
        let mut config = make_config("Anthropic Prod");
        config.is_active = true; // wird von `create` ignoriert, s. Doc-Kommentar

        store.create(&config).await.unwrap();

        let listed = store.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, config.id);
        assert_eq!(listed[0].display_name, "Anthropic Prod");
        assert!(!listed[0].is_active);
    }

    #[tokio::test]
    async fn test_update_fields_changes_metadata_but_not_credential_ref() {
        let store = in_memory_ai_provider_store().await;
        let config = make_config("Vorher");
        store.create(&config).await.unwrap();

        let update = AiProviderConfigUpdate {
            id: config.id,
            provider_type: ProviderType::OpenAi,
            display_name: "Nachher".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            model: "gpt-test".to_string(),
            supports_native_tool_calling: false,
            extra_headers: vec![("X-Title".to_string(), "Smart SSH".to_string())],
            attestation_url: Some("https://attest.example/report".to_string()),
            updated_at: Utc::now(),
        };
        store.update_fields(&update).await.unwrap();

        let listed = store.list().await.unwrap();
        assert_eq!(listed[0].display_name, "Nachher");
        assert_eq!(listed[0].provider_type, ProviderType::OpenAi);
        assert_eq!(
            listed[0].base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert!(!listed[0].supports_native_tool_calling);
        assert_eq!(
            listed[0].extra_headers,
            vec![("X-Title".to_string(), "Smart SSH".to_string())]
        );
        assert_eq!(
            listed[0].attestation_url.as_deref(),
            Some("https://attest.example/report")
        );
        assert_eq!(
            listed[0].credential_ref, config.credential_ref,
            "update_fields darf credential_ref nicht ändern"
        );
    }

    #[tokio::test]
    async fn test_update_fields_on_unknown_id_yields_not_found() {
        let store = in_memory_ai_provider_store().await;
        let update = AiProviderConfigUpdate {
            id: ProviderId::new(),
            provider_type: ProviderType::Ollama,
            display_name: "x".to_string(),
            base_url: None,
            model: "x".to_string(),
            supports_native_tool_calling: true,
            extra_headers: Vec::new(),
            attestation_url: None,
            updated_at: Utc::now(),
        };

        let result = store.update_fields(&update).await;

        assert!(matches!(result, Err(AiProviderStoreError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_set_active_deactivates_previously_active_provider() {
        let store = in_memory_ai_provider_store().await;
        let first = make_config("Erster");
        let second = make_config("Zweiter");
        store.create(&first).await.unwrap();
        store.create(&second).await.unwrap();

        store.set_active(&first.id, Utc::now()).await.unwrap();
        store.set_active(&second.id, Utc::now()).await.unwrap();

        let listed = store.list().await.unwrap();
        let active: Vec<_> = listed.iter().filter(|c| c.is_active).collect();
        assert_eq!(
            active.len(),
            1,
            "der Unique-Index erlaubt ohnehin nur einen aktiven Provider"
        );
        assert_eq!(active[0].id, second.id);
    }

    #[tokio::test]
    async fn test_set_active_on_unknown_id_does_not_touch_existing_active_provider() {
        let store = in_memory_ai_provider_store().await;
        let active = make_config("Aktiv");
        store.create(&active).await.unwrap();
        store.set_active(&active.id, Utc::now()).await.unwrap();

        let result = store.set_active(&ProviderId::new(), Utc::now()).await;

        assert!(matches!(result, Err(AiProviderStoreError::NotFound(_))));
        let listed = store.list().await.unwrap();
        assert!(
            listed[0].is_active,
            "fehlgeschlagenes set_active muss den bisherigen aktiven Provider unangetastet lassen (Rollback)"
        );
    }

    #[tokio::test]
    async fn test_delete_inactive_provider_succeeds() {
        let store = in_memory_ai_provider_store().await;
        let config = make_config("Löschbar");
        store.create(&config).await.unwrap();

        store.delete(&config.id).await.unwrap();

        assert!(store.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_delete_active_provider_is_forbidden() {
        let store = in_memory_ai_provider_store().await;
        let config = make_config("Aktiv");
        store.create(&config).await.unwrap();
        store.set_active(&config.id, Utc::now()).await.unwrap();

        let result = store.delete(&config.id).await;

        assert_eq!(
            result,
            Err(AiProviderStoreError::ActiveProviderDeletionForbidden(
                config.id
            ))
        );
        assert_eq!(
            store.list().await.unwrap().len(),
            1,
            "der aktive Provider darf nicht gelöscht worden sein"
        );
    }

    #[tokio::test]
    async fn test_delete_unknown_id_yields_not_found() {
        let store = in_memory_ai_provider_store().await;

        let result = store.delete(&ProviderId::new()).await;

        assert!(matches!(result, Err(AiProviderStoreError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_get_returns_single_config_by_id() {
        let store = in_memory_ai_provider_store().await;
        let config = make_config("Einzelabfrage");
        store.create(&config).await.unwrap();

        let fetched = store.get(&config.id).await.unwrap();

        assert_eq!(fetched.display_name, "Einzelabfrage");
        assert_eq!(fetched.credential_ref, config.credential_ref);
    }

    #[tokio::test]
    async fn test_get_unknown_id_yields_not_found() {
        let store = in_memory_ai_provider_store().await;

        let result = store.get(&ProviderId::new()).await;

        assert!(matches!(result, Err(AiProviderStoreError::NotFound(_))));
    }
}
