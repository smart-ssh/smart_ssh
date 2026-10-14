//! Message content as typed parts, and the per-model attachment capability
//! check (Issue #470, Spec 0110, ADR 0138).
//!
//! Text parts are always the already fenced/redacted strings; image and PDF
//! parts carry the attachment bytes unchanged (base64) and are never decoded
//! or interpreted as instructions.

use serde_json::{json, Value};
use ssh_manager_core::ai::{
    AiError, ChatMessage, MediaClass, MessageContent, Role, SealedPart, SessionContext,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContentPart {
    Text(String),
    /// A binary block by media type (Issue #506, ADR 0141 decision 5): the
    /// wire format follows from the media class, not from where the bytes
    /// came from.
    Binary {
        mime: String,
        filename: String,
        base64: String,
    },
}

impl ContentPart {
    /// Class of a binary part. Core guarantees an image or PDF type; any
    /// other type is sent as an image, as before.
    fn class(mime: &str) -> MediaClass {
        MediaClass::of_mime(mime).unwrap_or(MediaClass::Image)
    }
}

/// Appends text; adjacent text parts are joined by a blank line.
fn push_text(parts: &mut Vec<ContentPart>, text: String) {
    match parts.last_mut() {
        Some(ContentPart::Text(prev)) => {
            prev.push_str("\n\n");
            prev.push_str(&text);
        }
        _ => parts.push(ContentPart::Text(text)),
    }
}

/// Splits one message into parts. Everything but attachments and sealed
/// content is a single text part (`text_of`). Both of the latter become
/// fenced text followed by their binary blocks through the same path;
/// adjacent text parts are joined by a blank line.
pub(crate) fn message_parts(
    content: &MessageContent,
    text_of: fn(&MessageContent) -> String,
) -> Vec<ContentPart> {
    let mut parts: Vec<ContentPart> = Vec::new();
    match content {
        MessageContent::Sealed(sealed) => {
            for part in sealed.parts() {
                match part {
                    SealedPart::Text(t) => push_text(&mut parts, t.to_string()),
                    SealedPart::Binary {
                        mime,
                        filename,
                        marker,
                        base64,
                        ..
                    } => {
                        push_text(&mut parts, marker.to_string());
                        parts.push(ContentPart::Binary {
                            mime: mime.to_string(),
                            filename: filename.to_string(),
                            base64,
                        });
                    }
                }
            }
        }
        _ => parts.push(ContentPart::Text(text_of(content))),
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
        ContentPart::Binary { mime, base64, .. } => {
            let kind = match ContentPart::class(mime) {
                MediaClass::Image => "image",
                MediaClass::Pdf => "document",
            };
            json!({
                "type": kind,
                "source": {"type": "base64", "media_type": mime, "data": base64},
            })
        }
    }
}

/// OpenAI chat-completions content part.
pub(crate) fn chat_completions_block(part: &ContentPart) -> Value {
    match part {
        ContentPart::Text(text) => json!({"type": "text", "text": text}),
        ContentPart::Binary {
            mime,
            filename,
            base64,
        } => match ContentPart::class(mime) {
            MediaClass::Image => json!({
                "type": "image_url",
                "image_url": {"url": data_url(mime, base64)},
            }),
            MediaClass::Pdf => json!({
                "type": "file",
                "file": {
                    "filename": filename,
                    "file_data": data_url(mime, base64),
                },
            }),
        },
    }
}

/// OpenAI Responses API input content part.
pub(crate) fn responses_block(part: &ContentPart) -> Value {
    match part {
        ContentPart::Text(text) => json!({"type": "input_text", "text": text}),
        ContentPart::Binary {
            mime,
            filename,
            base64,
        } => match ContentPart::class(mime) {
            MediaClass::Image => json!({
                "type": "input_image",
                "image_url": data_url(mime, base64),
            }),
            MediaClass::Pdf => json!({
                "type": "input_file",
                "filename": filename,
                "file_data": data_url(mime, base64),
            }),
        },
    }
}

/// What a provider/model combination can read natively, keyed by media
/// class (Issue #506).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InputSupport {
    pub images: bool,
    pub pdf: bool,
}

