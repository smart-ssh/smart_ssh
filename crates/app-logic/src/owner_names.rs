//! Issue #653: owner/group names for the file browser's Owner column.
//!
//! SFTP v3 servers usually return only numeric uid/gid. Once per session the
//! names are looked up by reading `/etc/passwd` and `/etc/group` over SFTP
//! (never via a shell command). Unreadable or unparsable files simply yield
//! no names; the UI then shows the numbers.

use std::collections::HashMap;

use ssh_manager_core::ssh::{RemoteEntry, SftpSession};

/// Upper bound for the files read; a larger file is treated as unreadable.
const MAX_ID_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// uid → user name and gid → group name of one server.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IdNames {
    pub users: HashMap<u32, String>,
    pub groups: HashMap<u32, String>,
}

/// Parses `passwd`/`group` content (`name:x:id:...`). Malformed lines and
/// comments are skipped; the first entry for an id wins (as `getpwuid` does).
pub fn parse_id_file(content: &[u8]) -> HashMap<u32, String> {
    let text = String::from_utf8_lossy(content);
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split(':');
        let (Some(name), Some(_), Some(id)) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        if let Ok(id) = id.parse::<u32>() {
            map.entry(id).or_insert_with(|| name.to_string());
        }
    }
    map
}

async fn read_map(sftp: &mut dyn SftpSession, path: &str) -> HashMap<u32, String> {
    let mut progress = |_: u64| {};
    match sftp
        .read_file_capped(path, MAX_ID_FILE_BYTES + 1, &mut progress)
        .await
    {
        Ok(bytes) if bytes.len() as u64 <= MAX_ID_FILE_BYTES => parse_id_file(&bytes),
        _ => HashMap::new(),
    }
}

/// Reads both files; failures fall back to empty maps (numbers are shown).
pub async fn load_id_names(sftp: &mut dyn SftpSession) -> IdNames {
    IdNames {
        users: read_map(sftp, "/etc/passwd").await,
        groups: read_map(sftp, "/etc/group").await,
    }
}

/// Fills in missing owner/group names from the maps; names the server
/// already sent are kept.
pub fn apply_id_names(names: &IdNames, entries: &mut [RemoteEntry]) {
    for entry in entries {
        if entry.owner.is_none() {
            entry.owner = entry.uid.and_then(|u| names.users.get(&u).cloned());
        }
        if entry.group.is_none() {
            entry.group = entry.gid.and_then(|g| names.groups.get(&g).cloned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(uid: Option<u32>, gid: Option<u32>, owner: Option<&str>) -> RemoteEntry {
        RemoteEntry {
            name: "f".into(),
            path: "/f".into(),
            is_dir: false,
            size: 0,
            permissions: 0o644,
            modified: None,
            uid,
            gid,
            owner: owner.map(String::from),
            group: None,
        }
    }

    #[test]
    fn parses_passwd_and_skips_garbage() {
        let m = parse_id_file(
            b"# c\nroot:x:0:0:root:/root:/bin/sh\nbad line\nx:y:notnum:1\n:x:5:5\nalice:x:1000:1000::/h:/bin/sh\ndup:x:0:0\n",
        );
        assert_eq!(m.get(&0).map(String::as_str), Some("root"));
        assert_eq!(m.get(&1000).map(String::as_str), Some("alice"));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn apply_fills_missing_and_keeps_server_names() {
        let names = IdNames {
            users: HashMap::from([(0, "root".to_string()), (1, "bin".to_string())]),
            groups: HashMap::from([(0, "wheel".to_string())]),
        };
        let mut v = vec![
            entry(Some(0), Some(0), None),
            entry(Some(1), Some(7), Some("srv")),
        ];
        apply_id_names(&names, &mut v);
        assert_eq!(v[0].owner.as_deref(), Some("root"));
        assert_eq!(v[0].group.as_deref(), Some("wheel"));
        assert_eq!(v[1].owner.as_deref(), Some("srv"));
        assert_eq!(v[1].group, None);
    }

    #[test]
    fn apply_with_empty_maps_leaves_numbers_only() {
        let mut v = vec![entry(Some(5), Some(6), None)];
        apply_id_names(&IdNames::default(), &mut v);
        assert_eq!((v[0].owner.clone(), v[0].group.clone()), (None, None));
    }

    #[tokio::test]
    async fn load_reads_both_files_and_falls_back_when_unreadable() {
        use ssh_manager_core::ssh::mock::MockSftpSession;
        let mut ok = MockSftpSession::new()
            .with_file("/etc/passwd", "root:x:0:0::/root:/bin/sh\n")
            .with_file("/etc/group", "wheel:x:10:\n");
        let n = load_id_names(&mut ok).await;
        assert_eq!(n.users.get(&0).map(String::as_str), Some("root"));
        assert_eq!(n.groups.get(&10).map(String::as_str), Some("wheel"));

        let mut denied = MockSftpSession::new()
            .with_permission_denied("/etc/passwd")
            .with_file("/etc/group", "wheel:x:10:\n");
        let n = load_id_names(&mut denied).await;
        assert!(n.users.is_empty());
        assert_eq!(n.groups.len(), 1);

        let mut none = MockSftpSession::new();
        assert_eq!(load_id_names(&mut none).await, IdNames::default());
    }
}
