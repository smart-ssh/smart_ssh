//! Spec 0083: geteilte Test-Hilfsfunktion für `servers`/`connect` — ein
//! `Server`-Fixture, das beide Module in ihren Tests brauchen, an einer
//! Stelle statt dupliziert (dasselbe Muster wie
//! `orchestration/test_support.rs`).

use chrono::Utc;

use ssh_manager_core::profiles::{AuthMethod, GroupId, PostIngestPolicy, Server};
use ssh_manager_core::shared::ServerId;

pub(super) fn dummy_server(name: &str, group_id: Option<GroupId>) -> Server {
    let now = Utc::now();
    Server {
        id: ServerId::new(),
        name: name.to_string(),
        host: "example.invalid".to_string(),
        port: 22,
        username: "user".to_string(),
        group_id,
        tags: Vec::new(),
        auth: AuthMethod::Agent,
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        created_at: now,
        updated_at: now,
    }
}
