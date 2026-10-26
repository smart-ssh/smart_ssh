//! Tests for `dto`.
//!
//! Regressionstest für einen tatsächlich aufgetretenen Bug: serdes
//! `#[serde(tag = "...", rename_all = "camelCase")]` auf einem Enum
//! wandelt nur die Varianten-/Tag-Namen in camelCase um, **nicht** die
//! Feldnamen innerhalb der Struct-artigen Varianten (empirisch
//! verifiziert — `rename_all` ohne `rename_all_fields` lässt
//! `raw_key`/`key_content`/`exit_code` etc. unverändert). Das führte im
//! echten Betrieb dazu, dass `trust_host_key` nach einem
//! `test_connection`-Host-Key-Ereignis mit "missing required key
//! rawKey" fehlschlug, weil das Frontend `rawKey` erwartete, die
//! Payload aber `raw_key` enthielt. Jeder betroffene Typ braucht
//! zusätzlich `rename_all_fields = "camelCase"` — diese Tests fixieren
//! die tatsächliche JSON-Form, damit eine künftige Änderung so einen
//! Typ nicht wieder lautlos in diesen Zustand zurückfallen lässt.

use super::*;

#[test]
fn test_test_connection_result_host_key_unknown_uses_camel_case_fields() {
    let value = TestConnectionResult::HostKeyUnknown {
        host: "example.invalid".to_string(),
        port: 22,
        raw_key: vec![1, 2, 3],
        fingerprint: "SHA256:abc".to_string(),
        key_type: Some("ssh-ed25519".to_string()),
    };
    let json = serde_json::to_value(&value).unwrap();

    assert_eq!(json["kind"], "hostKeyUnknown");
    assert_eq!(json["rawKey"], serde_json::json!([1, 2, 3]));
    assert_eq!(json["keyType"], "ssh-ed25519");
    assert!(json.get("key_type").is_none());
    assert!(
        json.get("raw_key").is_none(),
        "raw_key darf nicht mehr im snake_case vorkommen"
    );
}

fn command_result_entry(exit_code: Option<i32>) -> ChatHistoryEntryDto {
    ChatHistoryEntryDto::CommandResult {
        role: ChatHistoryRoleDto::ActionResult,
        command: "uname -a".to_string(),
        stdout: "Linux\n".to_string(),
        stderr: String::new(),
        exit_code,
        cancelled: false,
        truncated: false,
    }
}

/// Issue #177: das Frontend liest `unusableServersToUnassign`.
#[test]
fn test_delete_group_result_serializes_unusable_servers_in_camel_case() {
    let result = DeleteGroupResult {
        child_groups_to_delete: vec![],
        servers_to_unassign: vec![],
        unusable_servers_to_unassign: vec![UnusableServerDto {
            id: ServerId::new(),
            name: "broken".into(),
            host: "h".into(),
            group_id: None,
            reason: UnusableReasonDto::UnknownAuthMethod,
        }],
        executed: false,
    };
    let json = serde_json::to_value(result).unwrap();
    let object = json.as_object().unwrap();
    assert!(object.contains_key("unusableServersToUnassign"), "{json}");
    assert!(
        !object.contains_key("unusable_servers_to_unassign"),
        "{json}"
    );
    assert_eq!(
        json["unusableServersToUnassign"][0]["groupId"],
        serde_json::Value::Null
    );
}

/// Issue #545: a stored question reaches the frontend as a `question`
/// entry with camelCase fields and the stored id.
#[test]
fn test_chat_history_question_entry_shape() {
    let id = uuid::Uuid::new_v4();
    let entry = ChatHistoryEntryDto::from(ssh_manager_core::ai::ChatMessage {
        role: ssh_manager_core::ai::Role::Assistant,
        content: ssh_manager_core::ai::MessageContent::Question {
            question_id: id,
            question: "Which?".to_string(),
            options: vec![ssh_manager_core::profiles::AskUserOption {
                label: "A".to_string(),
                description: None,
            }],
            multi_select: true,
        },
    });
    let json = serde_json::to_value(entry).unwrap();
    assert_eq!(json["type"], "question");
    assert_eq!(json["questionId"], id.to_string());
    assert_eq!(json["multiSelect"], true);
    assert_eq!(json["options"][0]["label"], "A");
    assert!(json["options"][0]["description"].is_null());
}

