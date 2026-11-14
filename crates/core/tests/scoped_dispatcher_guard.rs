//! Guard for issue #1336: the unit tests of this crate record log output
//! through the one global subscriber in `src/log_capture.rs`. A scoped
//! dispatcher anywhere in the crate sources can make that capture drop events.

use std::path::Path;

fn visit(dir: &Path, hits: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            visit(&path, hits);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).expect("read");
            for pat in ["with_default(", "set_default("] {
                if text.contains(&format!("tracing::subscriber::{pat}"))
                    || text.contains(&format!("dispatcher::{pat}"))
                {
                    hits.push(path.display().to_string());
                }
            }
        }
    }
}

#[test]
fn no_scoped_tracing_dispatcher_in_core_sources() {
    let mut hits = Vec::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut hits,
    );
    assert!(hits.is_empty(), "scoped dispatcher found: {hits:?}");
}
