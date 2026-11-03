//! Swappable sudo password and redactor of a session (Issue #355).

use ssh_manager_core::sync::{read_tolerating_poison, write_tolerating_poison};
use std::sync::Arc;

use ssh_manager_core::ai::OutputRedactor;

/// Issue #355: the stored sudo password of a session. Behind a lock so a
/// reconnect can replace it together with the redactor.
pub struct SwappableSudoPassword(std::sync::RwLock<Option<Arc<secrecy::SecretString>>>);

impl SwappableSudoPassword {
    pub fn new(value: Option<secrecy::SecretString>) -> Self {
        Self(std::sync::RwLock::new(value.map(Arc::new)))
    }

    pub fn get(&self) -> Option<Arc<secrecy::SecretString>> {
        read_tolerating_poison(&self.0).clone()
    }

    pub fn is_some(&self) -> bool {
        self.get().is_some()
    }

    pub fn is_none(&self) -> bool {
        self.get().is_none()
    }

    pub(super) fn replace(&self, value: Option<secrecy::SecretString>) {
        *write_tolerating_poison(&self.0) = value.map(Arc::new);
    }
}

/// Issue #355: the redactor of a session. Behind a lock so a reconnect can
/// rebuild it with the sudo password read again from the keychain; every
/// reader sees either the old or the new redactor, never none.
pub struct SwappableRedactor {
    inner: std::sync::RwLock<Arc<dyn OutputRedactor>>,
    /// Issue #464: literal secret values inserted by an outgoing transform.
    /// They survive a reconnect (the swap only replaces `inner`) and are
    /// masked before the pattern-based redactor runs.
    literal_secrets: std::sync::RwLock<Vec<Vec<u8>>>,
}

impl SwappableRedactor {
    pub fn new(redactor: Box<dyn OutputRedactor>) -> Self {
        Self {
            inner: std::sync::RwLock::new(Arc::from(redactor)),
            literal_secrets: std::sync::RwLock::new(Vec::new()),
        }
    }

    fn current(&self) -> Arc<dyn OutputRedactor> {
        read_tolerating_poison(&self.inner).clone()
    }

    pub(super) fn replace(&self, redactor: Box<dyn OutputRedactor>) {
        *write_tolerating_poison(&self.inner) = Arc::from(redactor);
    }

    /// Issue #635: replaces the pattern-based redactor **and** the literal
    /// secret set with exactly `secrets` (empty values ignored). Only the
    /// extension rebuild path calls this; every other swap (reconnect) keeps
    /// the literal set add-only. Text stored already redacted is not touched.
    pub(super) fn replace_with_literals(
        &self,
        redactor: Box<dyn OutputRedactor>,
        secrets: &[secrecy::SecretString],
    ) {
        use secrecy::ExposeSecret;
        // Build the complete new set first, then install it with a single
        // assignment under one write guard: a concurrent reader sees either
        // the old or the new set, never an empty one (no fail-open window).
        let mut fresh: Vec<Vec<u8>> = Vec::new();
        for secret in secrets {
            let value = secret.expose_secret().as_bytes();
            if !value.is_empty() && !fresh.iter().any(|k| k == value) {
                fresh.push(value.to_vec());
            }
        }
        // Longest first, so a secret that contains another is masked whole.
        fresh.sort_by_key(|k| std::cmp::Reverse(k.len()));
        let mut known = write_tolerating_poison(&self.literal_secrets);
        *known = fresh;
        drop(known);
        self.replace(redactor);
    }

    /// Issue #464: from now on `secrets` are masked wherever this redactor
    /// runs (live output, result, ledger, history, logs). Called before the
    /// command that uses them starts, so its own output is already covered.
    /// Empty values are ignored (they would match everywhere).
    pub fn add_secret_values(&self, secrets: &[secrecy::SecretString]) {
        use secrecy::ExposeSecret;
        let mut known = write_tolerating_poison(&self.literal_secrets);
        for secret in secrets {
            let value = secret.expose_secret().as_bytes();
            if !value.is_empty() && !known.iter().any(|k| k == value) {
                known.push(value.to_vec());
            }
        }
        // Longest first, so a secret that contains another is masked whole.
        known.sort_by_key(|k| std::cmp::Reverse(k.len()));
    }

    fn mask_literals(&self, bytes: &[u8]) -> Vec<u8> {
        let known = read_tolerating_poison(&self.literal_secrets);
        let placeholder = ssh_manager_core::ai::REDACTED_PLACEHOLDER.as_bytes();
        let mut out = bytes.to_vec();
        for secret in known.iter() {
            let mut next = Vec::with_capacity(out.len());
            let mut i = 0;
            while i < out.len() {
                if out[i..].starts_with(secret) {
                    next.extend_from_slice(placeholder);
                    i += secret.len();
                } else {
                    next.push(out[i]);
                    i += 1;
                }
            }
            out = next;
        }
        out
    }
}

impl OutputRedactor for SwappableRedactor {
    fn redact(
        &self,
        output: &ssh_manager_core::ssh::CommandOutput,
    ) -> ssh_manager_core::ssh::CommandOutput {
        let masked = ssh_manager_core::ssh::CommandOutput {
            stdout: self.mask_literals(&output.stdout),
            stderr: self.mask_literals(&output.stderr),
            exit_code: output.exit_code,
            truncated: output.truncated,
        };
        self.current().redact(&masked)
    }

    fn redact_text(&self, text: &str) -> String {
        let masked = String::from_utf8_lossy(&self.mask_literals(text.as_bytes())).into_owned();
        self.current().redact_text(&masked)
    }
}

impl AsRef<dyn OutputRedactor> for SwappableRedactor {
    fn as_ref(&self) -> &(dyn OutputRedactor + 'static) {
        self
    }
}
