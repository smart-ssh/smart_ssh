//! Issue #879: guard against new untracked background tasks.
//!
//! Production code under `crates/*/src` must not start tasks or threads
//! directly. Long-lived work goes through `app_logic::tasks::TaskSupervisor`
//! (scope, cancel, shutdown). Remaining direct sites are listed in
//! `ALLOWLIST` with a count and a reason. The list may only shrink: a new
//! site fails the test, and so does a stale or too generous entry.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Substrings that start a task or thread outside the supervisor.
const PATTERNS: &[&str] = &[
    "tokio::spawn(",
    "task::spawn(",
    "async_runtime::spawn(",
    "thread::spawn(",
    "thread::Builder::new(",
    "spawn_blocking(",
    "handle.spawn(",
    "runtime.spawn(",
];

/// (file relative to `crates/`, number of sites, reason). May only shrink.
const ALLOWLIST: &[(&str, usize, &str)] = &[
    ("app-logic/src/ai_usage.rs", 1, "the one usage worker, a tracked blocking task of the app scope"),
    ("app-logic/src/backup/import.rs", 1, "awaited"),
    ("app-logic/src/backup/key.rs", 1, "awaited by the caller (blocking key derivation helper)"),
    ("app-shell/src/android_service.rs", 1, "Android foreground-service call thread; blocks on a channel that lives for the process"),
    ("app-shell/src/commands/sftp/copy_content.rs", 2, "awaited"),
    ("app-shell/src/commands/sftp/download.rs", 1, "awaited"),
    ("app-shell/src/commands/sftp/edit_session.rs", 2, "awaited"),
    ("app-shell/src/commands/sftp/folder_upload.rs", 3, "awaited"),
    ("app-shell/src/commands/sftp/transfer.rs", 1, "awaited"),
    ("app-shell/src/commands/sftp/upload_clipboard.rs", 1, "awaited"),
    ("core/src/data_change.rs", 1, "short-lived flush thread of the change coalescer; core has no supervisor access"),
    ("mcp-server/src/config.rs", 1, "server task owned by its own CancellationToken and handle (mcp-server lifecycle)"),
    ("speech/src/audio.rs", 1, "audio capture thread owned by the recorder; speech cannot depend on app-logic"),
    ("speech/src/lib.rs", 1, "model download task with its own cancel channel; speech cannot depend on app-logic (#879 open question, option b)"),
    ("speech/src/recorder.rs", 4, "recorder timer/live tasks and awaited blocking work, owned by the recorder; speech cannot depend on app-logic"),
    ("ssh-transport/src/local.rs", 2, "awaited"),
    ("ssh-transport/src/sftp_tap.rs", 1, "fallback when no spawner was injected (transport used standalone); the app injects the session scope"),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Test files and the supervisor itself are not scanned.
fn is_excluded(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    rel == "app-logic/src/tasks.rs"
        || name == "tests.rs"
        || name == "test_events.rs"
        || name.ends_with("_tests.rs")
        || name.starts_with("tests_")
        || name.starts_with("test_support")
}

/// Number of spawn sites in the production part of `src`: comment lines and
/// `#[cfg(test)]` items (skipped by indentation, up to the closing brace at
/// the same indentation, as rustfmt lays them out) are ignored.
fn count_sites(src: &str) -> usize {
    let lines: Vec<&str> = src.lines().collect();
    let mut count = 0;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[cfg(test)]") {
            let indent = line.len() - trimmed.len();
            i += 1;
            // Skip further attributes, then the item itself.
            while i < lines.len() && lines[i].trim_start().starts_with("#[") {
                i += 1;
            }
            if i < lines.len() && !lines[i].trim_end().ends_with(';') {
                while i < lines.len() {
                    let l = lines[i];
                    let t = l.trim_start();
                    if l.len() - t.len() == indent && (t == "}" || t.starts_with("}")) {
                        break;
                    }
                    i += 1;
                }
            }
            i += 1;
            continue;
        }
        if !trimmed.starts_with("//") {
            count += PATTERNS.iter().filter(|p| line.contains(*p)).count();
        }
        i += 1;
    }
    count
}

fn scan() -> BTreeMap<String, usize> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = BTreeMap::new();
    for entry in fs::read_dir(&crates).expect("crates dir") {
        let src = entry.expect("entry").path().join("src");
        if !src.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        for file in files {
            let rel = file
                .strip_prefix(&crates)
                .expect("under crates")
                .to_string_lossy()
                .replace('\\', "/")
                // `crates/<crate>/src/...` -> `<crate>/src/...`
                .to_string();
            if is_excluded(&rel) {
                continue;
            }
            let n = count_sites(&fs::read_to_string(&file).expect("read"));
            if n > 0 {
                found.insert(rel, n);
            }
        }
    }
    found
}

/// Compares `found` with `allow`; returns one message per violation.
fn violations(found: &BTreeMap<String, usize>, allow: &[(&str, usize, &str)]) -> Vec<String> {
    let allowed: BTreeMap<&str, usize> = allow.iter().map(|(f, n, _)| (*f, *n)).collect();
    let mut out = Vec::new();
    for (file, n) in found {
        match allowed.get(file.as_str()) {
            None => out.push(format!(
                "{file}: {n} untracked spawn site(s). Start background work through \
                 `app_logic::tasks::TaskSupervisor` (e.g. `app_scope().spawn(..)`), or, if it \
                 is awaited in the same function, add an allowlist entry with a reason."
            )),
            Some(max) if n > max => out.push(format!(
                "{file}: {n} spawn site(s), allowlist permits {max}. Use `TaskSupervisor`."
            )),
            Some(max) if n < max => out.push(format!(
                "{file}: only {n} spawn site(s) left, allowlist says {max}. Lower the entry."
            )),
            Some(_) => {}
        }
    }
    for (file, max) in &allowed {
        if !found.contains_key(*file) {
            out.push(format!(
                "{file}: allowlist entry ({max}) is stale; remove it."
            ));
        }
    }
    out
}

#[test]
fn production_code_starts_no_untracked_tasks() {
    let problems = violations(&scan(), ALLOWLIST);
    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
}

#[test]
fn allowlist_entries_have_a_reason() {
    for (file, n, reason) in ALLOWLIST {
        assert!(*n > 0, "{file}: zero count");
        assert!(!reason.trim().is_empty(), "{file}: reason missing");
    }
}

#[test]
fn a_new_tokio_spawn_in_production_code_is_reported() {
    let fixture = "fn start() {\n    tokio::spawn(async {});\n}\n";
    let mut found = BTreeMap::new();
    found.insert("demo/src/lib.rs".to_string(), count_sites(fixture));
    let problems = violations(&found, &[]);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("TaskSupervisor"));
}

#[test]
fn test_modules_and_comments_are_not_counted() {
    let fixture = "// tokio::spawn(x)\nfn a() {}\n\n#[cfg(test)]\nmod tests {\n    fn t() {\n        tokio::spawn(async {});\n    }\n}\n\nfn b() {\n    std::thread::spawn(|| {});\n}\n";
    assert_eq!(count_sites(fixture), 1);
}
