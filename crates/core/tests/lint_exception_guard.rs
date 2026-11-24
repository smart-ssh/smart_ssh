//! Ratchet for the documented clippy exceptions (issue #896): production code
//! may carry at most `MAX_EXCEPTIONS` `#[expect(clippy::too_many_lines, ..)]` /
//! `#[expect(clippy::cognitive_complexity, ..)]` attributes. The number may
//! only fall: when a function is split and its attribute removed, lower the
//! constant in the same change.
//!
//! Excluded like in `lock_poison_guard.rs`: everything below a
//! `#[cfg(test)] mod …`, `tests/` directories, `*_tests.rs`, `tests_*.rs`,
//! `tests.rs` and `test_support*` files. Test code uses inner `#![expect(..)]`
//! or is not counted.

use std::path::{Path, PathBuf};

/// Current number of production exceptions. May only be lowered.
const MAX_EXCEPTIONS: usize = 24;

const MARKERS: [&str; 2] = [
    "#[expect(clippy::too_many_lines",
    "#[expect(clippy::cognitive_complexity",
];

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

/// Counts exception attributes; every one must carry a `reason`.
fn count(src: &str) -> (usize, Vec<usize>) {
    let mut n = 0;
    let mut missing_reason = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let t = line.trim_start();
        if MARKERS.iter().any(|m| t.starts_with(m)) {
            n += 1;
            if !t.contains("reason = \"") {
                missing_reason.push(i + 1);
            }
        }
    }
    (n, missing_reason)
}

#[test]
fn counter_detects_new_exceptions() {
    let src = "#[expect(clippy::too_many_lines, reason = \"x\")]\nfn a() {}\n\
               #[expect(clippy::cognitive_complexity)]\nfn b() {}\n\
               #[expect(clippy::large_futures, reason = \"x\")]\nfn c() {}\n";
    assert_eq!(count(src), (2, vec![3]));
    let tail = "fn p() {}\n#[cfg(test)]\nmod tests {\n#[expect(clippy::too_many_lines, reason = \"x\")]\nfn t() {}\n}\n";
    assert_eq!(count(production_part(tail)).0, 0);
}

#[test]
fn production_exceptions_only_fall() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    collect(&root, &mut files);
    let mut total = 0;
    for path in files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if is_test_file(name) {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read source");
        let (n, missing) = count(production_part(&src));
        assert!(
            missing.is_empty(),
            "{}: exception without `reason` at lines {missing:?}",
            path.display()
        );
        total += n;
    }
    assert!(
        total <= MAX_EXCEPTIONS,
        "{total} clippy::too_many_lines/cognitive_complexity exceptions in production code, \
         allowed {MAX_EXCEPTIONS}: split the function instead of adding an exception"
    );
    assert!(
        total >= MAX_EXCEPTIONS,
        "only {total} exceptions left (limit {MAX_EXCEPTIONS}): lower MAX_EXCEPTIONS in this test"
    );
}
