//! Extension point for per-server session data (Spec 0038, section 2a).
//!
//! An edition can register any number of [`SessionExtension`]s (combined by
//! [`CompositeSessionExtension`]). It contributes
//! (1) a text section for the AI session context of a server, (2) literal
//! secret values the per-server redactor must mask and (3) gets told when a
//! server or group was deleted. The core only calls the trait; it does not
//! know what the extension stores. Without an extension nothing changes:
//! no section in the prompt, the redactor holds the built-in patterns and
//! the sudo password only.
//!
//! Security: the extension text is untrusted for the prompt in the same
//! sense as notes. It is redacted first and fenced afterwards (Spec 0039,
//! ADR 0034 order). Known secrets only ever extend the redactor.

use secrecy::{ExposeSecret, SecretString};

use ssh_manager_core::ai::{fence_untrusted, DefaultOutputRedactor, OutputRedactor, UntrustedKind};
use ssh_manager_core::profiles::{GroupId, ProfileStore, Server};
use ssh_manager_core::shared::ServerId;

use crate::compaction::SystemContextParts;
use crate::state::AppState;

pub use ssh_manager_core::outgoing::ServerScope;

/// Registered through `Wiring`. All methods have no-op defaults so an
/// extension only implements what it needs.
#[async_trait::async_trait]
pub trait SessionExtension: Send + Sync {
    /// Text appended to the session system context of this server. The core
    /// redacts it and places it in its own fenced section after the core
    /// paragraphs; it is asked again on every turn. `language` is the prompt
    /// language of the session (the one the section heading uses). Async, so
    /// the extension can await reads from its own data store.
    async fn context_section(
        &self,
        _scope: &ServerScope,
        _language: crate::system_prompt::PromptLanguage,
    ) -> Option<String> {
        None
    }

    /// Literal values the redactor of this server must mask in addition to
    /// the sudo password. Matched literally, never as a regex.
    fn known_secrets(&self, _scope: &ServerScope) -> Vec<SecretString> {
        Vec::new()
    }

    /// Called after the core deleted the server.
    fn server_removed(&self, _server_id: ServerId) {}

    /// Called after the core deleted the group (also for every group removed
    /// by a cascade).
    fn group_removed(&self, _group_id: GroupId) {}
}

/// Combines several extensions into one (Spec 0038, section 2a). Sections
/// are asked sequentially in registration order and joined by a blank line;
/// secrets are united; removal callbacks reach every extension. The result
/// still goes through the single redact-then-fence path.
pub struct CompositeSessionExtension {
    extensions: Vec<std::sync::Arc<dyn SessionExtension>>,
}

impl CompositeSessionExtension {
    pub fn new(extensions: Vec<std::sync::Arc<dyn SessionExtension>>) -> Self {
        Self { extensions }
    }
}

#[async_trait::async_trait]
impl SessionExtension for CompositeSessionExtension {
    async fn context_section(
        &self,
        scope: &ServerScope,
        language: crate::system_prompt::PromptLanguage,
    ) -> Option<String> {
        let mut sections = Vec::new();
        for ext in &self.extensions {
            if let Some(text) = ext.context_section(scope, language).await {
                if !text.trim().is_empty() {
                    sections.push(text);
                }
            }
        }
        (!sections.is_empty()).then(|| sections.join("\n\n"))
    }

    fn known_secrets(&self, scope: &ServerScope) -> Vec<SecretString> {
        let mut out: Vec<SecretString> = Vec::new();
        for secret in self.extensions.iter().flat_map(|e| e.known_secrets(scope)) {
            if !out
                .iter()
                .any(|s| s.expose_secret() == secret.expose_secret())
            {
                out.push(secret);
            }
        }
        out
    }

    fn server_removed(&self, server_id: ServerId) {
        for ext in &self.extensions {
            ext.server_removed(server_id);
        }
    }

    fn group_removed(&self, group_id: GroupId) {
        for ext in &self.extensions {
            ext.group_removed(group_id);
        }
    }
}

