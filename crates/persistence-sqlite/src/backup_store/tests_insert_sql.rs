//! The generated statements are byte-identical to the former hand-written ones.

use super::*;

fn sql(table: &str, cols: &str, upsert: bool) -> String {
    insert_sql(table, cols, upsert).0
}

#[test]
fn group_statements_match_the_former_text() {
    let plain = "INSERT INTO groups (id, name, parent_id, notes, created_at, updated_at, is_copy) \
         VALUES (?, ?, ?, ?, ?, ?, ?)";
    assert_eq!(sql("groups", GROUP_COLS, false), plain);
    assert_eq!(
        sql("groups", GROUP_COLS, true),
        format!(
            "{plain} ON CONFLICT(id) DO UPDATE SET name = excluded.name, \
             parent_id = excluded.parent_id, notes = excluded.notes, \
             created_at = excluded.created_at, updated_at = excluded.updated_at, \
             is_copy = excluded.is_copy"
        )
    );
}

#[test]
fn rule_and_note_statements_match_the_former_text() {
    assert_eq!(
        sql("filter_rules", RULE_COLS, false),
        "INSERT INTO filter_rules (id, pattern_type, pattern_value, action, scope_type, \
         scope_value, priority, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
    );
    assert_eq!(
        sql("filter_rules", RULE_COLS, true),
        "INSERT INTO filter_rules (id, pattern_type, pattern_value, action, scope_type, \
         scope_value, priority, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET pattern_type = excluded.pattern_type, \
         pattern_value = excluded.pattern_value, action = excluded.action, \
         scope_type = excluded.scope_type, scope_value = excluded.scope_value, \
         priority = excluded.priority, created_at = excluded.created_at, \
         updated_at = excluded.updated_at"
    );
    assert_eq!(
        sql("note_revisions", NOTE_COLS, true),
        "INSERT INTO note_revisions (id, target_type, target_id, content, editor_type, \
         ai_provider, ai_model, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET target_type = excluded.target_type, \
         target_id = excluded.target_id, content = excluded.content, \
         editor_type = excluded.editor_type, ai_provider = excluded.ai_provider, \
         ai_model = excluded.ai_model, created_at = excluded.created_at"
    );
}

#[test]
fn ai_provider_statements_match_the_former_text() {
    assert_eq!(
        sql(
            "ai_provider_configs",
            super::ai_provider_writer::AI_PROVIDER_COLS,
            false
        ),
        "INSERT INTO ai_provider_configs (id, provider_type, display_name, base_url, model, \
         supports_native_tool_calling, credential_ref, is_active, created_at, updated_at, \
         extra_headers, attestation_url, max_tokens_override, web_research_enabled) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    );
    assert_eq!(
        sql(
            "ai_provider_configs",
            super::ai_provider_writer::AI_PROVIDER_COLS,
            true
        ),
        "INSERT INTO ai_provider_configs (id, provider_type, display_name, base_url, model, \
         supports_native_tool_calling, credential_ref, is_active, created_at, updated_at, \
         extra_headers, attestation_url, max_tokens_override, web_research_enabled) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET provider_type = excluded.provider_type, \
         display_name = excluded.display_name, base_url = excluded.base_url, \
         model = excluded.model, \
         supports_native_tool_calling = excluded.supports_native_tool_calling, \
         credential_ref = excluded.credential_ref, is_active = excluded.is_active, \
         created_at = excluded.created_at, updated_at = excluded.updated_at, \
         extra_headers = excluded.extra_headers, attestation_url = excluded.attestation_url, \
         max_tokens_override = excluded.max_tokens_override, \
         web_research_enabled = excluded.web_research_enabled"
    );
}

// Unknown optional row fields are ignored and `is_copy` round-trips
// (Spec 0109, C1/F7; issue #771).
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
