//! Unknown optional row fields are ignored and `is_copy` round-trips
//! (Spec 0109, C1/F7; issue #771).

use super::*;

fn group_json(extra: &str) -> String {
    format!(
        r#"{{"id":"{}","name":"g","parent_id":null,"notes":"","created_at":"t","updated_at":"t"{extra}}}"#,
        Uuid::new_v4()
    )
}

fn server_json(extra: &str) -> String {
    format!(
        r#"{{"id":"{}","name":"s","host":"h","port":22,"username":"u","group_id":null,
"auth_method":"{{}}","notes":"","jump_host_id":null,"created_at":"t","updated_at":"t",
"post_ingest_policy":"p","ai_injection_check_enabled":true,"sftp_server_path":null,
"start_directory":null{extra}}}"#,
        Uuid::new_v4()
    )
}

#[test]
fn unknown_optional_row_field_is_ignored() {
    let g: GroupRow =
        serde_json::from_str(&group_json(r#","future_field":{"x":1}"#)).expect("group");
    assert!(!g.is_copy);
    let s: ServerRow =
        serde_json::from_str(&server_json(r#","future_field":"v""#)).expect("server");
    assert!(!s.is_copy);
}

#[test]
fn is_copy_round_trips_and_defaults_to_false() {
    let mut g: GroupRow = serde_json::from_str(&group_json("")).unwrap();
    assert!(!g.is_copy);
    assert!(!serde_json::to_string(&g).unwrap().contains("is_copy"));
    g.is_copy = true;
    let back: GroupRow = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
    assert!(back.is_copy);

    let mut s: ServerRow = serde_json::from_str(&server_json("")).unwrap();
    assert!(!s.is_copy);
    s.is_copy = true;
    let back: ServerRow = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert!(back.is_copy);
}
