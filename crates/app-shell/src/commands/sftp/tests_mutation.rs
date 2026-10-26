//! Tests of the SFTP mutation commands of `sftp.rs`.

use ssh_transport::LocalFileSession;

use super::*;

// --- Issue #658: rows with progress and cancel for recursive delete/chmod ---

use std::sync::Arc;

use app_logic::events::TestEmitter;
use async_trait::async_trait;
use ssh_manager_core::ssh::{ChunkControl, ChunkSource, TransferEnd};

use crate::file_ops::FileOps;

/// Wraps a real local session and cancels the operation (as the row's
/// cancel button does) after `after` changing calls.
struct CancelAfter {
    inner: LocalFileSession,
    ops: Arc<FileOps>,
    session_id: SessionId,
    op_id: Uuid,
    after: usize,
    calls: usize,
}

impl CancelAfter {
    fn tick(&mut self) {
        self.calls += 1;
        if self.calls == self.after {
            assert!(self.ops.cancel(self.session_id, self.op_id));
        }
    }
}

#[async_trait]
impl SftpSession for CancelAfter {
    async fn list_dir(&mut self, p: &str) -> Result<Vec<RemoteEntry>, SshError> {
        self.inner.list_dir(p).await
    }
    async fn read_file(&mut self, p: &str) -> Result<Vec<u8>, SshError> {
        self.inner.read_file(p).await
    }
    async fn read_file_capped(
        &mut self,
        p: &str,
        max: u64,
        progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<Vec<u8>, SshError> {
        self.inner.read_file_capped(p, max, progress).await
    }
    async fn write_file(&mut self, p: &str, c: &[u8]) -> Result<(), SshError> {
        self.inner.write_file(p, c).await
    }
    async fn read_file_chunked(
        &mut self,
        p: &str,
        n: usize,
        f: &mut (dyn for<'a> FnMut(&'a [u8]) -> ChunkControl + Send),
    ) -> Result<TransferEnd, SshError> {
        self.inner.read_file_chunked(p, n, f).await
    }
    async fn write_file_chunked(
        &mut self,
        p: &str,
        n: usize,
        f: &mut (dyn for<'a> FnMut(&'a mut [u8]) -> ChunkSource + Send),
    ) -> Result<TransferEnd, SshError> {
        self.inner.write_file_chunked(p, n, f).await
    }
    async fn stat(&mut self, p: &str) -> Result<RemoteEntry, SshError> {
        self.inner.stat(p).await
    }
    async fn lstat(&mut self, p: &str) -> Result<RemoteEntry, SshError> {
        self.inner.lstat(p).await
    }
    async fn remove(&mut self, p: &str) -> Result<(), SshError> {
        self.inner.remove(p).await?;
        self.tick();
        Ok(())
    }
    async fn rename(&mut self, a: &str, b: &str) -> Result<(), SshError> {
        self.inner.rename(a, b).await
    }
    async fn posix_rename(&mut self, a: &str, b: &str) -> Result<bool, SshError> {
        self.inner.posix_rename(a, b).await
    }
    async fn create_dir(&mut self, p: &str) -> Result<(), SshError> {
        self.inner.create_dir(p).await
    }
    async fn create_file_exclusive(&mut self, p: &str) -> Result<(), SshError> {
        self.inner.create_file_exclusive(p).await
    }
    async fn remove_dir(&mut self, p: &str) -> Result<(), SshError> {
        self.inner.remove_dir(p).await?;
        self.tick();
        Ok(())
    }
    async fn set_owner(&mut self, _: &str, _: u32, _: u32) -> Result<(), SshError> {
        unreachable!()
    }
    async fn set_permissions(&mut self, p: &str, m: u32) -> Result<(), SshError> {
        self.inner.set_permissions(p, m).await?;
        self.tick();
        Ok(())
    }
}

fn tree(files: usize) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("big");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    for i in 0..files {
        std::fs::write(root.join(format!("f{i}.txt")), b"x").unwrap();
    }
    std::fs::write(root.join("sub/g.txt"), b"x").unwrap();
    (dir, root)
}

