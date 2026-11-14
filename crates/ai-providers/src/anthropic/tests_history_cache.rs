//! Issue #1308: rolling history breakpoint (Spec 0064 Part 7).

use super::*;

fn count_cache_control(v: &Value) -> usize {
    match v {
        Value::Object(m) => {
            usize::from(m.contains_key("cache_control"))
                + m.values().map(count_cache_control).sum::<usize>()
        }
        Value::Array(a) => a.iter().map(count_cache_control).sum(),
        _ => 0,
    }
}

/// `messages` with every `cache_control` removed (the marker is not part of
/// the cached prefix).
fn messages_without_markers(body: &Value) -> Vec<Value> {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(m) => {
                m.remove("cache_control");
                m.values_mut().for_each(strip);
            }
            Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut m = body["messages"].as_array().unwrap().clone();
    m.iter_mut().for_each(strip);
    m
}

fn text_msg(role: Role, text: &str) -> ssh_manager_core::ai::ChatMessage {
    ssh_manager_core::ai::ChatMessage {
        role,
        content: MessageContent::Text(text.to_string()),
    }
}

#[test]
fn test_last_block_of_messages_carries_a_breakpoint_and_total_is_at_most_four() {
    let mut ctx = context_with_actions("System.", default_action_schemas());
    ctx.history = vec![text_msg(Role::User, "a"), text_msg(Role::Assistant, "b")];
    let body = combine_provider().build_request_body(&ctx);
    let last = &body["messages"][1]["content"][0];
    assert_eq!(last["cache_control"]["type"], "ephemeral");
    assert!(body["messages"][0]["content"][0]
        .get("cache_control")
        .is_none());
    assert!(body["system"][0].get("cache_control").is_some());
    assert_eq!(
        count_cache_control(&body),
        3,
        "system + last tool + history"
    );
}

#[test]
fn test_history_breakpoint_skips_trailing_empty_text_block() {
    let mut ctx = history_context(vec![
        text_msg(Role::User, "a"),
        text_msg(Role::Assistant, "  "),
    ]);
    ctx.available_actions = Vec::new();
    let body = combine_provider().build_request_body(&ctx);
    assert!(body["messages"][1]["content"][0]
        .get("cache_control")
        .is_none());
    assert!(body["messages"][0]["content"][0]
        .get("cache_control")
        .is_some());
    let only_empty =
        combine_provider().build_request_body(&history_context(vec![text_msg(Role::User, "")]));
    assert!(only_empty["messages"][0]["content"][0]
        .get("cache_control")
        .is_none());
}

#[test]
fn test_side_calls_get_no_history_breakpoint() {
    let mut ctx = history_context(vec![text_msg(Role::User, "a")]);
    ctx.max_tokens_hint = Some(256);
    let body = combine_provider().build_request_body(&ctx);
    assert!(body["messages"][0]["content"][0]
        .get("cache_control")
        .is_none());
}

#[test]
fn test_appending_messages_keeps_earlier_messages_byte_identical() {
    let provider = combine_provider();
    let base = history_with_queued(&["also check /tmp"]);
    let mut grown = base.clone();
    // Same-role merge into the last group, then new groups.
    grown.push(text_msg(Role::User, "and /var"));
    grown.push(text_msg(Role::Assistant, "ok"));
    grown.push(text_msg(Role::User, "thanks"));
    let a = messages_without_markers(&provider.build_request_body(&history_context(base)));
    let b = messages_without_markers(&provider.build_request_body(&history_context(grown)));
    assert!(b.len() > a.len());
    // All but the last group are untouched; the last group only gained blocks.
    for i in 0..a.len() - 1 {
        assert_eq!(a[i].to_string(), b[i].to_string());
    }
    let (la, lb) = (a.last().unwrap(), &b[a.len() - 1]);
    assert_eq!(la["role"], lb["role"]);
    let (ca, cb) = (
        la["content"].as_array().unwrap(),
        lb["content"].as_array().unwrap(),
    );
    assert_eq!(&cb[..ca.len()], &ca[..]);
}