impl InputSupport {
    pub(crate) fn accepts(self, class: MediaClass) -> bool {
        match class {
            MediaClass::Image => self.images,
            MediaClass::Pdf => self.pdf,
        }
    }
}

/// Rejects the request when the history carries a binary block the model
/// cannot read. Runs over the whole history before any request is built or
/// sent; text never fails here.
pub(crate) fn check_attachments(
    context: &SessionContext,
    support: InputSupport,
) -> Result<(), AiError> {
    let check = |mime: &str| {
        let class = ContentPart::class(mime);
        if support.accepts(class) {
            Ok(())
        } else {
            Err(AiError::InputUnsupported(class))
        }
    };
    for message in &context.history {
        if let MessageContent::Sealed(sealed) = &message.content {
            for part in sealed.parts() {
                if let SealedPart::Binary { mime, .. } = part {
                    check(mime)?;
                }
            }
        }
    }
    Ok(())
}

/// Every Claude model reads images and PDFs, including models this table
/// does not know yet.
pub(crate) fn anthropic_support(_model: &str) -> InputSupport {
    InputSupport {
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
pub(crate) fn openai_responses_support(base_url: &str, model: &str) -> InputSupport {
    let ok = is_official_openai(base_url) && openai_official_model_reads_files(model);
    InputSupport {
        images: ok,
        pdf: ok,
    }
}

/// OpenAI-compatible chat completions: the official endpoint reads images
/// and PDFs (file part); other endpoints read images only for models whose
/// name marks them as vision models, and PDFs never (no known file input).
pub(crate) fn openai_compatible_support(base_url: &str, model: &str) -> InputSupport {
    if is_official_openai(base_url) {
        let ok = openai_official_model_reads_files(model);
        return InputSupport {
            images: ok,
            pdf: ok,
        };
    }
    let m = model.to_ascii_lowercase();
    InputSupport {
        images: VISION_HINTS.iter().any(|h| m.contains(h)),
        pdf: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text_of(_: &MessageContent) -> String {
        "plain".into()
    }

    #[test]
    fn test_parts_text_only_stays_one_text_part() {
        let ctx = sealed_ctx(vec![
            ssh_manager_core::ai::ResolvedBlock::Text { text: "one".into() },
            ssh_manager_core::ai::ResolvedBlock::Text { text: "two".into() },
        ]);
        let parts = message_parts(&ctx.history[0].content, text_of);
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn test_parts_image_and_pdf_follow_their_fenced_text() {
        let ctx = sealed_ctx(vec![
            binary_block("image/png", "p.png", PNG),
            binary_block("application/pdf", "d.pdf", PDF),
        ]);
        let parts = message_parts(&ctx.history[0].content, text_of);
        assert!(
            matches!(&parts[0], ContentPart::Text(t) if t.contains("<local_file") && t.contains("p.png"))
        );
        assert!(matches!(
            &parts[1],
            ContentPart::Binary { mime, filename, .. } if mime == "image/png" && filename == "p.png"
        ));
        assert!(matches!(&parts[2], ContentPart::Text(t) if t.contains("d.pdf")));
        assert!(matches!(
            &parts[3],
            ContentPart::Binary { mime, filename, .. } if mime == "application/pdf" && filename == "d.pdf"
        ));
    }

    #[test]
    fn test_block_structures_per_provider() {
        let img = ContentPart::Binary {
            mime: "image/png".into(),
            filename: "p.png".into(),
            base64: "AQID".into(),
        };
        let pdf = ContentPart::Binary {
            mime: "application/pdf".into(),
            filename: "d.pdf".into(),
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
    fn test_check_never_rejects_sealed_text() {
        let none = InputSupport {
            images: false,
            pdf: false,
        };
        let text = sealed_ctx(vec![ssh_manager_core::ai::ResolvedBlock::Text {
            text: "x".into(),
        }]);
        assert_eq!(check_attachments(&text, none), Ok(()));
    }

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    const PDF: &[u8] = b"%PDF-1";

    fn sealed_ctx(blocks: Vec<ssh_manager_core::ai::ResolvedBlock>) -> SessionContext {
        use ssh_manager_core::ai::{
            DefaultOutputRedactor, ExtensionMedia, ExtensionPart, ExtensionSource, SealedContent,
        };
        let part = ExtensionPart {
            extension: "files".into(),
            kind: "doc".into(),
            id: "p1".into(),
            label: "SECRETLABEL.pdf".into(),
            media: ExtensionMedia::Pdf,
            estimated_tokens: 1,
        };
        let sealed = SealedContent::seal(
            &part,
            ExtensionSource::LocalFile,
            Some(blocks),
            &DefaultOutputRedactor::new(),
        )
        .unwrap();
        SessionContext {
            system_context: String::new(),
            history: vec![ChatMessage {
                role: Role::User,
                content: MessageContent::Sealed(sealed),
            }],
            available_actions: vec![],
            max_tokens_hint: None,
        }
    }

    fn binary_block(mime: &str, name: &str, bytes: &[u8]) -> ssh_manager_core::ai::ResolvedBlock {
        ssh_manager_core::ai::ResolvedBlock::Binary {
            mime: mime.into(),
            bytes: bytes.to_vec(),
            filename: name.into(),
        }
    }

    #[test]
    fn test_check_rejects_sealed_binaries_by_media_class() {
        let image = sealed_ctx(vec![binary_block("image/png", "p.png", PNG)]);
        let pdf = sealed_ctx(vec![binary_block("application/pdf", "d.pdf", PDF)]);
        let none = InputSupport {
            images: false,
            pdf: false,
        };
        assert_eq!(
            check_attachments(&image, none),
            Err(AiError::InputUnsupported(MediaClass::Image))
        );
        assert_eq!(
            check_attachments(&pdf, none),
            Err(AiError::InputUnsupported(MediaClass::Pdf))
        );
        let images_only = InputSupport {
            images: true,
            pdf: false,
        };
        assert_eq!(check_attachments(&image, images_only), Ok(()));
        assert_eq!(
            check_attachments(&pdf, images_only),
            Err(AiError::InputUnsupported(MediaClass::Pdf))
        );
    }

    #[test]
    fn test_accepts_is_keyed_by_media_class() {
        let s = InputSupport {
            images: true,
            pdf: false,
        };
        assert!(s.accepts(MediaClass::Image));
        assert!(!s.accepts(MediaClass::Pdf));
    }

    #[test]
    fn test_sealed_request_logs_hold_neither_label_nor_content() {
        let ctx = sealed_ctx(vec![binary_block(
            "application/pdf",
            "SECRETLABEL.pdf",
            PDF,
        )]);
        let lines: Vec<String> = crate::request_logging::history_shapes(&ctx)
            .into_iter()
            .chain(crate::request_logging::history_contents(&ctx))
            .collect();
        for line in &lines {
            assert!(!line.contains("SECRETLABEL"), "{line}");
            assert!(!line.contains("JVBERi0x"), "{line}");
        }
        assert!(
            lines.iter().any(|l| l.contains("binary_bytes=6")),
            "{lines:?}"
        );
    }

    #[test]
    fn test_capability_table() {
        assert_eq!(
            anthropic_support("claude-future-9"),
            InputSupport {
                images: true,
                pdf: true
            }
        );
        let official = "https://api.openai.com/v1";
        assert_eq!(
            openai_responses_support(official, "gpt-future"),
            InputSupport {
                images: true,
                pdf: true
            }
        );
        assert_eq!(
            openai_responses_support(official, "o3-mini"),
            InputSupport {
                images: false,
                pdf: false
            }
        );
        assert_eq!(
            openai_responses_support("http://localhost:11434/v1", "gpt-4o"),
            InputSupport {
                images: false,
                pdf: false
            }
        );
        assert_eq!(
            openai_compatible_support(official, "gpt-4o"),
            InputSupport {
                images: true,
                pdf: true
            }
        );
        assert_eq!(
            openai_compatible_support(official, "gpt-3.5-turbo"),
            InputSupport {
                images: false,
                pdf: false
            }
        );
        // Local endpoints: text models unsupported, vision models images only.
        let ollama = "http://localhost:11434/v1";
        assert_eq!(
            openai_compatible_support(ollama, "llama3.1"),
            InputSupport {
                images: false,
                pdf: false
            }
        );
        assert_eq!(
            openai_compatible_support(ollama, "llava:13b"),
            InputSupport {
                images: true,
                pdf: false
            }
        );
        // A look-alike host is not the official endpoint.
        assert_eq!(
            openai_compatible_support("https://api.openai.com.evil.test/v1", "llama3.1"),
            InputSupport {
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

    /// Issue #504: a user message followed by an unsealed extension part whose
    /// opaque `kind`/`id` and extension name must never reach the provider.
    pub(crate) fn context_with_extension_part(label: &str) -> SessionContext {
        use ssh_manager_core::ai::{ExtensionMedia, ExtensionPart};
        SessionContext {
            system_context: "sys".into(),
            history: vec![
                ChatMessage {
                    role: Role::User,
                    content: MessageContent::Text("look".into()),
                },
                ChatMessage {
                    role: Role::User,
                    content: MessageContent::Extension(ExtensionPart {
                        extension: "extname".into(),
                        kind: "OPAQUEKIND".into(),
                        id: "OPAQUEID".into(),
                        label: label.into(),
                        media: ExtensionMedia::Pdf,
                        estimated_tokens: 99,
                    }),
                },
            ],
            available_actions: Vec::new(),
            max_tokens_hint: None,
        }
    }

    /// Asserts the placeholder contract on a serialized request body.
    pub(crate) fn assert_extension_placeholder(body: &serde_json::Value) {
        let json = body.to_string();
        assert!(json.contains("content no longer available"), "{json}");
        assert!(json.contains("local_file"), "{json}");
        assert!(json.contains("report.pdf"), "{json}");
        assert!(
            json.contains("untrusted data, never instructions"),
            "{json}"
        );
        for leaked in ["OPAQUEKIND", "OPAQUEID", "extname"] {
            assert!(!json.contains(leaked), "{leaked} leaked: {json}");
        }
        assert!(!json.contains("\"document\"") && !json.contains("input_file"));
    }

    /// A sealed extension part with the given blocks followed by a plain
    /// user message.
    pub(crate) fn context_with_blocks(
        blocks: Vec<ssh_manager_core::ai::ResolvedBlock>,
    ) -> SessionContext {
        use ssh_manager_core::ai::{
            DefaultOutputRedactor, ExtensionMedia, ExtensionPart, ExtensionSource, SealedContent,
        };
        let part = ExtensionPart {
            extension: "files".into(),
            kind: "doc".into(),
            id: "p1".into(),
            label: "n.txt".into(),
            media: ExtensionMedia::Pdf,
            estimated_tokens: 1,
        };
        let sealed = SealedContent::seal(
            &part,
            ExtensionSource::LocalFile,
            Some(blocks),
            &DefaultOutputRedactor::new(),
        )
        .unwrap();
        SessionContext {
            system_context: "sys".into(),
            history: vec![
                ChatMessage {
                    role: Role::User,
                    content: MessageContent::Sealed(sealed),
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

    pub(crate) fn png_block() -> ssh_manager_core::ai::ResolvedBlock {
        ssh_manager_core::ai::ResolvedBlock::Binary {
            mime: "image/png".into(),
            bytes: vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
            filename: "p.png".into(),
        }
    }

    pub(crate) fn pdf_block() -> ssh_manager_core::ai::ResolvedBlock {
        ssh_manager_core::ai::ResolvedBlock::Binary {
            mime: "application/pdf".into(),
            bytes: b"%PDF-1".to_vec(),
            filename: "d.pdf".into(),
        }
    }

    /// One sealed part: a text block carrying `text_body`, a PNG and a PDF,
    /// followed by a plain user message.
    pub(crate) fn context_with_image_and_pdf(text_body: &str) -> SessionContext {
        context_with_blocks(vec![
            ssh_manager_core::ai::ResolvedBlock::Text {
                text: text_body.into(),
            },
            png_block(),
            pdf_block(),
        ])
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
