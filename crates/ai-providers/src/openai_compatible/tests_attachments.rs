//! Attachment-handling unit tests for `openai_compatible` (the provider module).

use super::*;
use crate::content::test_fixtures::{
    context_with_blocks, context_with_image_and_pdf, first_events, pdf_block,
};
use ssh_manager_core::ai::{AiEvent, MediaClass};

fn budget() -> std::sync::Arc<crate::rate_limit_budget::ProviderBudgetGuard> {
    std::sync::Arc::new(crate::rate_limit_budget::ProviderBudgetGuard::new())
}

#[test]
fn test_openai_compatible_image_and_pdf_blocks() {
    let p = OpenAiCompatibleProvider::new(
        "https://api.openai.com/v1",
        "gpt-4o",
        "k",
        true,
        Vec::new(),
        budget(),
        None,
    );
    let body = p.build_request_body(&context_with_image_and_pdf("hello"));
    let content = body["messages"][1]["content"].as_array().expect("array");
    let types: Vec<&str> = content
        .iter()
        .map(|b| b["type"].as_str().unwrap())
        .collect();
    // Trailing texts (fenced PDF notice + next user message) coalesce.
    assert_eq!(types, ["text", "image_url", "text", "file", "text"]);
    assert_eq!(
        content[1]["image_url"]["url"],
        "data:image/png;base64,iVBORw0KGgo="
    );
    assert_eq!(
        content[3]["file"],
        json!({"filename":"d.pdf","file_data":"data:application/pdf;base64,JVBERi0x"})
    );
}

#[tokio::test]
async fn test_text_model_rejects_image_without_request() {
    let p = OpenAiCompatibleProvider::new(
        "http://127.0.0.1:1/v1",
        "llama3.1",
        "k",
        false,
        Vec::new(),
        budget(),
        None,
    );
    let events = first_events(&p, context_with_image_and_pdf("x")).await;
    assert_eq!(events.len(), 1);
    assert!(
        matches!(
            events[0],
            AiEvent::Error(AiError::InputUnsupported(MediaClass::Image))
        ),
        "{events:?}"
    );
}

#[tokio::test]
async fn test_vision_model_without_pdf_input_rejects_pdf_without_request() {
    let p = OpenAiCompatibleProvider::new(
        "http://127.0.0.1:1/v1",
        "llava:13b",
        "k",
        false,
        Vec::new(),
        budget(),
        None,
    );
    // Only the PDF remains unsupported.
    let ctx = context_with_blocks(vec![pdf_block()]);
    let events = first_events(&p, ctx).await;
    assert!(
        matches!(
            events[..],
            [AiEvent::Error(AiError::InputUnsupported(MediaClass::Pdf))]
        ),
        "{events:?}"
    );
}

/// Counter-proof for the two rejections above: text attachments alone
/// pass the check on the same text-only model (it would then try, and
/// fail to connect, instead of reporting an attachment error).
#[tokio::test]
async fn test_text_attachment_is_not_rejected_on_text_model() {
    let p = OpenAiCompatibleProvider::new(
        "http://127.0.0.1:1/v1",
        "llama3.1",
        "k",
        false,
        Vec::new(),
        budget(),
        None,
    );
    let ctx = context_with_blocks(vec![ssh_manager_core::ai::ResolvedBlock::Text {
        text: "x".into(),
    }]);
    let events = first_events(&p, ctx).await;
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AiEvent::Error(AiError::InputUnsupported(_)))),
        "{events:?}"
    );
}
