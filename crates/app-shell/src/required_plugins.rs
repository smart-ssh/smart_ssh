//! Tauri plugin crates an app crate that embeds `app-shell` must list as
//! **direct** dependencies (issue #891, follow-up to #795).
//!
//! The Tauri CLI only bundles the Android project of plugins that are direct
//! dependencies of the app crate, and `tauri-build` only grants plugin
//! permissions for direct dependencies. A plugin that is only reachable
//! through `app-shell` is therefore silently missing in the app.

/// Where an app crate needs the direct dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginTarget {
    /// Every target (plain `[dependencies]`).
    All,
    /// Everything except Android (`cfg(not(target_os = "android"))`).
    DesktopOnly,
    /// Android only (`cfg(target_os = "android")`).
    AndroidOnly,
}

/// One Tauri plugin crate an app crate has to depend on directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequiredPluginCrate {
    pub name: &'static str,
    pub target: PluginTarget,
}

const fn req(name: &'static str, target: PluginTarget) -> RequiredPluginCrate {
    RequiredPluginCrate { name, target }
}

/// Every `tauri-plugin-*` crate `app-shell` registers, with the target the
/// app crate needs it for. An app crate's `Cargo.toml` must list each as a
/// direct dependency (checked by `tests/direct_plugin_deps.rs` of the
/// community app; `app-shell` keeps this list honest in its own test).
pub const REQUIRED_DIRECT_PLUGIN_CRATES: &[RequiredPluginCrate] = &[
    req("tauri-plugin-dialog", PluginTarget::All),
    req("tauri-plugin-notification", PluginTarget::All),
    req("tauri-plugin-opener", PluginTarget::All),
    req("tauri-plugin-os", PluginTarget::All),
    req("tauri-plugin-store", PluginTarget::All),
    req("tauri-plugin-decoration", PluginTarget::DesktopOnly),
    req("tauri-plugin-fs", PluginTarget::AndroidOnly),
    // `app-shell` depends on and registers it everywhere (a no-op off
    // Android), but its Kotlin part and permissions matter on Android only.
    req("tauri-plugin-android-host", PluginTarget::AndroidOnly),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Plugin crates `app-shell` depends on that an app crate need not.
    const EXEMPT: &[(&str, &str)] = &[(
        "tauri-plugin-single-instance",
        "no webview permissions and no Android part",
    )];

    /// Crates whose direct-dependency target in the app is narrower than the
    /// `app-shell` dependency table (name, reason).
    const NARROWER_IN_APP: &[(&str, &str)] = &[(
        "tauri-plugin-android-host",
        "no-op off Android; Kotlin part and permissions are needed on Android only",
    )];

    fn target_of_table(key: &str) -> PluginTarget {
        match key {
            "cfg(target_os = \"android\")" => PluginTarget::AndroidOnly,
            "cfg(not(target_os = \"android\"))" => PluginTarget::DesktopOnly,
            other => panic!(
                "unknown target table `{other}` in app-shell Cargo.toml: map it in this test"
            ),
        }
    }

    fn plugin_deps(manifest: &toml::Table) -> BTreeMap<String, PluginTarget> {
        let mut found = BTreeMap::new();
        let mut collect = |table: Option<&toml::Value>, target| {
            if let Some(table) = table.and_then(|t| t.as_table()) {
                for name in table.keys().filter(|k| k.starts_with("tauri-plugin-")) {
                    found.insert(name.clone(), target);
                }
            }
        };
        collect(manifest.get("dependencies"), PluginTarget::All);
        if let Some(targets) = manifest.get("target").and_then(|t| t.as_table()) {
            for (key, value) in targets {
                collect(value.get("dependencies"), target_of_table(key));
            }
        }
        found
    }

    /// Counter-check: add a `tauri-plugin-*` dependency to
    /// `crates/app-shell/Cargo.toml` without an entry in
    /// `REQUIRED_DIRECT_PLUGIN_CRATES` (or an exemption above) and this fails.
    #[test]
    fn constant_matches_app_shell_plugin_dependencies() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let text = std::fs::read_to_string(path).expect("read app-shell Cargo.toml");
        let manifest: toml::Table = text.parse().expect("parse app-shell Cargo.toml");
        let deps = plugin_deps(&manifest);

        for (name, target) in &deps {
            if EXEMPT.iter().any(|(n, _)| n == name) {
                continue;
            }
            let entry = REQUIRED_DIRECT_PLUGIN_CRATES
                .iter()
                .find(|r| r.name == name)
                .unwrap_or_else(|| {
                    panic!("`{name}` is a dependency of app-shell but not in REQUIRED_DIRECT_PLUGIN_CRATES (or the exemption list)")
                });
            if NARROWER_IN_APP.iter().any(|(n, _)| n == name) {
                continue;
            }
            assert_eq!(
                entry.target, *target,
                "target of `{name}` differs from Cargo.toml"
            );
        }
        for r in REQUIRED_DIRECT_PLUGIN_CRATES {
            assert!(
                deps.contains_key(r.name),
                "`{}` is in REQUIRED_DIRECT_PLUGIN_CRATES but not a dependency of app-shell",
                r.name
            );
        }
        for (name, _) in EXEMPT {
            assert!(deps.contains_key(*name), "stale exemption `{name}`");
            assert!(
                REQUIRED_DIRECT_PLUGIN_CRATES
                    .iter()
                    .all(|r| r.name != *name),
                "`{name}` is exempt and listed"
            );
        }
    }
}
