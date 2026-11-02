//! Tests of the file-browser SFTP logic (issue #849). The rename-overwrite
//! tests moved here from `app-shell`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ssh_manager_core::ssh::mock::MockSftpSession;
use ssh_manager_core::ssh::{ChunkControl, ChunkSource, TransferEnd};

use super::*;

/// Progress double; can stop after a number of `is_user_stopped` checks.
#[derive(Default)]
struct Progress {
    stopped: AtomicBool,
    stop_after_checks: Mutex<Option<u64>>,
    checks: AtomicU64,
    begun: AtomicBool,
    done: AtomicU64,
    total: Mutex<Option<u64>>,
}

impl OpProgress for Progress {
    fn is_user_stopped(&self) -> bool {
        let n = self.checks.fetch_add(1, Ordering::SeqCst);
        if let Some(limit) = *self.stop_after_checks.lock().unwrap() {
            if n >= limit {
                return true;
            }
        }
        self.stopped.load(Ordering::SeqCst)
    }
    fn begin_entries(&self) {
        self.begun.store(true, Ordering::SeqCst);
    }
    fn set_files(&self, done: u64, total: Option<u64>) {
        self.done.store(done, Ordering::SeqCst);
        *self.total.lock().unwrap() = total;
    }
    fn add_files_done(&self) {
        self.done.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct Node {
    is_dir: bool,
    is_symlink: bool,
    permissions: u32,
    uid: u32,
    gid: u32,
    /// Content of a file (used for `/etc/passwd`, `/etc/group`).
    content: Vec<u8>,
}

/// In-memory tree. A symlink to a directory is `is_dir: false,
/// is_symlink: true` under `lstat`/`list_dir` (like a real server's lstat
/// view) and has the children of its target listed under the link path, so a
/// walk that wrongly entered it would find them. Log: every mutating call.
#[derive(Clone, Default)]
struct TreeSftp {
    nodes: Arc<Mutex<BTreeMap<String, Node>>>,
    log: Arc<Mutex<Vec<String>>>,
}

impl TreeSftp {
    fn add(self, path: &str, is_dir: bool, permissions: u32) -> Self {
        self.nodes.lock().unwrap().insert(
            path.to_string(),
            Node {
                is_dir,
                is_symlink: false,
                permissions,
                uid: 1000,
                gid: 1000,
                content: Vec::new(),
            },
        );
        self
    }
    fn dir(self, path: &str) -> Self {
        self.add(path, true, 0o755)
    }
    fn file(self, path: &str) -> Self {
        self.add(path, false, 0o644)
    }
    fn file_with(self, path: &str, content: &str) -> Self {
        let s = self.file(path);
        s.nodes.lock().unwrap().get_mut(path).unwrap().content = content.as_bytes().to_vec();
        s
    }
    /// A symlink pointing at a directory elsewhere in the tree.
    fn symlink_to_dir(self, path: &str) -> Self {
        let s = self.file(path);
        s.nodes.lock().unwrap().get_mut(path).unwrap().is_symlink = true;
        s
    }
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
    fn entry(path: &str, n: &Node) -> RemoteEntry {
        RemoteEntry {
            name: path.rsplit('/').next().unwrap().to_string(),
            path: path.to_string(),
            is_dir: n.is_dir,
            size: n.content.len() as u64,
            permissions: n.permissions,
            modified: None,
            uid: Some(n.uid),
            gid: Some(n.gid),
            owner: None,
            group: None,
            is_symlink: n.is_symlink,
        }
    }
    fn missing(path: &str) -> SshError {
        SshError::ChannelError(format!("not found: {path}"))
    }
}

#[async_trait]
impl SftpSession for TreeSftp {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        Ok(self
            .nodes
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| {
                p.strip_prefix(&prefix)
                    .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
            })
            .map(|(p, n)| Self::entry(p, n))
            .collect())
    }
    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        self.nodes
            .lock()
            .unwrap()
            .get(path)
            .map(|n| n.content.clone())
            .ok_or_else(|| Self::missing(path))
    }
    async fn read_file_capped(
        &mut self,
        path: &str,
        _max: u64,
        _progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<Vec<u8>, SshError> {
        self.read_file(path).await
    }
    async fn write_file(&mut self, _: &str, _: &[u8]) -> Result<(), SshError> {
        unreachable!()
    }
    async fn read_file_chunked(
        &mut self,
        _: &str,
        _: usize,
        _: &mut (dyn for<'a> FnMut(&'a [u8]) -> ChunkControl + Send),
    ) -> Result<TransferEnd, SshError> {
        unreachable!()
    }
    async fn write_file_chunked(
        &mut self,
        _: &str,
        _: usize,
        _: &mut (dyn for<'a> FnMut(&'a mut [u8]) -> ChunkSource + Send),
    ) -> Result<TransferEnd, SshError> {
        unreachable!()
    }
    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.lstat(path).await
    }
    async fn lstat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.nodes
            .lock()
            .unwrap()
            .get(path)
            .map(|n| Self::entry(path, n))
            .ok_or_else(|| Self::missing(path))
    }
    async fn remove(&mut self, path: &str) -> Result<(), SshError> {
        self.log.lock().unwrap().push(format!("remove {path}"));
        self.nodes
            .lock()
            .unwrap()
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| Self::missing(path))
    }
    async fn rename(&mut self, _: &str, _: &str) -> Result<(), SshError> {
        unreachable!()
    }
    async fn posix_rename(&mut self, _: &str, _: &str) -> Result<bool, SshError> {
        unreachable!()
    }
    async fn create_dir(&mut self, _: &str) -> Result<(), SshError> {
        unreachable!()
    }
    async fn create_file_exclusive(&mut self, _: &str) -> Result<(), SshError> {
        unreachable!()
    }
    async fn remove_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.log.lock().unwrap().push(format!("remove_dir {path}"));
        self.nodes
            .lock()
            .unwrap()
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| Self::missing(path))
    }
    async fn set_owner(&mut self, path: &str, uid: u32, gid: u32) -> Result<(), SshError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("set_owner {path} {uid} {gid}"));
        Ok(())
    }
    async fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), SshError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("set_permissions {path} {mode:o}"));
        Ok(())
    }
}

