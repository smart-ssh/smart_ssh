//! `ErrorCode` for [`AiError`] and the list of its codes.

use super::AiError;

impl crate::ipc_error::ErrorCode for AiError {
    fn code(&self) -> &'static str {
        AiError::code(self)
    }
}

impl AiError {
    /// Every code [`Self::code`] can return (uniqueness test in `app-logic`).
    pub const CODES: &'static [&'static str] = &[
        "AI_AUTH_FAILED",
        "AI_RATE_LIMITED",
        "AI_NETWORK_ERROR",
        "AI_INVALID_RESPONSE",
        "AI_CONTEXT_TOO_LARGE",
        "AI_PROVIDER_UNAVAILABLE",
        "AI_RESPONSE_TRUNCATED",
        "AI_MODEL_NOT_FOUND",
        "AI_MODEL_NO_TOOL_SUPPORT",
        "AI_LOCAL_PROVIDER_UNREACHABLE",
        "AI_TIMEOUT",
        "AI_TOTAL_DEADLINE",
        "AI_WEB_RESEARCH_REJECTED",
        "AI_ATTACHMENT_UNSUPPORTED",
        "AI_PROVIDER_OVERLOADED",
        "AI_PROVIDER_BILLING",
        "AI_PROVIDER_USAGE_LIMIT",
        "AI_PROVIDER_REJECTED_REQUEST",
    ];
}
