//! Shared rustls client configuration (Spec 0114, ADR 0148, issue #935).
//!
//! On Android the platform certificate verifier would need JNI
//! initialisation, so HTTPS clients use the bundled webpki roots. The crypto
//! provider is passed explicitly: with both `ring` and `aws-lc-rs` compiled
//! into the process, rustls cannot pick a process default and would panic.
//! Certificate verification is fully enabled.

use std::sync::Arc;

pub use rustls::ClientConfig;

/// Client config with the bundled webpki roots and an explicit `aws-lc-rs`
/// provider (independent of any process-level default).
pub fn webpki_client_config() -> rustls::ClientConfig {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("aws-lc-rs supports the safe default protocol versions")
    .with_root_certificates(roots)
    .with_no_client_auth()
}

/// Installs `aws-lc-rs` as the process-level default provider so
/// dependencies that build their own rustls config do not panic. Idempotent;
/// an already installed provider is kept.
pub fn install_default_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

/// Whether a process-level default provider is installed.
pub fn default_provider_installed() -> bool {
    rustls::crypto::CryptoProvider::get_default().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_builds_with_explicit_provider() {
        let cfg = webpki_client_config();
        assert!(!cfg.alpn_protocols.contains(&b"x".to_vec()));
    }

    #[test]
    fn install_is_idempotent() {
        install_default_crypto_provider();
        install_default_crypto_provider();
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
}
