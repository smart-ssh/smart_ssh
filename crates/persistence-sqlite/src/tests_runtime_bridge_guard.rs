//! Issue #898: guard — synchronous-over-asynchronous bridging in production
//! code exists only in `runtime_bridge.rs` and the app's start-up.

use std::path::{Path, PathBuf};

/// Files (relative to `crates/`) that may block on a future in production
/// code. Add to this list only with a reason.
const ALLOWED: &[&str] = &[
    // The bridge itself.
    "persistence-sqlite/src/runtime_bridge.rs",
    // `run()`: start-up, before any window exists.
    "app-shell/src/lib.rs",
    // Start-up and shutdown of the background task supervisor: called from
    // the main thread, outside any runtime context.
    "app-shell/src/background_tasks.rs",
    // Only compiled for tests (`#[cfg(test)] mod` in lib.rs).
    "app-shell/src/runtime_assumptions.rs",
];

const PATTERNS: &[&str] = &[
    "block_in_place",
    "block_on(",
    "Runtime::new",
    "new_current_thread",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            let name = path.file_name().unwrap().to_string_lossy();
            if name != "tests" && name != "target" {
                rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Production part of a file: test files are skipped, and everything from
/// the first `#[cfg(test)]` on is dropped. Comment lines do not count.
fn production_lines(path: &Path) -> Vec<(usize, String)> {
    let name = path.file_name().unwrap().to_string_lossy();
    if name.starts_with("tests") || name == "test_support.rs" || name == "property_tests.rs" {
        return Vec::new();
    }
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        if !line.trim_start().starts_with("//") {
            lines.push((i + 1, line.to_string()));
        }
    }
    lines
}

#[test]
fn blocking_on_futures_only_in_the_bridge_and_startup() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_files(crates, &mut files);
    assert!(files.len() > 50, "the scan must find the workspace");
    let mut offenders = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if ALLOWED.contains(&rel.as_str()) {
            continue;
        }
        for (line_no, line) in production_lines(&file) {
            if PATTERNS.iter().any(|p| line.contains(p)) {
                offenders.push(format!("{rel}:{line_no}: {}", line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "use persistence_sqlite::runtime_bridge::block_on_io instead:\n{}",
        offenders.join("\n")
    );
}