/// Scope of a stored server. A broken group chain is logged and leaves the
/// chain empty (the scope is context for the extension, not a security
/// decision).
pub async fn server_scope(store: &dyn ProfileStore, server: &Server) -> ServerScope {
    let group_chain = match server.group_id {
        None => Vec::new(),
        Some(group_id) => match store.group_chain(&group_id).await {
            Ok(chain) => chain.into_iter().map(|g| g.id).collect(),
            Err(err) => {
                tracing::warn!(
                    server_id = %server.id.0,
                    error = %err,
                    "group_chain failed — the extension scope has no groups"
                );
                Vec::new()
            }
        },
    };
    ServerScope {
        server_id: server.id,
        group_chain,
        tags: server.tags.clone(),
    }
}

/// Scope of a server by id. `local_tags` is `Some` for the local
/// pseudo-server (it has no stored row).
pub async fn scope_for_id(
    store: &dyn ProfileStore,
    server_id: ServerId,
    local_tags: Option<Vec<String>>,
) -> Option<ServerScope> {
    if let Some(tags) = local_tags {
        return Some(ServerScope {
            server_id,
            group_chain: Vec::new(),
            tags,
        });
    }
    match store.get_server(&server_id).await {
        Ok(server) => Some(server_scope(store, &server).await),
        Err(_) => None,
    }
}

/// Builds the redactor: built-in patterns, the sudo password, and the
/// extension secrets, each as an escaped literal. Empty values are skipped
/// (an empty literal would match everywhere).
pub fn redactor_with_secrets(
    sudo_password: Option<&SecretString>,
    extra_secrets: &[SecretString],
) -> Box<dyn OutputRedactor> {
    let patterns: Vec<regex::Regex> = sudo_password
        .into_iter()
        .chain(extra_secrets.iter())
        .map(|s| s.expose_secret())
        .filter(|s| !s.is_empty())
        .filter_map(|s| regex::Regex::new(&regex::escape(s)).ok())
        .collect();
    if patterns.is_empty() {
        Box::new(DefaultOutputRedactor::new())
    } else {
        Box::new(DefaultOutputRedactor::with_extra_patterns(patterns))
    }
}

/// Known secrets of the extension for `scope`; empty without an extension.
pub fn known_secrets(
    extension: Option<&dyn SessionExtension>,
    scope: Option<&ServerScope>,
) -> Vec<SecretString> {
    match (extension, scope) {
        (Some(ext), Some(scope)) => ext.known_secrets(scope),
        _ => Vec::new(),
    }
}

/// Heading above the extension section.
pub fn extension_heading(language: crate::system_prompt::PromptLanguage) -> &'static str {
    match language {
        crate::system_prompt::PromptLanguage::De => "\n\n## Erweiterungs-Kontext\n",
        crate::system_prompt::PromptLanguage::En
        | crate::system_prompt::PromptLanguage::Other(_) => "\n\n## Extension context\n",
    }
}

/// Appends the fenced extension section to `parts.base` (redact, then
/// fence). Returns whether a section was added — the caller treats that like
/// notes: untrusted content has entered the context (Spec 0039, section 5).
/// Without an extension, or with an empty section, `parts` stays untouched.
pub async fn append_extension_section(
    parts: &mut SystemContextParts,
    extension: Option<&dyn SessionExtension>,
    scope: Option<&ServerScope>,
    redactor: &dyn OutputRedactor,
) -> bool {
    let (Some(ext), Some(scope)) = (extension, scope) else {
        return false;
    };
    let Some(text) = ext.context_section(scope, parts.language).await else {
        return false;
    };
    if text.trim().is_empty() {
        return false;
    }
    let redacted = redactor.redact_text(&text);
    parts.base.push_str(extension_heading(parts.language));
    parts.base.push_str(&fence_untrusted(
        UntrustedKind::ServerNote,
        "Extension",
        &redacted,
    ));
    true
}

/// Rebuilds the redactor of every open session on `server_id` — for an
/// extension whose secret values changed. The swap goes through
/// `SwappableRedactor`, so history already in the session is re-redacted on
/// the next send.
pub async fn rebuild_server_redactors(state: &AppState, server_id: ServerId) {
    rebuild_redactors_for_sessions(
        &state.sessions,
        state.profile_store.as_ref(),
        state.credential_store.as_ref(),
        state.session_extension.as_deref(),
        server_id,
    )
    .await;
}