/// Issue #167: ohne `rename_all_fields` kam `exit_code` beim Frontend
/// an, das `exitCode` liest — ein fortgesetzter Chat zeigte deshalb
/// immer "exit code: —".
#[test]
fn test_chat_history_command_result_uses_camel_case_exit_code() {
    for (exit_code, expected) in [
        (Some(0), serde_json::json!(0)),
        (Some(1), serde_json::json!(1)),
        (None, serde_json::Value::Null),
    ] {
        let json = serde_json::to_value(command_result_entry(exit_code)).unwrap();
        let object = json.as_object().unwrap();

        assert!(
            object.contains_key("exitCode"),
            "exitCode fehlt für {exit_code:?}: {json}"
        );
        assert_eq!(json["exitCode"], expected, "für {exit_code:?}");
        assert!(
            !object.contains_key("exit_code"),
            "exit_code darf nicht im snake_case vorkommen: {json}"
        );
    }
}

/// Issue #504: an extension part lists label and media only, never the
/// opaque reference or the extension name (Spec 0110, §5).
#[test]
fn test_chat_history_extension_parts_list_label_and_media_without_reference() {
    let message = ssh_manager_core::ai::ChatMessage {
        role: ssh_manager_core::ai::Role::User,
        content: ssh_manager_core::ai::MessageContent::Extension(
            ssh_manager_core::ai::ExtensionPart {
                extension: "extname".to_string(),
                kind: "OPAQUEKIND".to_string(),
                id: "OPAQUEID".to_string(),
                label: "report.pdf".to_string(),
                media: ssh_manager_core::ai::ExtensionMedia::Pdf,
                estimated_tokens: 42,
            },
        ),
    };
    let json = serde_json::to_value(ChatHistoryEntryDto::from(message)).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "type": "extensionParts",
            "role": "user",
            "items": [{ "label": "report.pdf", "media": "pdf" }]
        })
    );
}

/// Issue #167: die volle JSON-Form aller Varianten, inklusive der
/// unveränderten Tag-Werte — `rename_all_fields` darf nur die Feldnamen
/// betreffen, nicht die Tags, auf die das Frontend per `type` matcht.
#[test]
fn test_chat_history_entry_json_shape_and_tags() {
    let role = || ChatHistoryRoleDto::Assistant;
    let text = ChatHistoryEntryDto::Text {
        role: role(),
        text: "hi".to_string(),
    };
    let rejected = ChatHistoryEntryDto::ActionRejected {
        role: role(),
        command: "rm -rf /".to_string(),
        reason: "blocked".to_string(),
    };
    let web = ChatHistoryEntryDto::WebActivity {
        role: role(),
        activity: WebActivityDto {
            kind: WebActivityKindDto::Fetch,
            input: "https://example.invalid".to_string(),
            results: vec![WebSourceDto {
                title: "Example".to_string(),
                url: "https://example.invalid".to_string(),
            }],
            cited: vec![],
            content_truncated: true,
            error_code: Some("WEB_FETCH_FAILED".to_string()),
        },
    };

    assert_eq!(
        serde_json::to_value(&text).unwrap(),
        serde_json::json!({ "type": "text", "role": "assistant", "text": "hi" })
    );
    assert_eq!(
        serde_json::to_value(command_result_entry(Some(2))).unwrap(),
        serde_json::json!({
            "type": "commandResult",
            "role": "action_result",
            "command": "uname -a",
            "stdout": "Linux\n",
            "stderr": "",
            "exitCode": 2,
            "cancelled": false,
            "truncated": false,
        })
    );
    assert_eq!(
        serde_json::to_value(&rejected).unwrap(),
        serde_json::json!({
            "type": "actionRejected",
            "role": "assistant",
            "command": "rm -rf /",
            "reason": "blocked",
        })
    );
    assert_eq!(
        serde_json::to_value(&web).unwrap(),
        serde_json::json!({
            "type": "webActivity",
            "role": "assistant",
            "activity": {
                "kind": "fetch",
                "input": "https://example.invalid",
                "results": [{ "title": "Example", "url": "https://example.invalid" }],
                "cited": [],
                "contentTruncated": true,
                "errorCode": "WEB_FETCH_FAILED",
            },
        })
    );
}

