//! Row writer for `ai_provider_configs` (split out of `backup_store.rs`).

use super::{insert_sql, kind, AiProviderRow, Conn, Res};

pub(super) const AI_PROVIDER_COLS: &str = "id, provider_type, display_name, base_url, model, \
     supports_native_tool_calling, credential_ref, is_active, created_at, updated_at, \
     extra_headers, attestation_url, max_tokens_override, web_research_enabled";

pub(super) async fn write_ai_provider(conn: &mut Conn, a: &AiProviderRow, upsert: bool) -> Res {
    sqlx::query(insert_sql("ai_provider_configs", AI_PROVIDER_COLS, upsert))
        .bind(a.id.to_string())
        .bind(&a.provider_type)
        .bind(&a.display_name)
        .bind(&a.base_url)
        .bind(&a.model)
        .bind(a.supports_native_tool_calling)
        .bind(&a.credential_ref)
        .bind(a.is_active)
        .bind(&a.created_at)
        .bind(&a.updated_at)
        .bind(&a.extra_headers)
        .bind(&a.attestation_url)
        .bind(a.max_tokens_override)
        .bind(a.web_research_enabled)
        .execute(&mut *conn)
        .await
        .map(drop)
        .map_err(|e| kind(&e))
}
