//! Issue #849: `app-logic` has no path to the elevated file-browser channel.
//! The compiler enforces it (`app-logic` does not depend on `app-shell`); this
//! source scan keeps a copy of such a type from being declared or used there
//! anyway. Comment lines are skipped (they may explain the boundary).

const FORBIDDEN: [&str; 4] = [
    "ElevatedSftpRegistry",
    "ElevatedSftp",
    "BrowserAccess",
    "BrowserChannel",
];

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether `line` contains `word` as a whole identifier.
fn mentions(line: &str, word: &str) -> bool {
    line.match_indices(word).any(|(i, _)| {
        let before = line[..i].chars().next_back();
        let after = line[i + word.len()..].chars().next();
        !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char)
    })
}

#[test]
fn test_app_logic_sources_never_use_the_elevated_channel_types() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../app-logic/src");
    let mut stack = vec![root.clone()];
    let mut scanned = 0usize;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("app-logic/src is readable") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).expect("source is readable");
                scanned += 1;
                for (n, line) in text.lines().enumerate() {
                    if line.trim_start().starts_with("//") {
                        continue;
                    }
                    for word in FORBIDDEN {
                        assert!(
                            !mentions(line, word),
                            "{}:{} mentions {word}",
                            path.display(),
                            n + 1
                        );
                    }
                }
            }
        }
    }
    assert!(
        scanned > 50,
        "scan found only {scanned} files under {root:?}"
    );
}

#[test]
fn test_the_scan_recognises_whole_identifiers_only() {
    assert!(mentions("use x::BrowserChannel;", "BrowserChannel"));
    assert!(mentions("ElevatedSftp::new()", "ElevatedSftp"));
    assert!(!mentions("ElevatedSftpRegistry", "ElevatedSftp"));
    assert!(!mentions("MyBrowserAccessor", "BrowserAccess"));
}