/// Issue #167, Weg wie beim Fortsetzen einer Sitzung: gespeicherte
/// `ChatMessage` → DTO → JSON trägt den echten Exit-Code (0 und ≠ 0).
#[test]
fn test_resumed_command_result_message_keeps_exit_code_in_json() {
    use ssh_manager_core::ai::{ChatMessage, MessageContent, Role};
    use ssh_manager_core::ssh::CommandOutput;

    for code in [0, 127] {
        let message = ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::CommandResult {
                command: "false".to_string(),
                output: CommandOutput {
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    exit_code: Some(code),
                    truncated: false,
                },
                cancelled: false,
                waiting_for_input: false,
            },
        };

        let json = serde_json::to_value(ChatHistoryEntryDto::from(message)).unwrap();

        assert_eq!(json["type"], "commandResult");
        assert_eq!(json["exitCode"], serde_json::json!(code));
    }
}

#[test]
fn test_test_connection_result_host_key_mismatch_uses_camel_case_fields() {
    let value = TestConnectionResult::HostKeyMismatch {
        host: "example.invalid".to_string(),
        port: 22,
        raw_key: vec![9],
        expected_fingerprint: "SHA256:old".to_string(),
        actual_fingerprint: "SHA256:new".to_string(),
        key_type: None,
    };
    let json = serde_json::to_value(&value).unwrap();

    assert_eq!(json["expectedFingerprint"], "SHA256:old");
    assert_eq!(json["actualFingerprint"], "SHA256:new");
}

#[test]
fn test_auth_method_input_private_key_deserializes_from_camel_case() {
    let json = serde_json::json!({
        "kind": "privateKey",
        "keyContent": "key-data",
        "passphrase": null,
    });

    let input: AuthMethodInput = serde_json::from_value(json).unwrap();

    assert!(matches!(
        input,
        AuthMethodInput::PrivateKey { key_content: Some(k), passphrase: None } if k == "key-data"
    ));
}

#[test]
fn test_auth_method_input_certificate_deserializes_from_camel_case() {
    let json = serde_json::json!({
        "kind": "certificate",
        "certContent": "cert-data",
        "keyContent": "key-data",
    });

    let input: AuthMethodInput = serde_json::from_value(json).unwrap();

    assert!(matches!(
        input,
        AuthMethodInput::Certificate { cert_content: Some(c), key_content: Some(k) }
            if c == "cert-data" && k == "key-data"
    ));
}

#[test]
fn test_format_unix_permissions_formats_rwx_triplets() {
    assert_eq!(format_unix_permissions(0o755), "rwxr-xr-x");
    assert_eq!(format_unix_permissions(0o644), "rw-r--r--");
    assert_eq!(format_unix_permissions(0o600), "rw-------");
    assert_eq!(format_unix_permissions(0), "---------");
}

fn dummy_entry(name: &str, is_dir: bool) -> RemoteEntryDto {
    RemoteEntryDto {
        name: name.to_string(),
        path: format!("/{name}"),
        is_dir,
        size: 0,
        permissions: String::new(),
        modified: None,
        permissions_octal: 0,
        uid: None,
        gid: None,
        owner: None,
        group: None,
    }
}

