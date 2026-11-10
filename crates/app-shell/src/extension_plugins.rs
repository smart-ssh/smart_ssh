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

/// A built plugin with its config type erased, so it can be registered on
/// either a `Builder` or an `AppHandle`.
trait ErasedPlugin<R: Runtime> {
    fn name(&self) -> &'static str;
    fn register_on_builder(self: Box<Self>, builder: Builder<R>) -> Builder<R>;
    fn register_on_app(self: Box<Self>, app: &AppHandle<R>) -> tauri::Result<()>;
}

struct Erased<P>(P);

impl<R: Runtime, P: tauri::plugin::Plugin<R> + 'static> ErasedPlugin<R> for Erased<P> {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn register_on_builder(self: Box<Self>, builder: Builder<R>) -> Builder<R> {
        builder.plugin(self.0)
    }
    fn register_on_app(self: Box<Self>, app: &AppHandle<R>) -> tauri::Result<()> {
        app.plugin(self.0)
    }
}

/// Factory for one extension plugin. A factory (not the plugin itself) works
/// with both `Builder::plugin` and `AppHandle::plugin`. The plugin's config
/// type is erased, so any `tauri::plugin::TauriPlugin<R, C>` fits; its config
/// (`plugins.<name>` in `tauri.conf.json`) is applied by Tauri on registration,
/// as for plugins registered on the builder (issue #1049).
pub struct UnlockedPluginFactory<R: Runtime>(Box<dyn FnOnce() -> Box<dyn ErasedPlugin<R>> + Send>);

impl<R: Runtime> UnlockedPluginFactory<R> {
    /// Wraps a function that builds the plugin, e.g.
    /// `UnlockedPluginFactory::new(|| my_plugin::init())`.
    pub fn new<P, F>(build: F) -> Self
    where
        P: tauri::plugin::Plugin<R> + 'static,
        F: FnOnce() -> P + Send + 'static,
    {
        Self(Box::new(move || Box::new(Erased(build()))))
    }

    /// Name the plugin registers with. The plugin is built lazily, so this
    /// builds it and drops it again without registering anything; the factory
    /// is consumed (create a second one to also register the plugin).
    pub fn into_name(self) -> &'static str {
        (self.0)().name()
    }
}

/// Keyring mode: register the plugins on the builder, in order.
pub(crate) fn register_on<R: Runtime>(
    builder: Builder<R>,
    factories: Vec<UnlockedPluginFactory<R>>,
) -> Builder<R> {
    factories.into_iter().fold(builder, |builder, factory| {
        (factory.0)().register_on_builder(builder)
    })
}

/// Plugins waiting for the unlock (password mode). Always managed, possibly
/// empty (a state that exists only sometimes is an error source in Tauri).
pub(crate) struct PendingExtensionPlugins<R: Runtime>(Mutex<Vec<UnlockedPluginFactory<R>>>);

impl<R: Runtime> PendingExtensionPlugins<R> {
    pub(crate) fn new(factories: Vec<UnlockedPluginFactory<R>>) -> Self {
        Self(Mutex::new(factories))
    }
}

/// The managed state for the waiting plugins.
pub(crate) fn pending<R: Runtime>(
    factories: Vec<UnlockedPluginFactory<R>>,
) -> PendingExtensionPlugins<R> {
    PendingExtensionPlugins::new(factories)
}