fn rows_of(emitter: &TestEmitter) -> Vec<(String, serde_json::Value)> {
    emitter.events.lock().unwrap().clone()
}

/// Runs `delete_recursive` the way `delete_impl_reporting` does.
async fn delete_with_rows(
    emitter: &TestEmitter,
    sftp: &mut dyn SftpSession,
    ops: &Arc<FileOps>,
    sid: SessionId,
    guard: &crate::file_ops::OpGuard,
    path: &str,
) -> Result<Option<()>, SshError> {
    let _ = ops;
    let mut rows = EntryRows::new(
        Some((emitter, sid)),
        SftpTransferKind::Delete,
        guard.handle(),
        path,
    );
    let result = rows.run(delete_recursive(sftp, path, guard.handle())).await;
    rows.finish(
        result
            .as_ref()
            .map(|r| r.is_some())
            .map_err(|e| e.to_string()),
    );
    result
}

#[tokio::test]
async fn test_recursive_delete_reports_monotonic_throttled_progress_with_total() {
    let (_keep, root) = tree(200);
    let emitter = TestEmitter::default();
    let ops = Arc::new(FileOps::default());
    let sid = Uuid::new_v4();
    let guard = ops.register(sid, Uuid::new_v4(), OpKind::Delete, "p", false);
    let mut sftp = LocalFileSession::new();

    delete_with_rows(
        &emitter,
        &mut sftp,
        &ops,
        sid,
        &guard,
        root.to_str().unwrap(),
    )
    .await
    .unwrap();

    let events = rows_of(&emitter);
    assert_eq!(events[0].0, "sftp-transfer-started");
    assert_eq!(events[0].1["kind"], "delete");
    let progress: Vec<_> = events
        .iter()
        .filter(|(n, _)| n == "sftp-transfer-progress")
        .map(|(_, p)| p["entries"].clone())
        .collect();
    // 201 files + 2 folders; far fewer events than entries (throttled).
    assert!(progress.len() < 20, "not throttled: {}", progress.len());
    let mut last = 0;
    for e in &progress {
        let done = e["done"].as_u64().unwrap();
        assert!(done >= last, "counts must be monotonic");
        last = done;
        if let Some(t) = e["total"].as_u64() {
            assert_eq!(t, 203);
        }
    }
    assert_eq!(progress.last().unwrap()["done"], 203);
    assert_eq!(progress.last().unwrap()["total"], 203);
    let (name, fin) = events.last().unwrap();
    assert_eq!(name, "sftp-transfer-finished");
    assert_eq!(fin["entriesDone"], 203);
    assert!(fin["error"].is_null() && fin.get("stopped").is_none());
    assert!(!root.exists());
}

#[tokio::test]
async fn test_recursive_delete_cancel_stops_within_one_entry_and_reports_count() {
    let (_keep, root) = tree(20);
    let emitter = TestEmitter::default();
    let ops = Arc::new(FileOps::default());
    let sid = Uuid::new_v4();
    let op_id = Uuid::new_v4();
    let guard = ops.register(sid, op_id, OpKind::Delete, "p", false);
    let mut sftp = CancelAfter {
        inner: LocalFileSession::new(),
        ops: ops.clone(),
        session_id: sid,
        op_id,
        after: 5,
        calls: 0,
    };

    let result = delete_with_rows(
        &emitter,
        &mut sftp,
        &ops,
        sid,
        &guard,
        root.to_str().unwrap(),
    )
    .await;

    assert!(result.unwrap().is_none(), "stopped by the user");
    assert_eq!(sftp.calls, 5, "no further entry after the cancel");
    let events = rows_of(&emitter);
    let (_, fin) = events.last().unwrap();
    assert_eq!(fin["stopped"], "cancelled");
    assert_eq!(fin["entriesDone"], 5);
    assert!(root.exists(), "the rest stays");
}

