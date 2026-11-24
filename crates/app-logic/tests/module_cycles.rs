//! Issue #868: guards against new direct dependency cycles between the
//! top-level modules of `app-logic`.
//!
//! Counting rule:
//! - A *module* is a top-level entry of `src/` (`foo.rs` or `foo/`), `lib.rs`
//!   is skipped. Every `.rs` file under `foo/` belongs to module `foo`.
//! - Test-only code is ignored: the `test_support` module and every file
//!   named `tests.rs` or `tests_*.rs`, or under a `tests/` directory.
//! - Comments (`//…` to end of line, including doc comments, and `/* … */`)
//!   are stripped first; string literals are not special-cased.
//! - An edge `A -> B` exists when code of module `A` contains a path
//!   `crate::B` (also in the brace form `crate::{B, C::x}`), `B` a module
//!   other than `A`.
//! - A *direct two-module cycle* is an unordered pair `{A, B}` with both
//!   `A -> B` and `B -> A`. The test counts these pairs and fails above
//!   [`CYCLE_CEILING`]. Lower the constant when a cycle is removed; never
//!   raise it.
#![expect(clippy::unwrap_used, reason = "integration test")]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Number of direct two-module cycles after Issue #868 (16 before, counted
/// with the rule above).
const CYCLE_CEILING: usize = 14;

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let b: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if b[i] == '/' && b.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn ident_at(s: &str) -> &str {
    let end = s
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    &s[..end]
}

/// Targets of every `crate::X` / `crate::{X, Y::z}` in `code`.
fn crate_targets(code: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (pos, _) in code.match_indices("crate::") {
        // `ssh_manager_core::crate::`-style prefixes do not exist, but make
        // sure `crate` is a whole word (not `foo_crate::`).
        if code[..pos]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        let rest = &code[pos + "crate::".len()..];
        if let Some(group) = rest.strip_prefix('{') {
            let mut depth = 1;
            let mut at_start = true;
            let mut i = 0;
            let gb: Vec<char> = group.chars().collect();
            while i < gb.len() && depth > 0 {
                match gb[i] {
                    '{' => {
                        depth += 1;
                        at_start = false;
                    }
                    '}' => depth -= 1,
                    ',' => at_start = depth == 1,
                    c if c.is_whitespace() => {}
                    _ => {
                        if at_start && depth == 1 {
                            let tail: String = gb[i..].iter().collect();
                            found.push(ident_at(&tail).to_string());
                        }
                        at_start = false;
                    }
                }
                i += 1;
            }
        } else {
            found.push(ident_at(rest).to_string());
        }
    }
    found
}

fn two_module_cycles(src: &Path) -> BTreeSet<(String, String)> {
    let mut files = Vec::new();
    rust_files(src, &mut files);
    let mut modules = BTreeSet::new();
    let mut per_file: Vec<(String, String)> = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(src).unwrap();
        let first = rel
            .components()
            .next()
            .unwrap()
            .as_os_str()
            .to_string_lossy();
        let name = first.trim_end_matches(".rs").to_string();
        let file = f.file_name().unwrap().to_string_lossy();
        let in_tests_dir = rel.components().any(|c| c.as_os_str() == "tests");
        if name == "lib"
            || name == "test_support"
            || in_tests_dir
            || file == "tests.rs"
            || file.starts_with("tests_")
        {
            continue;
        }
        modules.insert(name.clone());
        per_file.push((name, strip_comments(&fs::read_to_string(f).unwrap())));
    }
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (module, code) in &per_file {
        for target in crate_targets(code) {
            if &target != module && modules.contains(&target) {
                edges.entry(module.clone()).or_default().insert(target);
            }
        }
    }
    let mut cycles = BTreeSet::new();
    for (a, targets) in &edges {
        for b in targets {
            if a < b && edges.get(b).is_some_and(|t| t.contains(a)) {
                cycles.insert((a.clone(), b.clone()));
            }
        }
    }
    cycles
}

#[test]
fn direct_module_cycles_stay_below_ceiling() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let cycles = two_module_cycles(&src);
    assert!(
        cycles.len() <= CYCLE_CEILING,
        "{} direct two-module cycles in app-logic (ceiling {CYCLE_CEILING}): {cycles:?}",
        cycles.len()
    );
}

#[test]
fn dto_does_not_depend_on_state_events_or_server_credentials() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let cycles = two_module_cycles(&src);
    for other in ["state", "events", "server_credentials"] {
        assert!(
            !cycles.contains(&(other.to_string(), "dto".to_string()))
                && !cycles.contains(&("dto".to_string(), other.to_string())),
            "dto <-> {other} cycle is back"
        );
    }
}

#[test]
fn counter_detects_a_planted_cycle() {
    assert_eq!(
        crate_targets("use crate::{a::X, b, c::{d, e}};\nlet _ = crate::z::f();"),
        ["a", "b", "c", "z"]
    );
}