/// Password mode: register the waiting plugins after the gate opened. The
/// list is drained, so a second call registers nothing. A failing plugin is
/// logged and does not stop the others; the names of the failed plugins are
/// returned (for tests).
pub(crate) fn register_pending<R: Runtime>(app: &AppHandle<R>) -> Vec<&'static str> {
    use tauri::Manager;

    let mut failed = Vec::new();
    let Some(pending) = app.try_state::<PendingExtensionPlugins<R>>() else {
        return failed;
    };
    let factories = std::mem::take(&mut *ssh_manager_core::sync::lock_tolerating_poison(
        &pending.0,
    ));
    for factory in factories {
        let plugin = (factory.0)();
        let name = plugin.name();
        if let Err(err) = plugin.register_on_app(app) {
            tracing::error!(
                plugin = name,
                error = %err,
                "an extension plugin could not be registered after unlocking (Spec 0101, A16)"
            );
            failed.push(name);
        }
    }
    failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
    use tauri::webview::InvokeRequest;

    const DATA: &str = "data-from-ext-plugin-894";

    fn plugin(name: &'static str) -> UnlockedPluginFactory<MockRuntime> {
        UnlockedPluginFactory::new(move || {
            tauri::plugin::Builder::<MockRuntime>::new(name)
                .invoke_handler(|invoke: tauri::ipc::Invoke<MockRuntime>| {
                    invoke.resolver.resolve(DATA);
                    true
                })
                .build()
        })
    }

    #[test]
    fn test_into_name_returns_registered_name() {
        assert_eq!(plugin("name-probe-1070").into_name(), "name-probe-1070");
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

        assert!(register_pending(app.handle()).is_empty());
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

    fn failing_plugin(name: &'static str) -> UnlockedPluginFactory<MockRuntime> {
        UnlockedPluginFactory::new(move || {
            tauri::plugin::Builder::<MockRuntime>::new(name)
                .setup(|_, _| Err("setup refused".into()))
                .build()
        })
    }

    #[test]
    fn test_a_failing_registration_does_not_stop_the_next_one() {
        let app = mock_builder()
            .manage(PendingExtensionPlugins::<MockRuntime>::new(vec![
                plugin("ext894a"),
                failing_plugin("ext894bad"),
                plugin("ext894b"),
            ]))
            .build(context(&["ext894a", "ext894b", "ext894bad"]))
            .expect("app");
        let webview = webview(&app);
        let failed = register_pending(app.handle());
        assert_eq!(failed, vec!["ext894bad"]);
        assert!(call(&webview, "ext894a").is_ok());
        assert!(call(&webview, "ext894b")
            .expect("the plugin after the failing one is registered")
            .contains(DATA));
    }

    #[derive(serde::Deserialize)]
    struct Cfg {
        greeting: String,
    }

    fn configured_plugin(name: &'static str) -> UnlockedPluginFactory<MockRuntime> {
        UnlockedPluginFactory::new(move || {
            tauri::plugin::Builder::<MockRuntime, Cfg>::new(name)
                .setup(|app, api| {
                    use tauri::Manager;
                    app.manage(api.config().greeting.clone());
                    Ok(())
                })
                .invoke_handler(|invoke: tauri::ipc::Invoke<MockRuntime>| {
                    use tauri::Manager;
                    let greeting = invoke.message.webview().state::<String>().inner().clone();
                    invoke.resolver.resolve(greeting);
                    true
                })
                .build()
        })
    }

    fn configured_context(name: &str) -> tauri::Context<MockRuntime> {
        let mut context = context(&[name]);
        context.config_mut().plugins.0.insert(
            name.to_string(),
            serde_json::json!({ "greeting": "config-1049" }),
        );
        context
    }

    #[test]
    fn test_config_carrying_plugin_is_locked_then_gets_its_config_after_unlock() {
        let app = mock_builder()
            .manage(PendingExtensionPlugins::<MockRuntime>::new(vec![
                configured_plugin("ext1049"),
            ]))
            .build(configured_context("ext1049"))
            .expect("app");
        let webview = webview(&app);
        let locked = call(&webview, "ext1049").expect_err("locked");
        assert!(locked.to_string().contains("not found"), "got {locked}");

        assert!(register_pending(app.handle()).is_empty());
        assert!(call(&webview, "ext1049")
            .expect("reachable after unlock")
            .contains("config-1049"));
        // Once only.
        assert!(register_pending(app.handle()).is_empty());
    }

    #[test]
    fn test_config_carrying_plugin_in_keyring_mode_gets_its_config() {
        let app = register_on(mock_builder(), vec![configured_plugin("ext1049")])
            .build(configured_context("ext1049"))
            .expect("app");
        assert!(call(&webview(&app), "ext1049")
            .expect("reachable")
            .contains("config-1049"));
    }

    #[test]
    fn test_failing_config_carrying_plugin_is_logged_and_skipped() {
        // Config is missing, so deserialising `Cfg` fails at registration.
        let app = mock_builder()
            .manage(PendingExtensionPlugins::<MockRuntime>::new(vec![
                configured_plugin("ext1049bad"),
                plugin("ext1049ok"),
            ]))
            .build(context(&["ext1049bad", "ext1049ok"]))
            .expect("app");
        let webview = webview(&app);
        assert_eq!(register_pending(app.handle()), vec!["ext1049bad"]);
        assert!(call(&webview, "ext1049ok").is_ok());
    }
}