#[test]
fn test_sort_remote_entries_lists_directories_first_then_alphabetically() {
    let mut entries = vec![
        dummy_entry("zebra.txt", false),
        dummy_entry("Apps", true),
        dummy_entry("alpha.txt", false),
        dummy_entry("bin", true),
    ];

    sort_remote_entries(&mut entries);

    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["Apps", "bin", "alpha.txt", "zebra.txt"]);
}

// --- Spec 0049, Fund 1: Rand-Trimmen (Windows-Copy-Paste-`\r\n`) -------

fn ai_provider_config_input(api_key: &str, base_url: Option<&str>) -> AiProviderConfigInput {
    AiProviderConfigInput {
        provider_type: ProviderType::Anthropic,
        display_name: "Test".to_string(),
        base_url: base_url.map(|s| s.to_string()),
        model: "claude-sonnet-5".to_string(),
        supports_native_tool_calling: true,
        api_key: api_key.to_string(),
        extra_headers: Vec::new(),
        attestation_url: None,
        max_tokens_override: None,
        web_research_enabled: true,
    }
}

#[test]
fn test_ai_provider_config_input_trimmed_strips_trailing_crlf_from_api_key() {
    let config = ai_provider_config_input("sk-ant-secret\r\n", None).trimmed();
    assert_eq!(config.api_key, "sk-ant-secret");
}

#[test]
fn test_ai_provider_config_input_trimmed_keeps_interior_characters() {
    let config = ai_provider_config_input("  sk ant secret \t", None).trimmed();
    assert_eq!(
        config.api_key, "sk ant secret",
        "nur der Rand wird getrimmt, innenliegende Zeichen bleiben"
    );
}

#[test]
fn test_ai_provider_config_input_trimmed_strips_whitespace_from_base_url() {
    let config =
        ai_provider_config_input("sk-key", Some(" https://example.invalid/v1\r\n ")).trimmed();
    assert_eq!(
        config.base_url.as_deref(),
        Some("https://example.invalid/v1")
    );
}

#[test]
fn test_ai_provider_config_input_trimmed_leaves_none_base_url_as_none() {
    let config = ai_provider_config_input("sk-key", None).trimmed();
    assert_eq!(config.base_url, None);
}

// --- Spec 0073: geteilter Trim, inkl. unsichtbarer Randzeichen ------

// Spec 0073, T9: eine BOM am `api_key` — der typische Kopierunfall aus
// einer UTF-8-Datei mit BOM — fällt weg. Scheitert gegen den Stand vor
// Spec 0073, weil `str::trim` U+FEFF stehen lässt.
#[test]
fn test_t9_ai_provider_config_input_trimmed_strips_bom_from_api_key() {
    let config = ai_provider_config_input("\u{FEFF}sk-ant-secret\u{FEFF}", None).trimmed();
    assert_eq!(config.api_key, "sk-ant-secret");
}

// Spec 0073, T9 (Endpunkt-Felder): dasselbe für `base_url` und
// `attestation_url` — auch sie stehen in der §1-Tabelle.
#[test]
fn test_t9_ai_provider_config_input_trimmed_strips_invisible_chars_from_urls() {
    let mut input =
        ai_provider_config_input("sk-key", Some("\u{200B}https://example.invalid/v1\u{2060}"));
    input.attestation_url = Some("\u{FEFF}https://example.invalid/att \u{200B}".to_string());
    let config = input.trimmed();
    assert_eq!(
        config.base_url.as_deref(),
        Some("https://example.invalid/v1")
    );
    assert_eq!(
        config.attestation_url.as_deref(),
        Some("https://example.invalid/att")
    );
}

