//! Issue #1607: "Open locally" must work for dotfile edit copies
//! (`.env.local`). The opener plugin builds a `tauri::fs::Scope` whose
//! `require_literal_leading_dot` defaults to `true` on Unix, so `**` never
//! matches a component starting with `.`. `tauri.conf.json` switches that off
//! for the opener plugin; the scope itself stays the edit-session folder.

use std::fs;
use std::path::{Path, PathBuf};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::utils::config::FsScope;

fn conf() -> serde_json::Value {
    let raw = include_str!("../tauri.conf.json");
    serde_json::from_str(raw).expect("tauri.conf.json is valid JSON")
}

fn capability() -> serde_json::Value {
    let raw = include_str!("../capabilities/default.json");
    serde_json::from_str(raw).expect("default.json is valid JSON")
}

#[test]
fn config_disables_literal_leading_dot_for_opener() {
    assert_eq!(
        conf()["plugins"]["opener"]["requireLiteralLeadingDot"],
        serde_json::Value::Bool(false),
        "plugins.opener.requireLiteralLeadingDot must be false (issue #1607)"
    );
}

#[test]
fn capability_scope_stays_the_edit_session_folder() {
    let cap = capability();
    let entry = cap["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["identifier"] == "opener:allow-open-path")
        .expect("opener:allow-open-path entry");
    let allow = entry["allow"].as_array().unwrap();
    assert_eq!(allow.len(), 1);
    assert_eq!(allow[0]["path"], "$CACHE/smart-ssh/edit-sessions/**");
    assert_eq!(allow[0]["app"], true);
}

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"x").unwrap();
}

/// Builds a scope with the same pattern shape as the capability, rooted in a
/// fresh temp dir, and checks `paths` against it.
fn check(require_dot: Option<bool>, check: impl FnOnce(&dyn Fn(&Path) -> bool, &Path)) {
    let root: PathBuf = std::env::temp_dir().join(format!(
        "smart-ssh-1607-{}-{:?}",
        std::process::id(),
        require_dot
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let pattern = root.join("smart-ssh/edit-sessions/**");
    let scope = tauri::fs::Scope::new(
        &app,
        &FsScope::Scope {
            allow: vec![pattern],
            deny: vec![],
            require_literal_leading_dot: require_dot,
        },
    )
    .unwrap();
    check(&|p| scope.is_allowed(p), &root);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn dotfile_edit_copy_allowed_and_everything_else_rejected() {
    check(Some(false), |allowed, root| {
        let ok = root.join("smart-ssh/edit-sessions/data-1/abc/.env.local");
        touch(&ok);
        assert!(allowed(&ok), "dotfile edit copy must be allowed");
        let plain = root.join("smart-ssh/edit-sessions/data-1/abc/readme.txt");
        touch(&plain);
        assert!(allowed(&plain));

        let ssh = root.join(".ssh/id_ed25519");
        touch(&ssh);
        assert!(!allowed(&ssh), "dotfile outside edit-sessions");
        let other = root.join("smart-ssh/other/.env");
        touch(&other);
        assert!(!allowed(&other));
        let traversal = root.join("smart-ssh/edit-sessions/../../.ssh/id_ed25519");
        assert!(!allowed(&traversal), "traversal");
    });
}

#[test]
fn counter_proof_literal_leading_dot_rejects_dotfile() {
    check(Some(true), |allowed, root| {
        let ok = root.join("smart-ssh/edit-sessions/data-1/abc/.env.local");
        touch(&ok);
        assert!(!allowed(&ok));
    });
}
