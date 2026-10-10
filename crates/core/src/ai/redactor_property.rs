//! Property test (issue #247): the live redactor never lets through a secret
//! that the frozen reference (`redactor_reference`) removes.
//!
//! A failure means a redactor change redacts less than before. Fix the
//! redactor, not the reference.

use proptest::prelude::*;

use super::redactor::{DefaultOutputRedactor as Current, OutputRedactor as CurrentTrait};
use super::redactor_generators::document;
use super::redactor_reference::{
    DefaultOutputRedactor as Reference, OutputRedactor as ReferenceTrait,
};

static REFERENCE: std::sync::OnceLock<Reference> = std::sync::OnceLock::new();
static CURRENT: std::sync::OnceLock<Current> = std::sync::OnceLock::new();

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn current_redactor_redacts_at_least_what_the_reference_does(
        (text, secrets) in document()
    ) {
        let reference = REFERENCE.get_or_init(Reference::new).redact_text(&text);
        let current = CURRENT.get_or_init(Current::new).redact_text(&text);
        for secret in &secrets {
            if !reference.contains(secret.as_str()) {
                prop_assert!(
                    !current.contains(secret.as_str()),
                    "secret {secret:?} removed by the reference but present in current output\ninput: {text:?}\ncurrent: {current:?}"
                );
            }
        }
    }
}
