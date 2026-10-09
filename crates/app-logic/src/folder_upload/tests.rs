//! Issue #128: the folder walk. Every test plants real files; the grant set
//! is the real one.

use std::path::Path;

use super::*;
use crate::local_path_grants::LocalPathGrants;
use crate::state::SessionId;

fn write(path: &Path, content: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Grants `root` to a fresh session and returns the granted root plus the
/// snapshot the walk uses.
fn granted(root: &Path) -> (GrantedLocalPath, GrantSnapshot) {
    let grants = LocalPathGrants::new();
    let session = SessionId::new_v4();
    grants.grant(session, vec![root.to_path_buf()]);
    let root = grants.check(session, root, None).unwrap();
    (root, grants.snapshot(session, None))
}

fn rel_files(plan: &FolderUploadPlan) -> Vec<&str> {
    plan.files.iter().map(|f| f.relative.as_str()).collect()
}

#[test]
fn nested_tree_is_planned_with_parents_before_children() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    write(&root.join("a.txt"), b"a");
    write(&root.join("sub/b.txt"), b"b");
    write(&root.join("sub/deep/c.txt"), b"c");
    std::fs::create_dir_all(root.join("empty")).unwrap();
    let (granted_root, snap) = granted(&root);

    let plan = plan_folder(&granted_root, &snap).unwrap();

    assert_eq!(plan.folder_name, "proj");
    assert_eq!(plan.dirs, vec!["empty", "sub", "sub/deep"]);
    assert_eq!(
        rel_files(&plan),
        vec!["a.txt", "sub/b.txt", "sub/deep/c.txt"]
    );
    assert!(plan.skipped.is_empty());
}

#[test]
fn empty_folder_plans_nothing_but_has_a_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("empty");
    std::fs::create_dir_all(&root).unwrap();
    let (granted_root, snap) = granted(&root);

    let plan = plan_folder(&granted_root, &snap).unwrap();

    assert_eq!(plan.folder_name, "empty");
    assert!(plan.dirs.is_empty() && plan.files.is_empty());
}

#[test]
fn a_plain_file_is_not_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("f.txt");
    write(&file, b"x");
    let (granted_root, snap) = granted(&file);

    assert!(plan_folder(&granted_root, &snap).is_err());
}

/// Adversarial: a link to a file or folder outside the root is listed as
/// skipped and neither its target nor anything below it is planned.
#[cfg(unix)]
#[test]
fn symlinks_are_skipped_and_never_followed() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    let outside = dir.path().join("outside");
    write(&root.join("ok.txt"), b"ok");
    write(&outside.join("canary.txt"), b"SECRET");
    symlink(outside.join("canary.txt"), root.join("link-file")).unwrap();
    symlink(&outside, root.join("link-dir")).unwrap();
    // A link inside the root pointing to a sibling inside the root is
    // skipped as well (ADR 0125: no link is followed).
    symlink(root.join("ok.txt"), root.join("link-inside")).unwrap();
    // A loop.
    symlink(&root, root.join("loop")).unwrap();
    let (granted_root, snap) = granted(&root);

    let plan = plan_folder(&granted_root, &snap).unwrap();

    assert_eq!(rel_files(&plan), vec!["ok.txt"]);
    assert!(plan.dirs.is_empty());
    let mut skipped: Vec<_> = plan
        .skipped
        .iter()
        .map(|s| (s.path.as_str(), s.reason))
        .collect();
    skipped.sort_by_key(|s| s.0);
    assert_eq!(
        skipped,
        vec![
            ("link-dir", SkipReason::Symlink),
            ("link-file", SkipReason::Symlink),
            ("link-inside", SkipReason::Symlink),
            ("loop", SkipReason::Symlink),
        ]
    );
}

/// The snapshot refuses a path outside the grant, so a sibling of the
/// granted root is never planned even if the walk were pointed at it.
#[test]
fn sibling_of_the_granted_root_is_not_granted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    let sibling = dir.path().join("proj-secret");
    write(&root.join("a.txt"), b"a");
    write(&sibling.join("s.txt"), b"s");
    let (_granted_root, snap) = granted(&root);

    assert!(snap.check(&sibling.join("s.txt")).is_err());
    assert!(snap
        .check(&root.join("..").join("proj-secret").join("s.txt"))
        .is_err());
}

#[test]
fn very_deep_tree_stops_at_the_depth_limit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("deep");
    let mut p = root.clone();
    for _ in 0..(MAX_DEPTH + 3) {
        p = p.join("d");
    }
    std::fs::create_dir_all(&p).unwrap();
    let (granted_root, snap) = granted(&root);

    let plan = plan_folder(&granted_root, &snap).unwrap();

    assert_eq!(plan.dirs.len(), MAX_DEPTH);
    assert!(plan.skipped.iter().any(|s| s.reason == SkipReason::TooDeep));
}

#[test]
fn remote_join_does_not_double_slashes() {
    assert_eq!(join_remote("/", "x"), "/x");
    assert_eq!(join_remote("/home/u", "x/y"), "/home/u/x/y");
    assert_eq!(join_relative("", "a"), "a");
    assert_eq!(join_relative("a", "b"), "a/b");
}
