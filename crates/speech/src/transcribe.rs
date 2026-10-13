//! Local transcription behind a trait. The whisper.cpp engine runs only on
//! a verified model file (Spec 0108, 6).

use std::path::Path;

/// `None` = auto-detect, `Some(code)` = fixed language.
pub trait Transcriber: Send + Sync {
    fn transcribe(
        &self,
        model: &Path,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<String, TranscribeError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscribeError;

/// Maps the language setting to Whisper's input: `auto` (or empty) is
/// auto-detect, an ISO code Whisper knows is passed as is.
pub fn parse_language(setting: &str) -> Result<Option<String>, InvalidLanguage> {
    let s = setting.trim().to_ascii_lowercase();
    if s.is_empty() || s == "auto" {
        return Ok(None);
    }
    if whisper_rs::get_lang_id(&s).is_some() {
        Ok(Some(s))
    } else {
        Err(InvalidLanguage)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLanguage;

/// whisper.cpp on the CPU.
#[derive(Default)]
pub struct WhisperTranscriber;

impl Transcriber for WhisperTranscriber {
    fn transcribe(
        &self,
        model: &Path,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<String, TranscribeError> {
        use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
        let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default())
            .map_err(|_| TranscribeError)?;
        let mut state = ctx.create_state().map_err(|_| TranscribeError)?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(language.unwrap_or("auto")));
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state.full(params, samples).map_err(|_| TranscribeError)?;
        let mut text = String::new();
        for segment in state.as_iter() {
            text.push_str(&segment.to_string());
        }
        Ok(text.trim().to_owned())
    }
}