#[cfg(unix)] // LocalFileSession cannot chmod on Windows
#[tokio::test]
async fn test_recursive_chmod_reports_progress_and_cancel_count() {
    let (_keep, root) = tree(10);
    let emitter = TestEmitter::default();
    let ops = Arc::new(FileOps::default());
    let sid = Uuid::new_v4();
    let op_id = Uuid::new_v4();
    let guard = ops.register(sid, op_id, OpKind::Chmod, "p", false);
    let mut sftp = CancelAfter {
        inner: LocalFileSession::new(),
        ops: ops.clone(),
        session_id: sid,
        op_id,
        after: 4,
        calls: 0,
    };
    let path = root.to_str().unwrap();
    let mut rows = EntryRows::new(
        Some((&emitter, sid)),
        SftpTransferKind::Chmod,
        guard.handle(),
        path,
    );
    let result = rows
        .run(chmod_recursive(
            &mut sftp,
            path,
            0o755,
            true,
            false,
            guard.handle(),
        ))
        .await;
    rows.finish(
        result
            .as_ref()
            .map(|r| r.is_some())
            .map_err(|e| e.to_string()),
    );

    assert!(result.unwrap().is_none());
    assert_eq!(sftp.calls, 4);
    let events = rows_of(&emitter);
    assert_eq!(events[0].1["kind"], "chmod");
    let (_, fin) = events.last().unwrap();
    assert_eq!(fin["stopped"], "cancelled");
    assert_eq!(fin["entriesDone"], 4);
    // total is the whole tree: root + sub + 11 files
    let total = events
        .iter()
        .filter_map(|(_, p)| p["entries"]["total"].as_u64())
        .next_back();
    assert_eq!(total, Some(13));
}

#[tokio::test]
async fn test_single_file_delete_and_non_recursive_chmod_show_no_row() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("one.txt");
    std::fs::write(&file, b"x").unwrap();
    let emitter = TestEmitter::default();
    let ops = Arc::new(FileOps::default());
    let sid = Uuid::new_v4();
    let guard = ops.register(sid, Uuid::new_v4(), OpKind::Delete, "p", false);
    let mut sftp = LocalFileSession::new();
    let path = file.to_str().unwrap();

    delete_with_rows(&emitter, &mut sftp, &ops, sid, &guard, path)
        .await
        .unwrap();
    let mut rows = EntryRows::new(
        Some((&emitter, sid)),
        SftpTransferKind::Chmod,
        guard.handle(),
        path,
    );
    let r = rows
        .run(chmod_recursive(
            &mut sftp,
            path,
            0o600,
            false,
            false,
            guard.handle(),
        ))
        .await;
    rows.finish(r.as_ref().map(|r| r.is_some()).map_err(|e| e.to_string()));

    assert!(rows_of(&emitter).is_empty());
}

#[tokio::test]
async fn test_delete_recursive_removes_nested_files_and_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("a.txt"), b"a").unwrap();
    std::fs::write(root.join("sub/b.txt"), b"b").unwrap();
    let mut sftp = LocalFileSession::new();

    delete_recursive(&mut sftp, root.to_str().unwrap(), &OpHandle::detached())
        .await
        .expect("delete_recursive() sollte gelingen");

    assert!(!root.exists());
}

#[tokio::test]
async fn test_delete_recursive_on_a_plain_file_just_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("solo.txt");
    std::fs::write(&file, b"x").unwrap();
    let mut sftp = LocalFileSession::new();

    delete_recursive(&mut sftp, file.to_str().unwrap(), &OpHandle::detached())
        .await
        .unwrap();

    assert!(!file.exists());
}

/// Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets, ERHÖHTE
/// Priorität): "Löschen" auf einen Symlink, der auf ein Verzeichnis
/// AUSSERHALB des eigentlich gemeinten Baums zeigt, darf niemals in
/// dieses Ziel hinein rekursieren — nur der Symlink selbst wird
/// entfernt. Verifiziert gegen den un-gefixten Stand: mit `stat` statt
/// `lstat` als Root-Prüfung (der Zustand vor diesem Fix) schlägt dieser
/// Test fehl, weil `outside/victim.txt` dann mitgelöscht würde.
// Windows kennt weder Symlinks noch Unix-Rechtebits über diese API
// (ADR 0122, R1) — der Test bleibt auf Unix unverändert aktiv (Spec
// 0054, Review des Gesamtpakets, ERHÖHTE Priorität).
#[cfg(unix)]
#[tokio::test]
async fn test_delete_recursive_does_not_follow_a_symlink_at_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("victim.txt"), b"do not delete me").unwrap();
    let link = dir.path().join("link-to-outside");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let mut sftp = LocalFileSession::new();

    delete_recursive(&mut sftp, link.to_str().unwrap(), &OpHandle::detached())
        .await
        .expect("delete_recursive() sollte gelingen (löscht nur den Symlink)");

    assert!(!link.exists(), "der Symlink selbst sollte entfernt sein");
    assert!(
        outside.join("victim.txt").exists(),
        "das Symlink-Ziel außerhalb des Baums darf unangetastet bleiben"
    );
}

