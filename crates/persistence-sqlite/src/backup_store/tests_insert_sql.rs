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
        sql("ai_provider_configs", AI_PROVIDER_COLS, false),
        "INSERT INTO ai_provider_configs (id, provider_type, display_name, base_url, model, \
         supports_native_tool_calling, credential_ref, is_active, created_at, updated_at, \
         extra_headers, attestation_url, max_tokens_override, web_research_enabled) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    );
    assert_eq!(
        sql("ai_provider_configs", AI_PROVIDER_COLS, true),
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
