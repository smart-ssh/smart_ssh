//! Guard for the poison rule (Spec 0088, A2.3): production code never locks a
//! `std::sync` primitive with `.unwrap()`/`.expect(..)`; it uses
//! `ssh_manager_core::sync` instead.
//!
//! Excluded: everything below a `#[cfg(test)] mod …`, `tests/` directories,
//! `*_tests.rs`, `tests_*.rs`, `tests.rs` and `test_support*` files.

use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if matches!(
                name,
                "target" | "node_modules" | "tests" | "fixtures" | ".git"
            ) {
                continue;
            }
            collect(&path, out);
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

fn is_test_file(name: &str) -> bool {
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || name.starts_with("tests_")
        || name.starts_with("test_support")
}

/// Production part of a source file: everything before the first
/// `#[cfg(… test …)]` that is followed (past further attributes) by a `mod`.
fn production_part(src: &str) -> &str {
    let lines: Vec<&str> = src.lines().collect();
    let mut offset = 0;
    for (i, line) in lines.iter().enumerate() {
        let attr = line.trim();
        if attr.starts_with("#[cfg(") && attr.contains("test") && !attr.contains("not(test") {
            let next = lines[i + 1..]
                .iter()
                .map(|l| l.trim())
                .find(|l| !l.is_empty() && !l.starts_with("#["));
            if next.is_some_and(|l| {
                l.starts_with("mod ") || (l.starts_with("pub") && l.contains(" mod "))
            }) {
                return &src[..offset];
            }
        }
        offset += line.len() + 1;
    }
    src
}

/// Finds `.lock()` / `.read()` / `.write()` directly followed (whitespace
/// allowed) by `.unwrap(` or `.expect(`; returns the 1-based line numbers.
fn offenders(src: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for call in [".lock()", ".read()", ".write()"] {
        for (at, _) in src.match_indices(call) {
            let rest = src[at + call.len()..].trim_start();
            if rest.starts_with(".unwrap(") || rest.starts_with(".expect(") {
                found.push(src[..at].matches('\n').count() + 1);
            }
        }
    }
    found.sort_unstable();
    found
}

#[test]
fn test_no_lock_unwrap_or_expect_in_production_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for sub in ["crates", "apps"] {
        collect(&root.join(sub), &mut files);
    }
    assert!(files.len() > 100, "scan found suspiciously few files");

    let mut violations = Vec::new();
    for file in files {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if is_test_file(name) {
            continue;
        }
        let src = std::fs::read_to_string(&file).expect("read source");
        for line in offenders(production_part(&src)) {
            violations.push(format!("{}:{line}", file.display()));
        }
    }
    assert!(
        violations.is_empty(),
        "use ssh_manager_core::sync::*_tolerating_poison instead of lock/read/write().unwrap()/expect():\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_offender_detection_finds_split_and_plain_calls() {
    assert_eq!(
        offenders("a.lock().unwrap();\nb\n    .read()\n    .expect(\"x\");"),
        [1, 3]
    );
    assert!(offenders("a.lock().unwrap_or_else(|e| e.into_inner());").is_empty());
}
