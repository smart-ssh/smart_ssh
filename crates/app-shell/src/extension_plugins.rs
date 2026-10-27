//! Issue #894 (Spec 0038 section 2, Spec 0101 A16 / clarification 9):
//! extension plugins that are registered **only after unlock**.
//!
//! `Wiring::plugins` hooks run on the builder before the unlock decision, so
//! their commands would be callable while the app is locked (the startup gate
//! cannot see plugin commands). Plugins in `Wiring::plugins_after_unlock` get
//! the same treatment as the core's own plugins that touch files, settings or
//! the OS: registered at build time when the state is already present
//! (keyring mode), otherwise right after the gate opened.

use std::sync::Mutex;

use tauri::{AppHandle, Builder, Runtime};

/// Factory for one extension plugin. A factory (not the plugin itself) works
/// with both `Builder::plugin` and `AppHandle::plugin`.
pub type UnlockedPluginFactory<R> = Box<dyn FnOnce() -> tauri::plugin::TauriPlugin<R> + Send>;

/// Keyring mode: register the plugins on the builder, in order.
pub(crate) fn register_on<R: Runtime>(
    builder: Builder<R>,
    factories: Vec<UnlockedPluginFactory<R>>,
) -> Builder<R> {
    factories
        .into_iter()
        .fold(builder, |builder, factory| builder.plugin(factory()))
}

/// Plugins waiting for the unlock (password mode). Always managed, possibly
/// empty (a state that exists only sometimes is an error source in Tauri).
pub(crate) struct PendingExtensionPlugins<R: Runtime>(Mutex<Vec<UnlockedPluginFactory<R>>>);

impl<R: Runtime> PendingExtensionPlugins<R> {
    pub(crate) fn new(factories: Vec<UnlockedPluginFactory<R>>) -> Self {
        Self(Mutex::new(factories))
    }
}

/// Password mode: register the waiting plugins after the gate opened. The
/// list is drained, so a second call registers nothing. A failing plugin is
/// logged and does not stop the others.
pub(crate) fn register_pending<R: Runtime>(app: &AppHandle<R>) {
    use tauri::plugin::Plugin;
    use tauri::Manager;

    let Some(pending) = app.try_state::<PendingExtensionPlugins<R>>() else {
        return;
    };
    let factories = match pending.0.lock() {
        Ok(mut guard) => std::mem::take(&mut *guard),
        Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
    };
    for factory in factories {
        let plugin = factory();
        let name = plugin.name();
        if let Err(err) = app.plugin(plugin) {
            tracing::error!(
                plugin = name,
                error = %err,
                "an extension plugin could not be registered after unlocking (Spec 0101, A16)"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
    use tauri::webview::InvokeRequest;

    const DATA: &str = "data-from-ext-plugin-894";

    fn plugin(name: &'static str) -> UnlockedPluginFactory<MockRuntime> {
        Box::new(move || {
            tauri::plugin::Builder::<MockRuntime>::new(name)
                .invoke_handler(|invoke: tauri::ipc::Invoke<MockRuntime>| {
                    invoke.resolver.resolve(DATA);
                    true
                })
                .build()
        })
    }

    fn context(names: &[&str]) -> tauri::Context<MockRuntime> {
        let mut context = mock_context(noop_assets());
        for name in names {
            context.runtime_authority_mut().__allow_command(
                format!("plugin:{name}|ext_ping"),
                tauri::utils::acl::ExecutionContext::Local,
            );
        }
        context
    }

    fn call(
        webview: &tauri::WebviewWindow<MockRuntime>,
        name: &str,
    ) -> Result<String, serde_json::Value> {
        let origin = if cfg!(any(windows, target_os = "android")) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        };
        tauri::test::get_ipc_response(
            webview,
            InvokeRequest {
                cmd: format!("plugin:{name}|ext_ping"),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: origin.parse().expect("fixed URL"),
                body: tauri::ipc::InvokeBody::default(),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
        .map(|body| match body {
            tauri::ipc::InvokeResponseBody::Json(text) => text,
            tauri::ipc::InvokeResponseBody::Raw(b) => String::from_utf8_lossy(&b).to_string(),
        })
    }

    fn webview(app: &tauri::App<MockRuntime>) -> tauri::WebviewWindow<MockRuntime> {
        tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
            .build()
            .expect("webview")
    }

    #[test]
    fn test_locked_plugin_is_unreachable_until_the_unlock_path_ran() {
        let app = mock_builder()
            .manage(PendingExtensionPlugins::<MockRuntime>::new(vec![plugin(
                "ext894",
            )]))
            .build(context(&["ext894"]))
            .expect("app");
        let webview = webview(&app);

        let locked = call(&webview, "ext894").expect_err("must not be reachable while locked");
        assert!(locked.to_string().contains("not found"), "got {locked}");

        register_pending(app.handle());
        assert!(call(&webview, "ext894")
            .expect("reachable after unlock")
            .contains(DATA));

        // Exactly once: a second unlock path registers nothing (no duplicate).
        register_pending(app.handle());
        assert!(call(&webview, "ext894").is_ok());
    }

    #[test]
    fn test_keyring_mode_plugin_is_reachable_right_away() {
        let app = register_on(mock_builder(), vec![plugin("ext894")])
            .build(context(&["ext894"]))
            .expect("app");
        let webview = webview(&app);
        assert!(call(&webview, "ext894")
            .expect("reachable at once")
            .contains(DATA));
    }

    #[test]
    fn test_a_failing_registration_does_not_stop_the_next_one() {
        // The second factory repeats the first one's name: registration fails.
        let app = mock_builder()
            .manage(PendingExtensionPlugins::<MockRuntime>::new(vec![
                plugin("ext894a"),
                plugin("ext894a"),
                plugin("ext894b"),
            ]))
            .build(context(&["ext894a", "ext894b"]))
            .expect("app");
        let webview = webview(&app);
        register_pending(app.handle());
        assert!(call(&webview, "ext894a").is_ok());
        assert!(call(&webview, "ext894b")
            .expect("the plugin after the failing one is registered")
            .contains(DATA));
    }
}
