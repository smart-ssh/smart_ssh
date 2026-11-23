//! Guard for issue #1506 (ADR 0161): the core keeps no activity record. Production
//! code never refers to the removed tables, columns or stores again.
//!
//! Excluded like in `lock_poison_guard.rs`: everything below a `#[cfg(test)] mod …`,
//! `tests/` directories, `*_tests.rs`, `tests_*.rs`, `tests.rs` and `test_support*`
//! files. SQL migrations are not Rust files and are not scanned.

use std::path::{Path, PathBuf};

const REMOVED: [&str; 6] = [
    "ledger_entries",
    "activity_sessions",
    "activity_session_id",
    "ledger_store",
    "activity_session_store",
    "record_query",
];

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if matches!(
                name,
                "target" | "node_modules" | "tests" | "fixtures" | ".git" | "migrations"
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
        || name == "no_activity_record_guard.rs"
}

/// Everything before the first `#[cfg(… test …)]` that is followed by a `mod`.
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

fn offenders(src: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (n, line) in src.lines().enumerate() {
        for word in REMOVED {
            if line.contains(word) {
                found.push(format!("{}: `{word}`", n + 1));
            }
        }
    }
    found
}

#[test]
fn test_production_code_does_not_refer_to_the_removed_activity_record() {
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
        for hit in offenders(production_part(&src)) {
            violations.push(format!("{}:{hit}", file.display()));
        }
    }
    assert!(
        violations.is_empty(),
        "the core keeps no activity record (ADR 0161):\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_offender_detection_finds_a_removed_name() {
    assert_eq!(
        offenders("let a = 1;\nSELECT * FROM ledger_entries;"),
        ["2: `ledger_entries`"]
    );
    assert!(offenders("let a = 1;").is_empty());
}