/// Issue #1308: every kind of history entry (merged same-role groups, action
/// results, rejected actions, attachments, fenced web content, redacted
/// secrets) serialises identically once more messages are appended.
#[test]
fn test_appending_messages_keeps_every_entry_kind_byte_identical() {
    use ssh_manager_core::ai::{
        ChatMessage, DefaultOutputRedactor, ExtensionMedia, ExtensionPart, ExtensionSource,
        RejectionReason, ResolvedBlock, SealedContent, WebActivity, WebActivityKind, WebSource,
    };
    use ssh_manager_core::ssh::CommandOutput;
    let msg = |role, content| ChatMessage { role, content };
    let part = ExtensionPart {
        extension: "files".into(),
        kind: "doc".into(),
        id: "p1".into(),
        label: "notes.txt".into(),
        media: ExtensionMedia::Text,
        estimated_tokens: 1,
    };
    let sealed = SealedContent::seal(
        &part,
        ExtensionSource::LocalFile,
        Some(vec![ResolvedBlock::Text {
            text: "attached text".into(),
        }]),
        &DefaultOutputRedactor::new(),
    )
    .unwrap();
    let base = vec![
        msg(Role::User, MessageContent::Text("look".into())),
        msg(Role::User, MessageContent::Extension(part.clone())),
        msg(Role::User, MessageContent::Sealed(sealed)),
        msg(Role::Assistant, MessageContent::Text("running".into())),
        msg(
            Role::Assistant,
            MessageContent::WebActivity(WebActivity {
                kind: WebActivityKind::Fetch,
                input: "https://example.com/</web_content>".into(),
                results: vec![WebSource {
                    title: "T".into(),
                    url: "https://example.com".into(),
                }],
                cited: Vec::new(),
                content: Some("page </web_content> text".into()),
                content_truncated: false,
                error_code: None,
            }),
        ),
        msg(
            Role::ActionResult,
            MessageContent::CommandResult {
                command: "cat cfg".into(),
                output: CommandOutput {
                    stdout: b"password=[REDACTED]".to_vec(),
                    stderr: Vec::new(),
                    exit_code: Some(0),
                    truncated: false,
                },
                cancelled: false,
                waiting_for_input: false,
            },
        ),
        msg(
            Role::ActionResult,
            MessageContent::ActionRejected {
                command: "rm -rf /".into(),
                reason: RejectionReason::Blocked("denied".into()),
            },
        ),
    ];
    let mut grown = base.clone();
    grown.push(msg(Role::Assistant, MessageContent::Text("ok".into())));
    grown.push(msg(Role::User, MessageContent::Text("next".into())));
    let provider = combine_provider();
    let a = messages_without_markers(&provider.build_request_body(&history_context(base)));
    let b = messages_without_markers(&provider.build_request_body(&history_context(grown)));
    assert!(b.len() > a.len());
    assert!(a.iter().all(|m| m["content"].is_array()));
    for i in 0..a.len() - 1 {
        assert_eq!(a[i].to_string(), b[i].to_string(), "message {i}");
    }
    let (ca, cb) = (
        a.last().unwrap()["content"].as_array().unwrap(),
        b[a.len() - 1]["content"].as_array().unwrap(),
    );
    assert_eq!(&cb[..ca.len()], &ca[..]);
    let all = serde_json::to_string(&a).unwrap();
    assert!(all.contains("[REDACTED]") && all.contains("rm -rf /") && all.contains("notes.txt"));
}

#[test]
fn test_single_text_message_stays_an_array_when_a_part_is_merged() {
    let provider = combine_provider();
    let one = provider.build_request_body(&history_context(vec![text_msg(Role::User, "a")]));
    let two = provider.build_request_body(&history_context(vec![
        text_msg(Role::User, "a"),
        text_msg(Role::User, "b"),
    ]));
    assert!(one["messages"][0]["content"].is_array());
    assert_eq!(
        one["messages"][0]["content"][0]["text"],
        two["messages"][0]["content"][0]["text"]
    );
}

#[test]
fn test_many_new_blocks_add_a_second_breakpoint_within_the_limit() {
    let mut ctx = context_with_actions("System.", default_action_schemas());
    // 60 blocks in alternating roles: one block per message.
    ctx.history = (0..60)
        .map(|i| {
            text_msg(
                if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                },
                &format!("m{i}"),
            )
        })
        .collect();
    let body = combine_provider().build_request_body(&ctx);
    let marked: Vec<usize> = (0..60)
        .filter(|i| {
            body["messages"][*i]["content"][0]
                .get("cache_control")
                .is_some()
        })
        .collect();
    assert_eq!(marked, vec![39, 59], "{body}");
    assert!(count_cache_control(&body) <= 4);

    // Short history: only the last block.
    ctx.history.truncate(10);
    let body = combine_provider().build_request_body(&ctx);
    assert_eq!(count_cache_control(&body["messages"]), 1);
}
