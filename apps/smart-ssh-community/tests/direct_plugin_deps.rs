//! Issue #891: every Tauri plugin crate `app-shell` registers must be a
//! *direct* dependency of this app crate (Tauri CLI Android bundling and
//! `tauri-build` permissions only see direct dependencies; cause of #795).
//! Another app crate can copy this file unchanged.

use app_shell::{PluginTarget, REQUIRED_DIRECT_PLUGIN_CRATES};

/// Does `deps` (a `[dependencies]`-like table) contain `name`?
fn has(table: Option<&toml::Value>, name: &str) -> bool {
    table
        .and_then(|t| t.as_table())
        .is_some_and(|t| t.contains_key(name))
}

#[test]
fn app_lists_required_plugins_as_direct_dependencies() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let text = std::fs::read_to_string(path).expect("read Cargo.toml");
    let manifest: toml::Table = text.parse().expect("parse Cargo.toml");
    let in_target = |cfg: &str, name: &str| {
        has(
            manifest
                .get("target")
                .and_then(|t| t.get(cfg))
                .and_then(|t| t.get("dependencies")),
            name,
        )
    };
    let android = "cfg(target_os = \"android\")";
    let not_android = "cfg(not(target_os = \"android\"))";

    let mut problems = Vec::new();
    for req in REQUIRED_DIRECT_PLUGIN_CRATES {
        let name = req.name;
        let all = has(manifest.get("dependencies"), name);
        let (ok, table) = match req.target {
            PluginTarget::All => (all, "[dependencies]"),
            PluginTarget::DesktopOnly => (
                all || in_target(not_android, name),
                "[target.'cfg(not(target_os = \"android\"))'.dependencies] (or [dependencies])",
            ),
            PluginTarget::AndroidOnly => (
                all || in_target(android, name),
                "[target.'cfg(target_os = \"android\")'.dependencies] (or [dependencies])",
            ),
        };
        if !ok {
            problems.push(format!("`{name}` is missing; add it to {table}"));
        }
    }
    assert!(
        problems.is_empty(),
        "app-shell registers plugins that are not direct dependencies of this crate:\n{}",
        problems.join("\n")
    );
}