// Spec 0073, T11 / A4 / I4: ein `api_key`, der **nur** aus unsichtbaren
// Zeichen besteht, wird leer — und leer heißt nach Spec 0007,
// Abschnitt 8.2, „Credential unverändert lassen", nicht „löschen".
// Genau diesen leeren String liest der `!api_key.is_empty()`-Wächter in
// `commands::update_ai_provider` (Spec 0049, Fund 1): Er überspringt
// dann den `credential_store.set`. Das ist beabsichtigt — wer diese
// Semantik später umdreht, macht diesen Test rot.
#[test]
fn test_t11_api_key_of_only_invisible_chars_becomes_empty_meaning_unchanged() {
    let config = ai_provider_config_input("\u{FEFF}\u{200B}\u{2060}", None).trimmed();
    assert!(
        config.api_key.is_empty(),
        "ein nur aus unsichtbaren Zeichen bestehender Key muss leer werden, damit \
         `update_ai_provider` das bestehende Credential unverändert lässt"
    );
}

// Issue #690: the edit dialog is prefilled from this DTO; the stored key
// must never be part of what reaches the frontend.
#[test]
fn ai_provider_config_dto_serializes_without_any_key_field() {
    let dto = AiProviderConfigDto {
        id: ProviderId::new(),
        provider_type: ProviderType::Anthropic,
        display_name: "Claude".into(),
        base_url: None,
        model: "m".into(),
        supports_native_tool_calling: true,
        is_active: false,
        extra_headers: vec![],
        attestation_url: None,
        max_tokens_override: None,
        web_research_enabled: true,
    };
    let json = serde_json::to_value(&dto).unwrap();
    let keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
    assert!(
        keys.iter().all(|k| !k.to_lowercase().contains("key")
            && !k.to_lowercase().contains("secret")
            && !k.to_lowercase().contains("credential")),
        "DTO must not carry a credential field: {keys:?}"
    );
}

// Spec 0073, T12: Der Helfer gilt für Zugangsdaten und Endpunkte, nicht
// für Anzeigetexte. Ein Anzeigename mit einem Zero-Width-Space bleibt
// unverändert — auch am Rand.
//
// Bewusst nur `display_name`: Ob `model` getrimmt gehört, ist eine
// offene Frage (ADR 0064) und wird hier nicht durch eine Assertion
// vorentschieden.
#[test]
fn test_t12_display_name_is_not_trimmed() {
    let mut input = ai_provider_config_input("sk-key", None);
    input.display_name = "\u{200B}Mein Provider\u{200B}".to_string();
    let config = input.trimmed();
    assert_eq!(config.display_name, "\u{200B}Mein Provider\u{200B}");
}

// --- Spec 0065, Teil 4: max_tokens_override-Validierung -------------

#[test]
fn test_validate_max_tokens_override_accepts_none_automatic() {
    let mut config = ai_provider_config_input("sk-key", None);
    config.max_tokens_override = None;
    assert_eq!(config.validate_max_tokens_override(), Ok(()));
}

#[test]
fn test_validate_max_tokens_override_accepts_a_positive_value() {
    let mut config = ai_provider_config_input("sk-key", None);
    config.max_tokens_override = Some(16_384);
    assert_eq!(config.validate_max_tokens_override(), Ok(()));
}

#[test]
fn test_validate_max_tokens_override_rejects_zero() {
    let mut config = ai_provider_config_input("sk-key", None);
    config.max_tokens_override = Some(0);
    assert!(config.validate_max_tokens_override().is_err());
}

#[test]
fn test_validate_max_tokens_override_rejects_above_the_upper_bound() {
    let mut config = ai_provider_config_input("sk-key", None);
    config.max_tokens_override = Some(MAX_TOKENS_OVERRIDE_UPPER_BOUND + 1);
    assert!(config.validate_max_tokens_override().is_err());
}

#[test]
fn test_validate_max_tokens_override_accepts_exactly_the_upper_bound() {
    let mut config = ai_provider_config_input("sk-key", None);
    config.max_tokens_override = Some(MAX_TOKENS_OVERRIDE_UPPER_BOUND);
    assert_eq!(config.validate_max_tokens_override(), Ok(()));
}
