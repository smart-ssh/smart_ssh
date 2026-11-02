//! Guard: remote command execution on the SSH transport may only appear in
//! an explicit allowlist of production files (issue #830, security
//! invariant 1). A new call site elsewhere fails this test; add the file to
//! `ALLOWLIST` only with a reason and a review of the approval path.

use std::path::{Path, PathBuf};

/// `(file relative to `crates/`, reason)`.
const ALLOWLIST: &[(&str, &str)] = &[
    (
        "app-logic/src/orchestration/action_exec.rs",
        "approved AI/MCP command: execute_suggested_command takes an ApprovedCommand",
    ),
    (
        "app-logic/src/orchestration/remote_files.rs",
        "sudo fallback of a confirmed WriteRemoteFile: execute_privileged takes an ApprovedFileWrite",
    ),
    (
        "app-shell/src/commands/connect.rs",
        "fixed connect probes `uname -a` and `id -u`, no external input",
    ),
    (
        "app-shell/src/elevated_sftp.rs",
        "user-initiated elevated file browser: probe and open_sftp_via_exec with validated target user",
    ),
    (
        "core/src/ssh/transport.rs",
        "the SshTransport trait's default methods delegating to its own exec methods",
    ),
];

const METHODS: &[&str] = &[
    ".start_exec(",
    ".execute_streaming(",
    ".execute_with_stdin(",
    ".execute_cancellable(",
    ".execute_with_stdin_cancellable(",
    ".open_sftp_via_exec(",
];

/// Cuts the source at the first inline `#[cfg(test)] mod … {` block.
fn production_part(src: &str) -> &str {
    let mut offset = 0;
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]" {
            let next = lines[i + 1..]
                .iter()
                .find(|l| !l.trim_start().starts_with("#["))
                .map(|l| l.trim_start());
            if let Some(n) = next {
                let n = n
                    .strip_prefix("pub(crate) ")
                    .or_else(|| n.strip_prefix("pub "))
                    .unwrap_or(n);
                if n.starts_with("mod ") && n.trim_end().ends_with('{') {
                    return &src[..offset];
                }
            }
        }
        offset += line.len();
    }
    src
}

/// True if the production part of `src` calls the transport exec API.
fn has_exec_call(src: &str) -> bool {
    let prod = production_part(src);
    if METHODS.iter().any(|m| prod.contains(m)) {
        return true;
    }
    // `transport.execute(` / `transport.lock().await.execute(`; a plain
    // `.execute(` (SQL) is not matched because it needs a `transport`
    // receiver earlier in the same statement.
    let mut rest = prod;
    while let Some(pos) = rest.find("transport") {
        let after = &rest[pos..];
        let stmt_end = after.find([';', '{', '}']).unwrap_or(after.len());
        let stmt: String = after[..stmt_end]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if stmt.contains(".execute(") {
            return true;
        }
        rest = &after["transport".len()..];
    }
    false
}

fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.starts_with("tests") || name.starts_with("test_support") || name.ends_with("_tests.rs")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if name != "target" && name != "tests" && name != "node_modules" {
                collect(&path, out);
            }
        } else if name.ends_with(".rs") && !is_test_file(&path) {
            out.push(path);
        }
    }
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .to_path_buf()
}

fn offenders(files: &[(String, String)], allow: &[&str]) -> Vec<String> {
    files
        .iter()
        .filter(|(rel, src)| has_exec_call(src) && !allow.contains(&rel.as_str()))
        .map(|(rel, _)| rel.clone())
        .collect()
}

#[test]
fn exec_call_sites_are_allowlisted_and_not_stale() {
    let root = crates_dir();
    let mut paths = Vec::new();
    collect(&root, &mut paths);
    let files: Vec<(String, String)> = paths
        .iter()
        .map(|p| {
            let rel = p
                .strip_prefix(&root)
                .expect("under crates")
                .to_string_lossy()
                .replace('\\', "/");
            (rel, std::fs::read_to_string(p).expect("read source"))
        })
        .collect();
    let allow: Vec<&str> = ALLOWLIST.iter().map(|(f, _)| *f).collect();
    let bad = offenders(&files, &allow);
    assert!(
        bad.is_empty(),
        "transport exec call outside the allowlist: {bad:?}. Route AI/MCP commands through \
         ApprovedCommand or add the file to ALLOWLIST with a reason."
    );
    // Stale entries: file deleted/renamed, or no exec call left in it.
    for (file, reason) in ALLOWLIST {
        assert!(!reason.is_empty(), "{file}: reason missing");
        let src = files
            .iter()
            .find(|(rel, _)| rel == file)
            .unwrap_or_else(|| panic!("stale allowlist entry (file missing): {file}"));
        assert!(
            has_exec_call(&src.1),
            "stale allowlist entry (no exec call left): {file}"
        );
    }
}

#[test]
fn scanner_flags_start_exec_in_an_unlisted_file() {
    let sample = vec![(
        "app-logic/src/new_module.rs".to_string(),
        "fn f(t: &T) { t.transport.start_exec(cmd, sink).await; }".to_string(),
    )];
    assert_eq!(
        offenders(&sample, &[]),
        vec!["app-logic/src/new_module.rs".to_string()]
    );
    assert!(offenders(&sample, &["app-logic/src/new_module.rs"]).is_empty());
}

#[test]
fn scanner_ignores_sql_execute_and_inline_test_modules() {
    assert!(!has_exec_call("conn.execute(\"DELETE FROM t\", [])?;"));
    assert!(
        has_exec_call("let mut t = session.transport.lock().await;\n t\n.execute(c)")
            || has_exec_call("session.transport.lock().await.execute(c)")
    );
    let with_tests = "fn prod() {}\n#[cfg(test)]\nmod tests {\n fn t() { x.start_exec(c); }\n}\n";
    assert!(!has_exec_call(with_tests));
}
