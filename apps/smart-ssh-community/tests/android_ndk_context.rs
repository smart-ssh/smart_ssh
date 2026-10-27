//! Issue #820: Ab tao 0.37 legt tao den Android-`ndk-context` selbst in
//! `onCreate` an. Initialisiert die App ihn zusätzlich (frühere Umgehung aus
//! Issue #795: `Keyring.initializeNdkContext`), paniert die Rust-Seite beim
//! Start. Dieser Test koppelt die Umgehung an die tao-Version im Lockfile:
//! tao >= 0.37 => kein Kotlin-Aufruf und keine `Keyring.kt`; tao < 0.37 =>
//! Test scheitert, weil die App den Kontext dann wieder selbst anlegen muss
//! (siehe `docs/android-build.md`).

use std::fs;
use std::path::{Path, PathBuf};

fn app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Liest die Version des Pakets `tao` aus dem Text einer `Cargo.lock`.
fn tao_version(lock: &str) -> Option<(u64, u64)> {
    let mut lines = lock.lines();
    while let Some(l) = lines.next() {
        if l.trim() == "name = \"tao\"" {
            let v = lines.next()?.trim().strip_prefix("version = \"")?;
            let mut it = v.trim_end_matches('"').split('.');
            let major = it.next()?.parse().ok()?;
            let minor = it.next()?.parse().ok()?;
            return Some((major, minor));
        }
    }
    None
}

/// Entfernt `//`- und `/* */`-Kommentare (ausreichend für diese Prüfung).
fn strip_comments(src: &str) -> String {
    let mut out = String::new();
    let mut rest = src;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("/*") {
            rest = r.split_once("*/").map_or("", |(_, t)| t);
        } else if let Some(r) = rest.strip_prefix("//") {
            rest = r.split_once('\n').map_or("", |(_, t)| t);
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// Liefert Verstöße (Datei: Grund) für tao >= 0.37.
fn violations(java_root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    if java_root.join("io/crates/keyring/Keyring.kt").exists() {
        found.push("io/crates/keyring/Keyring.kt must not exist".to_string());
    }
    let mut stack = vec![java_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).expect("read_dir") {
            let p = e.expect("entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "kt") {
                let src = fs::read_to_string(&p).expect("read kt");
                if strip_comments(&src).contains("initializeNdkContext") {
                    found.push(format!("{} calls initializeNdkContext", p.display()));
                }
            }
        }
    }
    found
}

#[test]
fn app_does_not_initialize_ndk_context_with_tao_037_or_newer() {
    let lock = fs::read_to_string(app_dir().join("../../Cargo.lock")).expect("Cargo.lock");
    let (major, minor) = tao_version(&lock).expect("tao in Cargo.lock");
    assert!(
        (major, minor) >= (0, 37),
        "tao {major}.{minor} < 0.37 does not create the Android ndk-context: the app must \
         initialize it again (see docs/android-build.md) and this test must be adapted"
    );
    let v = violations(&app_dir().join("gen/android/app/src/main/java"));
    assert!(v.is_empty(), "double ndk-context initialization: {v:?}");
}

#[test]
fn counter_proof_fixture_calling_initialize_is_rejected() {
    let dir = std::env::temp_dir().join(format!("ndk_ctx_fixture_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("A.kt"),
        "// Keyring.initializeNdkContext(x) in a comment is fine\nclass A { fun f() { Keyring.initializeNdkContext(c) } }\n",
    )
    .unwrap();
    let v = violations(&dir);
    assert_eq!(v.len(), 1, "{v:?}");
    fs::write(dir.join("A.kt"), "/* initializeNdkContext */ class A\n").unwrap();
    assert!(violations(&dir).is_empty());
    fs::create_dir_all(dir.join("io/crates/keyring")).unwrap();
    fs::write(dir.join("io/crates/keyring/Keyring.kt"), "class K\n").unwrap();
    assert_eq!(violations(&dir).len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn tao_version_parser_reads_lock_format() {
    assert_eq!(
        tao_version("[[package]]\nname = \"tao\"\nversion = \"0.37.1\"\n"),
        Some((0, 37))
    );
}