/// [`rebuild_server_redactors`] on its parts, so tests need no `AppState`.
pub async fn rebuild_redactors_for_sessions(
    sessions_manager: &crate::session::SessionManager,
    profile_store: &dyn ProfileStore,
    credential_store: &dyn ssh_manager_core::profiles::CredentialStore,
    extension: Option<&dyn SessionExtension>,
    server_id: ServerId,
) {
    let sessions: Vec<_> = sessions_manager
        .snapshot()
        .into_iter()
        .filter(|e| e.server_id == server_id)
        .filter_map(|e| sessions_manager.get(e.session_id))
        .collect();
    if sessions.is_empty() {
        return;
    }
    let sudo =
        crate::server_redaction::read_sudo_password_for_redaction(credential_store, server_id);
    let group_chain = match profile_store.get_server(&server_id).await {
        Ok(server) => server_scope(profile_store, &server).await.group_chain,
        Err(_) => Vec::new(),
    };
    for session in sessions {
        let scope = ServerScope {
            server_id,
            group_chain: group_chain.clone(),
            tags: session.tags.clone(),
        };
        let secrets = known_secrets(extension, Some(&scope));
        // The only path that replaces (not extends) the literal secret set.
        session
            .swap_redactor_with_literals(redactor_with_secrets(sudo.as_ref(), &secrets), &secrets);
    }
}

/// Test double recording the removal callbacks.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct RecordingExtension {
    pub(crate) servers: std::sync::Mutex<Vec<ServerId>>,
    pub(crate) groups: std::sync::Mutex<Vec<GroupId>>,
}

