//! Message content as typed parts, and the per-model attachment capability
//! check (Issue #470, Spec 0110, ADR 0138).
//!
//! Text parts are always the already fenced/redacted strings; image and PDF
//! parts carry the attachment bytes unchanged (base64) and are never decoded
//! or interpreted as instructions.

use serde_json::{json, Value};
use ssh_manager_core::ai::{
    AiError, AttachmentKind, ChatMessage, MessageContent, Role, SessionContext,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContentPart {
    Text(String),
    Image { media_type: String, base64: String },
    Pdf { name: String, base64: String },
}

/// Splits one message into parts. Everything but attachments is a single
/// text part (`text_of`). Attachments become the fenced text of each file
/// followed by its binary block (images/PDFs); adjacent text parts are joined
/// by a blank line so text-only attachments keep their single-string form.
pub(crate) fn message_parts(
    content: &MessageContent,
    text_of: fn(&MessageContent) -> String,
) -> Vec<ContentPart> {
    let MessageContent::Attachments(attachments) = content else {
        return vec![ContentPart::Text(text_of(content))];
    };
    let mut parts: Vec<ContentPart> = Vec::new();
    for a in attachments {
        let text = a.to_model_text(None);
        match parts.last_mut() {
            Some(ContentPart::Text(prev)) => {
                prev.push_str("\n\n");
                prev.push_str(&text);
            }
            _ => parts.push(ContentPart::Text(text)),
        }
        match a.kind {
            AttachmentKind::Text => {}
            AttachmentKind::Image => parts.push(ContentPart::Image {
                media_type: a.media_type.clone(),
                base64: a.data_base64(),
            }),
            AttachmentKind::Pdf => parts.push(ContentPart::Pdf {
                name: a.name.clone(),
                base64: a.data_base64(),
            }),
        }
    }
    parts
}

/// Adjacent same-role messages become one group (Issue #245); the parts of
/// all grouped messages stay in order. Text parts of different messages are
/// NOT merged here, so each stays its own part.
pub(crate) fn group_parts(
    history: &[ChatMessage],
    role_of: fn(Role) -> &'static str,
    text_of: fn(&MessageContent) -> String,
) -> Vec<(&'static str, Vec<ContentPart>)> {
    let mut groups: Vec<(&'static str, Vec<ContentPart>)> = Vec::new();
    for message in history {
        let role = role_of(message.role);
        let parts = message_parts(&message.content, text_of);
        match groups.last_mut() {
            Some((last_role, existing)) if *last_role == role => existing.extend(parts),
            _ => groups.push((role, parts)),
        }
    }
    groups
}

fn data_url(media_type: &str, base64: &str) -> String {
    format!("data:{media_type};base64,{base64}")
}

/// Anthropic Messages API content block.
pub(crate) fn anthropic_block(part: &ContentPart) -> Value {
    match part {
        ContentPart::Text(text) => json!({"type": "text", "text": text}),
        ContentPart::Image { media_type, base64 } => json!({
            "type": "image",
            "source": {"type": "base64", "media_type": media_type, "data": base64},
        }),
        ContentPart::Pdf { base64, .. } => json!({
            "type": "document",
            "source": {"type": "base64", "media_type": "application/pdf", "data": base64},
        }),
    }
}

/// OpenAI chat-completions content part.
pub(crate) fn chat_completions_block(part: &ContentPart) -> Value {
    match part {
        ContentPart::Text(text) => json!({"type": "text", "text": text}),
        ContentPart::Image { media_type, base64 } => json!({
            "type": "image_url",
            "image_url": {"url": data_url(media_type, base64)},
        }),
        ContentPart::Pdf { name, base64 } => json!({
            "type": "file",
            "file": {
                "filename": name,
                "file_data": data_url("application/pdf", base64),
            },
        }),
    }
}

/// OpenAI Responses API input content part.
pub(crate) fn responses_block(part: &ContentPart) -> Value {
    match part {
        ContentPart::Text(text) => json!({"type": "input_text", "text": text}),
        ContentPart::Image { media_type, base64 } => json!({
            "type": "input_image",
            "image_url": data_url(media_type, base64),
        }),
        ContentPart::Pdf { name, base64 } => json!({
            "type": "input_file",
            "filename": name,
            "file_data": data_url("application/pdf", base64),
        }),
    }
}

/// What a provider/model combination can read natively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AttachmentSupport {
    pub images: bool,
    pub pdf: bool,
}

/// Rejects the request when the history carries an image or PDF the model
/// cannot read. Runs before any request is built or sent; text attachments
/// never fail here.
pub(crate) fn check_attachments(
    context: &SessionContext,
    support: AttachmentSupport,
) -> Result<(), AiError> {
    for message in &context.history {
        let MessageContent::Attachments(items) = &message.content else {
            continue;
        };
        for a in items {
            match a.kind {
                AttachmentKind::Image if !support.images => {
                    return Err(AiError::AttachmentUnsupported(AttachmentKind::Image))
                }
                AttachmentKind::Pdf if !support.pdf => {
                    return Err(AiError::AttachmentUnsupported(AttachmentKind::Pdf))
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Every Claude model reads images and PDFs, including models this table
/// does not know yet.
pub(crate) fn anthropic_support(_model: &str) -> AttachmentSupport {
    AttachmentSupport {
        images: true,
        pdf: true,
    }
}

const OPENAI_HOST: &str = "api.openai.com";

/// Official OpenAI models known to be text-only (prefix match, lowercase).
const OPENAI_TEXT_ONLY_PREFIXES: &[&str] = &[
    "gpt-3.5",
    "gpt-4-0314",
    "gpt-4-0613",
    "gpt-4-32k",
    "o1-mini",
    "o1-preview",
    "o3-mini",
    "gpt-oss",
    "text-",
    "davinci",
    "babbage",
];

/// Model-name fragments of vision-capable models on third-party or local
/// OpenAI-compatible endpoints. Anything else is treated as text-only.
const VISION_HINTS: &[&str] = &[
    "vision",
    "llava",
    "bakllava",
    "moondream",
    "minicpm-v",
    "pixtral",
    "gemma3",
    "llama4",
    "qwen2.5vl",
    "qwen2-vl",
    "qwen-vl",
    "gpt-4o",
    "gpt-4.1",
    "gpt-5",
    "claude",
    "gemini",
];

fn host_of(base_url: &str) -> Option<String> {
    reqwest::Url::parse(base_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
}

fn is_official_openai(base_url: &str) -> bool {
    host_of(base_url).as_deref() == Some(OPENAI_HOST)
}

fn openai_official_model_reads_files(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    !OPENAI_TEXT_ONLY_PREFIXES.iter().any(|p| m.starts_with(p))
}

/// OpenAI Responses: official endpoint only (unknown models there are
/// allowed); any other endpoint has no known support.
pub(crate) fn openai_responses_support(base_url: &str, model: &str) -> AttachmentSupport {
    let ok = is_official_openai(base_url) && openai_official_model_reads_files(model);
    AttachmentSupport {
        images: ok,
        pdf: ok,
    }
}

/// OpenAI-compatible chat completions: the official endpoint reads images
/// and PDFs (file part); other endpoints read images only for models whose
/// name marks them as vision models, and PDFs never (no known file input).
pub(crate) fn openai_compatible_support(base_url: &str, model: &str) -> AttachmentSupport {
    if is_official_openai(base_url) {
        let ok = openai_official_model_reads_files(model);
        return AttachmentSupport {
            images: ok,
            pdf: ok,
        };
    }
    let m = model.to_ascii_lowercase();
    AttachmentSupport {
        images: VISION_HINTS.iter().any(|h| m.contains(h)),
        pdf: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssh_manager_core::ai::ChatAttachment;

    fn att(kind: AttachmentKind, name: &str, media: &str, data: &[u8]) -> ChatAttachment {
        ChatAttachment {
            name: name.into(),
            media_type: media.into(),
            kind,
            data: data.to_vec(),
            byte_len: data.len() as u64,
        }
    }

    fn text_of(_: &MessageContent) -> String {
        "plain".into()
    }

    fn ctx(items: Vec<ChatAttachment>) -> SessionContext {
        SessionContext {
            system_context: String::new(),
            history: vec![ChatMessage {
                role: Role::User,
                content: MessageContent::Attachments(items),
            }],
            available_actions: vec![],
            max_tokens_hint: None,
        }
    }

    #[test]
    fn test_parts_text_only_stays_one_text_part() {
        let parts = message_parts(
            &MessageContent::Attachments(vec![
                att(AttachmentKind::Text, "a.txt", "text/plain", b"one"),
                att(AttachmentKind::Text, "b.txt", "text/plain", b"two"),
            ]),
            text_of,
        );
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn test_parts_image_and_pdf_follow_their_fenced_text() {
        let parts = message_parts(
            &MessageContent::Attachments(vec![
                att(AttachmentKind::Image, "p.png", "image/png", &[1, 2, 3]),
                att(AttachmentKind::Pdf, "d.pdf", "application/pdf", &[4, 5]),
            ]),
            text_of,
        );
        assert!(
            matches!(&parts[0], ContentPart::Text(t) if t.contains("<local_file>") && t.contains("p.png"))
        );
        assert_eq!(
            parts[1],
            ContentPart::Image {
                media_type: "image/png".into(),
                base64: "AQID".into()
            }
        );
        assert!(matches!(&parts[2], ContentPart::Text(t) if t.contains("d.pdf")));
        assert_eq!(
            parts[3],
            ContentPart::Pdf {
                name: "d.pdf".into(),
                base64: "BAU=".into()
            }
        );
    }

    #[test]
    fn test_block_structures_per_provider() {
        let img = ContentPart::Image {
            media_type: "image/png".into(),
            base64: "AQID".into(),
        };
        let pdf = ContentPart::Pdf {
            name: "d.pdf".into(),
            base64: "BAU=".into(),
        };
        assert_eq!(
            anthropic_block(&img),
            json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"AQID"}})
        );
        assert_eq!(
            anthropic_block(&pdf),
            json!({"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"BAU="}})
        );
        assert_eq!(
            chat_completions_block(&img),
            json!({"type":"image_url","image_url":{"url":"data:image/png;base64,AQID"}})
        );
        assert_eq!(
            chat_completions_block(&pdf),
            json!({"type":"file","file":{"filename":"d.pdf","file_data":"data:application/pdf;base64,BAU="}})
        );
        assert_eq!(
            responses_block(&img),
            json!({"type":"input_image","image_url":"data:image/png;base64,AQID"})
        );
        assert_eq!(
            responses_block(&pdf),
            json!({"type":"input_file","filename":"d.pdf","file_data":"data:application/pdf;base64,BAU="})
        );
    }

    #[test]
    fn test_check_rejects_unsupported_kinds_but_never_text() {
        let none = AttachmentSupport {
            images: false,
            pdf: false,
        };
        let image = ctx(vec![att(AttachmentKind::Image, "p.png", "image/png", &[1])]);
        let pdf = ctx(vec![att(
            AttachmentKind::Pdf,
            "d.pdf",
            "application/pdf",
            &[1],
        )]);
        let text = ctx(vec![att(AttachmentKind::Text, "a.txt", "text/plain", b"x")]);
        assert_eq!(
            check_attachments(&image, none),
            Err(AiError::AttachmentUnsupported(AttachmentKind::Image))
        );
        assert_eq!(
            check_attachments(&pdf, none),
            Err(AiError::AttachmentUnsupported(AttachmentKind::Pdf))
        );
        assert_eq!(check_attachments(&text, none), Ok(()));
        let all = AttachmentSupport {
            images: true,
            pdf: true,
        };
        assert_eq!(check_attachments(&image, all), Ok(()));
        assert_eq!(check_attachments(&pdf, all), Ok(()));
    }

    #[test]
    fn test_capability_table() {
        assert_eq!(
            anthropic_support("claude-future-9"),
            AttachmentSupport {
                images: true,
                pdf: true
            }
        );
        let official = "https://api.openai.com/v1";
        assert_eq!(
            openai_responses_support(official, "gpt-future"),
            AttachmentSupport {
                images: true,
                pdf: true
            }
        );
        assert_eq!(
            openai_responses_support(official, "o3-mini"),
            AttachmentSupport {
                images: false,
                pdf: false
            }
        );
        assert_eq!(
            openai_responses_support("http://localhost:11434/v1", "gpt-4o"),
            AttachmentSupport {
                images: false,
                pdf: false
            }
        );
        assert_eq!(
            openai_compatible_support(official, "gpt-4o"),
            AttachmentSupport {
                images: true,
                pdf: true
            }
        );
        assert_eq!(
            openai_compatible_support(official, "gpt-3.5-turbo"),
            AttachmentSupport {
                images: false,
                pdf: false
            }
        );
        // Local endpoints: text models unsupported, vision models images only.
        let ollama = "http://localhost:11434/v1";
        assert_eq!(
            openai_compatible_support(ollama, "llama3.1"),
            AttachmentSupport {
                images: false,
                pdf: false
            }
        );
        assert_eq!(
            openai_compatible_support(ollama, "llava:13b"),
            AttachmentSupport {
                images: true,
                pdf: false
            }
        );
        // A look-alike host is not the official endpoint.
        assert_eq!(
            openai_compatible_support("https://api.openai.com.evil.test/v1", "llama3.1"),
            AttachmentSupport {
                images: false,
                pdf: false
            }
        );
    }
}

/// Shared fixtures for the provider request-body tests (Issue #470).
#[cfg(test)]
pub(crate) mod test_fixtures {
    use super::*;
    use ssh_manager_core::ai::ChatAttachment;

    /// One user message: a text attachment carrying `secret_text`, a PNG and
    /// a PDF, followed by a plain user message.
    pub(crate) fn context_with_image_and_pdf(text_body: &str) -> SessionContext {
        let mk = |kind, name: &str, media: &str, data: &[u8]| ChatAttachment {
            name: name.into(),
            media_type: media.into(),
            kind,
            data: data.to_vec(),
            byte_len: data.len() as u64,
        };
        SessionContext {
            system_context: "sys".into(),
            history: vec![
                ChatMessage {
                    role: Role::User,
                    content: MessageContent::Attachments(vec![
                        mk(
                            AttachmentKind::Text,
                            "n.txt",
                            "text/plain",
                            text_body.as_bytes(),
                        ),
                        mk(AttachmentKind::Image, "p.png", "image/png", &[1, 2, 3]),
                        mk(AttachmentKind::Pdf, "d.pdf", "application/pdf", &[4, 5]),
                    ]),
                },
                ChatMessage {
                    role: Role::User,
                    content: MessageContent::Text("what is this?".into()),
                },
            ],
            available_actions: vec![],
            max_tokens_hint: None,
        }
    }

    /// `send()` against a closed local port: an attachment rejection arrives
    /// as the only event; if a request had been attempted, the event would be
    /// a connection error instead.
    pub(crate) async fn first_events(
        provider: &dyn ssh_manager_core::ai::AiProvider,
        context: SessionContext,
    ) -> Vec<ssh_manager_core::ai::AiEvent> {
        use futures::StreamExt;
        provider.send(context).collect().await
    }
}