fn entry(is_dir: bool, permissions: u32) -> RemoteEntry {
    TreeSftp::entry(
        "/x",
        &Node {
            is_dir,
            is_symlink: false,
            permissions,
            uid: 1,
            gid: 1,
            content: Vec::new(),
        },
    )
}

#[tokio::test]
async fn test_delete_recursive_never_descends_into_a_symlinked_directory() {
    // `/t/link` is a symlink to a directory with a file; the tree below the
    // link path must stay untouched, only the link itself is removed.
    let mut sftp = TreeSftp::default()
        .dir("/t")
        .file("/t/a")
        .symlink_to_dir("/t/link")
        .file("/t/link/secret");
    let op = Progress::default();
    assert_eq!(
        delete_recursive(&mut sftp, "/t", &op).await.unwrap(),
        Some(())
    );
    let log = sftp.log();
    assert!(log.contains(&"remove /t/link".to_string()), "{log:?}");
    assert!(!log.iter().any(|c| c.contains("/t/link/")), "{log:?}");
    assert!(sftp.nodes.lock().unwrap().contains_key("/t/link/secret"));
}

#[tokio::test]
async fn test_delete_recursive_on_a_root_symlink_only_removes_the_link() {
    let mut sftp = TreeSftp::default()
        .symlink_to_dir("/link")
        .file("/link/secret");
    let op = Progress::default();
    delete_recursive(&mut sftp, "/link", &op).await.unwrap();
    assert_eq!(sftp.log(), vec!["remove /link".to_string()]);
}