/// Gegenstück für rekursives chmod — dieselbe Symlink-Begründung.
/// Unix-only wie oben (ADR 0122, R1).
#[cfg(unix)]
#[tokio::test]
async fn test_chmod_recursive_does_not_follow_a_symlink_at_the_root() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let victim = outside.join("victim.txt");
    std::fs::write(&victim, b"x").unwrap();
    std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o644)).unwrap();
    let link = dir.path().join("link-to-outside");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let mut sftp = LocalFileSession::new();

    chmod_recursive(
        &mut sftp,
        link.to_str().unwrap(),
        0o700,
        true,
        false,
        &OpHandle::detached(),
    )
    .await
    .expect("chmod_recursive() sollte gelingen (setzt nur den Symlink selbst)");

    let victim_entry = sftp.stat(victim.to_str().unwrap()).await.unwrap();
    assert_eq!(
        victim_entry.permissions, 0o644,
        "das Symlink-Ziel außerhalb des Baums darf unangetastet bleiben"
    );
}

#[tokio::test]
async fn test_walk_dirs_and_count_files_counts_the_whole_tree() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("a.txt"), b"a").unwrap();
    std::fs::write(root.join("sub/b.txt"), b"b").unwrap();
    std::fs::write(root.join("sub/c.txt"), b"c").unwrap();
    let mut sftp = LocalFileSession::new();

    let (dirs, file_count) = walk_dirs_and_count_files(&mut sftp, root.to_str().unwrap())
        .await
        .unwrap();

    // `dirs` enthält den Wurzelordner selbst plus "sub" — s.
    // `DeletePreviewDto::dir_count`s Doc-Kommentar ("zählt den Ordner
    // selbst mit").
    assert_eq!(dirs.len(), 2);
    assert_eq!(file_count, 3);
}

// `LocalFileSession::set_permissions` liefert unter Windows absichtlich
// `Unsupported` (keine Unix-Rechtebits über diese API) —
// dieser Test erwartet Erfolg und bleibt deshalb wie sein Nachbar oben
// auf Unix beschränkt (ADR 0122, R1).
#[cfg(unix)]
#[tokio::test]
async fn test_chmod_recursive_without_recursive_flag_only_touches_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    std::fs::create_dir_all(&root).unwrap();
    let child = root.join("child.txt");
    std::fs::write(&child, b"x").unwrap();
    let mut sftp = LocalFileSession::new();

    chmod_recursive(
        &mut sftp,
        root.to_str().unwrap(),
        0o700,
        false,
        false,
        &OpHandle::detached(),
    )
    .await
    .unwrap();

    let root_entry = sftp.stat(root.to_str().unwrap()).await.unwrap();
    let child_entry = sftp.stat(child.to_str().unwrap()).await.unwrap();
    assert_eq!(root_entry.permissions, 0o700);
    assert_ne!(child_entry.permissions, 0o700);
}

