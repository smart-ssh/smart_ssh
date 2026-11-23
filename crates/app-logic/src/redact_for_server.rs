//! Redaction service for extensions (`ext::redact_for_server`): text that
//! leaves the device is masked with the server's current redactor.

use std::sync::Arc;

use secrecy::SecretString;

use ssh_manager_core::ai::{DefaultOutputRedactor, OutputRedactor};
use ssh_manager_core::profiles::{CredentialStore, ProfileStore};
use ssh_manager_core::shared::ServerId;

use crate::session_extension::SessionExtension;

/// Result of [`redact_for_server`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedText {
    pub text: String,
    /// The server no longer exists: only the generic patterns were applied.
    pub generic_only: bool,
    /// A secret lookup failed (keychain, profile store): masking may be
    /// incomplete.
    pub incomplete: bool,
}

/// Redacts `text` with the server's **current** redactor: built-in patterns,
/// the server's sudo password and the secrets the session extension knows
/// for it. For a server that no longer exists only the generic patterns
/// apply, and the result says so (`generic_only`). A failed secret lookup
/// does not fail the call but sets `incomplete`.
pub async fn redact_for_server(
    profile_store: &dyn ProfileStore,
    credential_store: Arc<dyn CredentialStore + Send + Sync>,
    extension: Option<&dyn SessionExtension>,
    server_id: ServerId,
    text: &str,
) -> RedactedText {
    let local_tags = crate::dto::is_local(server_id).then(Vec::new);
    let lookup = crate::session_extension::scope_lookup(profile_store, server_id, local_tags).await;
    if lookup.scope.is_none() {
        return RedactedText {
            text: DefaultOutputRedactor::new().redact_text(text),
            generic_only: true,
            incomplete: false,
        };
    }
    let sudo =
        crate::server_redaction::read_sudo_password_checked_async(credential_store, server_id)
            .await;
    let secrets: Vec<SecretString> =
        crate::session_extension::known_secrets(extension, lookup.scope.as_ref());
    let (redactor, incomplete) =
        crate::session_extension::redactor_for_connect(&sudo, &lookup, &secrets);
    RedactedText {
        text: redactor.redact_text(text),
        generic_only: false,
        incomplete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_extension::ServerScope;
    use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore, ServerBuilder};
    use ssh_manager_core::ai::REDACTED_PLACEHOLDER;

    const GENERIC: &str = "ghp_0123456789abcdefghijABCDEFGHIJ012345";
    const EXT_SECRET: &str = "zebra-only-the-extension-knows";

    struct Ext(ServerId);

    #[async_trait::async_trait]
    impl SessionExtension for Ext {
        async fn context_section(
            &self,
            _scope: &ServerScope,
            _language: crate::system_prompt::PromptLanguage,
        ) -> Option<String> {
            None
        }
        fn known_secrets(&self, scope: &ServerScope) -> Vec<SecretString> {
            if scope.server_id == self.0 {
                vec![SecretString::from(EXT_SECRET.to_string())]
            } else {
                Vec::new()
            }
        }
    }

    fn creds() -> Arc<dyn CredentialStore + Send + Sync> {
        Arc::new(InMemoryCredentialStore::new())
    }

    #[tokio::test]
    async fn test_redact_for_server_masks_extension_secret_and_generic_pattern() {
        let srv = ServerBuilder::new().name("web").build();
        let store = InMemoryProfileStore::new().with_server(srv.clone());
        let ext = Ext(srv.id);
        let text = format!("a {EXT_SECRET} b {GENERIC} c");
        let out = redact_for_server(&store, creds(), Some(&ext), srv.id, &text).await;
        assert!(!out.text.contains(EXT_SECRET), "{}", out.text);
        assert!(!out.text.contains(GENERIC), "{}", out.text);
        assert!(out.text.contains(REDACTED_PLACEHOLDER));
        assert!(!out.generic_only);
        assert!(!out.incomplete);
        // Without the extension the literal secret stays: proves it is the
        // extension's value that masks it.
        let plain = redact_for_server(&store, creds(), None, srv.id, &text).await;
        assert!(plain.text.contains(EXT_SECRET));
    }

    #[tokio::test]
    async fn test_redact_for_deleted_server_is_generic_only() {
        let gone = ServerId::new();
        let store = InMemoryProfileStore::new();
        let ext = Ext(gone);
        let text = format!("{EXT_SECRET} {GENERIC}");
        let out = redact_for_server(&store, creds(), Some(&ext), gone, &text).await;
        assert!(out.generic_only);
        assert!(!out.text.contains(GENERIC));
        // Only generic patterns: the extension secret is not applied.
        assert!(out.text.contains(EXT_SECRET));
    }
}