#[tokio::test]
async fn test_delete_recursive_removes_files_then_dirs_deepest_first() {
    let mut sftp = TreeSftp::default()
        .dir("/t")
        .dir("/t/d")
        .file("/t/d/f")
        .file("/t/a");
    let op = Progress::default();
    delete_recursive(&mut sftp, "/t", &op).await.unwrap();
    let log = sftp.log();
    let dir_d = log.iter().position(|c| c == "remove_dir /t/d").unwrap();
    let dir_t = log.iter().position(|c| c == "remove_dir /t").unwrap();
    let file_f = log.iter().position(|c| c == "remove /t/d/f").unwrap();
    assert!(file_f < dir_d && dir_d < dir_t, "{log:?}");
    assert!(op.begun.load(Ordering::SeqCst));
    assert_eq!(*op.total.lock().unwrap(), Some(4));
    assert_eq!(op.done.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn test_delete_recursive_stops_between_entries_when_stopped() {
    let mut sftp = TreeSftp::default()
        .dir("/t")
        .file("/t/a")
        .file("/t/b")
        .file("/t/c");
    let op = Progress::default();
    // checks: start, per-dir (1), then before/after each file
    *op.stop_after_checks.lock().unwrap() = Some(4);
    assert_eq!(delete_recursive(&mut sftp, "/t", &op).await.unwrap(), None);
    let log = sftp.log();
    assert!(!log.is_empty() && log.len() < 4, "{log:?}");
    assert!(!log.iter().any(|c| c.starts_with("remove_dir")), "{log:?}");
}

#[tokio::test]
async fn test_delete_recursive_already_stopped_touches_nothing() {
    let mut sftp = TreeSftp::default().dir("/t").file("/t/a");
    let op = Progress::default();
    op.stopped.store(true, Ordering::SeqCst);
    assert_eq!(delete_recursive(&mut sftp, "/t", &op).await.unwrap(), None);
    assert!(sftp.log().is_empty());
}

#[test]
fn test_recursive_entry_mode_for_files_ignores_special_bits_and_adds_no_execute() {
    let f = entry(false, 0o4644);
    assert_eq!(recursive_entry_mode(&f, 0o644, false), 0o644);
    assert_eq!(recursive_entry_mode(&f, 0o2755, true), 0o755);
    assert_eq!(recursive_entry_mode(&f, 0o600, false), 0o600);
}

#[test]
fn test_recursive_entry_mode_for_folders_adds_execute_where_readable() {
    let d = entry(true, 0o755);
    assert_eq!(recursive_entry_mode(&d, 0o644, false), 0o755);
    assert_eq!(recursive_entry_mode(&d, 0o600, false), 0o700);
    assert_eq!(recursive_entry_mode(&d, 0o000, false), 0o000);
}

#[test]
fn test_recursive_entry_mode_keeps_folder_special_bits_unless_applied() {
    let d = entry(true, 0o2755);
    assert_eq!(recursive_entry_mode(&d, 0o644, false), 0o2755);
    assert_eq!(recursive_entry_mode(&d, 0o1644, true), 0o1755);
    assert_eq!(recursive_entry_mode(&d, 0o644, true), 0o755);
}

#[tokio::test]
async fn test_chown_ids_resolve_names_numbers_and_reject_empty_input() {
    let mut sftp = TreeSftp::default()
        .dir("/etc")
        .file_with("/etc/passwd", "alice:x:1001:1001::/home/alice:/bin/sh\n")
        .file_with("/etc/group", "staff:x:50:\n");
    // names
    assert_eq!(
        resolve_chown_ids(&mut sftp, Some("alice"), Some("staff"))
            .await
            .unwrap(),
        (Some(1001), Some(50))
    );
    // numbers (no lookup needed) and trimming
    assert_eq!(
        resolve_chown_ids(&mut sftp, Some(" 7 "), None)
            .await
            .unwrap(),
        (Some(7), None)
    );
    // empty / blank input
    for (o, g) in [(None, None), (Some(""), Some("  "))] {
        let err = resolve_chown_ids(&mut sftp, o, g).await.unwrap_err();
        assert_eq!(err.code, Some(CHOWN_NOTHING_TO_CHANGE));
    }
    // unknown names
    let err = resolve_chown_ids(&mut sftp, Some("mallory"), None)
        .await
        .unwrap_err();
    assert_eq!(err.code, Some(CHOWN_UNKNOWN_USER));
    let err = resolve_chown_ids(&mut sftp, None, Some("nogroup"))
        .await
        .unwrap_err();
    assert_eq!(err.code, Some(CHOWN_UNKNOWN_GROUP));
}

#[tokio::test]
async fn test_chown_recursive_skips_symlinks_below_the_root() {
    let mut sftp = TreeSftp::default()
        .dir("/t")
        .file("/t/a")
        .symlink_to_dir("/t/link");
    let op = Progress::default();
    let n = chown_recursive(&mut sftp, "/t", Some(5), Some(6), true, &op)
        .await
        .unwrap();
    assert_eq!(n, Some(2));
    let log = sftp.log();
    assert!(!log.iter().any(|c| c.contains("/t/link")), "{log:?}");
    assert_eq!(log.last().unwrap(), "set_owner /t 5 6");
}

#[tokio::test]
async fn test_chmod_recursive_applies_entry_modes_deepest_first() {
    let mut sftp = TreeSftp::default().dir("/t").dir("/t/d").file("/t/d/f");
    let op = Progress::default();
    let n = chmod_recursive(&mut sftp, "/t", 0o600, true, false, &op)
        .await
        .unwrap();
    assert_eq!(n, Some(3));
    let log = sftp.log();
    assert_eq!(log.first().unwrap(), "set_permissions /t/d/f 600");
    assert_eq!(log.last().unwrap(), "set_permissions /t 700");
}

struct TreeLister(TreeSftp);

#[async_trait]
impl DirLister for TreeLister {
    async fn list_dir(&mut self, path: &str) -> CommandResult<Vec<RemoteEntry>> {
        Ok(self.0.list_dir(path).await?)
    }
}

#[derive(Default)]
struct Collect {
    seen: Vec<(String, String)>,
    fail_on: Option<String>,
}

#[async_trait]
impl TreeVisitor for Collect {
    async fn visit(&mut self, dir: &str, entry: RemoteEntry) -> CommandResult<()> {
        if self.fail_on.as_deref() == Some(entry.path.as_str()) {
            return Err("visitor failed".into());
        }
        self.seen.push((dir.to_string(), entry.path));
        Ok(())
    }
}

#[tokio::test]
async fn test_walk_tree_lists_parent_before_children_and_honours_stop() {
    let sftp = TreeSftp::default().dir("/t").dir("/t/d").file("/t/d/f");
    let mut lister = TreeLister(sftp);
    let mut v = Collect::default();
    walk_tree(&mut lister, "/t", &Progress::default(), &mut v)
        .await
        .unwrap()
        .unwrap();
    let pos = |p: &str| v.seen.iter().position(|(_, e)| e == p).unwrap();
    assert!(pos("/t/d") < pos("/t/d/f"));
    assert_eq!(v.seen[pos("/t/d/f")].0, "/t/d");

    let op = Progress::default();
    op.stopped.store(true, Ordering::SeqCst);
    let mut v = Collect::default();
    assert!(walk_tree(&mut lister, "/t", &op, &mut v)
        .await
        .unwrap()
        .is_none());
}

/// The visitor runs right after each listing: an entry it rejects stops the
/// walk before any later directory is listed.
#[tokio::test]
async fn test_walk_tree_visits_before_listing_next_directory() {
    struct Counting(TreeLister, std::sync::Arc<std::sync::Mutex<Vec<String>>>);
    #[async_trait]
    impl DirLister for Counting {
        async fn list_dir(&mut self, path: &str) -> CommandResult<Vec<RemoteEntry>> {
            self.1.lock().unwrap().push(path.to_string());
            self.0.list_dir(path).await
        }
    }
    let sftp = TreeSftp::default()
        .dir("/t")
        .dir("/t/a")
        .dir("/t/b")
        .file("/t/a/f");
    let listed = Default::default();
    let mut lister = Counting(TreeLister(sftp), std::sync::Arc::clone(&listed));
    let mut v = Collect {
        fail_on: Some("/t/a".into()),
        ..Default::default()
    };
    assert!(walk_tree(&mut lister, "/t", &Progress::default(), &mut v)
        .await
        .is_err());
    assert_eq!(*listed.lock().unwrap(), vec!["/t".to_string()]);
}

#[tokio::test]
async fn test_overwrite_uses_posix_rename_when_offered() {
    let mut sftp = MockSftpSession::new()
        .with_file("/a", "new")
        .with_file("/b", "old")
        .with_posix_rename();
    rename_replacing(&mut sftp, "/a", "/b").await.unwrap();
    assert_eq!(sftp.file_content("/b").unwrap(), b"new");
    assert!(sftp.file_content("/a").is_none());
    let calls = sftp.calls();
    assert!(calls.contains(&"posix_rename /a -> /b".to_string()));
    assert!(!calls.iter().any(|c| c.starts_with("remove ")));
}

#[tokio::test]
async fn test_overwrite_without_posix_rename_removes_then_renames() {
    let mut sftp = MockSftpSession::new()
        .with_file("/a", "new")
        .with_file("/b", "old");
    rename_replacing(&mut sftp, "/a", "/b").await.unwrap();
    assert_eq!(sftp.file_content("/b").unwrap(), b"new");
    let calls = sftp.calls();
    let rm = calls.iter().position(|c| c == "remove /b").unwrap();
    let mv = calls.iter().position(|c| c == "rename /a -> /b").unwrap();
    assert!(rm < mv);
}

#[tokio::test]
async fn test_plain_rename_onto_existing_file_does_not_overwrite() {
    // Ohne `overwrite` ruft der Befehl nur `rename`; der Mock überschreibt
    // zwar (er kennt keinen Konflikt), entscheidend ist hier: kein
    // `remove`, kein `posix_rename` auf diesem Pfad.
    let mut sftp = MockSftpSession::new()
        .with_file("/a", "new")
        .with_posix_rename();
    sftp.rename("/a", "/b").await.unwrap();
    let calls = sftp.calls();
    assert_eq!(calls, vec!["rename /a -> /b".to_string()]);
}

#[tokio::test]
async fn test_overwrite_with_failing_rename_names_the_situation() {
    let mut sftp = MockSftpSession::new()
        .with_file("/a", "new")
        .with_file("/b", "old")
        .with_rename_failure("/a");
    let err = rename_replacing(&mut sftp, "/a", "/b").await.unwrap_err();
    let SshError::ChannelError(text) = err else {
        panic!("unexpected error");
    };
    assert!(text.contains("bereits entfernt"), "{text}");
    assert!(text.contains("Quelle ist unverändert"), "{text}");
    assert!(sftp.file_content("/b").is_none());
    assert_eq!(sftp.file_content("/a").unwrap(), b"new");
}

#[tokio::test]
async fn test_overwrite_onto_folder_is_refused_untouched() {
    let mut sftp = MockSftpSession::new()
        .with_file("/a", "new")
        .with_dir("/d")
        .with_posix_rename();
    let err = rename_replacing(&mut sftp, "/a", "/d").await.unwrap_err();
    assert!(matches!(err, SshError::ChannelError(t) if t.contains("Ordner")));
    assert!(!sftp
        .calls()
        .iter()
        .any(|c| c.starts_with("remove") || c.contains("rename")));
}
