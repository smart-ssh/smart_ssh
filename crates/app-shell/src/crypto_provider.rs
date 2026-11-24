//! Test: the process-level rustls provider is installed and idempotent.

#[test]
fn init_installs_default_provider_and_is_idempotent() {
    crate::init_crypto_provider();
    crate::init_crypto_provider();
    assert!(tls_config::default_provider_installed());
}
