//! Redacting `Debug` impls for input types that carry plaintext secrets
//! (issue #848). A derived `Debug` would print them, e.g. through a future
//! `tracing::debug!(?input)`. Secret fields show only whether they are set.

use std::fmt;

use super::{AiProviderConfigInput, AuthMethodInput, ServerInput};

/// Prints `[REDACTED]` for a set secret, `None` for an absent one.
struct Secret<'a>(Option<&'a str>);

impl fmt::Debug for Secret<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(_) => f.write_str("Some([REDACTED])"),
            None => f.write_str("None"),
        }
    }
}

impl fmt::Debug for AiProviderConfigInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiProviderConfigInput")
            .field("provider_type", &self.provider_type)
            .field("display_name", &self.display_name)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field(
                "supports_native_tool_calling",
                &self.supports_native_tool_calling,
            )
            .field("api_key", &Secret(Some(&self.api_key)))
            // Header values may carry credentials (e.g. `Authorization`).
            .field(
                "extra_headers",
                &self.extra_headers.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            )
            .field("attestation_url", &self.attestation_url)
            .field("max_tokens_override", &self.max_tokens_override)
            .field("web_research_enabled", &self.web_research_enabled)
            .finish()
    }
}

impl fmt::Debug for ServerInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerInput")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("group_id", &self.group_id)
            .field("tags", &self.tags)
            .field("auth", &self.auth)
            .field("jump_host", &self.jump_host)
            .field("sudo_password", &Secret(self.sudo_password.as_deref()))
            .field("post_ingest_policy", &self.post_ingest_policy)
            .field(
                "ai_injection_check_enabled",
                &self.ai_injection_check_enabled,
            )
            .field("sftp_server_path", &self.sftp_server_path)
            .field("start_directory", &self.start_directory)
            .finish()
    }
}

impl fmt::Debug for AuthMethodInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password { value } => f
                .debug_struct("Password")
                .field("value", &Secret(value.as_deref()))
                .finish(),
            Self::NoPassword => f.write_str("NoPassword"),
            Self::PrivateKey {
                key_content,
                passphrase,
            } => f
                .debug_struct("PrivateKey")
                .field("key_content", &Secret(key_content.as_deref()))
                .field("passphrase", &Secret(passphrase.as_deref()))
                .finish(),
            Self::Agent => f.write_str("Agent"),
            Self::Certificate {
                cert_content,
                key_content,
            } => f
                .debug_struct("Certificate")
                .field("cert_content", &Secret(cert_content.as_deref()))
                .field("key_content", &Secret(key_content.as_deref()))
                .finish(),
            Self::IdentityFile { path, passphrase } => f
                .debug_struct("IdentityFile")
                .field("path", path)
                .field("passphrase", &Secret(passphrase.as_deref()))
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "s3cr3t-do-not-print";

    fn server(auth: serde_json::Value) -> ServerInput {
        serde_json::from_value(serde_json::json!({
            "name": "web-1", "host": "example.org", "port": 22, "username": "root",
            "groupId": null, "tags": [], "jumpHost": null,
            "sudoPassword": SECRET,
            "auth": auth,
        }))
        .unwrap()
    }

    fn auth(value: serde_json::Value) -> AuthMethodInput {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn test_auth_method_input_debug_hides_every_secret() {
        let variants = [
            auth(serde_json::json!({"kind":"password","value":SECRET})),
            auth(serde_json::json!({"kind":"privateKey","keyContent":SECRET,"passphrase":SECRET})),
            auth(
                serde_json::json!({"kind":"certificate","certContent":SECRET,"keyContent":SECRET}),
            ),
            auth(serde_json::json!({"kind":"identityFile","path":"/k","passphrase":SECRET})),
        ];
        for v in variants {
            let shown = format!("{v:?}");
            assert!(!shown.contains(SECRET), "{shown}");
            assert!(shown.contains("[REDACTED]"), "{shown}");
        }
        // Non-secret fields are still printed.
        let shown = format!(
            "{:?}",
            auth(serde_json::json!({"kind":"identityFile","path":"/home/u/key","passphrase":null}))
        );
        assert!(
            shown.contains("/home/u/key") && shown.contains("passphrase: None"),
            "{shown}"
        );
    }

    #[test]
    fn test_server_input_debug_hides_sudo_password_and_auth_secrets() {
        let input = server(serde_json::json!({"kind":"password","value":SECRET}));
        assert_eq!(input.sudo_password.as_deref(), Some(SECRET));
        let shown = format!("{input:?}");
        assert!(!shown.contains(SECRET), "{shown}");
        assert!(
            shown.contains("web-1") && shown.contains("example.org"),
            "{shown}"
        );
    }

    #[test]
    fn test_ai_provider_config_input_debug_hides_key_and_header_values() {
        let input: AiProviderConfigInput = serde_json::from_value(serde_json::json!({
            "providerType": "anthropic", "displayName": "Main", "baseUrl": null,
            "model": "m", "supportsNativeToolCalling": true, "apiKey": SECRET,
            "extraHeaders": [["X-Auth", SECRET]], "attestationUrl": null,
            "maxTokensOverride": null,
        }))
        .unwrap();
        assert_eq!(input.api_key, SECRET);
        assert_eq!(input.extra_headers[0].1, SECRET);
        assert!(input.web_research_enabled);
        let shown = format!("{input:?}");
        assert!(!shown.contains(SECRET), "{shown}");
        assert!(
            shown.contains("Main") && shown.contains("X-Auth"),
            "{shown}"
        );
    }
}
