use ssh_manager_core::profiles::ProfileStore;
use ssh_manager_core::shared::ServerId;
use tauri::AppHandle;

/// Spec 0038, section 2a: the scope handed to the session extension. The
/// local pseudo-server has no stored row, so its scope is built from the
/// synthetic server.
pub(crate) async fn extension_scope<R: tauri::Runtime>(
    app: &AppHandle<R>,
    profile_store: &dyn ProfileStore,
    server_id: ServerId,
) -> Option<app_logic::session_extension::ServerScope> {
    extension_scope_lookup(app, profile_store, server_id)
        .await
        .scope
}

/// [`extension_scope`] that also reports a store failure (issue #843).
pub(crate) async fn extension_scope_lookup<R: tauri::Runtime>(
    app: &AppHandle<R>,
    profile_store: &dyn ProfileStore,
    server_id: ServerId,
) -> app_logic::session_extension::ScopeLookup {
    let local_tags = app_logic::dto::is_local(server_id)
        .then(|| crate::local_server::synthetic_server(app).tags);
    app_logic::session_extension::scope_lookup(profile_store, server_id, local_tags).await
}
