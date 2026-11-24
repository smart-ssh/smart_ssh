//! Issue #791: the local file handle and URI opener are public API, and a
//! `Uri` handle is refused on desktop without touching the filesystem.

use app_shell::{Access, LocalFileHandle, NoUriOpener, UriOpener};

const URI: &str = "content://com.example.documents/document/primary%3Akey.pem";

#[test]
fn uri_handle_is_refused_by_no_uri_opener_and_creates_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let h = LocalFileHandle::Uri(URI.to_string());
    let o = NoUriOpener;

    let err = h.read_to_string(&o, 1024).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    let err = h.write_all(&o, b"x").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    let err = h.stage_into(&o, &dir.path().join("stage")).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    let err = o.open(URI, Access::Write).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);

    // Nothing was created: no staged file, no file named like the URI
    // in the temp dir or the working directory.
    let stage = dir.path().join("stage");
    assert_eq!(
        std::fs::read_dir(&stage).unwrap().count(),
        0,
        "no staged file"
    );
    let top: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(top.len(), 1, "only the (empty) staging dir");
    assert!(!std::path::Path::new(URI).exists());
    assert!(!std::path::Path::new("content:").exists());
}