#[cfg(test)]
impl SessionExtension for RecordingExtension {
    fn server_removed(&self, server_id: ServerId) {
        self.servers.lock().unwrap().push(server_id);
    }
    fn group_removed(&self, group_id: GroupId) {
        self.groups.lock().unwrap().push(group_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::system_prompt::PromptLanguage;
    use ssh_manager_core::ai::REDACTED_PLACEHOLDER;

    const SECRET: &str = "a.b*c(d)[e]+?^$|{1}\\x-secret";

    /// Returns a section and a secret for exactly one server.
    struct TestExtension {
        server: ServerId,
        section: Option<String>,
        secrets: Vec<String>,
    }

    #[async_trait::async_trait]
    impl SessionExtension for TestExtension {
        async fn context_section(
            &self,
            scope: &ServerScope,
            _language: PromptLanguage,
        ) -> Option<String> {
            (scope.server_id == self.server)
                .then(|| self.section.clone())
                .flatten()
        }
        fn known_secrets(&self, scope: &ServerScope) -> Vec<SecretString> {
            if scope.server_id != self.server {
                return Vec::new();
            }
            self.secrets
                .iter()
                .map(|s| SecretString::from(s.clone()))
                .collect()
        }
    }

    fn scope(server_id: ServerId) -> ServerScope {
        ServerScope {
            server_id,
            group_chain: Vec::new(),
            tags: Vec::new(),
        }
    }

    fn parts(language: PromptLanguage) -> SystemContextParts {
        SystemContextParts {
            base: "BASE".to_string(),
            note_sections: Vec::new(),
            language,
        }
    }

    #[tokio::test]
    async fn test_section_is_fenced_after_core_text_in_both_languages() {
        let id = ServerId::new();
        let ext = TestExtension {
            server: id,
            section: Some("deploys to /srv/app".to_string()),
            secrets: Vec::new(),
        };
        for (language, heading) in [
            (PromptLanguage::De, "## Erweiterungs-Kontext"),
            (PromptLanguage::En, "## Extension context"),
        ] {
            let mut p = parts(language);
            let redactor = DefaultOutputRedactor::new();
            let added =
                append_extension_section(&mut p, Some(&ext), Some(&scope(id)), &redactor).await;
            assert!(added);
            let text = p.assemble();
            assert!(text.starts_with("BASE"));
            let heading_at = text.find(heading).expect("heading");
            let fence_at = text.find("<server_note").expect("fence");
            assert!(heading_at < fence_at);
            assert!(text.contains("deploys to /srv/app"));
        }
    }

    #[tokio::test]
    async fn test_without_extension_or_section_the_context_is_unchanged() {
        let id = ServerId::new();
        let redactor = DefaultOutputRedactor::new();
        let before = parts(PromptLanguage::En).assemble();

        let mut none = parts(PromptLanguage::En);
        assert!(!append_extension_section(&mut none, None, Some(&scope(id)), &redactor).await);
        assert_eq!(none.assemble(), before);

        // another server: the extension returns no section
        let ext = TestExtension {
            server: id,
            section: Some("x".to_string()),
            secrets: Vec::new(),
        };
        let mut other = parts(PromptLanguage::En);
        assert!(
            !append_extension_section(
                &mut other,
                Some(&ext),
                Some(&scope(ServerId::new())),
                &redactor
            )
            .await
        );
        assert_eq!(other.assemble(), before);

        // whitespace-only section: no empty section either
        let blank = TestExtension {
            server: id,
            section: Some("  \n ".to_string()),
            secrets: Vec::new(),
        };
        let mut p = parts(PromptLanguage::De);
        let before_de = p.assemble();
        assert!(!append_extension_section(&mut p, Some(&blank), Some(&scope(id)), &redactor).await);
        assert_eq!(p.assemble(), before_de);
    }

    /// Answers per language and only after an actual `.await` point.
    struct LangExtension {
        gate: tokio::sync::RwLock<()>,
    }

    #[async_trait::async_trait]
    impl SessionExtension for LangExtension {
        async fn context_section(
            &self,
            _scope: &ServerScope,
            language: PromptLanguage,
        ) -> Option<String> {
            let _guard = self.gate.read().await;
            match language {
                PromptLanguage::De => Some("deutscher Text".to_string()),
                _ => Some("english text".to_string()),
            }
        }
    }

    #[tokio::test]
    async fn test_section_receives_the_prompt_language_and_may_await() {
        let ext = LangExtension {
            gate: tokio::sync::RwLock::new(()),
        };
        let id = ServerId::new();
        let redactor = DefaultOutputRedactor::new();
        for (language, present, absent) in [
            (PromptLanguage::De, "deutscher Text", "english text"),
            (PromptLanguage::En, "english text", "deutscher Text"),
        ] {
            let mut p = parts(language);
            assert!(
                append_extension_section(&mut p, Some(&ext), Some(&scope(id)), &redactor).await
            );
            let text = p.assemble();
            assert!(text.contains(present), "{text}");
            assert!(!text.contains(absent), "{text}");
        }
    }

    #[tokio::test]
    async fn test_section_is_redacted_before_it_is_fenced() {
        let id = ServerId::new();
        let ext = TestExtension {
            server: id,
            section: Some(format!("token is {SECRET}")),
            secrets: vec![SECRET.to_string()],
        };
        let secrets = known_secrets(Some(&ext), Some(&scope(id)));
        let redactor = redactor_with_secrets(None, &secrets);
        let mut p = parts(PromptLanguage::En);
        assert!(
            append_extension_section(&mut p, Some(&ext), Some(&scope(id)), redactor.as_ref()).await
        );
        let text = p.assemble();
        assert!(!text.contains(SECRET));
        assert!(text.contains(REDACTED_PLACEHOLDER));
    }

    #[test]
    fn test_known_secret_is_masked_literally_and_only_for_its_server() {
        let a = ServerId::new();
        let b = ServerId::new();
        let ext = TestExtension {
            server: a,
            section: None,
            secrets: vec![SECRET.to_string(), String::new()],
        };
        let text = format!("pre {SECRET} post");

        let for_a = redactor_with_secrets(None, &known_secrets(Some(&ext), Some(&scope(a))));
        let masked = for_a.redact_text(&text);
        assert!(!masked.contains(SECRET), "{masked}");
        // literal: the surrounding text survives, the empty secret masks nothing
        assert!(masked.starts_with("pre ") && masked.ends_with(" post"));

        let for_b = redactor_with_secrets(None, &known_secrets(Some(&ext), Some(&scope(b))));
        assert_eq!(for_b.redact_text(&text), text);
    }

    #[test]
    fn test_known_secret_does_not_replace_the_sudo_password() {
        let sudo = SecretString::from("sudo-pw-1234".to_string());
        let extra = [SecretString::from("ext-secret-xyz".to_string())];
        let redactor = redactor_with_secrets(Some(&sudo), &extra);
        let masked = redactor.redact_text("sudo-pw-1234 and ext-secret-xyz");
        assert!(!masked.contains("sudo-pw-1234"));
        assert!(!masked.contains("ext-secret-xyz"));
    }

    fn two_extensions(
        a_section: Option<&str>,
        b_section: Option<&str>,
        id: ServerId,
    ) -> CompositeSessionExtension {
        let mk = |sec: Option<&str>, secret: &str| -> std::sync::Arc<dyn SessionExtension> {
            std::sync::Arc::new(TestExtension {
                server: id,
                section: sec.map(str::to_string),
                secrets: vec![secret.to_string()],
            })
        };
        CompositeSessionExtension::new(vec![
            mk(a_section, "secret-from-a"),
            mk(b_section, "secret-from-b"),
        ])
    }

    #[tokio::test]
    async fn test_composite_joins_sections_in_order_and_drops_empty() {
        let id = ServerId::new();
        let both = two_extensions(Some("first"), Some("second"), id);
        let got = both.context_section(&scope(id), PromptLanguage::En).await;
        assert_eq!(got.as_deref(), Some("first\n\nsecond"));

        let only_b = two_extensions(None, Some("second"), id);
        let got = only_b.context_section(&scope(id), PromptLanguage::En).await;
        assert_eq!(got.as_deref(), Some("second"));

        let blank = two_extensions(Some("  \n"), None, id);
        assert!(blank
            .context_section(&scope(id), PromptLanguage::En)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn test_composite_one_fence_and_both_secrets_masked() {
        let id = ServerId::new();
        let ext = two_extensions(
            Some("a uses secret-from-a"),
            Some("b uses secret-from-b"),
            id,
        );
        let secrets = known_secrets(Some(&ext), Some(&scope(id)));
        assert_eq!(secrets.len(), 2);
        let redactor = redactor_with_secrets(None, &secrets);
        let mut p = parts(PromptLanguage::En);
        assert!(
            append_extension_section(&mut p, Some(&ext), Some(&scope(id)), redactor.as_ref()).await
        );
        let text = p.assemble();
        assert_eq!(text.matches("## Extension context").count(), 1, "{text}");
        assert_eq!(text.matches("<server_note").count(), 1, "{text}");
        assert!(text.contains("a uses") && text.contains("b uses"));
        assert!(!text.contains("secret-from-a") && !text.contains("secret-from-b"));
        let out = redactor.redact_text("x secret-from-a y secret-from-b");
        assert!(!out.contains("secret-from-"), "{out}");
    }

    #[test]
    fn test_composite_secrets_are_deduplicated_in_order() {
        let id = ServerId::new();
        let mk = |s: &[&str]| -> std::sync::Arc<dyn SessionExtension> {
            std::sync::Arc::new(TestExtension {
                server: id,
                section: None,
                secrets: s.iter().map(|x| x.to_string()).collect(),
            })
        };
        let c = CompositeSessionExtension::new(vec![mk(&["a", "b"]), mk(&["b", "c"])]);
        let got: Vec<String> = c
            .known_secrets(&scope(id))
            .iter()
            .map(|s| s.expose_secret().to_string())
            .collect();
        assert_eq!(got, ["a", "b", "c"]);
    }

    #[test]
    fn test_composite_removal_callbacks_reach_all_in_order() {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(u8, bool)>::new()));
        struct Logging(u8, std::sync::Arc<std::sync::Mutex<Vec<(u8, bool)>>>);
        impl SessionExtension for Logging {
            fn server_removed(&self, _: ServerId) {
                self.1.lock().unwrap().push((self.0, true));
            }
            fn group_removed(&self, _: GroupId) {
                self.1.lock().unwrap().push((self.0, false));
            }
        }
        let c = CompositeSessionExtension::new(vec![
            std::sync::Arc::new(Logging(1, log.clone())),
            std::sync::Arc::new(Logging(2, log.clone())),
        ]);
        c.server_removed(ServerId::new());
        c.group_removed(GroupId::new());
        assert_eq!(
            *log.lock().unwrap(),
            [(1, true), (2, true), (1, false), (2, false)]
        );
    }
}
