//! Issue #679: one handle for a local file chosen in (or written through) a
//! system file dialog. On the desktop the dialog returns a filesystem path;
//! on Android it returns a `content://` URI that is not a path at all.
//!
//! The rule this module enforces: a [`LocalFileHandle::Uri`] never reaches
//! `std::fs` by its string. It is opened only through a [`UriOpener`] (on
//! Android the file-descriptor lookup of the platform), and the resulting
//! descriptor is read or written like any other file. Transfers that need a
//! path (SFTP upload/download, `ssh_config` import) go through an app-private
//! staging copy (ADR 0150), so the content of the picker stays untrusted
//! input handled by the existing code.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Upper bound for files that are read completely into memory (key files,
/// `ssh_config`). Streamed copies have no limit of their own, like the
/// desktop transfers.
pub(crate) const MAX_TEXT_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// A local file the user picked or is saving to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LocalFileHandle {
    /// Filesystem path (desktop; a `file://` result on Android).
    Path(PathBuf),
    /// `content://` (or other non-file) URI of the Android document picker.
    Uri(String),
}

/// What a file is opened for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Read,
    /// Create or truncate.
    Write,
}

/// Opens a URI from the system picker as a file descriptor. This is the only
/// way a [`LocalFileHandle::Uri`] is opened.
pub(crate) trait UriOpener: Send + Sync {
    fn open(&self, uri: &str, access: Access) -> io::Result<std::fs::File>;
}

/// Desktop: dialogs return paths, so a URI means a bug; refuse it.
#[cfg_attr(target_os = "android", allow(dead_code))]
pub(crate) struct NoUriOpener;

impl UriOpener for NoUriOpener {
    fn open(&self, _uri: &str, _access: Access) -> io::Result<std::fs::File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "content URIs are only available on Android",
        ))
    }
}

#[cfg(target_os = "android")]
pub(crate) struct AndroidUriOpener(pub tauri::AppHandle);

#[cfg(target_os = "android")]
impl UriOpener for AndroidUriOpener {
    fn open(&self, uri: &str, access: Access) -> io::Result<std::fs::File> {
        use tauri_plugin_fs::{FsExt, OpenOptions};
        let url = uri
            .parse::<tauri::Url>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        let mut opts = OpenOptions::new();
        match access {
            Access::Read => opts.read(true),
            Access::Write => opts.write(true).truncate(true).create(true),
        };
        self.0
            .fs()
            .open(tauri_plugin_fs::FilePath::Url(url), opts.clone())
    }
}

/// The opener of this platform.
#[cfg(target_os = "android")]
pub(crate) fn platform_opener(app: &tauri::AppHandle) -> Box<dyn UriOpener> {
    Box::new(AndroidUriOpener(app.clone()))
}

#[cfg(not(target_os = "android"))]
pub(crate) fn platform_opener(_app: &tauri::AppHandle) -> Box<dyn UriOpener> {
    Box::new(NoUriOpener)
}

impl LocalFileHandle {
    /// Converts the result of a dialog.
    pub(crate) fn from_dialog(picked: tauri_plugin_dialog::FilePath) -> io::Result<Self> {
        match picked {
            tauri_plugin_dialog::FilePath::Path(p) => Ok(Self::Path(p)),
            tauri_plugin_dialog::FilePath::Url(url) => {
                if url.scheme() == "file" {
                    url.to_file_path().map(Self::Path).map_err(|()| {
                        io::Error::new(io::ErrorKind::InvalidInput, "invalid file URL")
                    })
                } else {
                    Ok(Self::Uri(url.to_string()))
                }
            }
        }
    }

    /// The filesystem path, if this is one. A URI has none.
    pub(crate) fn as_path(&self) -> Option<&Path> {
        match self {
            Self::Path(p) => Some(p),
            Self::Uri(_) => None,
        }
    }

    /// Text for messages and results. For a URI it is the URI itself.
    pub(crate) fn display(&self) -> String {
        match self {
            Self::Path(p) => p.display().to_string(),
            Self::Uri(u) => u.clone(),
        }
    }

