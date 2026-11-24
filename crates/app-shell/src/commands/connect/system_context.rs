//! Test-side wrapper of the Tauri-free system-context builder, which lives in
//! `app_logic::connect::session_builder` (issue #839). The production path
//! resolves the prompt language itself and calls the builder directly.

use tauri::AppHandle;

use ssh_manager_core::profiles::ProfileStore;
use ssh_manager_core::shared::ServerId;

pub(crate) async fn build_session_system_context<R: tauri::Runtime>(
    app: &AppHandle<R>,
    server_name: &str,
    server_id: &ServerId,
    tags: &[String],
    profile_store: &dyn ProfileStore,
    policy_store: &persistence_sqlite::SqlitePolicyStore,
) -> (app_logic::compaction::SystemContextParts, bool) {
    let local_notes = if app_logic::dto::is_local(*server_id) {
        crate::local_server::synthetic_server(app).notes
    } else {
        String::new()
    };
    app_logic::connect::session_builder::build_session_system_context(
        crate::ui_language::session_prompt_language(app),
        server_name,
        server_id,
        tags,
        &local_notes,
        profile_store,
        policy_store,
    )
    .await
}
