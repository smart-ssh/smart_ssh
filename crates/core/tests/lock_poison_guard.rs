//! Guard for the poison rule (Spec 0088, A2.3): production code never locks a
//! `std::sync` primitive with `.unwrap()`/`.expect(..)`; it uses
//! `ssh_manager_core::sync` instead.
//!
//! Excluded: everything below a `#[cfg(test)] mod …`, `tests/` directories,
//! `*_tests.rs`, `tests_*.rs`, `tests.rs`, `test_support*` and `test_exit_guard.rs`
//! files, plus `crates/core/src/sync.rs` (the helpers themselves). Inline
//! poison recovery (`.lock().unwrap_or_else(…into_inner…)`) is reported too.

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
        || name == "test_exit_guard.rs" // compiled for test builds only
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

fn floor_boundary(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Finds `.lock()` / `.read()` / `.write()` directly followed (whitespace
/// allowed) by `.unwrap(`, `.expect(` or an inline poison recovery
/// (`.unwrap_or_else(… into_inner …)`); returns the 1-based line numbers.
fn offenders(src: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for call in [".lock()", ".read()", ".write()"] {
        for (at, _) in src.match_indices(call) {
            let rest = src[at + call.len()..].trim_start();
            let inline_recovery = rest.starts_with(".unwrap_or_else(") && {
                let end = rest.find(';').unwrap_or(rest.len()).min(200);
                rest[..floor_boundary(rest, end)].contains("into_inner")
            };
            if rest.starts_with(".unwrap(") || rest.starts_with(".expect(") || inline_recovery {
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
        if is_test_file(name) || file.ends_with("crates/core/src/sync.rs") {
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
    assert_eq!(
        offenders("a.lock().unwrap_or_else(|e| e.into_inner());"),
        [1]
    );
    assert_eq!(
        offenders("x\n    .write()\n    .unwrap_or_else(PoisonError::into_inner);"),
        [2]
    );
    assert_eq!(
        offenders("a.lock().unwrap_or_else(std::sync::PoisonError::into_inner)"),
        [1]
    );
    assert!(offenders("a.lock().unwrap_or_else(|e| fallback(e));").is_empty());
}