// Dasselbe wie oben: `set_permissions` liefert unter Windows
// `Unsupported`, der Test bleibt auf Unix beschränkt (ADR 0122, R1).
#[cfg(unix)]
#[tokio::test]
async fn test_chmod_recursive_with_recursive_flag_touches_every_entry() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let nested = root.join("sub/child.txt");
    std::fs::write(&nested, b"x").unwrap();
    let mut sftp = LocalFileSession::new();

    // 0o700 statt 0o600: das Execute-Bit muss für Verzeichnisse
    // erhalten bleiben, sonst sperrt man sich beim rekursiven chmod
    // selbst aus dem eigenen Baum aus (Unix braucht `x` auf jedem
    // Vorfahren, um einen Pfad darunter überhaupt aufzulösen) — exakt
    // der Bug, den `chmod_recursive`s "erst lesend traversieren, dann
    // von unten nach oben setzen"-Reihenfolge verhindern soll; dieser
    // Test verifiziert das Ergebnis, nicht die Reihenfolge selbst.
    let changed = chmod_recursive(
        &mut sftp,
        root.to_str().unwrap(),
        0o700,
        true,
        false,
        &OpHandle::detached(),
    )
    .await
    .expect("chmod_recursive() sollte gelingen");

    // Spec 0067, Teil B: Anzahl für die Sammelmeldung — tree, sub,
    // sub/child.txt.
    assert_eq!(changed, Some(3));
    let root_entry = sftp.stat(root.to_str().unwrap()).await.unwrap();
    let sub_entry = sftp.stat(root.join("sub").to_str().unwrap()).await.unwrap();
    let nested_entry = sftp.stat(nested.to_str().unwrap()).await.unwrap();
    assert_eq!(root_entry.permissions, 0o700);
    assert_eq!(sub_entry.permissions, 0o700);
    assert_eq!(nested_entry.permissions, 0o700);
}

#[cfg(unix)]
fn mode_of(p: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o7777
}

/// Issue #624: builds root(dir) / f.txt / sub(dir) / sub/g.txt.
#[cfg(unix)]
fn chmod_tree(root_mode: u32) -> (tempfile::TempDir, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("f.txt"), b"x").unwrap();
    std::fs::write(root.join("sub/g.txt"), b"x").unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(root_mode)).unwrap();
    (dir, root)
}

#[cfg(unix)]
async fn run_chmod(root: &std::path::Path, mode: u32, special: bool) {
    let mut sftp = LocalFileSession::new();
    chmod_recursive(
        &mut sftp,
        root.to_str().unwrap(),
        mode,
        true,
        special,
        &OpHandle::detached(),
    )
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn test_chmod_recursive_644_gives_files_644_and_folders_755() {
    let (_d, root) = chmod_tree(0o755);
    run_chmod(&root, 0o644, false).await;
    assert_eq!(mode_of(&root), 0o755);
    assert_eq!(mode_of(&root.join("sub")), 0o755);
    assert_eq!(mode_of(&root.join("f.txt")), 0o644);
    assert_eq!(mode_of(&root.join("sub/g.txt")), 0o644);
}

#[cfg(unix)]
#[tokio::test]
async fn test_chmod_recursive_600_gives_files_600_and_folders_700() {
    let (_d, root) = chmod_tree(0o755);
    run_chmod(&root, 0o600, false).await;
    assert_eq!(mode_of(&root), 0o700);
    assert_eq!(mode_of(&root.join("sub")), 0o700);
    assert_eq!(mode_of(&root.join("f.txt")), 0o600);
    assert_eq!(mode_of(&root.join("sub/g.txt")), 0o600);
}

#[cfg(unix)]
#[tokio::test]
async fn test_chmod_recursive_keeps_folder_setgid_and_never_sets_it_on_files() {
    let (_d, root) = chmod_tree(0o2755);
    // The dialog started from 2755 and sends 2644 without having
    // changed the special bits.
    run_chmod(&root, 0o2644, false).await;
    assert_eq!(mode_of(&root), 0o2755);
    assert_eq!(mode_of(&root.join("sub")), 0o755);
    assert_eq!(mode_of(&root.join("f.txt")), 0o644);
    assert_eq!(mode_of(&root.join("sub/g.txt")), 0o644);
}

#[cfg(unix)]
#[tokio::test]
async fn test_chmod_recursive_applies_explicit_setgid_to_folders_only() {
    let (_d, root) = chmod_tree(0o755);
    run_chmod(&root, 0o2644, true).await;
    assert_eq!(mode_of(&root), 0o2755);
    assert_eq!(mode_of(&root.join("sub")), 0o2755);
    assert_eq!(mode_of(&root.join("f.txt")), 0o644);
    assert_eq!(mode_of(&root.join("sub/g.txt")), 0o644);
}
