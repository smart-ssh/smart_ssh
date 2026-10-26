//! Issue #816: an embedding crate can reach the one close path through the
//! public API alone. Compile-time check; nothing runs.

use app_logic::backup::OpenSessions;
use app_logic::state::SessionId;

fn assert_open_sessions<T: OpenSessions>() {}

#[allow(dead_code)]
async fn embedding_crate_closes_a_session<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    id: SessionId,
) -> bool {
    assert_open_sessions::<app_shell::sessions::AppOpenSessions<R>>();
    app_shell::sessions::close_session(app, id).await
}

#[test]
fn public_close_api_is_reachable_from_outside_the_crate() {
    // Also for the default runtime, like the app crates use it.
    assert_open_sessions::<app_shell::sessions::AppOpenSessions>();
    let _ = embedding_crate_closes_a_session::<tauri::Wry>;
}