    /// Best-effort file name (never empty, never contains a separator): the
    /// last path component, for a URI its percent-decoded last segment after
    /// `/` or `:` (`…/document/primary%3ADownload%2Fa.txt` → `a.txt`).
    pub(crate) fn file_name(&self) -> String {
        let raw = match self {
            Self::Path(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Self::Uri(u) => {
                let without_query = u.split(['?', '#']).next().unwrap_or(u);
                let decoded = percent_decode(without_query);
                decoded
                    .rsplit(['/', ':'])
                    .next()
                    .unwrap_or_default()
                    .to_string()
            }
        };
        sanitize_file_name(&raw)
    }

    fn open(&self, opener: &dyn UriOpener, access: Access) -> io::Result<std::fs::File> {
        match self {
            Self::Path(p) => match access {
                Access::Read => std::fs::File::open(p),
                Access::Write => std::fs::File::create(p),
            },
            Self::Uri(u) => opener.open(u, access),
        }
    }

    /// Reads the whole file as UTF-8 text, at most `max` bytes.
    pub(crate) fn read_to_string(&self, opener: &dyn UriOpener, max: u64) -> io::Result<String> {
        let file = self.open(opener, Access::Read)?;
        let mut buf = String::new();
        file.take(max + 1).read_to_string(&mut buf)?;
        if buf.len() as u64 > max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file is too large",
            ));
        }
        Ok(buf)
    }

    /// Creates or truncates the file and writes `bytes`.
    pub(crate) fn write_all(&self, opener: &dyn UriOpener, bytes: &[u8]) -> io::Result<()> {
        let mut file = self.open(opener, Access::Write)?;
        file.write_all(bytes)?;
        file.flush()
    }

    /// Copies the picked file into `dir` (created, owner-only on Unix) under
    /// its [`file_name`](Self::file_name) and returns the copy. A path handle
    /// is not copied: it is returned as is.
    pub(crate) fn stage_into(&self, opener: &dyn UriOpener, dir: &Path) -> io::Result<PathBuf> {
        let Self::Uri(_) = self else {
            return self
                .as_path()
                .map(Path::to_path_buf)
                .ok_or_else(|| io::Error::other("no path"));
        };
        create_private_dir(dir)?;
        let target = dir.join(self.file_name());
        let mut src = self.open(opener, Access::Read)?;
        let mut dst = create_private_file(&target)?;
        io::copy(&mut src, &mut dst)?;
        dst.flush()?;
        Ok(target)
    }

    /// Copies the local file `src` (e.g. a finished download) into this
    /// handle, replacing its content. Returns the number of bytes.
    pub(crate) fn copy_from(&self, opener: &dyn UriOpener, src: &Path) -> io::Result<u64> {
        let mut input = std::fs::File::open(src)?;
        let mut out = self.open(opener, Access::Write)?;
        let n = io::copy(&mut input, &mut out)?;
        out.flush()?;
        Ok(n)
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(v) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A file name that is safe as one path component.
fn sanitize_file_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == '\0' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || trimmed.chars().all(|c| c == '.') {
        "file".to_string()
    } else {
        trimmed.to_string()
    }
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn create_private_file(path: &Path) -> io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serves URIs from a map to files in a temp dir and records the calls.
    struct FakeOpener {
        files: Vec<(String, PathBuf)>,
        calls: Mutex<Vec<(String, Access)>>,
    }

    impl UriOpener for FakeOpener {
        fn open(&self, uri: &str, access: Access) -> io::Result<std::fs::File> {
            self.calls.lock().unwrap().push((uri.to_string(), access));
            let (_, path) = self
                .files
                .iter()
                .find(|(u, _)| u == uri)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown uri"))?;
            match access {
                Access::Read => std::fs::File::open(path),
                Access::Write => std::fs::File::create(path),
            }
        }
    }

    const URI: &str = "content://com.android.externalstorage.documents/document/primary%3ADownload%2Fkey%20file.pem";

    fn uri_handle() -> LocalFileHandle {
        LocalFileHandle::Uri(URI.to_string())
    }

    #[test]
    fn uri_is_read_only_through_the_opener() {
        let dir = tempfile::tempdir().unwrap();
        let backing = dir.path().join("backing");
        std::fs::write(&backing, "secret").unwrap();
        let opener = FakeOpener {
            files: vec![(URI.to_string(), backing)],
            calls: Mutex::new(vec![]),
        };
        let text = uri_handle().read_to_string(&opener, 100).unwrap();
        assert_eq!(text, "secret");
        assert_eq!(
            opener.calls.lock().unwrap().as_slice(),
            &[(URI.to_string(), Access::Read)]
        );
    }

    #[test]
    fn uri_never_reaches_std_fs() {
        // The desktop opener refuses; had the URI string been used as a path,
        // the write would have created `content:` under the working directory
        // or failed with a different error kind.
        let h = LocalFileHandle::Uri("content:/tmp/should-not-exist-679".to_string());
        let err = h.write_all(&NoUriOpener, b"x").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        let err = h.read_to_string(&NoUriOpener, 10).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        assert!(!Path::new("content:/tmp/should-not-exist-679").exists());
        assert!(h.as_path().is_none());
        let dir = tempfile::tempdir().unwrap();
        assert!(h.stage_into(&NoUriOpener, dir.path()).is_err());
    }

    #[test]
    fn path_handle_uses_the_filesystem_not_the_opener() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        let h = LocalFileHandle::Path(p.clone());
        h.write_all(&NoUriOpener, b"hello").unwrap();
        assert_eq!(h.read_to_string(&NoUriOpener, 100).unwrap(), "hello");
        assert_eq!(h.stage_into(&NoUriOpener, dir.path()).unwrap(), p);
    }

    #[test]
    fn size_limit_is_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big");
        std::fs::write(&p, "0123456789").unwrap();
        let h = LocalFileHandle::Path(p);
        assert!(h.read_to_string(&NoUriOpener, 9).is_err());
        assert!(h.read_to_string(&NoUriOpener, 10).is_ok());
    }

    #[test]
    fn staging_copies_uri_content_under_its_display_name() {
        let dir = tempfile::tempdir().unwrap();
        let backing = dir.path().join("backing");
        std::fs::write(&backing, "data").unwrap();
        let opener = FakeOpener {
            files: vec![(URI.to_string(), backing)],
            calls: Mutex::new(vec![]),
        };
        let stage = dir.path().join("stage");
        let staged = uri_handle().stage_into(&opener, &stage).unwrap();
        assert_eq!(staged, stage.join("key file.pem"));
        assert_eq!(std::fs::read_to_string(staged).unwrap(), "data");
    }

    #[test]
    fn copy_from_writes_through_the_opener() {
        let dir = tempfile::tempdir().unwrap();
        let backing = dir.path().join("backing");
        let src = dir.path().join("src");
        std::fs::write(&src, "download").unwrap();
        let opener = FakeOpener {
            files: vec![(URI.to_string(), backing.clone())],
            calls: Mutex::new(vec![]),
        };
        assert_eq!(uri_handle().copy_from(&opener, &src).unwrap(), 8);
        assert_eq!(std::fs::read_to_string(backing).unwrap(), "download");
        assert_eq!(opener.calls.lock().unwrap()[0].1, Access::Write);
    }

    #[test]
    fn file_names_are_single_safe_components() {
        let n = |u: &str| LocalFileHandle::Uri(u.to_string()).file_name();
        assert_eq!(n(URI), "key file.pem");
        assert_eq!(n("content://x/document/image%3A1234"), "1234");
        assert_eq!(n("content://x/a%2F..%2F.."), "file");
        assert_eq!(n("content://x/"), "file");
        assert_eq!(n("content://x/a%5C..%5Cb"), "a_.._b");
    }

    #[test]
    fn dialog_results_map_to_handles() {
        use tauri_plugin_dialog::FilePath;
        let u: tauri::Url = "content://x/doc/1".parse().unwrap();
        assert_eq!(
            LocalFileHandle::from_dialog(FilePath::Url(u)).unwrap(),
            LocalFileHandle::Uri("content://x/doc/1".into())
        );
        let f: tauri::Url = "file:///tmp/a.txt".parse().unwrap();
        assert_eq!(
            LocalFileHandle::from_dialog(FilePath::Url(f)).unwrap(),
            LocalFileHandle::Path(PathBuf::from("/tmp/a.txt"))
        );
        assert_eq!(
            LocalFileHandle::from_dialog(FilePath::Path("/a".into())).unwrap(),
            LocalFileHandle::Path(PathBuf::from("/a"))
        );
    }
}
